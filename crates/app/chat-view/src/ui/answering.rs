//! Someone answering in a place: a draft chat holds, framed as its post will be at the foot of its
//! list (its thread's, the channel marking only the root's reply row), and who holds the place as
//! chat's `Busy` names them, since which block, with Break for whoever may change the room.
use chat::{Block, Draft, Holder, Principal};
use view::design::Ladder;
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::listing::answering_key;
use crate::{Chat, Pane, words};

pub(crate) fn name(chat: &Chat, author: &Principal) -> String {
    match (chat.names.ready(), author) {
        (Some(names), author) => names.member(author),
        (None, Principal::Account(number)) => format!("account {number}"),
        (None, Principal::Root) => "the system".to_owned(),
    }
}

pub(crate) fn agent(chat: &Chat, author: &Principal) -> bool {
    chat.names
        .ready()
        .is_some_and(|names| crate::message::agent(names, author))
}

// who is answering under the root `id`, on its reply row in the channel; none when nobody is
pub(crate) fn mark(id: &str, names: &[String], theme: &Theme) -> Option<Stateful<Div>> {
    let said = words::answering_under(names)?;
    let mark = div()
        .id(format!("chat-message-{id}-answering"))
        .role(Role::Status)
        .aria_live(view::accesskit::Live::Polite)
        .aria_label(said.clone())
        .text_color(theme.muted)
        .child(said);
    Some(mark)
}

// the draft's row: `frame` sets the padding its list's posts take, `face` sits where their faces
// do, and `draw` draws the text as their blocks are drawn
pub(crate) fn row(
    chat: &Chat,
    draft: &Draft,
    (theme, ladder): (&Theme, &Ladder),
    frame: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
    face: AnyElement,
    draw: impl FnOnce(&str, &[Block]) -> Vec<AnyElement>,
    held: Option<Stateful<Div>>,
) -> AnyElement {
    let name = name(chat, &draft.author);
    let head = words::answering(&name);
    let doing = (!draft.doing.is_empty()).then(|| words::doing(&draft.doing));
    let key = format!("chat-{}", answering_key(draft));
    let body = match draft.text.is_empty() {
        true => Vec::new(),
        false => draw(&key, &chat::parse_message(&draft.text)),
    };
    let said = (!body.is_empty()).then(|| div().flex().flex_col().gap_1().children(body));
    let muted = |text: String| {
        div()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(text)
    };
    let author = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::SM)
        .child(
            div()
                .text_type(ladder.body)
                .font_weight(FontWeight::MEDIUM)
                .child(name),
        )
        .child(muted(words::ANSWERING.to_owned()));
    let column = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(design::space::XXS)
        .child(author)
        .children(doing.map(muted))
        .children(said)
        .children(held);
    let row = div()
        .id(key)
        .w_full()
        .flex()
        .gap(design::space::MD)
        .py(design::space::SM)
        .role(Role::Status)
        .aria_live(view::accesskit::Live::Polite)
        .aria_label(head);
    frame(row).child(face).child(column).into_any_element()
}

pub(crate) fn busy(
    chat: &Chat,
    pane: Pane,
    holder: &Holder,
    (theme, ladder): (&Theme, &Ladder),
    frame: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
    cx: &mut Context<Chat>,
) -> AnyElement {
    let said = words::busy(&name(chat, &holder.principal), holder.height);
    frame(hold(chat, pane, said, (theme, ladder), cx)).into_any_element()
}

pub(crate) fn hold(
    chat: &Chat,
    pane: Pane,
    said: String,
    (theme, ladder): (&Theme, &Ladder),
    cx: &mut Context<Chat>,
) -> Stateful<Div> {
    let (id, thread) = match pane {
        Pane::Timeline => ("chat-busy-timeline", None),
        Pane::Thread => {
            let thread = chat.room.as_ref().and_then(|room| room.thread.as_ref());
            ("chat-busy-thread", thread.map(|thread| thread.root))
        }
    };
    let line = div()
        .id(id)
        .w_full()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .py(design::space::SM)
        .role(Role::Status)
        .aria_live(view::accesskit::Live::Polite)
        .aria_label(said.clone())
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(said),
        );
    if !chat.may_change_room() {
        return line;
    }
    let channel_id = chat.room_id();
    let broke = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        let channel_id = channel_id.clone();
        chat.submit(chat::Op::Break { channel_id, thread }, cx);
    });
    let button = design::button(format!("{id}-break"), words::BREAK, ladder, theme, broke);
    line.child(button.small())
}
