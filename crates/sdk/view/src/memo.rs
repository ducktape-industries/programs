//! Subtrees lowered once and kept while what they read stays the same:
//! [`memo`] for any element, [`AnyView::cached`](crate::AnyView::cached) for
//! a child entity that did not notify.
use std::any::Any;
use std::collections::HashMap;

use crate::element::{AnyElement, Element, IntoElement, Lowering};
use crate::{App, Window, slots, wire};

// What a memo is kept by in its parent: the id its author gave it, or the
// entity a cached view draws.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MemoId {
    Named(wire::ElementIdWire),
    Entity(u64),
}

type Build = Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement>;

// A subtree lowered once and kept: built and lowered again only when its
// deps differ from the ones it was last lowered with, or when something
// every subtree may read changed (the theme, the pane, the device, a whole
// frame, a resync). Made by [`memo`] and [`Context::memo`](crate::Context::memo).
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

// Where a memo sits: the authored path around it, the list row it is the
// root of, and its id.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub path: Vec<wire::ElementIdWire>,
    pub row: Option<usize>,
    pub id: MemoId,
}

// One kept subtree: the deps it was lowered with, the node (without the
// picture bytes it first carried: the host has them), what its lowering
// did besides (its routes, the scope around them, its dense marks), and
// the frame that last drew it.
pub(crate) struct Entry {
    pub deps: Box<dyn Any>,
    pub node: wire::Node,
    pub journal: slots::Journal,
    pub dense: Vec<Vec<wire::ElementIdWire>>,
    pub seen: u64,
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
}

impl Cache {
    pub fn begin_frame(&mut self) {
        self.frame += 1;
        self.anew.clear();
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

// The node as a later frame sends it: the pictures it carried are the
// host's by then, named by hash alone.
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
