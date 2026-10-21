//! The `wake` program: an account arms a wake on one event (a block's height, a pod's end, a forge
//! ref or change, a files path, a program's code, a clear chat place), and the first time it happens
//! wake tells that account, through attribution, as a `Result` on its object `wake/<id>` (`rules.rs`).
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use ::program::{AccountNumber, ProgramId};
pub use abi::role::egress::Event;
pub use program::Wake;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{BY_ACCOUNT, WAKES};

pub const PROGRAM: &str = "wake";
pub const KIND: &str = "wake";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum WaitFor {
    Height {
        height: u64,
    },
    Pod {
        name: String,
    },
    Message {
        channel: String,
        from: Option<AccountNumber>,
        mentions_me: bool,
    },
    Change {
        repo: String,
        n: u64,
    },
    Ref {
        repo: String,
        reference: String,
    },
    File {
        path: String,
    },
    Settled {
        repo: String,
        n: u64,
        until: Until,
    },
    Program {
        program: ProgramId,
    },
    Clear {
        channel_id: String,
        thread: Option<u64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Until {
    Merged,
    Closed,
    Either,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Key {
    Ref {
        repo: String,
        reference: String,
    },
    Change {
        repo: String,
        n: u64,
    },
    Path {
        path: String,
    },
    Place {
        channel_id: String,
        thread: Option<u64>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Fired {
    Moved {
        repo: String,
        reference: String,
        moved: String,
    },
    Ended {
        repo: String,
        n: u64,
        how: String,
        ending: Until,
    },
    Filed(files::SnapshotRow),
    Cleared {
        channel_id: String,
        thread: Option<u64>,
    },
    LeftQueue {
        repo: String,
        n: u64,
        how: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Firing {
    pub key: Key,
    pub fired: Fired,
    pub lo: u64,
    pub hi: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct WakeRow {
    pub id: u64,
    pub account: AccountNumber,
    pub made: u64,
    pub event: WaitFor,
    pub ended: Option<Ended>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Ended {
    pub height: u64,
    pub text: String,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Wakes an account once on one event: a block's height, a pod of the pods service ending, a forge ref moving, a forge change moving, closing or merging, a files path changing, a program's code changing, or a chat place clearing of everyone else's hold and draft. When it happens the account is told through attribution, as a Result on the object wake/<id>, which ChangesTo reads back."
)]
pub enum Op {
    #[tool_op(description = "Arm a wake for your account on a height, a pod's end, a forge change or ref, a files path, a change settling as until says, a program's next scheduled code change (refused while none is scheduled), or a chat place (a channel's foot, or a thread by its root's seq) holding no one else's hold or draft, not counting those whose holder waits there too (at once when it already is). A chat message is refused: attribution already tells you each message for you. Returns the wake's id.", returns = u64, example = crate::examples::wake_on)]
    WakeOn { event: WaitFor },
    #[tool_op(description = "Cancel an open wake. Its account only.", example = crate::examples::cancel)]
    Cancel { id: u64 },
    #[tool_op(description = "This program's own scheduled entry only: a wake's block has come.")]
    Due,
    #[tool_op(description = "This program's own scheduled entry follows a pod a wake is open on, the block after it is armed; anyone may send it to restart following an open pod wake from the next block.", example = crate::examples::pod_wait)]
    PodWait { id: u64 },
    #[tool_op(
        description = "From the program bound to the egress role only: one event of a pod wake's stream."
    )]
    PodEvent { id: u64, event: Event },
    #[tool_op(description = "From forge only: a ref moved, or a change opened, closed or merged.")]
    Forged(forge::Watched),
    #[tool_op(description = "From files only: a commit, for the wakes on a files path.")]
    Filed(files::SnapshotRow),
    #[tool_op(
        description = "This program's own scheduled entry only: ends the first open wake on a ref, change or path with an id in (lo, hi] as what happened, and books the two halves of the rest for the next block."
    )]
    Fan { firing: Firing },
    #[tool_op(
        description = "From chat only: a hold or a draft ended in a place, for the wakes waiting for it to clear."
    )]
    Placed(chat::PlaceEvent),
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(Wake)]
pub enum Query {
    #[ask(Reply::Wake(Option<WakeRow>))]
    #[reads(WAKES)]
    #[tool_query(description = "One wake: its account, what it waits for, and how it ended. A cancelled wake is gone.", example = crate::examples::one)]
    Wake { id: u64 },
    #[ask(Reply::Wakes(PageResponse<WakeRow>))]
    #[reads(BY_ACCOUNT, WAKES)]
    #[tool_query(description = "An account's wakes, open and ended, oldest first.", example = crate::examples::wakes)]
    Wakes {
        account: AccountNumber,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Wake(Option<WakeRow>),
    Wakes(PageResponse<WakeRow>),
}

pub fn pod_event_as(id: u64) -> Vec<u8> {
    let mut deliver = vec![4];
    deliver.extend(id.to_le_bytes());
    deliver
}

pub fn forged_as() -> Vec<u8> {
    vec![5]
}

pub fn filed_as() -> Vec<u8> {
    vec![6]
}

pub fn placed_as() -> Vec<u8> {
    vec![8]
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let wake = |id: &u64| field("wake", Value::count(*id));
    let (title, fields) = match op {
        Op::WakeOn { event } => (format!("Wake on {}", rules::what(event)), Vec::new()),
        Op::Cancel { id } => (format!("Cancel wake {id}"), vec![wake(id)]),
        Op::Due => ("A wake's block has come".to_owned(), Vec::new()),
        Op::PodWait { id } => (format!("Follow wake {id}'s pod"), vec![wake(id)]),
        Op::PodEvent { id, .. } => (format!("Hear wake {id}'s pod"), vec![wake(id)]),
        Op::Forged(_) => ("Hear a ref or a change forge moved".to_owned(), Vec::new()),
        Op::Filed(_) => ("Hear a files commit".to_owned(), Vec::new()),
        Op::Fan { firing } => (format!("End the next wake up to {}", firing.hi), Vec::new()),
        Op::Placed(_) => ("Hear a place's hold or draft end".to_owned(), Vec::new()),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "WakeOn", "Cancel", "Due", "PodWait", "PodEvent", "Forged", "Filed", "Fan", "Placed"
        ]
    );
}

#[test]
fn wait_for_variants_only_append() {
    assert_eq!(
        describe::variants::<WaitFor>(),
        [
            "Height", "Pod", "Message", "Change", "Ref", "File", "Settled", "Program", "Clear"
        ]
    );
}

#[test]
fn the_tags_are_the_ops_own() {
    let event = Event {
        id: 1,
        seq: 0,
        data: Vec::new(),
    };
    let heard = Op::PodEvent { id: 9, event };
    assert_eq!(abi::encode(&heard)[..9], pod_event_as(9));
    let closed = forge::Watched::Closed {
        repo: "notes".into(),
        n: 1,
        by: ::program::Principal::Root,
    };
    assert_eq!(abi::encode(&Op::Forged(closed))[..1], forged_as());
    let cleared = chat::PlaceEvent::Cleared {
        channel_id: "general".into(),
        thread: None,
    };
    assert_eq!(abi::encode(&Op::Placed(cleared))[..1], placed_as());
}
