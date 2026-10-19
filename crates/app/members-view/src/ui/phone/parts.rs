//! What the roster and the member page draw the same way, in the Expo
//! kit's shapes (mobile `src/kit/`) where the SDK's kit has no piece yet:
//! an account's face, its name line and the quiet status line. Reads one
//! [`Row`].
use view::design::{self, Face, Ladder, Type, space};
use view::prelude::*;
use view::{Div, FontWeight};

use crate::{Row, words};

/// An account that no longer acts shows its face at this opacity
/// (`Avatar.tsx:38`, `off`).
// SDK gap: the kit's face, 0.5 when off; the SDK builds it later
const NOT_ACTING_OPACITY: f32 = 0.5;

// An account's initial, or its picture once read (the kit's `Avatar`):
// round for a person, the agent tint for an agent, square and in the data
// face for a program; faded when it no longer acts.
// SDK gap: the kit's face, 0.5 when off; the SDK builds it later
pub(super) fn face(
    row: &Row,
    picture: Option<std::sync::Arc<view::Image>>,
    size: Face,
    theme: &Theme,
) -> Div {
    let pictured = picture.is_some();
    let avatar = design::avatar_with(&row.name, picture, size, theme);
    let avatar = row.group().face(avatar, pictured, theme);
    avatar.when(design::note(&row.kind).is_some(), |avatar| {
        avatar.opacity(NOT_ACTING_OPACITY)
    })
}

/// An account's name on one line in `role`, faint when it no longer acts,
/// with `you` after the reader's own: secondary, faint, regular, two
/// spaces on (`index.tsx:57-60`, `[number].tsx:84-88`).
pub(super) fn name_line(row: &Row, me: bool, role: Type, ladder: &Ladder, theme: &Theme) -> Div {
    div()
        .flex()
        .items_baseline()
        .min_w_0()
        .text_type(role)
        .when(design::note(&row.kind).is_some(), |line| {
            line.text_color(theme.faint)
        })
        .child(div().min_w_0().truncate().child(row.name.clone()))
        .when(me, |line| {
            line.child(
                div()
                    .flex_none()
                    .text_type(ladder.secondary)
                    .font_weight(FontWeight::NORMAL)
                    .text_color(theme.faint)
                    .child(format!("  {}", words::YOU)),
            )
        })
}

/// One muted status line (the kit's `Quiet`): secondary, 16 all round.
// SDK gap: `quiet` at the ladder's sizes; the SDK builds it later
pub(super) fn quiet(
    id: &'static str,
    text: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    div()
        .id(id)
        .p(space::BLOCK)
        .text_type(ladder.secondary)
        .text_color(theme.muted)
        .child(text.into())
        .into_any_element()
}
