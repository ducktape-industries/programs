//! Explorer: the chain as this node lists it. Finalized blocks and the
//! transactions each carries come from `chain.blocks` and `chain.block`,
//! read with no payload; every transaction, newest first, and what each
//! account did, from `activity`; one transaction opened by its place, its
//! whole payload from `chain.tx` and its op's title from
//! `program.describe`; who signed them from `identity`; the validators
//! from `valset`; the programs from `programs`.
//!
//! What the node does not keep is not shown: there is no per-block state
//! root or write set. The block list is the last [`WINDOW`] blocks, read a
//! page at a time and then followed at the head as `chain.heads` pushes
//! it; the transaction list is the frames `activity` recorded, newest
//! first, read a page at a time as it is scrolled. A write to activity
//! reads a list's first page again in place while that is all it holds,
//! and tells a list read on past it that newer rows landed above it
//! (`Explorer::reread_first` in `load.rs`). A row in any transaction list,
//! a block's own included, shows only what its read carries: how the
//! transaction ran, its program, its signer's account and its place. An
//! op's title shows only on that transaction's page.
//!
//! The state is one struct (`state.rs`) and the window (`chain.rs`). Reads
//! are typed (`queries.rs`) and loaded in `load.rs`, re-read on the live
//! heads `watch.rs` follows; where the reader goes lands in `actions.rs`;
//! what the reader changes on the Programs page, by account or by vote, in
//! `acting.rs`; the sentences both designs show, once, in `words.rs`. `ui/`
//! draws the desktop's pages and `ui/phone/` the phone's screens, picked
//! once in [`Explorer`]'s render.
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
