//! Account (the `settings` program's view): the seated key's account, its
//! picture and keys, its agents, policies and invites; `ui/` draws for a
//! pointer and `ui/phone/` for a finger, from one state (`state.rs`).
mod actions;
mod avatar;
mod photo;
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
        Capability::File,
        Capability::Blob,
    ];
    const MIN_WINDOW_WIDTH: u32 = 560;

    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            ttl: 1,
            ..Self::default()
        }
    }

    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.pictures.is_none() {
            self.pictures = Some(cx.new(|_| view::pictures::Pictures::default()));
        }
        self.ask_hosted(cx);
        self.watch(cx);
    }
}

impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
