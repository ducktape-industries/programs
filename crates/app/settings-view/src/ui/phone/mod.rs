//! Expo `app/account/index.tsx` and `src/account/Agents.tsx`: the Account
//! screen as the phone app draws it, the view's second design (owner
//! 2026-10-16: the phone gets its own screens; the model is shared).
//! `lib.rs` picks it once, when the window's pointer is a finger; nothing
//! under here asks again. It draws in the SDK's touch look
//! ([`design::TOUCH`]).
//!
//! It reads the state the desktop reads (`state.rs`), plus the sheet open
//! over it, and the words both designs show (`words.rs`); it writes only
//! through `actions.rs`. It never reaches into the desktop's `ui/` files.
//! One file per Expo source:
//! - `account.rs`: `app/account/index.tsx`, the screen and its Account pane;
//! - `agents.rs`: `src/account/Agents.tsx`, the Agents pane and its sheets;
//! - `parts.rs`: the Expo kit's pieces both draw.
use view::prelude::*;

use crate::Settings;

mod account;
mod agents;
mod parts;

/// The screen, and the sheet drawn over it; Back closes that sheet, and is
/// there exactly while one is drawn.
pub(crate) fn render(view: &Settings, cx: &mut Context<Settings>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::TOUCH;
    let sheet = crate::ui::photo::sheet(view, cx, ladder, &theme)
        .or_else(|| agents::sheet(view, cx, ladder, &theme))
        .or_else(|| crate::ui::policies::sheet(view, cx, ladder, &theme));
    div()
        .id("settings")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_type(ladder.body)
        .when(sheet.is_some(), |root| {
            root.on_back(cx.listener(|v: &mut Settings, _: &BackEvent, _, cx| v.back(cx)))
        })
        .child(account::screen(view, cx, ladder, &theme))
        .children(sheet)
}
