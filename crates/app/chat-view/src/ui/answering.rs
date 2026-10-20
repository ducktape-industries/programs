//! Someone answering in the room before they post (chat's `Drafting`): who and what they say they
//! are doing, muted, over their text so far drawn as their post will be, at the foot of the list the
//! answer goes in. It is drawn while chat holds the draft and goes when chat drops it.
use chat::{Block, Draft, Principal};
use view::design::Ladder;
use view::prelude::*;

use crate::listing::answering_key;
use crate::{Chat, words};

pub(crate) fn row(
    chat: &Chat,
    draft: &Draft,
    theme: &Theme,
    ladder: &Ladder,
    draw: impl FnOnce(&str, &[Block]) -> Vec<AnyElement>,
) -> AnyElement {
    let name = match (chat.names.ready(), &draft.author) {
        (Some(names), author) => names.member(author),
        (None, Principal::Account(number)) => format!("account {number}"),
        (None, Principal::Root) => "the system".to_owned(),
    };
    let head = words::answering(&name);
    let doing = (!draft.doing.is_empty()).then(|| words::doing(&draft.doing));
    let key = format!("chat-{}", answering_key(draft));
    let body = match draft.text.is_empty() {
        true => Vec::new(),
        false => draw(&key, &chat::parse_message(&draft.text)),
    };
    let said = (!body.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .text_type(ladder.body)
            .children(body)
    });
    let muted = |text: String| {
        div()
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(text)
    };
    div()
        .id(key)
        .w_full()
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .role(Role::Status)
        .aria_live(view::accesskit::Live::Polite)
        .aria_label(head.clone())
        .child(muted(head))
        .children(doing.map(muted))
        .children(said)
        .into_any_element()
}
