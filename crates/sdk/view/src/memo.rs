//! Subtrees lowered once and kept while what they read stays the same:
//! [`memo`] for any element, [`AnyView::cached`](crate::AnyView::cached) for
//! a child entity that did not notify.
use std::any::Any;
use std::collections::HashMap;

use crate::element::{AnyElement, Element, IntoElement, Lowering};
use crate::{App, Window, slots, wire};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MemoId {
    Named(wire::ElementIdWire),
    Entity(u64),
}

pub(crate) type Build = Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement>;

// A subtree lowered once and kept: built and lowered again only when its
// deps differ from the ones it was last lowered with, or when something
// every subtree may read changed (the theme, the pane, the device, a
// resync). Made by [`memo`] and [`Context::memo`](crate::Context::memo).
pub struct Memo {
    pub(crate) id: MemoId,
    pub(crate) deps: Box<dyn Any>,
    pub(crate) same: fn(&dyn Any, &dyn Any) -> bool,
    pub(crate) build: Build,
}

// `build`'s element, kept under `id` among its siblings while `deps`
// compare equal: `memo(format!("row-{seq}"), (row.clone(), hovered), move |_, _| row_card(..))`.
// The element must lower to a node whose id is `id`, and `deps` must hold
// everything `build` reads; in a debug build every reuse lowers `build`
// again and fails, naming the memo, when the two differ.
pub fn memo<D: PartialEq + 'static, E: IntoElement>(
    id: impl Into<gpui::ElementId>,
    deps: D,
    build: impl FnOnce(&mut Window, &mut App) -> E + 'static,
) -> Memo {
    Memo {
        id: MemoId::Named(crate::element::wire_id(id.into())),
        deps: Box::new(deps),
        same: same::<D>,
        build: Box::new(move |window, app| build(window, app).into_any_element()),
    }
}

pub(crate) fn same<D: PartialEq + 'static>(kept: &dyn Any, deps: &dyn Any) -> bool {
    kept.downcast_ref::<D>()
        .zip(deps.downcast_ref::<D>())
        .is_some_and(|(kept, deps)| kept == deps)
}

impl std::fmt::Display for MemoId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named(id) => match id.name() {
                Some(name) => f.write_str(name),
                None => write!(f, "{id:?}"),
            },
            Self::Entity(entity) => write!(f, "entity {entity}"),
        }
    }
}

impl Element for Memo {
    fn defers(&self) -> bool {
        true
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.memo(*self)
    }
}

impl IntoElement for Memo {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Memo {}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub path: Vec<wire::ElementIdWire>,
    pub row: Option<usize>,
    pub id: MemoId,
}

pub(crate) struct Entry {
    pub deps: Box<dyn Any>,
    pub shell: wire::Node,
    pub journal: slots::Journal,
    pub dense: Vec<Vec<wire::ElementIdWire>>,
    pub seen: u64,
}

pub(crate) struct Hit {
    pub path: Vec<wire::ElementIdWire>,
    pub id: MemoId,
    pub check: Option<wire::Node>,
}

#[derive(Default)]
pub(crate) struct Cache {
    pub entries: HashMap<Key, Entry>,
    pub frame: u64,
    // Each entity's count of notifies, its own and those of every entity
    // drawn inside it, and the entity each was last drawn inside: what a
    // cached view is kept by.
    pub versions: HashMap<u64, u64>,
    pub parents: HashMap<u64, u64>,
    // The memos the frame lowered anew, in the order it reached them.
    pub anew: Vec<MemoId>,
    // The memos the frame kept, standing in its tree as their shells.
    pub hits: Vec<Hit>,
}

impl Cache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.anew.clear();
        self.hits.clear();
    }

    // Drops every subtree the frame did not draw, as the routes it took
    // are freed.
    pub fn end_frame(&mut self) {
        let frame = self.frame;
        self.entries.retain(|_, entry| entry.seen == frame);
    }

    // Something any subtree may read changed: every one lowers again.
    pub fn forget(&mut self) {
        self.entries.clear();
    }

    pub fn notified(&mut self, entity: u64) {
        let mut at = Some(entity);
        let mut walked = 0;
        while let Some(entity) = at {
            *self.versions.entry(entity).or_default() += 1;
            at = self.parents.get(&entity).copied();
            walked += 1;
            if walked > self.parents.len() {
                break;
            }
        }
    }

    pub fn version(&self, entity: u64) -> u64 {
        self.versions.get(&entity).copied().unwrap_or_default()
    }
}

pub(crate) fn without_pictures(node: &mut wire::Node) {
    node.for_each_mut(&mut |node| match node {
        wire::Node::Svg {
            source: wire::SvgSource::Data { bytes, .. },
            ..
        } => *bytes = None,
        wire::Node::Image { data, .. } => *data = None,
        _ => {}
    });
}

pub(crate) fn shell(node: &mut wire::Node) -> wire::Node {
    let mut shell = match node.child_list_mut() {
        Some(list) => {
            let children = std::mem::take(list);
            let shell = node.clone();
            *node.child_list_mut().expect("a list") = children;
            shell
        }
        None => {
            let parts = node.children_mut();
            let children: Vec<wire::Node> = parts
                .iter_mut()
                .map(|slot| std::mem::replace(slot, wire::Node::empty()))
                .collect();
            let shell = node.clone();
            for (slot, child) in node.children_mut().iter_mut().zip(children) {
                *slot = child;
            }
            shell
        }
    };
    without_pictures(&mut shell);
    shell
}

fn find(
    root: &wire::Node,
    targets: &HashMap<&[wire::ElementIdWire], usize>,
) -> HashMap<Vec<u32>, usize> {
    fn walk(
        node: &wire::Node,
        row: Option<usize>,
        path: &mut Vec<wire::ElementIdWire>,
        at: &mut Vec<u32>,
        targets: &HashMap<&[wire::ElementIdWire], usize>,
        found: &mut HashMap<Vec<u32>, usize>,
    ) {
        let segment = wire::identity::segment(node.identity().cloned(), row);
        let entered = segment.is_some();
        if let Some(segment) = segment {
            path.push(segment);
            if let Some(&target) = targets.get(path.as_slice()) {
                found.insert(at.clone(), target);
                path.pop();
                return;
            }
        }
        for (index, child) in node.children().iter().enumerate() {
            at.push(index as u32);
            walk(
                child,
                wire::identity::row(node, index),
                path,
                at,
                targets,
                found,
            );
            at.pop();
        }
        if entered {
            path.pop();
        }
    }
    let mut found = HashMap::new();
    if !targets.is_empty() {
        walk(
            root,
            None,
            &mut Vec::new(),
            &mut Vec::new(),
            targets,
            &mut found,
        );
    }
    found
}

fn check(hit: &Hit, kept: &wire::Node) {
    let Some(now) = &hit.check else {
        return;
    };
    let (mut was, mut now) = (kept.clone(), now.clone());
    let patches = wire::diff(&mut was, &mut now);
    assert!(
        patches.is_empty(),
        "memo {} at {:?} was kept, but lowering it again draws something else ({} patches, \
         the first {:?}): its deps leave out something it reads",
        hit.id,
        hit.path,
        patches.len(),
        patches.first()
    );
}

fn at_mut<'a>(mut node: &'a mut wire::Node, at: &[u32]) -> Option<&'a mut wire::Node> {
    for index in at {
        node = node.children_mut().get_mut(*index as usize)?;
    }
    Some(node)
}

fn placed<'a>(
    new: &'a mut wire::Node,
    patches: &'a mut [wire::Patch],
    at: &[u32],
) -> Option<&'a mut wire::Node> {
    for patch in patches.iter_mut() {
        let (base, node) = match patch {
            wire::Patch::Replace { path, node } => (path.clone(), node),
            wire::Patch::Insert { path, index, node } => {
                let mut base = path.clone();
                base.push(*index);
                (base, node)
            }
            _ => continue,
        };
        if let Some(rest) = at.strip_prefix(base.as_slice()) {
            return at_mut(node, rest);
        }
    }
    at_mut(new, at)
}

pub(crate) fn keeping(
    last: &mut wire::Node,
    new: &mut wire::Node,
    hits: &[Hit],
    checks: bool,
) -> Option<Vec<wire::Patch>> {
    let targets: HashMap<&[wire::ElementIdWire], usize> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| (hit.path.as_slice(), index))
        .collect();
    let standing = find(new, &targets);
    let mut kept = vec![false; hits.len()];
    let mut keep = |at: &[u32], old: &wire::Node, shell: &wire::Node| {
        let Some(&hit) = standing.get(at) else {
            return wire::Kept::Diff;
        };
        let same = old.identity() == shell.identity()
            && std::mem::discriminant(old) == std::mem::discriminant(shell);
        if !same {
            return wire::Kept::Carry;
        }
        check(&hits[hit], old);
        kept[hit] = true;
        wire::Kept::Keep
    };
    let mut patches = wire::diff_keeping(last, new, &mut keep);
    let rest: HashMap<&[wire::ElementIdWire], usize> =
        targets.into_iter().filter(|(_, hit)| !kept[*hit]).collect();
    if rest.is_empty() {
        return Some(patches);
    }
    let behind = find(last, &rest);
    let mut from: HashMap<usize, Vec<u32>> =
        behind.into_iter().map(|(at, hit)| (hit, at)).collect();
    for (at, hit) in standing {
        if kept[hit] {
            continue;
        }
        let node = from
            .remove(&hit)
            .and_then(|from| at_mut(last, &from))
            .map(|node| std::mem::replace(node, wire::Node::empty()));
        let Some(node) = node else {
            assert!(
                !checks,
                "memo {} at {:?} was kept, but the last tree has no node at its place",
                hits[hit].id, hits[hit].path
            );
            return None;
        };
        check(&hits[hit], &node);
        let Some(shell) = placed(new, &mut patches, &at) else {
            assert!(!checks, "memo {} stands nowhere in its frame", hits[hit].id);
            return None;
        };
        *shell = node;
    }
    Some(patches)
}
