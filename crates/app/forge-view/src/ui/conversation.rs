//! A change's conversation: its body, its reviews, and the replies in
//! chat's hidden channel beneath them, with a composer at the end.
use view::prelude::*;

use crate::Forge;
use crate::select::{ChangeEvent, Picture};
use crate::state::verdict_verb;
use crate::ui::components::{badge, button, empty_state, id, quiet};
use crate::ui::scroller;
use crate::words;
use view::design::Ladder;

/// A review's body and comments start under its author's name, past the
/// avatar and the gap beside it.
const UNDER_NAME: Pixels = px(design::height::AVATAR as f32 + design::spacing::SM as f32);

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some((change, _, _, reviews)) = forge.change() else {
        return div().into_any_element();
    };
    let mut column = scroller("forge-conversation")
        .px(crate::ui::PAGE_X)
        .py(design::space::BLOCK)
        .gap(design::space::LG);
    if !change.body.trim().is_empty() {
        column = column.child(
            div()
                .id(id("forge-change-body"))
                .pb(design::space::LG)
                .border_b_1()
                .border_color(theme.border)
                .child(markdown::desktop::render(
                    "forge-change-body-text",
                    &change.body,
                    theme,
                    &crate::markdown::links(Vec::new(), cx),
                )),
        );
    }
    // Until chat answers, the reviews stand on their own; once it has, each
    // sits in the timeline where forge posted its line.
    let placed = |review: &forge::Review| match forge.talk_rows() {
        Some(Loadable::Ready(rows) | Loadable::Reloading(rows, _)) => {
            rows.iter().any(|row| row.message_id == review.message_id)
        }
        _ => false,
    };
    for review in reviews.items.iter().filter(|review| !placed(review)) {
        column = column.child(review_card(forge, review, cx, theme, ladder));
    }
    column = column.child(messages(forge, cx, theme, ladder));
    // a reply on its way, or one that did not land, over the box
    let reply = forge.reply_scope();
    if let Some(scope) = reply.filter(|scope| !forge.pending_in(scope).is_empty()) {
        column = column.child(crate::ui::pending(forge, &scope, theme));
    }
    column
        .child(composer(forge, cx, theme, ladder))
        .into_any_element()
}

/// One review as a timeline event: who, what they concluded (the verdict
/// is the verb), the block it landed in, then what it says and the line
/// comments it carried, quoted.
fn review_card(
    forge: &Forge,
    review: &forge::Review,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let author = forge.principal_name(&review.author);
    let outdated = forge.outdated(&review.draft.commit_oid);
    let mut head = timeline_head(
        &author,
        forge.picture(&review.author, cx),
        Some(author.clone()),
        verdict_verb(review.draft.verdict),
        review.height,
        theme,
    );
    if outdated {
        head = head.child(badge(
            id(format!("forge-review-outdated-{}", review.id)),
            words::OUTDATED,
            ladder,
            theme.warning,
            theme.warning_soft,
        ));
    }
    let mut card = div()
        .id(id(format!("forge-review-{}", review.id)))
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .child(head);
    if !review.draft.body.trim().is_empty() {
        card = card.child(div().pl(UNDER_NAME).child(review.draft.body.clone()));
    }
    for comment in &review.draft.comments {
        card = card.child(
            div().pl(UNDER_NAME).child(
                div()
                    .pl(design::space::MD)
                    .border_l_1()
                    .border_color(theme.border_strong)
                    .child(
                        design::mono(words::line_comment(
                            &comment.path,
                            comment.line,
                            &comment.body,
                        ))
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted),
                    ),
            ),
        );
    }
    card.into_any_element()
}

/// A timeline line's head: the avatar, the name (bold), what they did
/// (quiet), and the block it landed in on the right.
fn timeline_head(
    avatar: &str,
    picture: Picture,
    who: Option<String>,
    what: &str,
    height: u64,
    theme: &Theme,
) -> view::Div {
    div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(
            design::avatar_with(avatar, picture, design::size::AVATAR, theme)
                .border_1()
                .border_color(theme.border)
                .font_weight(view::FontWeight::SEMIBOLD),
        )
        .children(who.map(crate::ui::bold))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(quiet(what.to_owned(), theme)),
        )
        .child(
            design::mono(words::block(height))
                .text_size(design::text::CAPTION)
                .text_color(theme.faint),
        )
}

/// One line of the change's history: who did it, if forge recorded it,
/// what happened, and the block it landed in.
fn event(
    key: String,
    who: Option<String>,
    picture: Picture,
    what: String,
    height: u64,
    theme: &Theme,
) -> AnyElement {
    let avatar = who.clone().unwrap_or_default();
    div()
        .id(id(format!("forge-event-{key}")))
        .child(timeline_head(&avatar, picture, who, &what, height, theme))
        .into_any_element()
}

/// A line forge itself posted, as [`Forge::change_event`] reads it: a
/// review's card, or what happened to the change.
fn forge_line(
    forge: &Forge,
    row: &chat::MsgRow,
    what: ChangeEvent,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let (change, ..) = forge.change()?;
    let key = row.message_id.clone();
    let (by, what) = match what {
        ChangeEvent::Reply => return None,
        ChangeEvent::Review(review) => return Some(review_card(forge, review, cx, theme, ladder)),
        ChangeEvent::Opened => (Some(&change.author), words::OPENED_IT.to_owned()),
        ChangeEvent::Merged { by, oid } => (by, words::merged_into(&change.into, oid)),
        ChangeEvent::Closed { by } => (by, words::CLOSED_IT.to_owned()),
    };
    let who = by.map(|by| forge.principal_name(by));
    let picture = by.and_then(|by| forge.picture(by, cx));
    Some(event(key, who, picture, what, row.height, theme))
}

/// The hidden chat channel of this change, in chat's row shape.
fn messages(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    if forge.change().is_none() {
        return div().into_any_element();
    }
    // forge's own lines are told by their author, whom the roster names
    let naming = forge.names.is_idle() || forge.names.is_loading();
    match forge.talk_rows() {
        None | Some(Loadable::Idle) | Some(Loadable::Loading(_)) => {
            quiet(words::READING_CONVERSATION, theme)
        }
        Some(Loadable::Ready(_) | Loadable::Reloading(_, _)) if naming => {
            quiet(words::READING_CONVERSATION, theme)
        }
        Some(Loadable::Failed(refusal)) => div()
            .id(id("forge-conversation-refused"))
            .p_2()
            .bg(theme.danger_soft)
            .text_size(design::text::SECONDARY)
            .child(design::sentence(refusal))
            .into_any_element(),
        Some(Loadable::Ready(rows) | Loadable::Reloading(rows, _)) if rows.is_empty() => {
            empty_state(
                id("forge-conversation-empty"),
                "message-square",
                "No replies yet",
                "This change's channel is quiet.",
                ladder,
                theme,
            )
            .into_any_element()
        }
        Some(Loadable::Ready(rows) | Loadable::Reloading(rows, _)) => {
            let mut column = div()
                .id(id("forge-conversation-messages"))
                .flex()
                .flex_col()
                .gap(design::space::BLOCK);
            for message in rows {
                match forge.change_event(message) {
                    Some(ChangeEvent::Reply) => {}
                    Some(what) => {
                        column =
                            column.children(forge_line(forge, message, what, cx, theme, ladder));
                        continue;
                    }
                    None => continue,
                }
                let author = forge.principal_name(&message.author);
                let picture = forge.picture(&message.author, cx);
                column = column.child(
                    div()
                        .id(id(format!("forge-message-{}", message.message_id)))
                        .flex()
                        .flex_col()
                        .gap(design::space::XS)
                        .child(timeline_head(
                            &author,
                            picture,
                            Some(author.clone()),
                            "replied",
                            message.height,
                            theme,
                        ))
                        .child(div().pl(UNDER_NAME).child(message.text.clone())),
                );
            }
            column.into_any_element()
        }
    }
}

fn composer(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let send = cx.listener(|forge, _: &ClickEvent, window, cx| forge.post_reply(window, cx));
    let replying = forge.reply_scope().is_some_and(|scope| forge.busy(&scope));
    div()
        .id(id("forge-composer"))
        .flex()
        .gap(design::space::SM)
        .items_start()
        .pt(design::space::SM)
        .border_t_1()
        .border_color(theme.border)
        .child(
            Textarea::new(id("forge-reply"), &forge.reply, "Reply")
                .min_h(design::size::CONTROL)
                .flex_1()
                .px_2()
                .py(design::space::XS)
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground)
                .placeholder(words::REPLY_IN_CHANGE),
        )
        .child(
            button(id("forge-reply-send"), "Send", ladder, theme, send)
                .kind(design::Kind::Outline)
                .enabled(forge.may_write() && !forge.reply.is_blank() && !replying),
        )
        .into_any_element()
}
