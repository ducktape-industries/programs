//! Files: duckfs (the `files` program) as a file browser: folders, files, the snapshot history and
//! its pins, and the writes a path's guard lets the reader make (`policies.rs`). State `state.rs`,
//! reads `queries.rs`/`load.rs`/`watch.rs`, writes `write.rs`, words `words.rs`, pages `ui/`.
mod actions;
mod load;
mod menu;
mod policies;
mod queries;
mod state;
mod ui;
mod upload;
mod watch;
mod words;
mod write;

pub use state::Files;

use view::design::Ladder;
use view::prelude::*;

pub(crate) type Cx<'a, 'b> = &'a mut Context<'b, Files>;

#[derive(Clone, Copy)]
pub(crate) struct Kit {
    pub(crate) theme: Theme,
    pub(crate) ladder: Ladder,
}

impl View for Files {
    const NAME: &'static str = "Files";
    const DESCRIPTION: &'static str =
        "The network's files: your home, what is shared, and every snapshot.";
    const ICON: &'static str = "icons/folder.svg";
    const TINT: Tint = Tint::Amber;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Host,
        Capability::Program,
        Capability::Op,
        Capability::File,
        Capability::Clipboard,
    ];
    const MIN_WINDOW_WIDTH: u32 = 480;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.watch(cx);
        self.read_all(cx);
    }
}

impl Render for Files {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.width = window.viewport_size().width.into();
        self.settle_policy(cx);
        if window.touch() {
            ui::phone::render(self, cx)
        } else {
            ui::render(self, cx)
        }
    }
}

export_view!(Files);

#[cfg(test)]
mod tests;
