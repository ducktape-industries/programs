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
    /// Takes the children out, leaving an empty list or an empty stand-in
    /// per slot: what is left is the node's own fields, which is what a
    /// [`Patch::Props`] carries and what two nodes are compared by.
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

    /// Puts [`Node::detach`]ed children back. `None` when the arity does
    /// not fit, in which case nothing was moved.
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
        // a node this build could not read holds what the view patches
        // there, unseen: the patch changes nothing this host draws
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

/// A [`Patch`] with its path taken off.
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
    diff_node(old, new, false, &mut Vec::new(), &mut patches);
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
    diff_node(old, new, true, &mut Vec::new(), &mut patches);
    patches
}

/// The subtree a patch carries: `new` itself, moved out behind an empty
/// stand-in, or a copy of it.
fn carry(new: &mut Node, take: bool) -> Node {
    match take {
        true => std::mem::replace(new, Node::empty()),
        false => new.clone(),
    }
}

/// Each node's own fields are compared once, on the way down: comparing a
/// whole subtree first at every level compared a deep changed subtree once
/// per level above it. `take` is [`diff_taking`]'s: carry a subtree by
/// moving it out of `new`, not by copying it.
fn diff_node(
    old: &mut Node,
    new: &mut Node,
    take: bool,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let same_kind = std::mem::discriminant(old) == std::mem::discriminant(new);
    let same_arity = new.child_list_mut().is_some() || old.children().len() == new.children().len();
    if !(same_kind && same_arity) {
        out.push(Patch::Replace {
            path: path.clone(),
            node: carry(new, take),
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
        (true, None) => diff_list(&mut old_children, &mut new_children, take, path, out),
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

/// A list's rows are its item indices from `range_start` on: rows at the
/// same index are the same row, whatever their keys. Scrolling a row into
/// the window is one insert, not the whole window sent again.
fn diff_rows(
    old: &mut [Node],
    new: &mut [Node],
    old_start: usize,
    new_start: usize,
    take: bool,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let old_end = old_start + old.len();
    let new_end = new_start + new.len();
    let (start, end) = (old_start.max(new_start), old_end.min(new_end));
    if start >= end {
        return diff_list(old, new, take, path, out);
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
                node: carry(row, take),
            }),
        }
    }
}

/// A list of children: the runs it shares with the old list at the front
/// and at the back are diffed in place, matched by position, and only the
/// middle is matched by key. A child matches the one across from it when
/// both carry one identity, or neither does and they are the same kind.
fn diff_list(
    old: &mut [Node],
    new: &mut [Node],
    take: bool,
    path: &mut Vec<u32>,
    out: &mut Vec<Patch>,
) {
    let shared = old.len().min(new.len());
    let mut prefix = 0;
    while prefix < shared && matches(&old[prefix], &new[prefix]) {
        prefix += 1;
    }
    // Two lists of one length that match all the way have no back to match
    // and no run to place: each child is diffed in place, where the front
    // found it. That is every list of a tree whose shape did not change,
    // so it is not walked a second and a third time to learn nothing.
    let whole = prefix == old.len() && prefix == new.len();
    let mut suffix = 0;
    while !whole
        && suffix < shared
        && matches(&old[old.len() - 1 - suffix], &new[new.len() - 1 - suffix])
    {
        suffix += 1;
    }
    if prefix + suffix > shared {
        // Every child of the shorter list matches one across from it, and
        // the run the longer list has besides may sit anywhere from
        // `shared - suffix` to `prefix`: it starts where the children stop
        // being equal, so a banner shown over a body of its own kind is
        // the one inserted and the body is not sent again.
        // ponytail: a run whose neighbours both changed lands at the
        // front, which costs a props patch more than the ideal place.
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
    diff_keyed(
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

/// Whether `old` and `new` are one child at one position (see
/// [`diff_list`]).
fn matches(old: &Node, new: &Node) -> bool {
    match (old.identity(), new.identity()) {
        (Some(a), Some(b)) => a == b,
        (None, None) => std::mem::discriminant(old) == std::mem::discriminant(new),
        _ => false,
    }
}

/// Whether `old` and `new` agree on their own fields, children set aside.
fn own_fields_equal(old: &mut Node, new: &mut Node) -> bool {
    let (old_children, new_children) = (old.detach(), new.detach());
    let same = old == new;
    old.attach(old_children).expect("its own children");
    new.attach(new_children).expect("its own children");
    same
}

/// The middle of a list, at `offset` in it: an identity that appears
/// once on each side is a child that survives; every other child, unkeyed
/// or a duplicate, is removed and inserted afresh. Typed GPUI IDs stay
/// typed all the way through this map.
fn diff_keyed(
    old: &mut [Node],
    new: &mut [Node],
    offset: usize,
    take: bool,
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
    // The middle as the host has it after the patches so far: old indices.
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
                node: carry(new_child, take),
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
