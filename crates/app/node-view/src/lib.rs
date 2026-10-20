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
    pub(crate) status: Loadable<NodeStatus>,
    pub(crate) nodes: Loadable<Vec<Node>>,
    pub(crate) network: Loadable<NetworkStatus>,
    pub(crate) recent: Recent,
    pub(crate) open: Option<Vec<u8>>,
    #[serde(skip)]
    pub(crate) door: bool,
    #[serde(skip)]
    pub(crate) chain: Option<String>,
    #[serde(skip)]
    pub(crate) ticks: u64,
    #[serde(skip)]
    pub(crate) answered: u64,
    #[serde(skip)]
    pub(crate) moved: u64,
    #[serde(skip)]
    pub(crate) width: f32,
    #[serde(skip)]
    pub(crate) asking: Asking,
    #[serde(skip)]
    pub(crate) reader: String,
    #[serde(skip)]
    pub(crate) reader_account: Option<u64>,
    #[serde(skip)]
    pub(crate) reading_reader: Option<Task<()>>,
    #[serde(skip)]
    pub(crate) voting: Voting,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Screen {
    Nodes,
    Node(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unheard {
    NotVoting,
    NoNetwork,
}

#[derive(Default)]
pub(crate) struct Asking {
    status: bool,
    network: bool,
    blocks: bool,
}

const TICK: i64 = 1_000;
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
        cx.follow::<Changes<Programs>>((), |view, head, cx| match head {
            Ok(_) => view.read_programs(cx),
            Err(refusal) => cx.log_refused("the programs role's live heads", &refusal),
        })
        .detach();
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
        self.width = window.viewport_size().width.into();
        if window.touch() {
            ui::phone::render(self, cx).into_any_element()
        } else {
            ui::render(self, cx).into_any_element()
        }
    }
}

impl Nodes {
    pub(crate) fn read(&mut self, cx: &mut Context<Self>) {
        let work = queries::nodes(cx.host());
        cx.load(self, work, |view| &mut view.nodes);
    }

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

    pub(crate) fn read_network(&mut self, cx: &mut Context<Self>) {
        if std::mem::replace(&mut self.asking.network, true) {
            return;
        }
        let ask = cx.host().ask::<ChainNetwork>(());
        cx.land(ask, move |view, answer, cx| {
            view.asking.network = false;
            match answer {
                Ok(network) => view.network = Loadable::Ready(network),
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

    pub(crate) fn pages(&self) -> Pages<Screen> {
        let mut pages = Pages::root(Screen::Nodes);
        if let Some(key) = &self.open {
            pages.push(Screen::Node(key.clone()));
        }
        pages
    }

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

    pub(crate) fn answering(&self) -> bool {
        self.status.ready().is_some() && self.ticks - self.answered <= SILENT_TICKS
    }

    pub(crate) fn heard(&self) -> Option<&NetworkStatus> {
        let this = self.status.ready().map(|status| status.identity.as_slice());
        self.network.ready().filter(|network| {
            network
                .members
                .iter()
                .any(|peer| this == Some(peer.key.as_slice()) && peer.signed.is_some())
        })
    }

    pub(crate) fn cells(&self, node: &Node) -> Row {
        self.heard()
            .map_or(row::BLANK, |network| row::synced(node, network))
    }

    pub(crate) fn is_this(&self, node: &Node) -> bool {
        self.status
            .ready()
            .is_some_and(|status| status.identity == node.key)
    }

    pub(crate) fn address_line(&self, node: &Node) -> String {
        let address = node.shown_address();
        match self.is_this(node) {
            true => format!("{address} · {}", words::THIS_NODE),
            false => address.to_owned(),
        }
    }

    pub(crate) fn scheduled_ahead(&self) -> Option<Vec<&programs::Pending>> {
        let schedule = self.voting.scheduled.ready()?;
        let tip = self.status.ready().map_or(0, |status| status.height);
        let ahead = schedule.changes.iter().filter(|change| change.height > tip);
        Some(ahead.collect())
    }

    pub(crate) fn retry_status(&mut self, cx: &mut Context<Self>) {
        self.status = Loadable::Idle;
        self.read_status(cx);
        cx.notify();
    }

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
