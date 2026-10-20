//! Every `WidgetCommand` variant once: the payload of `host.widget`, but
//! also its own tree-side (MessagePack) type, sampled here the same
//! way `nodes.rs` samples `Node` so its bytes are pinned too.
use super::*;

fn every_atom_child() -> Vec<ElementIdWire> {
    let atoms = [
        ElementIdAtom::View(1),
        ElementIdAtom::Integer(2),
        ElementIdAtom::Name("atom".into()),
        ElementIdAtom::Uuid([1; 16]),
        ElementIdAtom::FocusHandle(3),
        ElementIdAtom::NamedInteger("atom".into(), 4),
        ElementIdAtom::Path(b"a/b".to_vec()),
        ElementIdAtom::CodeLocation {
            file: "widget.rs".into(),
            line: 1,
            column: 2,
        },
        ElementIdAtom::OpaqueId([2; 20]),
    ];
    atoms
        .into_iter()
        .map(|base| ElementIdWire::NamedChild {
            base,
            names: vec!["child".into()],
        })
        .collect()
}

pub fn every_widget_command() -> Vec<WidgetCommand> {
    let target = |name: &str| vec![id(name)];
    vec![
        WidgetCommand::Replace {
            target: target("field"),
            generation: 2,
            revision: 4,
            range: TextRange { start: 0, end: 5 },
            text: "@alice".into(),
            token: Some("<@1>".into()),
            cursor: TextRange { start: 6, end: 6 },
        },
        WidgetCommand::FocusPrevious,
        WidgetCommand::FocusNext,
        WidgetCommand::Focus {
            target: every_atom_child(),
        },
        WidgetCommand::FocusHandle { handle: 7 },
        WidgetCommand::CursorFront {
            target: target("field"),
        },
        WidgetCommand::CursorEnd {
            target: target("field"),
        },
        WidgetCommand::Cursor {
            target: target("field"),
            position: 3,
        },
        WidgetCommand::SelectAll {
            target: target("field"),
        },
        WidgetCommand::Select {
            target: target("field"),
            start: 0,
            end: 3,
        },
        WidgetCommand::Snap {
            target: target("handle"),
            x: 0.25,
            y: 0.75,
        },
        WidgetCommand::SnapEnd {
            target: target("handle"),
        },
        WidgetCommand::ScrollTo {
            target: target("list"),
            x: 10.0,
            y: 20.0,
        },
        WidgetCommand::ScrollBy {
            target: target("list"),
            x: 1.0,
            y: -1.0,
        },
    ]
}
