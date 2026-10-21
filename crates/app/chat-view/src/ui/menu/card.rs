//! An agent author's card, opened from its avatar: what it is, whom it
//! answers, and the ways to its work in this room and to its profile. The
//! card reads identity only; what the agent did is the program its script
//! calls to show.
use view::prelude::*;

use super::{Item, Press};
use crate::ui::badge;
use crate::{Chat, Menu, Pane};
use chat::view::Names;
use view::design::Ladder;

const WORK_RECORD: &str = "chat-card-work-record";
const OPEN_PROFILE: &str = "chat-card-open-profile";
const PICTURE: &str = "chat-card-picture";
const WIDTH: f32 = 280.;

/// The card's first button, which takes the keys on open.
pub fn focus_key(chat: &Chat, pane: Pane, seq: u64) -> &'static str {
    let author = chat.author(pane, seq);
    match author.and_then(|author| chat.work_record(&author)) {
        Some(_) => WORK_RECORD,
        None => OPEN_PROFILE,
    }
}

pub(super) fn card(
    chat: &Chat,
    menu: &Menu,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(author) = chat.author(menu.pane, menu.seq) else {
        return div().into_any_element();
    };
    let Some(account) = author.account() else {
        return div().into_any_element();
    };
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    let name = names.member(&author);
    let mut who = div().flex().flex_col().items_start().gap_1().child(
        div()
            .id("chat-card-name")
            .text_size(design::text::BODY)
            .font_weight(view::FontWeight::SEMIBOLD)
            .child(name.clone()),
    );
    if let Some(label) = names.badge(&author) {
        who = who.child(badge(
            "chat-card-badge",
            label,
            ladder,
            theme.agent,
            theme.agent_soft,
        ));
    }
    let anyone = identity::Ownership::Anyone;
    let answers = chat.ownerships.ready().map(|ownerships| {
        crate::names::answers(names, ownerships.get(&account).unwrap_or(&anyone))
    });
    let answers = answers.map(|line| {
        div()
            .id("chat-card-answers")
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(line)
    });
    let open = |link: String| {
        Some(Box::new(cx.listener(move |chat, _: &ClickEvent, _, cx| {
            chat.close_menu();
            cx.notify();
            chat.open_link(link.clone(), cx);
        })) as Press)
    };
    let work = chat
        .work_record(&author)
        .map(|link| Item::text(WORK_RECORD, "View work record", open(link), *theme));
    let profile = Item::text(
        OPEN_PROFILE,
        "Open profile",
        open(account.to_string()),
        *theme,
    );
    let picture = chat.faces.of(account, cx);
    let pictured = picture.is_some();
    let face = crate::ui::sidebar::avatar(
        "chat-card-avatar",
        &name,
        true,
        picture,
        theme.surface_raised,
        theme,
    );
    let face = match pictured {
        true => {
            let enlarge = {
                let name = name.clone();
                cx.listener(move |chat, _: &ClickEvent, _, cx| {
                    chat.enlarge_face(account, name.clone(), cx)
                })
            };
            div()
                .id(PICTURE)
                .role(Role::Button)
                .aria_label(format!("Open {name}’s picture"))
                .focusable()
                .cursor_pointer()
                .on_click(enlarge)
                .child(face)
                .into_any_element()
        }
        false => face.into_any_element(),
    };
    div()
        .id("chat-card")
        .w(px(WIDTH))
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .child(div().flex().items_center().gap_2().child(face).child(who))
        .children(answers)
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .children(work)
                .child(profile),
        )
        .into_any_element()
}
