//! Copied row identity, preserving native equality and virtual-list bit identity.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformListSizing {
    Infer,
    #[default]
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformListHorizontalSizing {
    #[default]
    FitList,
    Unconstrained,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UniformListScrollStrategy {
    Top,
    Center,
    Bottom,
    Nearest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UniformListScrollRequest {
    pub index: usize,
    pub strategy: UniformListScrollStrategy,
    pub offset: usize,
    pub strict: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListAlignment {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListSizingBehavior {
    Infer,
    Auto,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ListOffset {
    pub item_ix: usize,
    pub offset_in_item: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ListCommand {
    Reset {
        count: usize,
    },
    Splice {
        start: usize,
        end: usize,
        count: usize,
    },
    Remeasure {
        start: usize,
        end: usize,
    },
    ScrollTo(ListOffset),
    ScrollToEnd,
    ScrollToRevealItem(usize),
    SetFollowMode {
        tail: bool,
    },
    PauseFollowingTail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListRequest {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ListScroll {
    pub visible_start: usize,
    pub visible_end: usize,
    pub count: usize,
    pub is_scrolled: bool,
    pub is_following_tail: bool,
    pub offset: ListOffset,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ListKey {
    Bool(bool),
    Integer(i64),
    Float(f64),
}

impl ListKey {
    pub fn virtual_key(self) -> u64 {
        match self {
            Self::Bool(value) => u64::from(value),
            Self::Integer(value) => value.cast_unsigned(),
            Self::Float(value) => value.to_bits(),
        }
    }
}

impl From<bool> for ListKey {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}
impl From<i64> for ListKey {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<f64> for ListKey {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl PartialEq for ListKey {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Integer(a), Self::Integer(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

#[cfg(test)]
mod variable_tests {
    use super::*;
    use crate::styles::testing::sanitize_plain;
    use crate::{Frame, Node, StyleId};

    fn node(commands: Vec<ListCommand>, children: usize) -> Node {
        list("rows", commands, children)
    }

    fn list(id: &'static str, commands: Vec<ListCommand>, children: usize) -> Node {
        let id = crate::ElementIdWire::Name(id.into());
        Node::List {
            path: vec![id.clone()],
            id,
            item_count: usize::MAX,
            alignment: ListAlignment::Bottom,
            overdraw: f32::INFINITY,
            sizing: ListSizingBehavior::Auto,
            following_tail: true,
            revision: 3,
            commands,
            request_handler: 1,
            scroll_handler: Some(2),
            range_start: 99_990,
            style: StyleId(0),
            interactivity: Default::default(),
            children: (0..children).map(|_| Node::Space).collect(),
        }
    }

    #[test]
    fn one_wire_walk_makes_geometry_finite() {
        let mut frame = Frame {
            root: Some(node(
                vec![ListCommand::ScrollTo(ListOffset {
                    item_ix: usize::MAX,
                    offset_in_item: f32::INFINITY,
                })],
                20,
            )),
            ..Frame::default()
        };
        sanitize_plain(&mut frame).unwrap();
        let Node::List {
            overdraw,
            commands,
            children,
            ..
        } = frame.root.unwrap()
        else {
            panic!()
        };
        assert_eq!(overdraw, f32::MAX);
        assert_eq!(
            commands,
            [ListCommand::ScrollTo(ListOffset {
                item_ix: usize::MAX,
                offset_in_item: f32::MAX,
            })]
        );
        assert_eq!(children.len(), 20);
    }

    #[test]
    fn a_list_cannot_address_another_authored_ancestor() {
        let mut root = node(Vec::new(), 0);
        let Node::List { path, .. } = &mut root else {
            unreachable!()
        };
        path.push(crate::ElementIdWire::Name("other-room".into()));
        let mut frame = Frame {
            root: Some(root),
            ..Default::default()
        };
        assert_eq!(
            sanitize_plain(&mut frame),
            Err(crate::Refused::Invalid("list authored path is invalid"))
        );
    }

    #[test]
    fn sibling_lists_file_their_rows_apart() {
        let pair = |first: &'static str, second: &'static str| Frame {
            root: Some(Node::Container(crate::ContainerNode {
                id: Some(crate::ElementIdWire::Name("page".into())),
                style: StyleId(0),
                interactivity: Default::default(),
                children: [first, second]
                    .into_iter()
                    .map(|id| {
                        let mut list = list(id, Vec::new(), 2);
                        let Node::List {
                            path,
                            item_count,
                            range_start,
                            ..
                        } = &mut list
                        else {
                            unreachable!()
                        };
                        path.insert(0, crate::ElementIdWire::Name("page".into()));
                        (*item_count, *range_start) = (2, 0);
                        list
                    })
                    .collect(),
            })),
            ..Default::default()
        };
        assert!(sanitize_plain(&mut pair("timeline", "thread")).is_ok());
        let Err(crate::Refused::Duplicate(duplicate)) = sanitize_plain(&mut pair("rows", "rows"))
        else {
            panic!("one id on two lists in a scope is refused")
        };
        assert_eq!(
            duplicate.to_string(),
            "duplicate typed element identity among siblings: rows twice under page"
        );
    }
}
