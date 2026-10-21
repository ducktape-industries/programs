//! `HoldHead`: one folder of the head a call, the first after `after` in path order (the root and
//! the snapshot when none), put again on its line with each file object in it, the same bytes;
//! each call books the next a block later, so the one-time hold runs as a chain of bounded calls.
use ::program::{BlobId, Error, ExecCtx, Reply};

use crate::objects::{
    At, FILE, SNAPSHOT, SNAPSHOT_LINE, TREE, Tree, body, decoded, line, put_on, tree,
};
use crate::paths::{join, segments};
use crate::state::HEAD;
use crate::{Node, Op, PROGRAM};

pub(crate) fn hold_head(ctx: &ExecCtx, after: Option<String>) -> Result<(), Error> {
    let held = hold_next(ctx, after)?;
    if let Some(path) = &held {
        let scheduler = ctx.env().role(abi::role::SCHEDULER)?.clone();
        let next = abi::role::scheduler::Op::Schedule {
            height: ctx.env().height.saturating_add(1),
            target: PROGRAM.into(),
            payload: abi::encode(&Op::HoldHead {
                after: Some(path.clone()),
            }),
        };
        ctx.emit(scheduler, &next, Reply::None);
    }
    ctx.set_return_data(abi::encode(&held));
    Ok(())
}

fn hold_next(ctx: &ExecCtx, after: Option<String>) -> Result<Option<String>, Error> {
    let Some(head) = HEAD.get(ctx)? else {
        return Ok(None);
    };
    let next = match after {
        None => {
            let snapshot = body(ctx, SNAPSHOT, &head.id, At::Head)?;
            put_on(ctx, SNAPSHOT, snapshot, SNAPSHOT_LINE.to_vec())?;
            Some((Vec::new(), head.root))
        }
        Some(after) => next_folder(ctx, &head.root, &segments(&after)?)?,
    };
    let Some((folder, id)) = next else {
        return Ok(None);
    };
    let bytes = body(ctx, TREE, &id, At::Head)?;
    let dir: Tree = decoded(TREE, &id, &bytes)?;
    put_on(ctx, TREE, bytes, line(&folder))?;
    let mut path = folder.clone();
    for (name, node) in &dir.entries {
        let Node::File { file, .. } = node else {
            continue;
        };
        path.push(name.clone());
        put_on(ctx, FILE, body(ctx, FILE, file, At::Head)?, line(&path))?;
        path.pop();
    }
    Ok(Some(join(&folder)))
}

fn next_folder(
    ctx: &ExecCtx,
    root: &BlobId,
    after: &[String],
) -> Result<Option<(Vec<String>, BlobId)>, Error> {
    let mut trees = vec![tree(ctx, root)?];
    for name in after {
        let below = trees.last().and_then(|dir| dir.entries.get(name)).cloned();
        let Some(Node::Dir { tree: id, .. }) = below else {
            break;
        };
        trees.push(tree(ctx, &id)?);
    }
    if trees.len() > after.len() {
        if let Some((name, id)) = first_folder(&trees[after.len()], None) {
            let mut path = after.to_vec();
            path.push(name);
            return Ok(Some((path, id)));
        }
        trees.pop();
    }
    while let Some(dir) = trees.pop() {
        let depth = trees.len();
        if let Some((name, id)) = first_folder(&dir, Some(&after[depth])) {
            let mut path = after[..depth].to_vec();
            path.push(name);
            return Ok(Some((path, id)));
        }
    }
    Ok(None)
}

fn first_folder(dir: &Tree, after: Option<&String>) -> Option<(String, BlobId)> {
    dir.entries
        .iter()
        .filter(|(name, _)| after.is_none_or(|after| *name > after))
        .find_map(|(name, node)| match node {
            Node::Dir { tree, .. } => Some((name.clone(), *tree)),
            Node::File { .. } | Node::Symlink { .. } => None,
        })
}
