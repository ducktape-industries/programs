//! Patches: the mutation list a view sends instead of a whole tree, the
//! host-side [`apply`], and the [`diff`] that produces one.

use crate::*;
use serde::{Deserialize, Serialize};

/// One edit to the tree the host holds. `path` is the child index at every
/// level from the root down (`[]` is the root itself); children are
/// addressed by index at the moment the patch is applied, so a sequence
/// reads like edits to a live document. The vocabulary is a virtual DOM's
/// mutation list: replace, re-prop, insert, remove, move.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Patch {
    /// The subtree at `path` becomes `node`.
    Replace { path: Vec<u32>, node: Node },
    /// The node at `path` takes `node`'s own fields and keeps its children:
    /// `node` carries none (an empty list, or an empty stand-in per slot).
    Props { path: Vec<u32>, node: Node },
    /// `node` becomes child `index` of the list at `path`.
    Insert {
        path: Vec<u32>,
        index: u32,
        node: Node,
    },
    /// Child `index` of the list at `path` goes away.
    Remove { path: Vec<u32>, index: u32 },
    /// Child `from` of the list at `path` is taken out and put back at `to`.
    Move { path: Vec<u32>, from: u32, to: u32 },
}

impl Node {
    fn detach(&mut self) -> Vec<Node> {
        match self.child_list_mut() {
            Some(list) => std::mem::take(list),
            None => self
                .children_mut()
                .iter_mut()
                .map(|slot| std::mem::replace(slot, Node::empty()))
                .collect(),
        }
    }

    fn attach(&mut self, children: Vec<Node>) -> Option<()> {
        if let Some(list) = self.child_list_mut() {
            *list = children;
            return Some(());
        }
        let slots = self.children_mut();
        if slots.len() != children.len() {
            return None;
        }
        for (slot, child) in slots.iter_mut().zip(children) {
            *slot = child;
        }
        Some(())
    }
}

pub fn apply(
    root: &mut Node,
    patches: Vec<Patch>,
    styles: &Styles,
) -> Result<SanitizeReport, Refused> {
    for patch in patches {
        apply_one(root, patch)?;
    }
    sanitize_tree(root, styles)
}

fn apply_one(root: &mut Node, patch: Patch) -> Result<(), &'static str> {
    let (path, edit) = match patch {
        Patch::Replace { path, node } => (path, Edit::Replace(node)),
        Patch::Props { path, node } => (path, Edit::Props(node)),
        Patch::Insert { path, index, node } => (path, Edit::Insert(index, node)),
        Patch::Remove { path, index } => (path, Edit::Remove(index)),
        Patch::Move { path, from, to } => (path, Edit::Move(from, to)),
    };
    let mut target = root;
    for index in path {
        if matches!(target, Node::Unknown { .. }) {
            return Ok(());
        }
        target = target
            .children_mut()
            .get_mut(index as usize)
            .ok_or("a path to no node")?;
    }
    if matches!(target, Node::Unknown { .. }) && !matches!(edit, Edit::Replace(_)) {
        return Ok(());
    }
    match edit {
        Edit::Replace(node) => *target = node,
        Edit::Props(mut node) => {
            let children = target.detach();
            node.attach(children).ok_or("props of another arity")?;
            *target = node;
        }
        Edit::Insert(index, node) => {
            let list = target.child_list_mut().ok_or("a list edit on no list")?;
            if index as usize > list.len() {
                return Err("an index past the list");
            }
            list.insert(index as usize, node);
        }
        Edit::Remove(index) => {
            let list = target.child_list_mut().ok_or("a list edit on no list")?;
            if index as usize >= list.len() {
                return Err("an index past the list");
            }
            list.remove(index as usize);
        }
        Edit::Move(from, to) => {
            let list = target.child_list_mut().ok_or("a list edit on no list")?;
            if from as usize >= list.len() || to as usize >= list.len() {
                return Err("an index past the list");
            }
            let node = list.remove(from as usize);
            list.insert(to as usize, node);
        }
    }
    Ok(())
}

enum Edit {
    Replace(Node),
    Props(Node),
    Insert(u32, Node),
    Remove(u32),
    Move(u32, u32),
}

/// The patches that turn `old` into `new`: `apply(old, diff(old, new))`
/// leaves `old == new`. Both are borrowed mutably only to compare a node's
/// own fields with its children set aside; each is put back as it was.
///
/// A list of children is matched by position over the runs it shares with
/// the old one at the front and at the back, unkeyed children included,
/// and by key in between — a keyed child that moved is a [`Patch::Move`],
/// one that left a [`Patch::Remove`], a new one a [`Patch::Insert`]. Keys
/// are what [`sanitize`] already makes unique on the host.
pub fn diff(old: &mut Node, new: &mut Node) -> Vec<Patch> {
    let mut patches = Vec::new();
    let take = &mut Take {
        take: false,
        kept: None,
    };
    diff_node(old, new, take, &mut Vec::new(), &mut patches);
    patches
}

/// [`diff`], except that every subtree a [`Patch::Replace`] or
/// [`Patch::Insert`] carries is taken out of `new` instead of copied, and an
/// empty stand-in is left in its place. A view that sends the patches and
/// keeps `new` as the next frame's base moves each changed subtree out and
/// back instead of cloning it: a patch's path and index are the position of
/// its node in `new`, since removes come first at every level and the
/// inserts and moves before an index have already been emitted when it is
/// reached. Plumbing for view's driver, not a view's API.
#[doc(hidden)]
pub fn diff_taking(old: &mut Node, new: &mut Node) -> Vec<Patch> {
    let mut patches = Vec::new();
    let take = &mut Take {
        take: true,
        kept: None,
    };
    diff_node(old, new, take, &mut Vec::new(), &mut patches);
    patches
}

// What [`diff_keeping`] does with a node of `new`: diff it as any other,
// keep the node of `old` across from it, or send it whole.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    Diff,
    Keep,
    Carry,
}

// [`diff_taking`], where some nodes of `new` stand for the node of `old`
// across from them: `kept` hears each node of `new`, at its position, with
// the node of `old` it is matched with, before the two are compared.
// `Keep` moves the node of `old` into `new` in its place, never compared
// and never copied; `Carry` sends the node of `new` whole. `old` is left
// with what `new` gave up, so it is no base for a diff after. Plumbing for
// view's driver, not a view's API.
#[doc(hidden)]
pub fn diff_keeping(old: &mut Node, new: &mut Node, kept: &mut KeptBy<'_>) -> Vec<Patch> {
    let mut patches = Vec::new();
    let take = &mut Take {
        take: true,
        kept: Some(kept),
    };
    diff_node(old, new, take, &mut Vec::new(), &mut patches);
    patches
}

type KeptBy<'a> = dyn FnMut(&[u32], &Node, &Node) -> Kept + 'a;

struct Take<'a> {
    take: bool,
    kept: Option<&'a mut KeptBy<'a>>,
}

fn move_or_clone_subtree(new: &mut Node, take: &Take<'_>) -> Node {
    match take.take {
        true => std::mem::replace(new, Node::empty()),
        false => new.clone(),
    }
}

fn diff_node(
    old: &mut Node,
    new: &mut Node,
    take: &mut Take<'_>,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let kept = match take.kept.as_mut() {
        Some(kept) => kept(path, old, new),
        None => Kept::Diff,
    };
    match kept {
        Kept::Diff => {}
        Kept::Keep => {
            std::mem::swap(old, new);
            return;
        }
        Kept::Carry => {
            out.push(Patch::Replace {
                path: path.clone(),
                node: move_or_clone_subtree(new, take),
            });
            return;
        }
    }
    let same_kind = std::mem::discriminant(old) == std::mem::discriminant(new);
    let same_arity = new.child_list_mut().is_some() || old.children().len() == new.children().len();
    if !(same_kind && same_arity) {
        out.push(Patch::Replace {
            path: path.clone(),
            node: move_or_clone_subtree(new, take),
        });
        return;
    }
    let old_children = old.detach();
    let new_children = new.detach();
    if old != new {
        out.push(Patch::Props {
            path: path.clone(),
            node: new.clone(),
        });
    }
    let mut old_children = old_children;
    let mut new_children = new_children;
    let rows = match (&*old, &*new) {
        (Node::List { range_start: a, .. }, Node::List { range_start: b, .. }) => Some((*a, *b)),
        _ => None,
    };
    match (old.child_list_mut().is_some(), rows) {
        (true, Some((a, b))) => {
            diff_rows(&mut old_children, &mut new_children, a, b, take, path, out)
        }
        (true, None) => diff_children_by_position_then_key(
            &mut old_children,
            &mut new_children,
            take,
            path,
            out,
        ),
        (false, _) => {
            for (index, (old_child, new_child)) in
                old_children.iter_mut().zip(&mut new_children).enumerate()
            {
                path.push(index as u32);
                diff_node(old_child, new_child, take, path, out);
                path.pop();
            }
        }
    }
    old.attach(old_children).expect("its own children");
    new.attach(new_children).expect("its own children");
}

fn diff_rows(
    old: &mut [Node],
    new: &mut [Node],
    old_start: usize,
    new_start: usize,
    take: &mut Take<'_>,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let old_end = old_start + old.len();
    let new_end = new_start + new.len();
    let (start, end) = (old_start.max(new_start), old_end.min(new_end));
    if start >= end {
        return diff_children_by_position_then_key(old, new, take, path, out);
    }
    let remove = |count: usize, index: usize, out: &mut Vec<Patch>| {
        for _ in 0..count {
            out.push(Patch::Remove {
                path: path.clone(),
                index: index as u32,
            });
        }
    };
    remove(old_end - end, end - old_start, out);
    remove(start - old_start, 0, out);
    for (index, row) in new.iter_mut().enumerate() {
        let item = new_start + index;
        match (start..end).contains(&item) {
            true => {
                path.push(index as u32);
                diff_node(&mut old[item - old_start], row, take, path, out);
                path.pop();
            }
            false => out.push(Patch::Insert {
                path: path.clone(),
                index: index as u32,
                node: move_or_clone_subtree(row, take),
            }),
        }
    }
}

fn diff_children_by_position_then_key(
    old: &mut [Node],
    new: &mut [Node],
    take: &mut Take<'_>,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let shared = old.len().min(new.len());
    let mut prefix = 0;
    while prefix < shared && matches(&old[prefix], &new[prefix]) {
        prefix += 1;
    }
    let whole = prefix == old.len() && prefix == new.len();
    let mut suffix = 0;
    while !whole
        && suffix < shared
        && matches(&old[old.len() - 1 - suffix], &new[new.len() - 1 - suffix])
    {
        suffix += 1;
    }
    if prefix + suffix > shared {
        let mut equal = 0;
        while equal < prefix && own_fields_equal(&mut old[equal], &mut new[equal]) {
            equal += 1;
        }
        prefix = equal.clamp(shared - suffix, prefix);
        suffix = shared - prefix;
    }
    for index in 0..prefix {
        path.push(index as u32);
        diff_node(&mut old[index], &mut new[index], take, path, out);
        path.pop();
    }
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    diff_unique_keyed_middle(
        &mut old[prefix..old_end],
        &mut new[prefix..new_end],
        prefix,
        take,
        path,
        out,
    );
    for back in 0..suffix {
        path.push((new_end + back) as u32);
        diff_node(
            &mut old[old_end + back],
            &mut new[new_end + back],
            take,
            path,
            out,
        );
        path.pop();
    }
}

fn matches(old: &Node, new: &Node) -> bool {
    match (old.identity(), new.identity()) {
        (Some(a), Some(b)) => a == b,
        (None, None) => std::mem::discriminant(old) == std::mem::discriminant(new),
        _ => false,
    }
}

fn own_fields_equal(old: &mut Node, new: &mut Node) -> bool {
    let (old_children, new_children) = (old.detach(), new.detach());
    let same = old == new;
    old.attach(old_children).expect("its own children");
    new.attach(new_children).expect("its own children");
    same
}

fn diff_unique_keyed_middle(
    old: &mut [Node],
    new: &mut [Node],
    offset: usize,
    take: &mut Take<'_>,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let unique = |nodes: &[Node]| -> std::collections::HashMap<ElementIdWire, usize> {
        let mut seen = std::collections::HashMap::new();
        for (index, node) in nodes.iter().enumerate() {
            if let Some(identity) = node.identity() {
                seen.entry(identity.clone())
                    .and_modify(|at| *at = usize::MAX)
                    .or_insert(index);
            }
        }
        seen.retain(|_, at| *at != usize::MAX);
        seen
    };
    let old_keys = unique(old);
    let new_keys = unique(new);
    let mut live: Vec<usize> = Vec::with_capacity(new.len());
    for (index, node) in old.iter().enumerate() {
        let survives = node.identity().is_some_and(|identity| {
            let owned = identity.to_owned();
            old_keys.contains_key(&owned) && new_keys.contains_key(&owned)
        });
        match survives {
            true => live.push(index),
            false => out.push(Patch::Remove {
                path: path.clone(),
                index: (offset + live.len()) as u32,
            }),
        }
    }
    for (index, new_child) in new.iter_mut().enumerate() {
        let wanted = new_child.identity().and_then(|identity| {
            let owned = identity.to_owned();
            new_keys
                .contains_key(&owned)
                .then(|| old_keys.get(&owned).copied())
                .flatten()
        });
        let Some(wanted) = wanted else {
            out.push(Patch::Insert {
                path: path.clone(),
                index: (offset + index) as u32,
                node: move_or_clone_subtree(new_child, take),
            });
            live.insert(index, usize::MAX);
            continue;
        };
        let at = live[index..]
            .iter()
            .position(|old_index| *old_index == wanted)
            .expect("a surviving child is still live")
            + index;
        if at != index {
            out.push(Patch::Move {
                path: path.clone(),
                from: (offset + at) as u32,
                to: (offset + index) as u32,
            });
            live.remove(at);
            live.insert(index, wanted);
        }
        path.push((offset + index) as u32);
        diff_node(&mut old[wanted], new_child, take, path, out);
        path.pop();
    }
}
