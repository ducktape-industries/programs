//! The phone's screens. Under a finger the view is the Expo app's Nodes
//! tab, word for word (mobile `6f23151`): the list (`nodes.rs`, Expo
//! `app/(tabs)/nodes/index.tsx`) and one member's page (`node.rs`,
//! `app/(tabs)/nodes/[key].tsx`), with what both draw in `parts.rs` (the
//! confirm sheet a press that enacts raises, a vote's outcome strip, the
//! kit pieces the SDK has no helper for yet). The root picks this tree
//! once (`lib.rs`); every size is the SDK's touch ladder.
//!
//! Reads the whole view state; writes only through the view's own
//! presses: a member's page opens (`Nodes::open`), the door's sheet
//! (`Nodes::door`), Back (`Nodes::back`) and a vote (`Nodes::press`).
use view::design::Ladder;
use view::prelude::*;

use crate::{Nodes, Screen};

mod node;
mod nodes;
mod parts;

const LADDER: Ladder = design::TOUCH;

pub(crate) fn render(view: &Nodes, cx: &mut Context<Nodes>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let pages = view.pages();
    let page = match pages.top() {
        Screen::Nodes => nodes::render(view, cx, &theme).into_any_element(),
        Screen::Node(key) => node::render(view, key, cx, &theme).into_any_element(),
    };
    let sheets: Vec<AnyElement> = nodes::door(view, cx, &theme)
        .into_iter()
        .chain(parts::confirm(view, cx, &theme))
        .collect();
    let back = (pages.depth() > 0 || !sheets.is_empty())
        .then(|| cx.listener(|view: &mut Nodes, _: &BackEvent, _, cx| view.back(cx)));
    div()
        .id("nodes")
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_type(LADDER.body)
        .when_some(back, |root, back| root.on_back(back))
        .child(page)
        .children(sheets)
}
