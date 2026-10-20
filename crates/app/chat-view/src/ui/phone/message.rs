//! One phone message row (Expo `MessageRow.tsx`): face, byline, blocks,
//! reactions and the way into its thread. A tap does nothing; a long press
//! opens the actions sheet, and on a reaction names who chose it.
use view::prelude::*;
use view::{Div, Stateful};
use view::{FontWeight, HighlightStyle, UnderlineStyle};

use super::parts::{HAIRLINE, face};
use super::rich;
use crate::message::{ChatMessage, SystemLine};
use crate::words;
use crate::{Chat, Mode, Pane};
use chat::Reaction;
use view::design::Ladder;

const FADED: f32 = 0.5;
const CHIP_HEIGHT: Pixels = px(30.);
const CHIP_WIDTH: Pixels = px(40.);
const CHIP_EMOJI: design::Type = design::Type::lined(14., 18.);
const CHIP_PLUS: Pixels = px(14.);
const THREAD_BUTTON: Pixels = px(32.);
const THREAD_ICON: Pixels = px(16.);
const ACTIONS: i32 = 1;

pub(super) fn row(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    set: (usize, usize),
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let (seq, rev) = (message.seq, message.rev);
    let chosen = chat.menu.as_ref().is_some_and(|menu| {
        menu.pane == pane && menu.seq == seq && matches!(menu.mode, Mode::Toolbar | Mode::More)
    });
    let actions = cx.listener(move |chat, event: &ClickEvent, _window, cx| {
        if matches!(event, ClickEvent::Keyboard(_)) {
            cx.notify();
            chat.show_menu(pane, seq, rev, Mode::More);
        }
    });
    let held = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.show_menu(pane, seq, rev, Mode::More);
    });
    let picture = message
        .from
        .account()
        .and_then(|account| chat.faces.of(account, cx));
    let row = div()
        .id(format!("chat-message-{}-row", message.id))
        .relative()
        .w_full()
        .flex()
        .gap(design::space::MD)
        .px(design::space::BLOCK)
        .pt(match message.show_author {
            true => design::space::MD,
            false => design::space::HAIR,
        })
        .pb(design::space::HAIR)
        .when(chosen, |row| row.bg(theme.accent_soft))
        .role(Role::Group)
        .aria_label(format!("{}: {}", message.author, message.body))
        .aria_position_in_set(set.0)
        .aria_size_of_set(set.1)
        .focusable()
        .on_click(actions)
        .on_aux_click(held)
        .child(
            div()
                .w(ladder.avatar.side)
                .flex_none()
                .children(message.show_author.then(|| {
                    face(
                        &message.author,
                        message.agent,
                        picture,
                        ladder.avatar,
                        theme,
                    )
                    .id(format!("chat-message-{}-avatar", message.id))
                })),
        )
        .child(content(chat, message, pane, cx, theme, ladder));
    let row = design::dense(row);
    match message.pending {
        true => row.into_any_element(),
        false => {
            let chat = cx.weak_entity();
            row.custom_action(ACTIONS, words::MESSAGE_ACTIONS)
                .on_a11y_action(
                    view::wire::Action::CustomAction,
                    move |_: Option<&view::wire::ActionData>, _: &mut Window, app: &mut App| {
                        let _ = chat.update(app, |chat, cx| {
                            cx.notify();
                            chat.show_menu(pane, seq, rev, Mode::More);
                        });
                    },
                )
                .into_any_element()
        }
    }
}

// the face over a draft, in the column a post's face takes
pub(super) fn still_face(
    chat: &Chat,
    author: &chat::Principal,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let name = crate::ui::answering::name(chat, author);
    let agent = crate::ui::answering::agent(chat, author);
    let picture = author
        .account()
        .and_then(|account| chat.faces.of(account, cx));
    div()
        .w(ladder.avatar.side)
        .flex_none()
        .child(face(&name, agent, picture, ladder.avatar, theme))
        .into_any_element()
}

fn content(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let reacted = !message.deleted && !message.pending && !message.reactions.is_empty();
    let threaded = pane == Pane::Timeline && message.thread.is_none() && message.reply_count > 0;
    div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap(design::space::XXS)
        .children(message.show_author.then(|| byline(message, theme, ladder)))
        .child(body(chat, message, cx, theme, ladder))
        .children(reacted.then(|| reactions(chat, message, pane, cx, theme, ladder)))
        .children(threaded.then(|| thread_button(message, cx, theme, ladder)))
}

fn byline(message: &ChatMessage, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let badge = message
        .badge
        .as_ref()
        .filter(|_| message.agent)
        .map(|label| {
            design::badge(
                format!("chat-message-{}-badge", message.id),
                label.clone(),
                ladder,
                theme.agent,
                theme.agent_soft,
            )
        });
    div()
        .id(format!("chat-message-{}-header", message.id))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::SM)
        .child(
            div()
                .flex_shrink_1()
                .min_w(px(0.))
                .truncate()
                .text_type(ladder.body)
                .font_weight(FontWeight::MEDIUM)
                .child(message.author.clone()),
        )
        .children(badge)
        .children((message.time > 0).then(|| {
            div()
                .id(format!("chat-message-{}-time", message.id))
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .whitespace_nowrap()
                .child(design::clock(message.time))
        }))
}

fn body(
    chat: &Chat,
    message: &ChatMessage,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let said: Vec<AnyElement> = match (&message.system, message.deleted) {
        (_, true) => vec![
            div()
                .italic()
                .text_color(theme.muted)
                .child(words::MESSAGE_DELETED)
                .into_any_element(),
        ],
        (Some(post), false) => vec![program_line(chat, message, post, cx, theme)],
        (None, false) => rich::blocks(chat, message, cx, theme, ladder),
    };
    div()
        .flex()
        .flex_col()
        .gap(design::space::XXS)
        .when(message.pending, |body| body.opacity(FADED))
        .children(said)
        .children((message.edited && !message.deleted).then(|| {
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(words::EDITED)
        }))
}

// A program's own post: the program in medium, what it says, and the way to where the program
// shows the room it is about.
fn program_line(
    chat: &Chat,
    message: &ChatMessage,
    post: &SystemLine,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let open = words::open_in(&post.program);
    let text = format!("{} {}  {open}", post.program, post.said);
    let link = text.len() - open.len()..text.len();
    let highlights = vec![
        (
            0..post.program.len(),
            HighlightStyle {
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
        ),
        (
            link.clone(),
            HighlightStyle {
                color: Some(theme.foreground),
                underline: Some(UnderlineStyle {
                    thickness: HAIRLINE,
                    color: None,
                    wavy: false,
                }),
                ..Default::default()
            },
        ),
    ];
    let target = crate::links::program_link(&chat.session.chain_id, &post.room);
    let line = InteractiveText::new(
        ElementId::from(format!("chat-message-{}-program", message.id)),
        StyledText::new(text).with_highlights(highlights),
    );
    let line = match target {
        Some(target) => line.on_click(
            vec![link],
            cx.processor(move |chat, _: usize, _window, cx| chat.open_link(target.clone(), cx)),
        ),
        None => line,
    };
    div().text_color(theme.muted).child(line).into_any_element()
}

fn reactions(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let (seq, rev) = (message.seq, message.rev);
    let chips: Vec<AnyElement> = message
        .reactions
        .iter()
        .map(|reaction| {
            let (emoji, add) = (reaction.emoji.clone(), !reaction.reacted_by_me);
            let react = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.react(seq, emoji.clone(), add, cx)
            });
            let shown = reaction.emoji.clone();
            let held = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.reactors_of = shown.clone();
                chat.show_menu(pane, seq, rev, Mode::Reactors);
            });
            let id = format!("chat-message-{}-reaction-{}", message.id, reaction.emoji);
            let who = chat.reactor_names(seq, &reaction.emoji).join(", ");
            let presses = Presses { react, held };
            emoji_chip(id, reaction, &who, presses, theme, ladder)
        })
        .collect();
    let pick = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.show_menu(pane, seq, rev, Mode::Reactions);
    });
    let add = add_chip(
        format!("chat-message-{}-reaction-add", message.id),
        pick,
        theme,
        ladder,
    );
    div()
        .flex()
        .flex_wrap()
        .gap(design::space::XS)
        .mt(design::space::XXS)
        .children(chips)
        .child(add)
}

fn chip(mine: bool, theme: &Theme) -> Div {
    div()
        .relative()
        .h(CHIP_HEIGHT)
        .min_w(CHIP_WIDTH)
        .px(design::space::SM)
        .flex()
        .items_center()
        .justify_center()
        .gap(design::space::XXS)
        .border_1()
        .border_color(if mine { theme.accent } else { theme.border })
        .when(mine, |chip| chip.bg(theme.accent_soft))
}

struct Presses<R, H> {
    react: R,
    held: H,
}

fn emoji_chip<R, H>(
    id: String,
    reaction: &Reaction,
    who: &str,
    presses: Presses<R, H>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement
where
    R: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    H: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let mine = reaction.reacted_by_me;
    let hit = design::hit(id, CHIP_WIDTH, CHIP_HEIGHT, HAIRLINE, ladder)
        .role(Role::Button)
        .aria_label(format!("{} {}", reaction.emoji, reaction.count))
        .when(!who.is_empty(), |hit| hit.aria_description(who.to_owned()))
        .aria_toggled(mine.into())
        .focusable()
        .on_click(presses.react)
        .consumes_click()
        .on_aux_click(presses.held);
    chip(mine, theme)
        .child(div().text_type(CHIP_EMOJI).child(reaction.emoji.clone()))
        .child(
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_type(ladder.caption)
                .child(reaction.count.to_string()),
        )
        .child(hit)
        .into_any_element()
}

fn add_chip(
    id: String,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let hit = design::hit(id, CHIP_WIDTH, CHIP_HEIGHT, HAIRLINE, ladder)
        .role(Role::Button)
        .aria_label(words::ADD_REACTION)
        .focusable()
        .on_click(click)
        .consumes_click();
    chip(false, theme)
        .child(design::icon("plus", CHIP_PLUS).text_color(theme.muted))
        .child(hit)
        .into_any_element()
}

fn thread_button(
    message: &ChatMessage,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let seq = message.seq;
    let replies = design::plural(message.reply_count, "reply", "replies");
    let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.open_thread(seq, cx)
    });
    let hit = design::hit(
        format!("chat-message-{}-replies", message.id),
        ladder.hit,
        THREAD_BUTTON,
        px(0.),
        ladder,
    )
    .role(Role::Button)
    .aria_label(words::open_thread(&replies))
    .focusable()
    .on_click(open)
    .consumes_click();
    div()
        .relative()
        .self_start()
        .h(THREAD_BUTTON)
        .flex()
        .items_center()
        .gap(design::space::XS)
        .text_type(ladder.secondary)
        .font_weight(FontWeight::MEDIUM)
        .child(design::icon("message-square", THREAD_ICON).text_color(theme.muted))
        .child(replies)
        .child(design::icon("chevron-right", THREAD_ICON).text_color(theme.muted))
        .child(hit)
        .into_any_element()
}
