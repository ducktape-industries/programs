//! Account: who the seated key is (its account, its keys, the agents it
//! manages) and invites to the network, one pane each behind a left menu.
//! Under a finger it is the Expo app's Account screen instead: the same
//! panes under a strip, an agent's forms in sheets (`ui/phone/`). The
//! program is still `settings` (`programs`'s view); only its name on
//! screen is Account. The node's own status is the Nodes view's, and the
//! app's preferences the host's gear.
//!
//! The state is one struct (`state.rs`). Reads are typed asks of identity,
//! valset and access (`queries.rs`), re-read on their live heads (`watch.rs`);
//! presses land in `actions.rs`; `ui/` (the desktop) and `ui/phone/` (the
//! phone) draw and never mutate, both worded by `words.rs`.
mod actions;
mod policies;
mod queries;
mod state;
mod ui;
mod watch;
mod words;

pub use state::Settings;

use view::prelude::*;

impl View for Settings {
    const NAME: &'static str = "Account";
    const DESCRIPTION: &'static str =
        "Your account, its keys, the agents it manages, your policies, and invites.";
    const ICON: &'static str = "icons/circle-user.svg";
    const TINT: Tint = Tint::Pink;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Program,
        Capability::Op,
        Capability::Invite,
        Capability::Host,
        Capability::Clipboard,
    ];
    // at 480 the Agents page's "Create agent" and "Add key" are cut
    const MIN_WINDOW_WIDTH: u32 = 560;

    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            // an invite lasts a week unless the reader picks otherwise
            ttl: 1,
            ..Self::default()
        }
    }

    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.watch(cx);
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // one wasm, two designs, the model shared (owner 2026-10-16: the
        // phone gets its own screens): a finger gets the Expo app's, a
        // pointer the desktop's
        if window.touch() {
            ui::phone::render(self, cx).into_any_element()
        } else {
            ui::render(self, cx).into_any_element()
        }
    }
}

export_view!(Settings);

#[cfg(test)]
mod tests;
