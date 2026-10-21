//! Reading a node at a path, and the copy-on-write edit a commit applies: only the directories on
//! a touched path are opened and written back, each on its path's line, and a moved or new node is
//! put on the line where it lands. A node's creator and write list sit beside it in its parent.
use std::collections::BTreeMap;

use ::program::{
    AccountNumber, BlobId, Error, ExecCtx, QueryCtx, already_exists, invalid, not_found,
    wrong_state,
};

use crate::Node;
use crate::objects::{Access, At, FILE, TREE, Tree, body, decoded, line, put_on, tree_at};
use crate::paths::{Roots, join};

pub(crate) fn node_at(
    ctx: &QueryCtx,
    root: &BlobId,
    segments: &[String],
    at: At,
) -> Result<Option<Node>, Error> {
    let mut dir = *root;
    for (depth, name) in segments.iter().enumerate() {
        let Some(node) = tree_at(ctx, &dir, at)?.entries.remove(name) else {
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lines {
    Held,
    Due,
}

enum Slot {
    Kept(Node, Lines),
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
    Leaf,
    Gone,
}

type Bodies = BTreeMap<BlobId, Vec<u8>>;

pub(crate) struct TreeEdit {
    root: Slot,
    bodies: Bodies,
    made: Vec<Vec<String>>,
}

impl TreeEdit {
    pub(crate) fn over(root: Option<BlobId>) -> TreeEdit {
        let root = match root {
            Some(tree) => Slot::Kept(Node::Dir { tree, entries: 0 }, Lines::Held),
            None => Slot::Open(Dir::default()),
        };
        TreeEdit {
            root,
            bodies: Bodies::new(),
            made: Vec::new(),
        }
    }

    pub(crate) fn access_along(
        &self,
        ctx: &QueryCtx,
        segments: &[String],
    ) -> Result<Vec<Option<Access>>, Error> {
        let mut rows = Vec::with_capacity(segments.len());
        let mut at = Walk::Edited(&self.root);
        for name in segments {
            let (row, next) = self.step(ctx, at, name)?;
            rows.push(row);
            at = next;
        }
        Ok(rows)
    }

    pub(crate) fn holds(&self, ctx: &QueryCtx, segments: &[String]) -> Result<bool, Error> {
        let mut at = Walk::Edited(&self.root);
        for name in segments {
            at = self.step(ctx, at, name)?.1;
        }
        Ok(!matches!(at, Walk::Gone))
    }

    pub(crate) fn keep(&mut self, id: BlobId, body: Vec<u8>) {
        self.bodies.insert(id, body);
    }

    pub(crate) fn made(&mut self) -> Vec<Vec<String>> {
        std::mem::take(&mut self.made)
    }

    pub(crate) fn put(
        &mut self,
        ctx: &QueryCtx,
        segments: &[String],
        node: Node,
        by: Option<AccountNumber>,
        roots: &Roots,
    ) -> Result<(), Error> {
        let (name, dirs) = split(segments)?;
        let parent = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            dirs,
            Create::Missing(by, roots),
        )?;
        parent
            .children
            .insert(name.clone(), Slot::Kept(node, Lines::Due));
        created(parent, segments, by, roots);
        Ok(())
    }

    pub(crate) fn mkdir(
        &mut self,
        ctx: &QueryCtx,
        segments: &[String],
        by: Option<AccountNumber>,
        roots: &Roots,
    ) -> Result<(), Error> {
        let (name, dirs) = split(segments)?;
        let parent = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            dirs,
            Create::Missing(by, roots),
        )?;
        if parent.children.contains_key(name) {
            return Err(already_exists(format!("{} exists", join(segments))));
        }
        parent
            .children
            .insert(name.clone(), Slot::Open(Dir::default()));
        created(parent, segments, by, roots);
        Ok(())
    }

    pub(crate) fn remove(&mut self, ctx: &QueryCtx, segments: &[String]) -> Result<(), Error> {
        let (name, dirs) = split(segments)?;
        let parent = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            dirs,
            Create::Never,
        )?;
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
        let source = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            from_dirs,
            Create::Never,
        )?;
        if !source.children.contains_key(from_name) {
            return Err(gone());
        }
        let target = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            to_dirs,
            Create::Never,
        )?;
        if target.children.contains_key(to_name) {
            return Err(already_exists(format!("{} exists", join(to))));
        }
        let source = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            from_dirs,
            Create::Never,
        )?;
        let Some(mut moved) = source.children.remove(from_name) else {
            return Err(gone());
        };
        let access = source.access.remove(from_name);
        due(ctx, &mut moved, &mut self.bodies)?;
        let target = open_dir(
            &mut self.root,
            &self.bodies,
            &mut self.made,
            ctx,
            to_dirs,
            Create::Never,
        )?;
        target.children.insert(to_name.clone(), moved);
        if let Some(access) = access {
            target.access.insert(to_name.clone(), access);
        }
        Ok(())
    }

    pub(crate) fn build(self, ctx: &ExecCtx) -> Result<(BlobId, u64), Error> {
        match self.root {
            Slot::Kept(Node::Dir { tree, .. }, _) => {
                let entries = crate::objects::tree(ctx, &tree)?.entries.len() as u64;
                Ok((tree, entries))
            }
            Slot::Kept(other, _) => Err(::program::corrupt(
                TREE,
                &[],
                format!("the root is a {}", other.kind()),
            )),
            Slot::Open(dir) => write(ctx, &self.bodies, dir, &mut Vec::new()),
        }
    }

    fn step<'e>(
        &self,
        ctx: &QueryCtx,
        at: Walk<'e>,
        name: &str,
    ) -> Result<(Option<Access>, Walk<'e>), Error> {
        match at {
            Walk::Edited(Slot::Open(dir)) => {
                let next = dir.children.get(name).map_or(Walk::Gone, Walk::Edited);
                Ok((dir.access.get(name).cloned(), next))
            }
            Walk::Edited(Slot::Kept(Node::Dir { tree, .. }, _)) => self.stored(ctx, *tree, name),
            Walk::Stored(tree) => self.stored(ctx, tree, name),
            Walk::Edited(Slot::Kept(Node::File { .. } | Node::Symlink { .. }, _))
            | Walk::Leaf
            | Walk::Gone => Ok((None, Walk::Gone)),
        }
    }

    fn stored<'e>(
        &self,
        ctx: &QueryCtx,
        dir: BlobId,
        name: &str,
    ) -> Result<(Option<Access>, Walk<'e>), Error> {
        let mut dir = tree_of(ctx, &self.bodies, &dir)?;
        let next = match dir.entries.remove(name) {
            Some(Node::Dir { tree, .. }) => Walk::Stored(tree),
            Some(Node::File { .. } | Node::Symlink { .. }) => Walk::Leaf,
            None => Walk::Gone,
        };
        Ok((dir.access.remove(name), next))
    }
}

fn body_of(ctx: &QueryCtx, bodies: &Bodies, kind: &str, id: &BlobId) -> Result<Vec<u8>, Error> {
    match bodies.get(id) {
        Some(body) => Ok(body.clone()),
        None => body(ctx, kind, id, At::Head),
    }
}

fn tree_of(ctx: &QueryCtx, bodies: &Bodies, id: &BlobId) -> Result<Tree, Error> {
    decoded(TREE, id, &body_of(ctx, bodies, TREE, id)?)
}

fn gather(ctx: &QueryCtx, node: &Node, bodies: &mut Bodies) -> Result<(), Error> {
    match node {
        Node::File { file, .. } => {
            let body = body_of(ctx, bodies, FILE, file)?;
            bodies.insert(*file, body);
        }
        Node::Dir { tree, .. } => {
            let body = body_of(ctx, bodies, TREE, tree)?;
            let children: Tree = decoded(TREE, tree, &body)?;
            bodies.insert(*tree, body);
            for child in children.entries.values() {
                gather(ctx, child, bodies)?;
            }
        }
        Node::Symlink { .. } => {}
    }
    Ok(())
}

fn due(ctx: &QueryCtx, slot: &mut Slot, bodies: &mut Bodies) -> Result<(), Error> {
    match slot {
        Slot::Open(dir) => {
            for child in dir.children.values_mut() {
                due(ctx, child, bodies)?;
            }
        }
        Slot::Kept(node, lines) => {
            gather(ctx, node, bodies)?;
            *lines = Lines::Due;
        }
    }
    Ok(())
}

pub(crate) fn hold(
    ctx: &ExecCtx,
    bodies: &BTreeMap<BlobId, Vec<u8>>,
    node: &Node,
    path: &mut Vec<String>,
) -> Result<(), Error> {
    match node {
        Node::File { file, .. } => {
            put_on(ctx, FILE, body_of(ctx, bodies, FILE, file)?, line(path))?;
        }
        Node::Dir { tree, .. } => {
            let body = body_of(ctx, bodies, TREE, tree)?;
            let children: Tree = decoded(TREE, tree, &body)?;
            put_on(ctx, TREE, body, line(path))?;
            for (name, child) in &children.entries {
                path.push(name.clone());
                hold(ctx, bodies, child, path)?;
                path.pop();
            }
        }
        Node::Symlink { .. } => {}
    }
    Ok(())
}

fn created(dir: &mut Dir, segments: &[String], by: Option<AccountNumber>, roots: &Roots) {
    let Some(creator) = by else {
        return;
    };
    let Some(name) = segments.last() else {
        return;
    };
    if roots.is_namespace_root(segments) {
        return;
    }
    dir.access.entry(name.clone()).or_insert(Access {
        creator: Some(creator),
        write: None,
    });
}

#[derive(Clone, Copy)]
enum Create<'r> {
    Missing(Option<AccountNumber>, &'r Roots),
    Never,
}

fn split(segments: &[String]) -> Result<(&String, &[String]), Error> {
    segments.split_last().ok_or_else(|| {
        invalid("the root is not a path a change names: name a path beneath the root")
    })
}

fn open(slot: &mut Slot, bodies: &Bodies, ctx: &QueryCtx, path: &[String]) -> Result<(), Error> {
    let Slot::Kept(node, lines) = slot else {
        return Ok(());
    };
    let Node::Dir { tree, .. } = node else {
        return Err(wrong_state(format!(
            "{} is a {}, not a directory",
            join(path),
            node.kind()
        )));
    };
    let stored = tree_of(ctx, bodies, tree)?;
    let lines = *lines;
    let children = stored
        .entries
        .into_iter()
        .map(|(name, node)| (name, Slot::Kept(node, lines)))
        .collect();
    *slot = Slot::Open(Dir {
        children,
        access: stored.access,
    });
    Ok(())
}

fn open_dir<'e>(
    root: &'e mut Slot,
    bodies: &Bodies,
    made: &mut Vec<Vec<String>>,
    ctx: &QueryCtx,
    dirs: &[String],
    create: Create<'_>,
) -> Result<&'e mut Dir, Error> {
    let mut slot = root;
    for (depth, name) in dirs.iter().enumerate() {
        open(slot, bodies, ctx, &dirs[..depth])?;
        let Slot::Open(dir) = slot else {
            unreachable!("open leaves a directory open");
        };
        let missing = !dir.children.contains_key(name);
        if missing {
            let Create::Missing(by, roots) = create else {
                return Err(not_found(format!("no directory {}", join(&dirs[..=depth]))));
            };
            dir.children
                .insert(name.clone(), Slot::Open(Dir::default()));
            created(dir, &dirs[..=depth], by, roots);
            made.push(dirs[..=depth].to_vec());
        }
        slot = dir
            .children
            .get_mut(name)
            .ok_or_else(|| not_found(format!("no directory {}", join(&dirs[..=depth]))))?;
    }
    open(slot, bodies, ctx, dirs)?;
    let Slot::Open(dir) = slot else {
        unreachable!("open leaves a directory open");
    };
    Ok(dir)
}

fn write(
    ctx: &ExecCtx,
    bodies: &Bodies,
    dir: Dir,
    path: &mut Vec<String>,
) -> Result<(BlobId, u64), Error> {
    let mut entries = BTreeMap::new();
    for (name, slot) in dir.children {
        path.push(name.clone());
        let node = match slot {
            Slot::Kept(node, Lines::Held) => node,
            Slot::Kept(node, Lines::Due) => {
                hold(ctx, bodies, &node, path)?;
                node
            }
            Slot::Open(child) => {
                let (tree, entries) = write(ctx, bodies, child, path)?;
                Node::Dir { tree, entries }
            }
        };
        path.pop();
        entries.insert(name, node);
    }
    let count = entries.len() as u64;
    let tree = Tree {
        entries,
        access: dir.access,
    };
    let id = put_on(ctx, TREE, abi::encode(&tree), line(path))?;
    Ok((id, count))
}
