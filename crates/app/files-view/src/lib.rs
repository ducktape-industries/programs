//! Files: duckfs, the network's versioned filesystem (the `files`
//! program), as a file browser. A folder lists its entries at the newest
//! snapshot, a file shows its picture or its text (else its size), and
//! the history lists the snapshots every commit made and the pins that
//! name some of them. Where the access program lets the reader write
//! (anywhere no write list covers, else where the nearest list allows
//! them), they make folders and text files, with an optional write list of
//! their own set in access once the commit lands, upload files from their
//! device, edit text, rename, move and remove, each one commit with a
//! message.
//!
//! The state is one struct (`state.rs`) and a stack of pages both designs
//! draw. Reads are typed (`queries.rs`) and loaded in `load.rs`, read again
//! on every commit `watch.rs` hears of; where the reader goes (a link
//! opened into Files too) is `actions.rs`, what they write `write.rs`,
//! what a right press offers `menu.rs`; every sentence is in `words.rs`.
//! `ui/` draws the desktop's pages, `ui/phone/` the phone's screens: the
//! root picks one, once, by the device. Element ids: a container is
//! `files-<name>`, a control or a row `files/<what>/<key>`, or
//! `files/<what>:<path>` where a path is the key.
mod actions;
mod load;
mod menu;
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

/// What both designs draw in.
pub(crate) type Cx<'a, 'b> = &'a mut Context<'b, Files>;

/// What both designs draw with: the theme, and the ladder of sizes, the
/// pointer's (`design::POINTER`) or the finger's (`design::TOUCH`).
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
        // the session (who reads, so which home is theirs) and the links
        // opened into the view
        Capability::Host,
        // files' reads and live heads, and identity's names
        Capability::Program,
        // a commit, and an upload's chunks
        Capability::Op,
        // the files an upload sends, as the reader picks them
        Capability::File,
        // a row's Copy link
        Capability::Clipboard,
    ];
    const TARGETS: &'static [&'static str] = &[files::PROGRAM, access::PROGRAM, identity::PROGRAM];
    /// The history's rows, a message beside who made it and when, fit
    /// here; a folder's crumbs wrap.
    const MIN_WINDOW_WIDTH: u32 = 480;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.watch(cx);
        // what landed while the view was away: the follow hears only what
        // lands from now
        self.read_all(cx);
    }
}

impl Render for Files {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.width = window.viewport_size().width.into();
        // two designs, picked once: under a finger the phone's screens
        // (`ui/phone`), under a pointer the desktop's pages
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
