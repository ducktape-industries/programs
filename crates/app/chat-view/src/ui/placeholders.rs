//! Chat's placeholders while a read is out: the room list's rows and a pane's messages as the
//! SDK's static shapes, laid out as the rows that replace them, on the desktop and the phone.
use view::design::placeholder::{self, CAPTION, ROWS, TITLE};
use view::design::{Ladder, Type};
use view::prelude::*;
use view::{Div, Stateful};

use crate::Pane;
use crate::words;

// The room list's rows while the rooms are read, as a channel's row lays out
// (`sidebar.rs` `channel_button`): the lead column, then the name's bar, on the sidebar's ground.
pub(crate) fn sidebar_rooms(theme: &Theme) -> Stateful<Div> {
    let fill = theme.sidebar_raised;
    let body = Type::size(design::text::BODY);
    placeholder::region("chat-sidebar-loading", words::LOADING_ROOMS)
        .flex()
        .flex_col()
        .gap_1()
        .children((0..ROWS).map(move |_| {
            div()
                .flex()
                .w_full()
                .items_center()
                .gap_1()
                .min_h(design::size::CONTROL)
                .px_1()
                .child(design::lead(placeholder::bar(fill).size(body.size)))
                .child(placeholder::line(body, TITLE, fill).flex_1().min_w_0())
        }))
}

// A pane's messages while they are read, as a message lays out (`message.rs` `card`): the
// author's face, then the author's line over the body's, the newest at the foot.
pub(crate) fn messages(pane: Pane, theme: &Theme) -> Stateful<Div> {
    let fill = theme.surface_raised;
    let body = Type::size(design::text::BODY);
    let (id, label) = loading(pane);
    placeholder::region(id, label)
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .justify_end()
        .overflow_hidden()
        .children((0..ROWS).map(move |_| {
            div()
                .flex()
                .gap(design::space::MD)
                .px_4()
                .pt(design::space::SM + design::space::XXS)
                .pb(design::space::HAIR)
                .child(placeholder::bar(fill).size_7().rounded(design::space::XS))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(design::space::XS)
                        .child(placeholder::line(body, TITLE, fill))
                        .child(placeholder::line(body, CAPTION, fill)),
                )
        }))
}

// The phone's room list while the rooms are read, as its rows lay out (`phone/rooms.rs`
// `channel_row`): a row with its lead, then the name's bar.
pub(crate) fn phone_rooms(theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let fill = theme.surface_raised;
    let (theme, ladder) = (*theme, *ladder);
    placeholder::region("chat-rooms-loading", words::LOADING_ROOMS)
        .flex()
        .flex_col()
        .children((0..ROWS).map(move |row| {
            design::row(format!("chat-rooms-loading-{row}"), &ladder, &theme)
                .child(design::lead(placeholder::bar(fill).size(ladder.icon_row)))
                .child(
                    placeholder::line(ladder.body, TITLE, fill)
                        .flex_1()
                        .min_w_0(),
                )
        }))
}

// The phone's messages while they are read, as its rows lay out (`phone/message.rs`): the
// author's face in its column, then the author's line over the body's, the newest at the foot.
pub(crate) fn phone_messages(pane: Pane, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let fill = theme.surface_raised;
    let ladder = *ladder;
    let (id, label) = loading(pane);
    placeholder::region(id, label)
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .justify_end()
        .overflow_hidden()
        .children((0..ROWS).map(move |_| {
            div()
                .w_full()
                .flex()
                .gap(design::space::MD)
                .px(design::space::BLOCK)
                .pt(design::space::MD)
                .pb(design::space::HAIR)
                .child(
                    div()
                        .w(ladder.avatar.side)
                        .flex_none()
                        .child(placeholder::face(ladder.avatar, fill)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(placeholder::line(ladder.caption, TITLE, fill))
                        .child(placeholder::line(ladder.body, CAPTION, fill)),
                )
        }))
}

// The room's pane while there is no room to open yet: an empty region that says the rooms are
// read.
pub(crate) fn no_room() -> Stateful<Div> {
    placeholder::region("chat-no-room-loading", words::LOADING_CHANNELS).size_full()
}

fn loading(pane: Pane) -> (&'static str, &'static str) {
    match pane {
        Pane::Timeline => ("chat-messages-loading", words::LOADING_MESSAGES),
        Pane::Thread => ("chat-replies-loading", words::LOADING_THREAD),
    }
}
