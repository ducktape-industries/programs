//! Someone answering in the room before they post (chat's `Drafting`): who, what they say they are
//! doing and their text so far, muted at the foot of the list the answer goes in. It is drawn while
//! chat holds the draft and goes when chat drops it: their post landed, or they stopped.
use chat::{Draft, Principal};
use view::design::Ladder;
use view::prelude::*;

use crate::listing::answering_key;
use crate::{Chat, words};

pub(crate) fn row(chat: &Chat, draft: &Draft, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let name = match (chat.names.ready(), &draft.author) {
        (Some(names), author) => names.member(author),
        (None, Principal::Account(number)) => format!("account {number}"),
        (None, Principal::Root) => "the system".to_owned(),
    };
    let head = words::answering(&name);
    let doing = (!draft.doing.is_empty()).then(|| words::doing(&draft.doing));
    let text = (!draft.text.is_empty()).then(|| draft.text.clone());
    div()
        .id(format!("chat-{}", answering_key(draft)))
        .w_full()
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .text_color(theme.muted)
        .role(Role::Status)
        .aria_live(view::accesskit::Live::Polite)
        .aria_label(head.clone())
        .child(div().text_type(ladder.caption).child(head))
        .children(doing.map(|doing| div().text_type(ladder.caption).child(doing)))
        .children(text.map(|text| div().text_type(ladder.body).child(text)))
        .into_any_element()
}
