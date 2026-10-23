//! Explorer: the chain as this node lists it. Block facts come from the window (`chain.facts`, the
//! last [`WINDOW`] blocks, followed at the head), a list's transactions a range at a time from
//! `chain.txs`, one transaction whole from `chain.tx`, the rest from activity and the system programs.
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

pub use chain::{BlockRow, Chain, Shown, TxList, TxRow};
pub use state::{Accounts, Explorer, Network, Note, Route};

use view::prelude::*;

pub const WINDOW: usize = 1_000;

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
        Capability::Blob,
        Capability::Host,
        Capability::Clock,
        Capability::Clipboard,
        Capability::Link,
    ];
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
        self.shown.wanted.clear();
        self.width = window.viewport_size().width.into();
        self.touch = window.touch();
        let tall: f32 = window.viewport_size().height.into();
        let row: f32 = ui::ROW_H.into();
        self.rows_fit = (tall / row).ceil() as usize;
        self.read_manifest(cx);
        self.read_latest(cx);
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
