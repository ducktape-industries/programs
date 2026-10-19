//! Explorer: the chain as this node lists it. Blocks and their transactions come from the window
//! (`chain.blocks`, the last [`WINDOW`] blocks, followed at the head); one transaction's payload from
//! `chain.tx`; what an account did from `activity`; the rest from the system programs.
mod acting;
mod actions;
mod chain;
mod decode;
mod load;
mod queries;
mod state;
pub(crate) mod ui;
mod watch;
mod words;

pub use chain::{BlockRow, Chain, TxRow};
pub use state::{Accounts, Explorer, Network, Note, Route};

use view::prelude::*;

pub const WINDOW: usize = 1_000;

/// What both designs draw in.
pub(crate) type Cx<'a, 'b> = &'a mut Context<'b, Explorer>;

impl View for Explorer {
    const NAME: &'static str = "Explorer";
    const DESCRIPTION: &'static str =
        "The chain as this node keeps it: blocks, transactions, accounts and programs.";
    const ICON: &'static str = "icons/compass.svg";
    const TINT: Tint = Tint::Lime;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Chain,
        Capability::Program,
        Capability::Op,
        // what a program's code declares, on its page (`blob.manifest`), and
        // each account's picture (`blob.get`)
        Capability::Blob,
        Capability::Host,
        Capability::Clock,
        Capability::Clipboard,
        // an account page's Open DM opens chat on the reader's DM room
        Capability::Link,
    ];
    // at 560 a block's hash is cut at the right edge
    const MIN_WINDOW_WIDTH: u32 = 640;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.pictures.is_none() {
            self.pictures = Some(cx.new(|_| view::pictures::Pictures::default()));
        }
        self.watch(cx);
        self.read_all(cx);
    }
}

impl Render for Explorer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the pane's width, as the host lays the frame out: the Overview
        // lays its panels out to it in the frame that shows them
        self.width = window.viewport_size().width.into();
        self.touch = window.touch();
        // a program page asks what its code declares once the roster
        // names the code, whichever lands first
        self.read_manifest(cx);
        // two designs, picked once: under a finger the Expo app's screens
        // (`ui/phone`), under a pointer the desktop's pages
        if self.touch {
            self.read_rows(cx);
            ui::phone::render(self, cx)
        } else {
            ui::render(self, cx)
        }
    }
}

export_view!(Explorer);

#[cfg(test)]
mod tests;
