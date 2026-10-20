//! Reading a node at a path, and the copy-on-write edit a commit applies: only the
//! directories on a touched path are opened, and only those are written back. A node's
//! creator and write list sit beside it in its parent's tree and move and go with it.
use std::collections::BTreeMap;

use ::program::{
    AccountNumber, BlobId, Error, ExecCtx, QueryCtx, already_exists, invalid, not_found,
    wrong_state,
};

use crate::Node;
use crate::objects::{Access, put_tree, tree};
use crate::paths::{is_namespace_root, join};

pub(crate) fn node_at(
    ctx: &QueryCtx,
    root: &BlobId,
    segments: &[String],
) -> Result<Option<Node>, Error> {
    let mut dir = *root;
    for (depth, name) in segments.iter().enumerate() {
        let Some(node) = tree(ctx, &dir)?.entries.remove(name) else {
            return Ok(None);
        };
        let last = depth + 1 == segments.len();
        if last {
            return Ok(Some(node));
        }
        let Node::Dir { tree, .. } = node else {
            return Ok(None);
        };
        dir = tree;
    }
    Ok(None)
}

enum Slot {
    Kept(Node),
    Open(Dir),
}

#[derive(Default)]
struct Dir {
    children: BTreeMap<String, Slot>,
    access: BTreeMap<String, Access>,
}

enum Walk<'e> {
    Edited(&'e Slot),
    Stored(BlobId),
    Gone,
}

pub(crate) struct TreeEdit {
    root: Slot,
}

impl TreeEdit {
    pub(crate) fn over(root: Option<BlobId>) -> TreeEdit {
        let root = match root {
            Some(tree) => Slot::Kept(Node::Dir { tree, entries: 0 }),
            None => Slot::Open(Dir::default()),
        };
        TreeEdit { root }
    }

    pub(crate) fn access_along(
        &self,
        ctx: &QueryCtx,
        segments: &[String],
    ) -> Result<Vec<Option<Access>>, Error> {
        let mut rows = Vec::with_capacity(segments.len());
        let mut at = Walk::Edited(&self.root);
        for name in segments {
            let (row, next) = step(ctx, at, name)?;
            rows.push(row);
            at = next;
        }
        Ok(rows)
    }

    pub(crate) fn put(
        &mut self,
        ctx: &QueryCtx,
        segments: &[String],
        node: Node,
        by: Option<AccountNumber>,
    ) -> Result<(), Error> {
        let (name, dirs) = split(segments)?;
        let parent = open_dir(&mut self.root, ctx, dirs, Create::Missing(by))?;
        parent.children.insert(name.clone(), Slot::Kept(node));
        created(parent, segments, by);
        Ok(())
    }

    pub(crate) fn mkdir(
        &mut self,
        ctx: &QueryCtx,
        segments: &[String],
        by: Option<AccountNumber>,
    ) -> Result<(), Error> {
        let (name, dirs) = split(segments)?;
        let parent = open_dir(&mut self.root, ctx, dirs, Create::Missing(by))?;
        if parent.children.contains_key(name) {
            return Err(already_exists(format!("{} exists", join(segments))));
        }
        parent
            .children
            .insert(name.clone(), Slot::Open(Dir::default()));
        created(parent, segments, by);
        Ok(())
    }

    pub(crate) fn remove(&mut self, ctx: &QueryCtx, segments: &[String]) -> Result<(), Error> {
        let (name, dirs) = split(segments)?;
        let parent = open_dir(&mut self.root, ctx, dirs, Create::Never)?;
        if parent.children.remove(name).is_none() {
            return Err(not_found(format!("nothing is at {}", join(segments))));
        }
        parent.access.remove(name);
        Ok(())
    }

    pub(crate) fn rename(
        &mut self,
        ctx: &QueryCtx,
        from: &[String],
        to: &[String],
    ) -> Result<(), Error> {
        let (from_name, from_dirs) = split(from)?;
        let (to_name, to_dirs) = split(to)?;
        let into_itself = to.len() >= from.len() && to[..from.len()] == *from;
        if into_itself {
            return Err(invalid(format!(
                "{} cannot move into itself: choose a destination outside that directory",
                join(from)
            )));
        }
        let gone = || not_found(format!("nothing is at {}", join(from)));
        let source = open_dir(&mut self.root, ctx, from_dirs, Create::Never)?;
        if !source.children.contains_key(from_name) {
            return Err(gone());
        }
        let target = open_dir(&mut self.root, ctx, to_dirs, Create::Never)?;
        if target.children.contains_key(to_name) {
            return Err(already_exists(format!("{} exists", join(to))));
        }
        let source = open_dir(&mut self.root, ctx, from_dirs, Create::Never)?;
        let Some(moved) = source.children.remove(from_name) else {
            return Err(gone());
        };
        let access = source.access.remove(from_name);
        let target = open_dir(&mut self.root, ctx, to_dirs, Create::Never)?;
        target.children.insert(to_name.clone(), moved);
        if let Some(access) = access {
            target.access.insert(to_name.clone(), access);
        }
        Ok(())
    }

    pub(crate) fn build(self, ctx: &ExecCtx) -> Result<(BlobId, u64), Error> {
        match self.root {
            Slot::Kept(Node::Dir { tree, .. }) => {
                let entries = crate::objects::tree(ctx, &tree)?.entries.len() as u64;
                Ok((tree, entries))
            }
            Slot::Kept(other) => Err(::program::corrupt(
                crate::TREE,
                &[],
                format!("the root is a {}", other.kind()),
            )),
            Slot::Open(dir) => write(ctx, dir),
        }
    }
}

fn step<'e>(ctx: &QueryCtx, at: Walk<'e>, name: &str) -> Result<(Option<Access>, Walk<'e>), Error> {
    match at {
        Walk::Edited(Slot::Open(dir)) => {
            let next = dir.children.get(name).map_or(Walk::Gone, Walk::Edited);
            Ok((dir.access.get(name).cloned(), next))
        }
        Walk::Edited(Slot::Kept(Node::Dir { tree, .. })) => stored(ctx, *tree, name),
        Walk::Stored(tree) => stored(ctx, tree, name),
        Walk::Edited(Slot::Kept(Node::File { .. } | Node::Symlink { .. })) | Walk::Gone => {
            Ok((None, Walk::Gone))
        }
    }
}

fn stored<'e>(
    ctx: &QueryCtx,
    dir: BlobId,
    name: &str,
) -> Result<(Option<Access>, Walk<'e>), Error> {
    let mut dir = tree(ctx, &dir)?;
    let next = match dir.entries.remove(name) {
        Some(Node::Dir { tree, .. }) => Walk::Stored(tree),
        Some(Node::File { .. } | Node::Symlink { .. }) | None => Walk::Gone,
    };
    Ok((dir.access.remove(name), next))
}

fn created(dir: &mut Dir, segments: &[String], by: Option<AccountNumber>) {
    let Some(creator) = by else {
        return;
    };
    let Some(name) = segments.last() else {
        return;
    };
    if is_namespace_root(segments) {
        return;
    }
    dir.access.entry(name.clone()).or_insert(Access {
        creator: Some(creator),
        write: None,
    });
}

#[derive(Clone, Copy)]
enum Create {
    Missing(Option<AccountNumber>),
    Never,
}

fn split(segments: &[String]) -> Result<(&String, &[String]), Error> {
    segments.split_last().ok_or_else(|| {
        invalid("the root is not a path a change names: name a path beneath the root")
    })
}

fn open(slot: &mut Slot, ctx: &QueryCtx, path: &[String]) -> Result<(), Error> {
    let Slot::Kept(node) = slot else {
        return Ok(());
    };
    let Node::Dir { tree, .. } = node else {
        return Err(wrong_state(format!(
            "{} is a {}, not a directory",
            join(path),
            node.kind()
        )));
    };
    let stored = crate::objects::tree(ctx, tree)?;
    let children = stored
        .entries
        .into_iter()
        .map(|(name, node)| (name, Slot::Kept(node)))
        .collect();
    *slot = Slot::Open(Dir {
        children,
        access: stored.access,
    });
    Ok(())
}

fn open_dir<'e>(
    root: &'e mut Slot,
    ctx: &QueryCtx,
    dirs: &[String],
    create: Create,
) -> Result<&'e mut Dir, Error> {
    let mut slot = root;
    for (depth, name) in dirs.iter().enumerate() {
        open(slot, ctx, &dirs[..depth])?;
        let Slot::Open(dir) = slot else {
            unreachable!("open leaves a directory open");
        };
        let missing = !dir.children.contains_key(name);
        if missing {
            let Create::Missing(by) = create else {
                return Err(not_found(format!("no directory {}", join(&dirs[..=depth]))));
            };
            dir.children
                .insert(name.clone(), Slot::Open(Dir::default()));
            created(dir, &dirs[..=depth], by);
        }
        slot = dir
            .children
            .get_mut(name)
            .ok_or_else(|| not_found(format!("no directory {}", join(&dirs[..=depth]))))?;
    }
    open(slot, ctx, dirs)?;
    let Slot::Open(dir) = slot else {
        unreachable!("open leaves a directory open");
    };
    Ok(dir)
}

fn write(ctx: &ExecCtx, dir: Dir) -> Result<(BlobId, u64), Error> {
    let mut entries = BTreeMap::new();
    for (name, slot) in dir.children {
        let node = match slot {
            Slot::Kept(node) => node,
            Slot::Open(child) => {
                let (tree, entries) = write(ctx, child)?;
                Node::Dir { tree, entries }
            }
        };
        entries.insert(name, node);
    }
    let count = entries.len() as u64;
    let id = put_tree(
        ctx,
        &crate::Tree {
            entries,
            access: dir.access,
        },
    )?;
    Ok((id, count))
}
