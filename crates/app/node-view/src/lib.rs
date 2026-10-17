//! Nodes: one sheet for the network this app talks to. The connected node's
//! own numbers head it (height, last block, block time, the epoch), then
//! every member once, validators then residents: its key and address, and
//! for a validator the blocks it led of the last 64 and how far its newest
//! finalize vote is from this node's tip as `chain.network` reports it (one
//! status word: In sync, N behind, Quiet). A resident's sync state is not
//! reported: its row is its key and address. A node that is not voting (it
//! reports no vote of its own) hears none, so then no validator's is
//! reported either. A node that does not serve `chain.network` says so in
//! one line, and the validators' cells stay empty.
//!
//! The members are valset's (`queries.rs`), each with who operates it as
//! identity says, re-read on its live heads; the status, the network and
//! the strip's blocks (`recent.rs`) follow the clock, one ask of each in
//! flight at a time. A reader whose seated key votes for a validator gets
//! the vote (`voting.rs`, drawn by `motions.rs`): motions on each row and
//! the door, the motions that wait, and the scheduled changes with who
//! made each. A program is changed in Explorer's Programs.
//!
//! Under a finger the view is the Expo app's Nodes tab instead
//! (`ui/phone/`): the list, a member's own page, the door and a press that
//! enacts as sheets; the desktop's sheet is unchanged. Every word both
//! show is `words.rs`'s.
use programs::Programs;
use serde::{Deserialize, Serialize};
use valset::Valset;
use view::methods;
use view::prelude::*;

mod motions;
mod queries;
mod recent;
mod row;
mod table;
mod ui;
mod voting;
mod words;

use queries::Node;
use recent::Recent;
use row::Row;
use view::design::Pages;
use voting::Voting;

#[derive(Serialize, Deserialize, Default)]
pub struct Nodes {
    /// the connected node's own status
    pub(crate) status: Loadable<NodeStatus>,
    pub(crate) nodes: Loadable<Vec<Node>>,
    /// each validator's newest finalize vote as the connected node heard
    /// it; `Failed` only where it does not serve `chain.network`
    pub(crate) network: Loadable<NetworkStatus>,
    pub(crate) recent: Recent,
    /// the member whose page is open over the list on the phone
    pub(crate) open: Option<Vec<u8>>,
    /// the door's sheet is up (the phone's door control)
    #[serde(skip)]
    pub(crate) door: bool,
    /// the chain the host session is on, once it says
    #[serde(skip)]
    pub(crate) chain: Option<String>,
    /// clock ticks since the view opened, and the tick the node last
    /// answered its status on and the one its height last moved on
    #[serde(skip)]
    pub(crate) ticks: u64,
    #[serde(skip)]
    pub(crate) answered: u64,
    #[serde(skip)]
    pub(crate) moved: u64,
    /// the pane's width, read from the window each render
    #[serde(skip)]
    pub(crate) width: f32,
    /// the asks in flight: the clock asks again only once one lands
    #[serde(skip)]
    pub(crate) asking: Asking,
    /// the reader's account name, as identity names it, for the first
    /// page's avatar; empty while it is read, and where the session has
    /// no account or identity profiles none
    #[serde(skip)]
    pub(crate) reader: String,
    /// the session's account `reader` is read for
    #[serde(skip)]
    pub(crate) reader_account: Option<u64>,
    /// the read of `reader`: a newer session's replaces and cancels it
    #[serde(skip)]
    pub(crate) reading_reader: Option<Task<()>>,
    /// the vote, read again on every open
    #[serde(skip)]
    pub(crate) voting: Voting,
}

/// A screen of the view's [`Pages`]: the list, or one member's page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Screen {
    Nodes,
    Node(Vec<u8>),
}

/// Why no validator's cells are filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unheard {
    /// this node hears no votes: it reports none of its own
    NotVoting,
    /// this node does not serve `chain.network`
    NoNetwork,
}

#[derive(Default)]
pub(crate) struct Asking {
    status: bool,
    network: bool,
    blocks: bool,
}

/// How often the node is asked, in milliseconds.
const TICK: i64 = 1_000;
/// Ticks without a status answer before the node reads Not answering.
pub(crate) const SILENT_TICKS: u64 = 3;

impl View for Nodes {
    const NAME: &'static str = "Nodes";
    const DESCRIPTION: &'static str = "The network this app talks to: its height and epoch, every member, with how far each validator's signature is from the tip, and the validators' vote.";
    const ICON: &'static str = "icons/server.svg";
    const TINT: Tint = Tint::Blue;
    const CAPABILITIES: &'static [Capability] = &[
        Capability::Chain,
        Capability::Program,
        Capability::Op,
        Capability::Host,
        Capability::Clock,
    ];
    const TARGETS: &'static [&'static str] =
        &[valset::PROGRAM, programs::PROGRAM, identity::PROGRAM];
    // the set's table fits from 480 up
    const MIN_WINDOW_WIDTH: u32 = 480;

    fn attach(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<Changes<Valset>>((), |view, head, cx| match head {
            Ok(_) => {
                view.read(cx);
                view.read_voices(cx);
            }
            Err(refusal) => cx.log_refused("valset's live heads", &refusal),
        })
        .detach();
        cx.follow::<HostSession>((), |view, session, cx| match session {
            Ok(session) => view.session_changed(session, cx),
            Err(refusal) => cx.log_refused("the session", &refusal),
        })
        .detach();
        // a change scheduled, cancelled or landed
        cx.follow::<Changes<Programs>>((), |view, head, cx| match head {
            Ok(_) => view.read_programs(cx),
            Err(refusal) => cx.log_refused("the programs role's live heads", &refusal),
        })
        .detach();
        // the header's age is drawn from the ticks
        cx.follow::<ClockTicks>(TICK, |view, tick, cx| match tick {
            Ok(()) => {
                view.ticks += 1;
                cx.notify();
                view.read_status(cx);
                view.read_network(cx);
            }
            Err(refusal) => cx.log_refused("the clock", &refusal),
        })
        .detach();
        self.read(cx);
        self.read_status(cx);
        self.read_network(cx);
    }
}

impl Render for Nodes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // the pane's width, as the host lays the frame out: the table fits
        // to it in the frame that shows it
        self.width = window.viewport_size().width.into();
        // the one pick between the two designs: a finger gets the phone's
        // screens (the Expo app's), a pointer the desktop's sheet
        if window.touch() {
            ui::phone::render(self, cx).into_any_element()
        } else {
            ui::render(self, cx).into_any_element()
        }
    }
}

impl Nodes {
    /// One read of the members — the boot, a retry, a restore, a live
    /// head. What is already on screen stays there while it runs.
    pub(crate) fn read(&mut self, cx: &mut Context<Self>) {
        let work = queries::nodes(cx.host());
        cx.load(self, work, |view| &mut view.nodes);
    }

    /// The node's status. A refused re-read keeps the numbers on screen;
    /// the header says Not answering once [`SILENT_TICKS`] pass without one.
    pub(crate) fn read_status(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.asking.status, true) {
            return;
        }
        let ask = cx.host().ask::<ChainStatus>(());
        cx.land(ask, |view, answer, cx| view.status_landed(answer, cx))
            .detach();
    }

    fn status_landed(&mut self, answer: Result<NodeStatus, Error>, cx: &mut Context<Self>) {
        self.asking.status = false;
        match answer {
            Ok(status) => {
                if self.status.ready().map(|now| now.height) != Some(status.height) {
                    self.moved = self.ticks;
                }
                self.answered = self.ticks;
                self.status = Loadable::Ready(status);
                self.pull(cx);
            }
            Err(refusal) if self.status.ready().is_none() => {
                self.status = Loadable::Failed(refusal)
            }
            Err(refusal) => cx.log_refused("the node's status", &refusal),
        }
        cx.notify();
    }

    /// Each validator's newest finalize vote as the node heard it. A node
    /// that does not serve it (`unknown_request`) is logged once and the
    /// sheet says so until an answer; any other refusal (a node restarting,
    /// a dropped link) is logged and keeps the last answer, as the status
    /// does.
    pub(crate) fn read_network(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.asking.network, true) {
            return;
        }
        let ask = cx.host().ask::<ChainNetwork>(());
        cx.land(ask, move |view, answer, cx| {
            view.asking.network = false;
            match answer {
                Ok(network) => view.network = Loadable::Ready(network),
                // a node that does not serve `chain.network`
                Err(refusal) if refusal.code == methods::refusal::UNKNOWN_REQUEST => {
                    if view.network.failed().is_none() {
                        cx.log_refused("the validators' votes", &refusal);
                    }
                    view.network = Loadable::Failed(refusal);
                }
                Err(refusal) => cx.log_refused("the validators' votes", &refusal),
            }
            cx.notify();
        })
        .detach();
    }

    /// Reads the next page the strip wants at the node's height, if any,
    /// and on until it wants none; a refusal waits for the next status.
    fn pull(&mut self, cx: &mut Context<Self>) {
        let Some(head) = self.status.ready().map(|status| status.height) else {
            return;
        };
        if self.asking.blocks {
            return;
        }
        let Some(page) = self.recent.next(head) else {
            return;
        };
        self.asking.blocks = true;
        let ask = cx.host().ask::<ChainBlocks>(page.clone());
        cx.land(ask, move |view, answer, cx| {
            view.asking.blocks = false;
            match answer {
                Ok(blocks) => {
                    view.recent.land(head, blocks);
                    // a page that filled nothing is not asked again
                    // until the next status
                    let wanted = page.before.map_or(head, |before| before - 1);
                    if view.recent.led.contains_key(&wanted) {
                        view.pull(cx);
                    }
                }
                Err(refusal) => cx.log_refused("the recent blocks", &refusal),
            }
            cx.notify();
        })
        .detach();
    }

    /// Where the reader is: the list, and the member whose page is open
    /// over it; read off `open`, so the stack and the data cannot disagree.
    /// The phone draws the top page alone; the desktop has every member in
    /// its table and draws no page.
    pub(crate) fn pages(&self) -> Pages<Screen> {
        let mut pages = Pages::root(Screen::Nodes);
        if let Some(key) = &self.open {
            pages.push(Screen::Node(key.clone()));
        }
        pages
    }

    /// The phone's Back: the confirm sheet closes first, then the door's,
    /// then the member's page.
    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        if self.voting.confirming.is_some() {
            self.voting.confirming = None;
        } else if self.door {
            self.door = false;
        } else {
            self.close_page();
        }
        cx.notify();
    }

    /// The member's page closes, and a landed vote's message pressed on it
    /// goes with it.
    pub(crate) fn close_page(&mut self) {
        let Some(page) = self.open.take() else {
            return;
        };
        let landed_here = matches!(self.voting.outcome, Some(Ok(_)))
            && self.voting.pressed_on.as_ref() == Some(&page);
        if landed_here {
            self.voting.outcome = None;
        }
    }

    /// Whether the node answered its status within [`SILENT_TICKS`].
    pub(crate) fn answering(&self) -> bool {
        self.status.ready().is_some() && self.ticks - self.answered <= SILENT_TICKS
    }

    /// The votes, where this node hears them: only a node seated as a
    /// validator does, its own among them; without its own, it hears none
    /// (a resident, one not seated yet, one catching up after a restart).
    pub(crate) fn heard(&self) -> Option<&NetworkStatus> {
        let this = self.status.ready().map(|status| status.identity.as_slice());
        self.network.ready().filter(|network| {
            network
                .members
                .iter()
                .any(|peer| this == Some(peer.key.as_slice()) && peer.signed.is_some())
        })
    }

    /// A member's cells as this node heard them: blank where it hears no
    /// votes.
    pub(crate) fn cells(&self, node: &Node) -> Row {
        self.heard()
            .map_or(row::BLANK, |network| row::synced(node, network))
    }

    /// Whether `node` is the node this app talks to.
    pub(crate) fn is_this(&self, node: &Node) -> bool {
        self.status
            .ready()
            .is_some_and(|status| status.identity == node.key)
    }

    /// A member's address, and `this node` after the one this app talks to.
    pub(crate) fn address_line(&self, node: &Node) -> String {
        let address = node.shown_address();
        match self.is_this(node) {
            true => format!("{address} · {}", words::THIS_NODE),
            false => address.to_owned(),
        }
    }

    /// The changes scheduled past the tip, once the schedule is read; one
    /// at or below the tip has landed.
    pub(crate) fn scheduled_ahead(&self) -> Option<Vec<&programs::Pending>> {
        let schedule = self.voting.scheduled.ready()?;
        let tip = self.status.ready().map_or(0, |status| status.height);
        let ahead = schedule.changes.iter().filter(|change| change.height > tip);
        Some(ahead.collect())
    }

    /// Retry on a refused status: read it again from nothing.
    pub(crate) fn retry_status(&mut self, cx: &mut Context<Self>) {
        self.status = Loadable::Idle;
        self.read_status(cx);
        cx.notify();
    }

    /// Why the validators' cells are empty, once the node said anything of
    /// the network.
    pub(crate) fn unheard(&self) -> Option<Unheard> {
        match &self.network {
            Loadable::Failed(_) => Some(Unheard::NoNetwork),
            network if network.ready().is_some() && self.heard().is_none() => {
                Some(Unheard::NotVoting)
            }
            _ => None,
        }
    }
}

export_view!(Nodes);

#[cfg(test)]
mod tests;
