//! The `resources` program: kinds of resource with their owner's policy, the offers
//! providers make of them, the requests callers make, the assignments the controller places with
//! their reason, and the use each assignment met (README.md). Its rules are in `rules.rs`.
#[cfg(test)]
mod contract;
mod examples;
mod program;
mod rules;
mod state;

pub use ::program::{AccountNumber, BlobId, ProgramId};
pub use ownership::PolicyId;
pub use program::Resources;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "resources";

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct OfferId {
    pub provider: AccountNumber,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Counter {
    Provider,
    Requester,
    Chain,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Meter {
    pub name: String,
    pub unit: String,
    pub reported_by: Counter,
    pub checked_by: Option<Counter>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct KindSpec {
    pub meters: Vec<Meter>,
    pub slot: String,
    pub lapse: Option<u64>,
    pub notary: Option<String>,
    pub walk: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct KindRow {
    pub name: String,
    pub owner: AccountNumber,
    pub spec: KindSpec,
    pub registered: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reach {
    Egress { service: String, node: Vec<u8> },
    Fleet { machine: String },
    Direct,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Terms {
    pub serves: Option<PolicyId>,
    pub notarizes: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Isolation {
    None,
    Container,
    Landlock { abi: u32 },
    Seatbelt,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Offer {
    pub name: String,
    pub kind: String,
    pub labels: Vec<(String, String)>,
    pub capacity: u64,
    pub reach: Reach,
    pub terms: Terms,
    pub isolation: Isolation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum OfferState {
    Ready,
    Missed,
    Lapsed,
    Draining,
    Gone,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct OfferRow {
    pub provider: AccountNumber,
    pub offer: Offer,
    pub state: OfferState,
    pub busy: u64,
    pub idle_since: Option<u64>,
    pub since: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct BeatRow {
    pub height: u64,
    pub free: u64,
    pub readings: Vec<(String, u64)>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct OfferView {
    pub offer: OfferRow,
    pub beat: Option<BeatRow>,
    pub admitted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct AdmittedRow {
    pub provider: AccountNumber,
    pub by: AccountNumber,
    pub height: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Selector {
    Is(String, String),
    Has(String),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Pin {
    Only(OfferId),
    Prefer(OfferId),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Needs {
    pub units: u64,
    pub labels: Vec<Selector>,
    pub pin: Option<Pin>,
    pub isolation: Isolation,
    pub notarized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum OnLoss {
    Retry,
    Park,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Notify {
    Program {
        program: ProgramId,
        deliver: Vec<u8>,
    },
    Chat {
        channel: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Spec {
    pub kind: String,
    pub needs: Needs,
    pub class: String,
    pub on_loss: OnLoss,
    pub notify: Option<Notify>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RequestRow {
    pub id: u64,
    pub by: AccountNumber,
    pub via: Option<ProgramId>,
    pub spec: Spec,
    pub made: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum RequestState {
    Queued,
    Placed,
    Lost,
    Ended,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct RequestView {
    pub request: RequestRow,
    pub attempt: u32,
    pub state: RequestState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Outcome {
    Passed,
    Failed,
    Errored,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum AssignState {
    Assigned,
    Claimed,
    Stopping,
    Ended(Outcome),
    Declined,
    Missed,
    Lost,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum OfferRule {
    Only,
    Preferred,
    FreeLongest,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Reason {
    pub class: String,
    pub group: String,
    pub group_used: u64,
    pub group_weight: u32,
    pub rule: OfferRule,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Assignment {
    pub request: u64,
    pub attempt: u32,
    pub offer: OfferId,
    pub notary: Option<OfferId>,
    pub height: u64,
    pub reason: Reason,
    pub state: AssignState,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Placement {
    pub request: u64,
    pub attempt: u32,
    pub offer: OfferId,
    pub reach: Reach,
    pub notary: Option<OfferId>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Event {
    Assigned(Placement),
    Lost {
        request: u64,
        attempt: u32,
    },
    Ended {
        request: u64,
        attempt: u32,
        outcome: Outcome,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Standing {
    Counted,
    Attested,
    Claimed,
    Confirmed,
    Disputed { meters: Vec<String> },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Usage {
    pub provider: Vec<(String, u64)>,
    pub provider_at: Option<u64>,
    pub evidence: Option<BlobId>,
    pub requester: Vec<(String, u64)>,
    pub requester_at: Option<u64>,
    pub chain: Vec<(String, u64)>,
    pub standing: Standing,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Class {
    pub name: String,
    pub may_use: Option<PolicyId>,
    pub notarized: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Group {
    pub name: String,
    pub members: PolicyId,
    pub weight: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Quota {
    pub group: String,
    pub meter: String,
    pub window: u64,
    pub most: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Share {
    pub meter: Option<String>,
    pub window: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Policy {
    pub classes: Vec<Class>,
    pub groups: Vec<Group>,
    pub quotas: Vec<Quota>,
    pub share: Share,
    pub tolerance_ppm: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct LedgerRow {
    pub meter: String,
    pub window_start: u64,
    pub used: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Record {
    pub assigned: u64,
    pub claimed: u64,
    pub declined: u64,
    pub missed: u64,
    pub lost: u64,
    pub ended: u64,
    pub disputed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Waiting {
    ClassAhead {
        class: String,
    },
    GroupAhead {
        group: String,
    },
    OverQuota {
        group: String,
        meter: String,
        until: u64,
    },
    ClassRefuses,
    NoOfferTakes,
    NoNotary {
        kind: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct WhyView {
    pub state: RequestState,
    pub assignment: Option<Assignment>,
    pub waiting: Vec<Waiting>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Passed {
    NotAdmitted,
    NotReady(OfferState),
    Full,
    Labels,
    Isolation,
    Terms,
    Declined,
    NotNotarizing,
    NotPinned,
    NoNotary,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PassedOverRow {
    pub offer: OfferId,
    pub passed: Passed,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "The network's resource controller: what each provider offers, who asked for what, which provider each request was placed on and why, and the use each placement met. A kind's owner sets its policy and which providers it admits; providers offer and beat; anyone requests; placement follows the kind's policy alone."
)]
pub enum Op {
    #[tool_op(description = "Register a kind under a name nobody holds, which you then own: what one use is measured in, what one slot of capacity means, how many blocks an offer may be silent before its work is lost (none: the egress wait, DEFAULT_WAIT) and which kind notarizes its uses (none: no request of it can ask for a notarized use). `walk` is how many queued requests one step of a walk of its queue examines before the walk goes on at the next block (0 is refused). A meter whose checker is its own reporter is refused. Naming a notary kind needs its consent: whom that kind's row ownership policy lists, its owner always. As its owner, or whom the kind row's ownership policy lists, replace that spec.", example = crate::examples::register_kind)]
    RegisterKind { name: String, spec: KindSpec },
    #[tool_op(description = "Replace a kind's policy: its classes (highest first), its groups with their weights, its quotas, what fair share counts and the gap tolerated between a provider's and a requester's count. The kind's owner, or whom the kind row's ownership policy lists. Placements from the next block follow it.", example = crate::examples::set_policy)]
    SetPolicy { kind: String, policy: Policy },
    #[tool_op(description = "Put a provider on a kind's admitted list: from this frame on its offers of that kind take work. The kind's owner, or whom the kind row's ownership policy lists.", example = crate::examples::admit)]
    Admit {
        kind: String,
        provider: AccountNumber,
    },
    #[tool_op(description = "Take a provider off a kind's admitted list: nothing more is placed on its offers of that kind from this frame, and what they hold is lost at the next block and placed again by each request's on_loss. The kind's owner, or whom the kind row's ownership policy lists.", example = crate::examples::evict)]
    Evict {
        kind: String,
        provider: AccountNumber,
    },
    #[tool_op(description = "As a provider, declare or replace your offer `name` of a kind: its labels, how many slots it takes at once, how it is reached, your terms and the isolation you enforce. It takes work once the kind admits you, and a step of the kind's queue walk is booked for the next block.", example = crate::examples::offer)]
    Offer { offer: Offer },
    #[tool_op(description = "As a provider, withdraw your offer `name`: it takes nothing new, and is gone once what it holds ends.", example = crate::examples::withdraw)]
    Withdraw { name: String },
    #[tool_op(description = "As a provider, say your offer `name` is there, with the slots you see free and your own readings (a model account's window use). A missed or lapsed offer is ready again.", example = crate::examples::beat)]
    Beat {
        name: String,
        free: u64,
        readings: Vec<(String, u64)>,
    },
    #[tool_op(description = "Ask for a use of a kind: what it needs, its class, what a loss makes of it and who is told. It is placed in this frame on an offer that takes it now, unless one of the first `walk` queued requests of its kind takes that offer, else queued until one does. The request is the sending account's: a program that runs an account asks as it through the kernel, and is kept as the request's `via`. Returns the request's id.", returns = u64, example = crate::examples::request)]
    Request { spec: Spec },
    #[tool_op(description = "Ask for a one-shot use of a kind and have it placed in this frame, for a client that reaches the provider itself (egress for a model call). The placement is claimed at once. Refused, saying why, when no admitted offer takes it now or a queued request of its kind takes that offer first. Returns where it went.", returns = Placement, example = crate::examples::pick)]
    Pick { spec: Spec },
    #[tool_op(description = "Cancel a request: a queued one ends at once, a placed one is stopping until its provider reports its end. Whom the request row's policy admits; the account it is for always.", example = crate::examples::cancel)]
    Cancel { request: u64 },
    #[tool_op(description = "As the provider of the offer a request is assigned to, claim it at that attempt.", example = crate::examples::claim)]
    Claim { request: u64, attempt: u32 },
    #[tool_op(description = "As the provider of the offer a request is assigned to, give it back saying why: it is placed again, never on that offer.", example = crate::examples::decline)]
    Decline {
        request: u64,
        attempt: u32,
        why: String,
    },
    #[tool_op(description = "As the provider of the offer a request was placed on, report your count of the meters the kind says the provider reports or checks, and any evidence as a blob. Report as often as you like, each replacing your counts before; the report that carries an outcome ends the assignment and is the last.", example = crate::examples::report)]
    Report {
        request: u64,
        attempt: u32,
        outcome: Option<Outcome>,
        readings: Vec<(String, u64)>,
        evidence: Option<BlobId>,
    },
    #[tool_op(description = "As the program a request came through, report the chain's count (blocks, bytes) of the meters the kind says the chain reports or checks. Report as often as you like, each replacing your counts before; the report that carries an outcome ends the assignment and is the last.", example = crate::examples::count)]
    Count {
        request: u64,
        attempt: u32,
        outcome: Option<Outcome>,
        readings: Vec<(String, u64)>,
    },
    #[tool_op(description = "As the program a request came through, say its provider never answered: the assignment and its offer are missed, the offer is passed over until it beats, and the request is placed again or parked lost by its on_loss.", example = crate::examples::missed)]
    Missed { request: u64, attempt: u32 },
    #[tool_op(description = "As the account a request is for, give your count of the meters the kind says you report or check, replacing any count you gave before: the use is confirmed, or disputed where a checked meter's two counts differ past the kind's tolerance.", example = crate::examples::confirm)]
    Confirm {
        request: u64,
        attempt: u32,
        readings: Vec<(String, u64)>,
    },
    #[tool_op(description = "Check whether an offer went silent past its kind's lapse: its assignments are lost and placed again by each request's on_loss. resources books this itself; anyone may send it.", example = crate::examples::lapse)]
    Lapse { offer: OfferId },
    #[tool_op(description = "Check whether an assignment went unclaimed past its kind's lapse: its offer is missed and the request is placed again. resources books this itself; anyone may send it.", example = crate::examples::unclaimed)]
    Unclaimed { request: u64, attempt: u32 },
    #[tool_op(description = "Take the next step of a walk of a kind's queue: examine up to the kind's `walk` queued requests in the policy's order, from where the last step stopped, place each one a free admitted offer takes, and book the next step for the next block only when it examined that many with free slots left (a walk out of free slots or at the queue's end ends). resources books it for the next block whenever capacity frees or appears, a policy takes effect or a quota window ends; anyone may send it, and it acts only on what the chain shows.", example = crate::examples::place)]
    Place { kind: String },
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
pub enum Query {
    #[ask(Reply::Kind(Option<KindRow>))]
    #[tool_query(description = "A kind: its owner, meters, slot, lapse and notary kind.", example = crate::examples::kind)]
    Kind { name: String },
    #[ask(Reply::Kinds(PageResponse<KindRow>))]
    #[tool_query(description = "Every kind, by name.", example = crate::examples::kinds)]
    Kinds { page: PageRequest },
    #[ask(Reply::Policy(Option<Policy>))]
    #[tool_query(description = "A kind's policy: classes, groups, quotas, fair share and tolerance.", example = crate::examples::policy)]
    Policy { kind: String },
    #[ask(Reply::Admitted(PageResponse<AdmittedRow>))]
    #[tool_query(description = "A kind's admitted providers, each with who admitted it and when.", example = crate::examples::admitted)]
    Admitted { kind: String, page: PageRequest },
    #[ask(Reply::Offer(Option<OfferView>))]
    #[tool_query(description = "An offer: what it declares, its state, what it holds, its last beat and whether its kind admits its provider.", example = crate::examples::offer_query)]
    Offer { offer: OfferId },
    #[ask(Reply::Offers(PageResponse<OfferView>))]
    #[tool_query(description = "A kind's offers, by provider and name.", example = crate::examples::offers)]
    Offers { kind: String, page: PageRequest },
    #[ask(Reply::Request(Option<RequestView>))]
    #[tool_query(description = "A request as it was asked, its attempt and its state.", example = crate::examples::request_query)]
    Request { id: u64 },
    #[ask(Reply::Requests(PageResponse<RequestView>))]
    #[tool_query(description = "A kind's queued requests, by class, group and id.", example = crate::examples::queued)]
    Queued { kind: String, page: PageRequest },
    #[ask(Reply::Assignment(Option<Assignment>))]
    #[tool_query(description = "A request's assignment at one attempt: the offer, the notary, when, why and its state.", example = crate::examples::assignment)]
    Assignment { request: u64, attempt: u32 },
    #[ask(Reply::Assignments(PageResponse<Assignment>))]
    #[tool_query(description = "The assignments an offer holds now.", example = crate::examples::holds)]
    Holds { offer: OfferId, page: PageRequest },
    #[ask(Reply::Usage(Option<Usage>))]
    #[tool_query(description = "What one assignment used: the provider's count, the requester's, the chain's, any evidence, and what it rests on.", example = crate::examples::usage)]
    Usage { request: u64, attempt: u32 },
    #[ask(Reply::Ledger(PageResponse<LedgerRow>))]
    #[tool_query(description = "A group's use of a kind, per meter and window: what fair share and quotas read.", example = crate::examples::ledger)]
    Ledger {
        kind: String,
        group: String,
        page: PageRequest,
    },
    #[ask(Reply::Record(Option<Record>))]
    #[tool_query(description = "An offer's running counts: assigned, claimed, declined, missed, lost, ended and disputed.", example = crate::examples::record)]
    Record { offer: OfferId },
    #[ask(Reply::Why(Option<WhyView>))]
    #[tool_query(description = "Why a request is where it is: its newest assignment and the reason it was chosen, or what holds it queued.", example = crate::examples::why)]
    Why { request: u64 },
    #[ask(Reply::PassedOver(PageResponse<PassedOverRow>))]
    #[tool_query(description = "Each offer of a request's kind that would not take it now, and why.", example = crate::examples::passed_over)]
    PassedOver { request: u64, page: PageRequest },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Kind(Option<KindRow>),
    Kinds(PageResponse<KindRow>),
    Policy(Option<Policy>),
    Admitted(PageResponse<AdmittedRow>),
    Offer(Option<OfferView>),
    Offers(PageResponse<OfferView>),
    Request(Option<RequestView>),
    Requests(PageResponse<RequestView>),
    Assignment(Option<Assignment>),
    Assignments(PageResponse<Assignment>),
    Usage(Option<Usage>),
    Ledger(PageResponse<LedgerRow>),
    Record(Option<Record>),
    Why(Option<WhyView>),
    PassedOver(PageResponse<PassedOverRow>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let kind = |name: &str| field("kind", Value::text(name));
    let request = |id: &u64| field("request", Value::count(*id));
    let (title, fields) = match op {
        Op::RegisterKind { name, spec } => (
            format!("Register kind {name}"),
            vec![field("walk", Value::count(spec.walk))],
        ),
        Op::SetPolicy { kind: name, policy } => (
            format!("Set the policy of {name}"),
            vec![
                field("classes", Value::count(policy.classes.len() as u64)),
                field("groups", Value::count(policy.groups.len() as u64)),
            ],
        ),
        Op::Admit {
            kind: name,
            provider,
        } => (
            format!("Admit account {provider} to {name}"),
            vec![kind(name)],
        ),
        Op::Evict {
            kind: name,
            provider,
        } => (
            format!("Evict account {provider} from {name}"),
            vec![kind(name)],
        ),
        Op::Offer { offer } => (
            format!("Offer {} of {}", offer.name, offer.kind),
            vec![field("slots", Value::count(offer.capacity))],
        ),
        Op::Withdraw { name } => (format!("Withdraw offer {name}"), Vec::new()),
        Op::Beat { name, free, .. } => (
            format!("Offer {name} beats"),
            vec![field("free", Value::count(*free))],
        ),
        Op::Request { spec } => (
            format!("Request {}", spec.kind),
            vec![field("units", Value::count(spec.needs.units))],
        ),
        Op::Pick { spec } => (
            format!("Pick {} now", spec.kind),
            vec![field("units", Value::count(spec.needs.units))],
        ),
        Op::Cancel { request: id } => (format!("Cancel request {id}"), vec![request(id)]),
        Op::Claim { request: id, .. } => (format!("Claim request {id}"), vec![request(id)]),
        Op::Decline {
            request: id, why, ..
        } => (
            format!("Decline request {id}"),
            vec![request(id), field("why", Value::text(why))],
        ),
        Op::Report {
            request: id,
            outcome,
            ..
        } => (
            match outcome {
                Some(outcome) => format!("Report request {id} {outcome:?}"),
                None => format!("Report request {id}'s counts"),
            },
            vec![request(id)],
        ),
        Op::Count {
            request: id,
            outcome,
            ..
        } => (
            match outcome {
                Some(outcome) => format!("Count request {id} {outcome:?}"),
                None => format!("Count request {id}'s use"),
            },
            vec![request(id)],
        ),
        Op::Missed { request: id, .. } => (
            format!("Say request {id}'s provider never answered"),
            vec![request(id)],
        ),
        Op::Confirm { request: id, .. } => {
            (format!("Confirm request {id}'s use"), vec![request(id)])
        }
        Op::Lapse { offer } => (
            format!("Check offer {} of {}", offer.name, offer.provider),
            Vec::new(),
        ),
        Op::Unclaimed { request: id, .. } => {
            (format!("Check request {id} was claimed"), vec![request(id)])
        }
        Op::Place { kind: name } => (format!("Walk the queue of {name}"), vec![kind(name)]),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[cfg(test)]
mod tests {
    use ::program::tools::Described;

    use crate::{Op, Query, Reply};

    #[test]
    fn op_variants_only_append() {
        assert_eq!(
            describe::variants::<Op>(),
            [
                "RegisterKind",
                "SetPolicy",
                "Admit",
                "Evict",
                "Offer",
                "Withdraw",
                "Beat",
                "Request",
                "Pick",
                "Cancel",
                "Claim",
                "Decline",
                "Report",
                "Count",
                "Missed",
                "Confirm",
                "Lapse",
                "Unclaimed",
                "Place"
            ]
        );
    }

    #[test]
    fn query_and_reply_variants_only_append() {
        assert_eq!(
            describe::variants::<Query>(),
            [
                "Kind",
                "Kinds",
                "Policy",
                "Admitted",
                "Offer",
                "Offers",
                "Request",
                "Queued",
                "Assignment",
                "Holds",
                "Usage",
                "Ledger",
                "Record",
                "Why",
                "PassedOver"
            ]
        );
        assert_eq!(
            describe::variants::<Reply>(),
            [
                "Kind",
                "Kinds",
                "Policy",
                "Admitted",
                "Offer",
                "Offers",
                "Request",
                "Requests",
                "Assignment",
                "Assignments",
                "Usage",
                "Ledger",
                "Record",
                "Why",
                "PassedOver"
            ]
        );
    }

    #[test]
    fn every_op_and_query_has_a_description_and_an_example() {
        let tools = Op::tools().into_iter().chain(Query::tools());
        for tool in tools {
            assert!(
                !tool.description.is_empty(),
                "{} has no description",
                tool.variant
            );
            assert!(tool.example.is_some(), "{} has no example", tool.variant);
        }
        for tool in Query::tools() {
            assert!(!tool.reply.is_empty(), "{} names no reply", tool.variant);
        }
    }
}
