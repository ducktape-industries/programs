//! Typed identity: the path a node is filed under, and when two collide.
//!
//! One rule, run by everything that walks a tree. The view's lowering,
//! the host's renderer and its walks file every node by it ([`segment`],
//! [`row`]), so a path means one node on both sides. Whether two nodes
//! meet is the host's to say, since the view is not trusted: its
//! sanitizer claims every id ([`Scopes`]) and refuses a frame that claims
//! one twice, naming the id and its scope. A view's tests run that same
//! sanitizer on every frame, so a duplicate fails its test in those words.
//!
//! - An id is unique among the identified nodes under its nearest
//!   identified ancestor, as gpui's `GlobalElementId` is: an id-less
//!   wrapper is transparent, an identified node starts a fresh scope.
//! - A list ([`Node::List`], [`Node::UniformList`]) is a scope of its own,
//!   under the id its author gave it: two lists under one parent are told
//!   apart by their ids, and a path through a list names it.
//! - A row of a list ([`Node::List`], [`Node::UniformList`]) is filed under
//!   its own id, or else under its index ([`row`]): the id `.id(index)`
//!   would give it. Its ids are scoped under the row, so two rows that
//!   author the same id inside them do not collide. A row named by its own
//!   id keeps that name wherever it moves; an id-less row is its place.
//!
//! Why a duplicate is refused rather than drawn: gpui files a node's
//! accessibility node by the hash of its `GlobalElementId`
//! (`GlobalElementId::accesskit_node_id`), and a second node under one id
//! is dropped from the accessibility tree with its subtree (a debug build
//! asserts "Duplicate a11y node id"); the two also share every piece of
//! gpui element state kept under that id (hover, scroll, focus). Taking
//! either one silently is the bug, so neither is.

use crate::{ElementIdWire, Node};
use std::collections::HashSet;
use std::fmt;

/// The id a node is filed under: its own, else, as row `row` of a list,
/// its index.
pub fn segment(own: Option<ElementIdWire>, row: Option<usize>) -> Option<ElementIdWire> {
    own.or_else(|| row.map(|index| ElementIdWire::Integer(index as u64)))
}

/// The index child `child` of `parent` has as a row of its list, for the
/// two nodes whose children are rows.
pub fn row(parent: &Node, child: usize) -> Option<usize> {
    match parent {
        Node::List { range_start, .. } => Some(range_start + child),
        Node::UniformList { indices, .. } => indices.get(child).map(|index| *index as usize),
        _ => None,
    }
}

/// One id claimed twice in one scope: the id, and the path of the scope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DuplicateIdentity {
    pub id: ElementIdWire,
    pub scope: Vec<ElementIdWire>,
}

impl fmt::Display for DuplicateIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "duplicate typed element identity among siblings: {} twice under ",
            Shown(&self.id)
        )?;
        if self.scope.is_empty() {
            return f.write_str("the root");
        }
        for (at, id) in self.scope.iter().enumerate() {
            if at > 0 {
                f.write_str(" > ")?;
            }
            write!(f, "{}", Shown(id))?;
        }
        Ok(())
    }
}

impl std::error::Error for DuplicateIdentity {}

struct Shown<'a>(&'a ElementIdWire);

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            ElementIdWire::Name(name) => f.write_str(name),
            ElementIdWire::Integer(index) => write!(f, "{index}"),
            id => write!(f, "{id:?}"),
        }
    }
}

/// The sanitizer's walk: the scopes open while a tree is walked and the
/// path of the node being walked. One set holds every id claimed in the
/// walk, beside the number of the scope it was claimed in.
#[derive(Default)]
pub struct Scopes {
    claimed: HashSet<(usize, ElementIdWire)>,
    open: Vec<usize>,
    opened: usize,
    path: Vec<ElementIdWire>,
}

impl Scopes {
    /// Enters a node filed under `segment` ([`segment`]): claims it in the
    /// scope it is in and opens its own. `Ok(false)` for a node filed under
    /// no id, which opens nothing.
    pub fn enter(&mut self, segment: Option<ElementIdWire>) -> Result<bool, DuplicateIdentity> {
        let Some(id) = segment else {
            return Ok(false);
        };
        let scope = self.open.last().copied().unwrap_or(0);
        if !self.claimed.insert((scope, id.clone())) {
            return Err(DuplicateIdentity {
                id,
                scope: self.path.clone(),
            });
        }
        self.opened += 1;
        self.open.push(self.opened);
        self.path.push(id);
        Ok(true)
    }

    /// Leaves the node [`Scopes::enter`] entered.
    pub fn leave(&mut self, entered: bool) {
        if entered {
            self.open.pop();
            self.path.pop();
        }
    }

    /// The path of the node being walked: its id last, when it has one.
    pub fn path(&self) -> &[ElementIdWire] {
        &self.path
    }
}
