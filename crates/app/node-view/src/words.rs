//! Every word both of the view's trees show, once (Expo
//! `src/network/words.ts` and the Nodes screens' own strings): the reads'
//! states, the section heads, the pulse, the epoch, the vote's labels and
//! captions, why the validators' cells are empty, and the footnote. The
//! desktop's sheet (`ui.rs`, `table.rs`, `motions.rs`) and the phone's
//! screens (`ui/phone/`) read them here; a sentence only one tree shows
//! stays in that tree. What a motion does in words is `voting.rs`'s
//! (`words`, and the labels in `motions_on`), what a vote came to
//! `voting::said`'s, and a validator's status word `row.rs`'s.
use programs::Pending as Scheduled;
use valset::Pending;
use view::prelude::*;

use crate::Unheard;
use crate::queries::Schedule;
use crate::voting::{self, Voting};

pub(crate) const NODES: &str = "Nodes";
pub(crate) const DOOR: &str = "The door";
pub(crate) const NO_VALUE: &str = "—";
pub(crate) const READING_STATUS: &str = "Reading node status…";
pub(crate) const READING_MEMBERS: &str = "Reading the members…";
pub(crate) const NO_MEMBERS: &str = "No members";
pub(crate) const NO_MEMBERS_WHY: &str = "The validator set of this network is empty.";
pub(crate) const KEY: &str = "Key";
pub(crate) const ADDRESS: &str = "Address";
pub(crate) const HEIGHT: &str = "Height";
pub(crate) const BEHIND: &str = "Behind";
pub(crate) const STATUS: &str = "Status";
pub(crate) const BLOCK_TIME: &str = "Block time";
pub(crate) const VALIDATORS: &str = "Validators";
pub(crate) const RESIDENTS: &str = "Residents";
pub(crate) const THIS_NODE: &str = "this node";
pub(crate) const PROPOSED: &str = "Proposed";
pub(crate) const DOESNT_PROPOSE: &str = "Doesn't propose";
pub(crate) const VOTED: &str = "voted";
pub(crate) const NO_NETWORK: &str =
    "This node doesn't report its validators' votes. Update the node.";
pub(crate) const NOT_VOTING: &str =
    "This node isn't voting right now, so it can't see the validators' votes.";

pub(crate) const MOTIONS: &str = "Motions";
pub(crate) const READING_MOTIONS: &str = "Reading the motions…";
pub(crate) const NO_MOTION: &str = "No motion waits for votes.";
pub(crate) const VOTE: &str = "Vote";
pub(crate) const SCHEDULED: &str = "Scheduled changes";
pub(crate) const READING_SCHEDULE: &str = "Reading the schedule…";
pub(crate) const NO_CHANGE: &str = "No change is scheduled.";
pub(crate) const PROPOSE_CANCEL: &str = "Propose cancel";
pub(crate) const CHANGE_ELSEWHERE: &str = "Change a program in Explorer, Programs.";
pub(crate) const ENACTS_NOW: &str =
    "Your vote reaches the quorum: this happens now, and a vote can't be taken back.";
pub(crate) const VOTE_AND_ENACT: &str = "Vote and enact";
pub(crate) const NOT_NOW: &str = "Not now";
pub(crate) const DISMISS: &str = "Dismiss";

pub(crate) fn door(open: bool) -> Door {
    match open {
        true => Door {
            label: "Open the door",
            meaning: "anyone enrolls without an invite",
        },
        false => Door {
            label: "Close the door",
            meaning: "enrolling takes an invite",
        },
    }
}

pub(crate) struct Door {
    pub(crate) label: &'static str,
    pub(crate) meaning: &'static str,
}

pub(crate) fn pulse(answering: bool) -> &'static str {
    match answering {
        true => "In sync",
        false => "Not answering",
    }
}

pub(crate) fn block_age(seconds: u64) -> String {
    match seconds {
        0 => "just now".to_owned(),
        seconds => format!("{} ago", design::ago(seconds * 1000, 0)),
    }
}

pub(crate) fn last_answer(seconds: u64) -> String {
    format!("Last answer {} ago", design::ago(seconds * 1000, 0))
}

pub(crate) fn unheard(why: Unheard) -> &'static str {
    match why {
        Unheard::NotVoting => NOT_VOTING,
        Unheard::NoNetwork => NO_NETWORK,
    }
}

pub(crate) fn footnote() -> String {
    format!(
        "Height: the newest block the validator voted to finalize. Quiet: none for {}.",
        design::plural(crate::row::QUIET, "block", "blocks")
    )
}

pub(crate) fn span(span: &std::ops::RangeInclusive<u64>) -> String {
    format!(
        "last {} · {} → {}",
        design::plural(span.end() - span.start() + 1, "block", "blocks"),
        design::grouped(*span.start()),
        design::grouped(*span.end()),
    )
}

pub(crate) fn network(chain_id: &str) -> &str {
    chain_id
        .split_once('#')
        .map_or(chain_id, |(label, _)| label)
}

pub(crate) struct EpochPlace {
    pub(crate) epoch: u64,
    pub(crate) into: u64,
    pub(crate) length: u64,
}

impl EpochPlace {
    pub(crate) fn of(status: &NodeStatus) -> Option<Self> {
        let length = status.epoch_length;
        (length > 0).then(|| Self {
            epoch: status.epoch,
            into: (status.height + 1) % length,
            length,
        })
    }

    pub(crate) fn place(&self) -> String {
        format!(
            "Epoch {} · {} / {}",
            design::grouped(self.epoch),
            self.into,
            self.length
        )
    }

    pub(crate) fn next(&self) -> String {
        format!(
            "Next epoch in {}",
            design::plural(self.length - self.into, "block", "blocks")
        )
    }
}

pub(crate) fn pending_caption(voting: &Voting, pending: &Pending, needed: u64) -> String {
    let cast = voting.voices.len() - voting.uncast(&pending.motion);
    format!(
        "{} of {needed} · {}",
        pending.voters.len(),
        yours(cast, voting.voices.len())
    )
}

fn yours(cast: usize, voices: usize) -> String {
    match (cast, voices) {
        (0, 1) => "yours has not voted".into(),
        (1, 1) => "yours voted".into(),
        (cast, voices) => format!("{cast} of your {voices} voted"),
    }
}

pub(crate) fn scheduled_caption(
    change: &Scheduled,
    schedule: &Schedule,
    status: Option<&NodeStatus>,
) -> String {
    let by = match change.by {
        Some(account) => voting::named(&schedule.names, account),
        None => "vote".into(),
    };
    let Some(status) = status else {
        return format!("by {by}");
    };
    let blocks = change.height.saturating_sub(status.height);
    match change.by {
        Some(_) => format!(
            "by {by} · validators can cancel for {} min",
            (blocks * status.block_time_ms).div_ceil(60_000)
        ),
        None => format!("by {by} · in {}", design::plural(blocks, "block", "blocks")),
    }
}
