//! What the phone's chat screens share (Expo `src/chat/parts.tsx`, and the
//! kit pieces the SDK does not draw yet: `Notice`, `Quiet`, a small
//! `Button` with an icon): a person's face, the room's notice and
//! write-gate strip. Reads the open room's notice and gate; writes
//! only what their actions do (dismiss the notice, unarchive).
use view::prelude::*;
use view::{Div, Stateful};

use crate::Chat;
use crate::session::Gate;
use crate::words;
use view::FontWeight;
use view::design::Ladder;

pub(super) const HAIRLINE: Pixels = px(1.);
const NOTICE_ICON: Pixels = px(18.);
const NOTICE_ACTION_HEIGHT: Pixels = px(30.);
const SMALL_BUTTON: Pixels = px(36.);
const SMALL_BUTTON_ICON: Pixels = px(16.);

pub(super) fn face(
    name: &str,
    agent: bool,
    picture: Option<std::sync::Arc<view::Image>>,
    size: design::Face,
    theme: &Theme,
) -> Div {
    match agent {
        true => design::agent_avatar(name, picture, size, theme),
        false => design::avatar_with(name, picture, size, theme),
    }
}

pub(super) fn quiet(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    div()
        .id(id)
        .p(design::space::BLOCK)
        .text_type(ladder.secondary)
        .text_color(theme.muted)
        .child(text.into())
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NoticeKind {
    Danger,
    Warning,
    Quiet,
}

pub(super) fn notice(
    id: impl Into<ElementId>,
    kind: NoticeKind,
    icon: Option<&'static str>,
    words: impl Into<SharedString>,
    action: Option<AnyElement>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let (ground, tone) = match kind {
        NoticeKind::Danger => (theme.danger_soft, theme.danger),
        NoticeKind::Warning => (theme.warning_soft, theme.warning),
        NoticeKind::Quiet => (theme.surface, theme.muted),
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(design::space::MD)
        .py(design::space::MD)
        .px(design::space::BLOCK)
        .bg(ground)
        .when(kind == NoticeKind::Quiet, |strip| {
            strip.border_b_1().border_color(theme.border)
        })
        .when(kind == NoticeKind::Danger, |strip| {
            strip
                .role(Role::Alert)
                .aria_live(view::accesskit::Live::Assertive)
        })
        .text_type(ladder.secondary)
        .text_color(tone)
        .children(icon.map(|icon| design::icon(icon, NOTICE_ICON).text_color(tone)))
        .child(div().flex_1().min_w(px(0.)).child(words.into()))
        .children(action)
}

pub(super) fn notice_action(
    id: impl Into<ElementId>,
    label: &'static str,
    ladder: &Ladder,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .relative()
        .h(NOTICE_ACTION_HEIGHT)
        .flex()
        .items_center()
        .pl(design::space::SM)
        .font_weight(FontWeight::MEDIUM)
        .child(label)
        .child(
            design::hit(id, ladder.hit, NOTICE_ACTION_HEIGHT, px(0.), ladder)
                .role(Role::Button)
                .aria_label(label)
                .focusable()
                .on_click(click),
        )
        .into_any_element()
}

pub(super) fn small_button(
    id: impl Into<ElementId>,
    icon: &'static str,
    label: &'static str,
    theme: &Theme,
    ladder: &Ladder,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    div()
        .relative()
        .h(SMALL_BUTTON)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(design::space::SM)
        .px(design::space::LG)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_type(ladder.secondary)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.foreground)
        .child(design::icon(icon, SMALL_BUTTON_ICON))
        .child(label)
        .child(
            design::hit(id, ladder.hit, SMALL_BUTTON, HAIRLINE, ladder)
                .role(Role::Button)
                .aria_label(label)
                .focusable()
                .on_click(click),
        )
}

pub(super) fn names_notice(chat: &Chat, theme: &Theme, ladder: &Ladder) -> Option<AnyElement> {
    let refusal = chat.names.failed()?;
    let words = words::unreadable_names(&design::sentence(refusal));
    Some(
        notice(
            "chat-names-unread",
            NoticeKind::Warning,
            None,
            words,
            None,
            theme,
            ladder,
        )
        .into_any_element(),
    )
}

pub(super) fn room_notice(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    if chat.notice.is_empty() {
        return None;
    }
    let dismiss = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.notice.clear();
        cx.notify();
    });
    let dismiss = notice_action("chat-room-notice-dismiss", words::DISMISS, ladder, dismiss);
    let words = chat.notice.clone();
    Some(
        notice(
            "chat-room-notice",
            NoticeKind::Danger,
            None,
            words,
            Some(dismiss),
            theme,
            ladder,
        )
        .into_any_element(),
    )
}

pub(super) fn gate_strip(
    chat: &Chat,
    gate: Gate,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let (kind, icon) = match gate {
        Gate::Archived => (NoticeKind::Warning, None),
        Gate::NotMember => (NoticeKind::Quiet, Some("lock")),
        Gate::NoAccount => (NoticeKind::Quiet, None),
    };
    let unarchive = (gate == Gate::Archived && chat.may_change_room()).then(|| {
        let unarchive = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.set_archived(false, cx);
        });
        notice_action("chat-room-unarchive", words::UNARCHIVE, ladder, unarchive)
    });
    notice(
        "chat-room-write-refusal",
        kind,
        icon,
        words::gate(gate),
        unarchive,
        theme,
        ladder,
    )
    .into_any_element()
}
