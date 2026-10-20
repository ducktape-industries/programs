//! Where the keyboard is, by the host's rules: a node holds focus when a
//! test focuses it, when a press lands on it or inside it, when the view
//! moves focus there itself (`Window::focus`, `FocusHandle::focus`,
//! `Window::focus_next`), when Tab moves it, or when a dialog opens and
//! takes the keyboard; it loses focus when it leaves the tree. Keys go down
//! the focus path and back up it, and nowhere else.

use super::{authored_path, chain};
use crate::wire::{ElementIdWire, Node, WidgetCommand};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Focus {
    Path(Vec<ElementIdWire>),
    Handle(u64),
}

impl Focus {
    pub(super) fn chain<'a>(&self, root: &'a Node) -> Option<Vec<&'a Node>> {
        chain(root, &mut |chain| {
            let node = chain.last().unwrap();
            match self {
                Self::Path(path) => node.identity().is_some() && authored_path(chain) == *path,
                Self::Handle(handle) => node
                    .interactivity()
                    .is_some_and(|interactivity| interactivity.focus_handle == Some(*handle)),
            }
        })
    }

    pub(super) fn at(chain: &[&Node]) -> Self {
        let node = chain.last().expect("a node");
        match node.interactivity().and_then(|i| i.focus_handle) {
            Some(handle) => Self::Handle(handle),
            None => Self::Path(authored_path(chain)),
        }
    }

    pub(super) fn moved_by(
        command: &WidgetCommand,
        root: &Node,
        focus: Option<&Self>,
    ) -> Option<Self> {
        match command {
            WidgetCommand::Focus { target } => {
                let focus = Self::Path(target.clone());
                let Some(chain) = focus.chain(root) else {
                    let mut named = Vec::new();
                    super::chain(root, &mut |chain| {
                        let ends = authored_path(chain).ends_with(target);
                        if chain.last().unwrap().identity().is_some() && ends {
                            named.push(authored_path(chain));
                        }
                        false
                    });
                    panic!(
                        "the host refuses to focus {target:?}: no node has that path ({} end \
                         with it: {named:?}; `Window::focus_path` names one)",
                        named.len()
                    )
                };
                assert!(
                    matches!(
                        chain.last().unwrap(),
                        Node::Container(_) | Node::Field { .. }
                    ),
                    "the host refuses to focus {target:?}: it focuses a container or a field"
                );
                Some(focus)
            }
            WidgetCommand::FocusHandle { handle } => {
                let focus = Self::Handle(*handle);
                focus.chain(root).is_some().then_some(focus)
            }
            WidgetCommand::FocusNext => tab(root, focus, true),
            WidgetCommand::FocusPrevious => tab(root, focus, false),
            _ => None,
        }
    }
}

pub(super) fn holds_focus(node: &Node) -> bool {
    match node {
        Node::Field { .. } => true,
        _ => node.interactivity().is_some_and(|interactivity| {
            interactivity.focusable || interactivity.focus_handle.is_some()
        }),
    }
}

fn tab_stop(node: &Node) -> bool {
    match node {
        Node::Field { .. } => true,
        _ => node.interactivity().is_some_and(|interactivity| {
            interactivity
                .tab_stop
                .unwrap_or(interactivity.tab_index.is_some())
                && holds_focus(node)
        }),
    }
}

pub(super) fn tab(root: &Node, focus: Option<&Focus>, forward: bool) -> Option<Focus> {
    let held = focus.and_then(|focus| focus.chain(root));
    let trap = held.and_then(|held| {
        open_dialogs(root)
            .into_iter()
            .rev()
            .find(|dialog| in_modal_layer(&held, dialog))
    });
    let mut stops = Vec::new();
    chain(root, &mut |chain| {
        let trapped = trap
            .as_deref()
            .is_none_or(|dialog| in_modal_layer(chain, dialog));
        if trapped && tab_stop(chain.last().unwrap()) {
            stops.push(Focus::at(chain));
        }
        false
    });
    let at = focus.and_then(|focus| stops.iter().position(|stop| stop == focus));
    let next = match (at, forward) {
        (Some(at), true) => (at + 1) % stops.len(),
        (Some(at), false) => (at + stops.len() - 1) % stops.len(),
        (None, true) => 0,
        (None, false) => stops.len().checked_sub(1)?,
    };
    stops.into_iter().nth(next)
}

fn in_modal_layer(chain: &[&Node], dialog: &[ElementIdWire]) -> bool {
    chain
        .windows(2)
        .enumerate()
        .any(|(at, pair)| match pair[0] {
            Node::Overlay { children, .. } => {
                children.len() == 2
                    && std::ptr::eq(pair[1], &children[1])
                    && authored_path(&chain[..=at]) == dialog
            }
            Node::Sheet { child, .. } => {
                std::ptr::eq(pair[1], &**child) && authored_path(&chain[..=at]) == dialog
            }
            _ => false,
        })
}

pub(super) fn pressed(chain: &[&Node]) -> Option<Focus> {
    (1..=chain.len())
        .rev()
        .find(|end| holds_focus(chain[end - 1]))
        .map(|end| Focus::at(&chain[..end]))
}

pub(super) fn open_dialogs(root: &Node) -> Vec<Vec<ElementIdWire>> {
    let mut out = Vec::new();
    chain(root, &mut |chain| {
        match chain.last().unwrap() {
            Node::Overlay {
                label: Some(_),
                children,
                ..
            } if children.len() == 2 => out.push(authored_path(chain)),
            Node::Sheet { label: Some(_), .. } => out.push(authored_path(chain)),
            _ => {}
        }
        false
    });
    out
}

pub(super) fn entered(
    root: &Node,
    dialog: &[ElementIdWire],
    focus: Option<&Focus>,
) -> Option<Focus> {
    let inside = |chain: &[&Node]| in_modal_layer(chain, dialog);
    if focus
        .and_then(|focus| focus.chain(root))
        .is_some_and(|chain| inside(&chain))
    {
        return None;
    }
    chain(root, &mut |chain| {
        inside(chain) && tab_stop(chain.last().unwrap())
    })
    .map(|chain| Focus::at(&chain))
}
