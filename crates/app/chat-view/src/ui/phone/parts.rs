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

// SDK gap: the hairline token, kept local until the kit takes it (kit.md §2)
/// A hairline (mobile `src/design/theme.ts:83`, `size.hairline`).
pub(super) const HAIRLINE: Pixels = px(1.);
// SDK gap: the kit Notice, kept local until the kit takes it (kit.md §2)
/// The kit `Notice`'s icon (mobile `src/kit/Notice.tsx:49`).
const NOTICE_ICON: Pixels = px(18.);
/// A notice action's drawn height: its secondary line and `gap.xs` over
/// and under (mobile `src/kit/Notice.tsx:60`); its press reaches `hit`.
const NOTICE_ACTION_HEIGHT: Pixels = px(30.);
// SDK gap: a Button's icon (mobile `src/kit/Button.tsx:75`), kept local
// until the kit takes it; kit.md does not list it yet
/// A small kit `Button` (mobile `src/design/theme.ts:76`, `size.small`)
/// and its icon (mobile `src/kit/Button.tsx:75`).
const SMALL_BUTTON: Pixels = px(36.);
const SMALL_BUTTON_ICON: Pixels = px(16.);

// SDK gap: the agent avatar, kept local until the kit takes it (kit.md §2)
/// A person's round initial at `size` (the kit's `Avatar`), on the agent
/// tint for an agent.
pub(super) fn face(name: &str, agent: bool, size: design::Face, theme: &Theme) -> Div {
    design::avatar(name, size, theme).when(agent, |face| {
        face.bg(theme.agent_soft).text_color(theme.agent)
    })
}

// SDK gap: quiet text at the ladder's sizes, kept local until the kit takes it (kit.md §2)
/// The kit `Quiet`: one muted line of secondary text, 16 in all round.
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

// SDK gap: the kit Notice, kept local until the kit takes it (kit.md §2)
/// What a kit `Notice` says by its ground and tone.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NoticeKind {
    Danger,
    Warning,
    /// on the surface, a hairline under it
    Quiet,
}

// SDK gap: the kit Notice, kept local until the kit takes it (kit.md §2)
/// The kit `Notice` (mobile `src/kit/Notice.tsx`): secondary words on its
/// kind's ground, 16 in and 10 over and under, an icon before them and
/// its one action at the right. A danger notice is an alert.
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
        // the words take the room the icon and the action leave, and wrap in it
        .child(div().flex_1().min_w(px(0.)).child(words.into()))
        .children(action)
}

// SDK gap: the kit Notice, kept local until the kit takes it (kit.md §2)
/// A notice's action: secondary medium in the notice's tone, its press
/// reaching to `hit`.
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

// SDK gap: a Button's icon, as above
/// The kit's small outline `Button` with an icon before its label
/// (`design::button(..).small()` has no icon): 36 tall, 12 in, secondary
/// medium; its press reaches `hit`. Drawn on the window's ground, so it
/// reads over a list.
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

/// The notice over a channel's screens (Expo `ChannelNotice`): a write the
/// program refused, in danger, until dismissed.
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

// The strip in the composer's place where the reader may not write (Expo `GateStrip`, the
// desktop's words): archived in warning, with Unarchive for whoever may change the room;
// members-only with its lock; no account, quiet.
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
