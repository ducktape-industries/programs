//! Someone answering in the room before they post (chat's `Drafting`), framed as their post will be:
//! their face, their name and what they are doing, muted, over their text so far in the post's own
//! column. It is drawn while chat holds the draft and goes when chat drops it.
use chat::{Block, Draft, Principal};
use view::design::Ladder;
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::listing::answering_key;
use crate::{Chat, words};

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

// the draft's row: `frame` sets the padding its list's posts take, `face` sits where their faces
// do, and `draw` draws the text as their blocks are drawn
pub(crate) fn row(
    chat: &Chat,
    draft: &Draft,
    (theme, ladder): (&Theme, &Ladder),
    frame: impl FnOnce(Stateful<Div>) -> Stateful<Div>,
    face: AnyElement,
    draw: impl FnOnce(&str, &[Block]) -> Vec<AnyElement>,
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
        .children(said);
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
