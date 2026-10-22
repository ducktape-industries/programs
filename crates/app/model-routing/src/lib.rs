//! The `model-routing` program: which gateway account each conversation of a caller calls a model
//! provider on. A caller records each gateway's pool and each account that goes full, and a
//! conversation's pin moves only when its account goes full or leaves its pool (`rules.rs`).
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use ::program::AccountNumber;
pub use program::ModelRouting;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{FULL, PINS, POOLS};

pub const PROGRAM: &str = "model-routing";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct GatewayAccount {
    pub service: String,
    pub account: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Until {
    Reset(u64),
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Episode {
    pub until: Until,
    pub since: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Choice {
    Pinned(GatewayAccount),
    Chosen(GatewayAccount),
    Discover(String),
    Wait(Option<u64>),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PoolRow {
    pub provider: String,
    pub service: String,
    pub accounts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct FullRow {
    pub provider: String,
    pub service: String,
    pub account: String,
    pub until: Until,
    pub since: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PinRow {
    pub conversation: u64,
    pub provider: String,
    pub pinned: GatewayAccount,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Which gateway account each of your conversations calls a model provider on, kept in your own namespace (your account). You record each gateway's pool (the accounts an egress service reaches, in the gateway's order) and each account that goes full; Pin gives a conversation the account it calls on and keeps it there until that account goes full or leaves its pool. Nothing is written unless it changes."
)]
pub enum Op {
    #[tool_op(description = "Record the whole pool an egress service reaches for a provider: the account labels in the gateway's order, empty when it holds none. A label named twice, or a blank one, is refused. Written only when it differs from the pool kept; then the full episode of each account it leaves out whose reset has passed is dropped, and an account that comes back keeps an episode still in force.", example = crate::examples::pool)]
    Pool {
        provider: String,
        service: String,
        accounts: Vec<String>,
    },
    #[tool_op(description = "Record that an account refused for its quota (a 429): full until the reset the provider gave (seconds since the epoch), or Unknown when it gave none. Written only when it makes the account full for longer: a first report, a later reset, or a reset where the reset was unknown; a report of the same or an earlier reset, or Unknown while a reset is in force, writes nothing.", example = crate::examples::full)]
    Full {
        provider: String,
        service: String,
        account: String,
        until: Until,
    },
    #[tool_op(description = "Record that a gateway's status, read at read_at (seconds since the epoch), says a full account has room again: its full episode ends, whatever its reset, when the episode began before read_at. Writes nothing when the account is not full on record or its episode began at read_at or later, so a status read before a 429 that lands after it never ends the episode the 429 began.", example = crate::examples::available)]
    Available {
        provider: String,
        service: String,
        account: String,
        read_at: u64,
    },
    #[tool_op(description = "Choose the account a conversation calls the provider on, over the services of the provider's route in the order given, and pin it. The pin stands while its account is in its service's pool and not full. Otherwise the walk starts after a full pinned account (else at the first), wraps, and takes the first account with room; a service whose pool was never recorded is answered Discover before any account past it; with no account with room it answers Wait with the earliest reset on record (null when none is known). Writes only a new pin. Returns the Choice.", returns = crate::Choice, example = crate::examples::pin)]
    Pin {
        conversation: u64,
        provider: String,
        services: Vec<String>,
    },
    #[tool_op(description = "Drop every pin of a conversation that closed.", example = crate::examples::forget)]
    Forget { conversation: u64 },
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
#[ask(ModelRouting)]
pub enum Query {
    #[ask(Reply::Choice(Choice))]
    #[reads(POOLS, FULL, PINS)]
    #[tool_query(description = "What Pin would answer now for a consumer's conversation, without pinning: Pinned when its pin stands, Chosen for the account Pin would move it to, Discover or Wait.", example = crate::examples::resolve)]
    Resolve {
        consumer: AccountNumber,
        conversation: u64,
        provider: String,
        services: Vec<String>,
    },
    #[ask(Reply::Pools(PageResponse<PoolRow>))]
    #[reads(POOLS)]
    #[tool_query(description = "A consumer's recorded pools, by provider and service, each in the gateway's order.", example = crate::examples::pools)]
    Pools {
        consumer: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Fulls(PageResponse<FullRow>))]
    #[reads(FULL)]
    #[tool_query(description = "A consumer's full episodes, by provider, service and account, each with its reset and the chain time (seconds) it began; one whose reset has passed is listed until a later report replaces it, and no longer counts.", example = crate::examples::fulls)]
    Fulls {
        consumer: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Pins(PageResponse<PinRow>))]
    #[reads(PINS)]
    #[tool_query(description = "A consumer's pins, by conversation and provider.", example = crate::examples::pins)]
    Pins {
        consumer: AccountNumber,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Choice(Choice),
    Pools(PageResponse<PoolRow>),
    Fulls(PageResponse<FullRow>),
    Pins(PageResponse<PinRow>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Pool {
            provider,
            service,
            accounts,
        } => (
            format!("Record the {provider} pool of {service}"),
            vec![field("accounts", Value::count(accounts.len() as u64))],
        ),
        Op::Full {
            provider,
            service,
            account,
            ..
        } => (
            format!("Mark {account} on {service} full for {provider}"),
            Vec::new(),
        ),
        Op::Available {
            provider,
            service,
            account,
            ..
        } => (
            format!("Mark {account} on {service} available for {provider}"),
            Vec::new(),
        ),
        Op::Pin {
            conversation,
            provider,
            ..
        } => (
            format!("Pin conversation {conversation} for {provider}"),
            Vec::new(),
        ),
        Op::Forget { conversation } => (
            format!("Forget the pins of conversation {conversation}"),
            Vec::new(),
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Pool", "Full", "Available", "Pin", "Forget"]
    );
}

#[test]
fn query_and_reply_variants_only_append() {
    assert_eq!(
        describe::variants::<Query>(),
        ["Resolve", "Pools", "Fulls", "Pins"]
    );
    assert_eq!(
        describe::variants::<Reply>(),
        ["Choice", "Pools", "Fulls", "Pins"]
    );
}

#[test]
fn until_and_choice_variants_only_append() {
    assert_eq!(describe::variants::<Until>(), ["Reset", "Unknown"]);
    assert_eq!(
        describe::variants::<Choice>(),
        ["Pinned", "Chosen", "Discover", "Wait"]
    );
}
