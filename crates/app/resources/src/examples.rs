//! A working value of each resources op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use crate::{
    Class, Counter, Group, Isolation, KindSpec, Meter, Needs, Notify, Offer, OfferId, OnLoss, Op,
    Outcome, PageRequest, Pin, Policy, Query, Reach, Selector, Share, Spec, Terms,
};

fn page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

fn model() -> String {
    "model.anthropic".into()
}

fn account() -> OfferId {
    OfferId {
        provider: 16,
        name: "max-1".into(),
    }
}

fn meter(name: &str, unit: &str, reported_by: Counter, checked_by: Option<Counter>) -> Meter {
    Meter {
        name: name.into(),
        unit: unit.into(),
        reported_by,
        checked_by,
    }
}

fn spec() -> Spec {
    Spec {
        kind: model(),
        needs: Needs {
            units: 1,
            labels: vec![Selector::Is("model".into(), "claude-opus-4".into())],
            pin: Some(Pin::Prefer(account())),
            isolation: Isolation::None,
            notarized: false,
        },
        class: String::new(),
        on_loss: OnLoss::Retry,
        notify: Some(Notify::Chat {
            channel: "j-460".into(),
        }),
    }
}

pub(crate) fn register_kind() -> Op {
    Op::RegisterKind {
        name: model(),
        spec: KindSpec {
            meters: vec![
                meter(
                    "tokens_in",
                    "token",
                    Counter::Provider,
                    Some(Counter::Requester),
                ),
                meter(
                    "tokens_out",
                    "token",
                    Counter::Provider,
                    Some(Counter::Requester),
                ),
                meter("calls", "call", Counter::Chain, None),
            ],
            slot: "a concurrent call".into(),
            lapse: None,
            notary: None,
            walk: 100,
        },
    }
}

pub(crate) fn set_policy() -> Op {
    Op::SetPolicy {
        kind: model(),
        policy: Policy {
            classes: vec![
                Class {
                    name: "workers".into(),
                    may_use: Some(98),
                    notarized: false,
                },
                Class {
                    name: String::new(),
                    may_use: None,
                    notarized: false,
                },
            ],
            groups: vec![Group {
                name: "reviewers".into(),
                members: 99,
                weight: 2,
            }],
            quotas: Vec::new(),
            share: Share {
                meter: Some("tokens_out".into()),
                window: None,
            },
            tolerance_ppm: 0,
        },
    }
}

pub(crate) fn admit() -> Op {
    Op::Admit {
        kind: model(),
        provider: 16,
    }
}

pub(crate) fn evict() -> Op {
    Op::Evict {
        kind: model(),
        provider: 16,
    }
}

pub(crate) fn offer() -> Op {
    Op::Offer {
        offer: Offer {
            name: "max-1".into(),
            kind: model(),
            labels: vec![
                ("model".into(), "claude-opus-4".into()),
                ("plan".into(), "max".into()),
            ],
            capacity: 4,
            reach: Reach::Egress {
                service: "models".into(),
                node: vec![0; 32],
            },
            terms: Terms {
                serves: None,
                notarizes: false,
            },
            isolation: Isolation::None,
        },
    }
}

pub(crate) fn withdraw() -> Op {
    Op::Withdraw {
        name: "max-1".into(),
    }
}

pub(crate) fn beat() -> Op {
    Op::Beat {
        name: "max-1".into(),
        free: 3,
        readings: vec![("window_5h_used_ppm".into(), 420_000)],
    }
}

pub(crate) fn request() -> Op {
    Op::Request { spec: spec() }
}

pub(crate) fn pick() -> Op {
    Op::Pick {
        spec: Spec {
            notify: None,
            ..spec()
        },
    }
}

pub(crate) fn cancel() -> Op {
    Op::Cancel { request: 7 }
}

pub(crate) fn claim() -> Op {
    Op::Claim {
        request: 7,
        attempt: 1,
    }
}

pub(crate) fn decline() -> Op {
    Op::Decline {
        request: 7,
        attempt: 1,
        why: "the account's weekly window is spent".into(),
    }
}

pub(crate) fn report() -> Op {
    Op::Report {
        request: 7,
        attempt: 1,
        outcome: None,
        readings: vec![("tokens_in".into(), 1200), ("tokens_out".into(), 300)],
        evidence: None,
    }
}

pub(crate) fn count() -> Op {
    Op::Count {
        request: 7,
        attempt: 1,
        outcome: Some(Outcome::Passed),
        readings: vec![("calls".into(), 1)],
    }
}

pub(crate) fn missed() -> Op {
    Op::Missed {
        request: 7,
        attempt: 1,
    }
}

pub(crate) fn confirm() -> Op {
    Op::Confirm {
        request: 7,
        attempt: 1,
        readings: vec![("tokens_in".into(), 1200), ("tokens_out".into(), 300)],
    }
}

pub(crate) fn lapse() -> Op {
    Op::Lapse { offer: account() }
}

pub(crate) fn unclaimed() -> Op {
    Op::Unclaimed {
        request: 7,
        attempt: 1,
    }
}

pub(crate) fn place() -> Op {
    Op::Place { kind: model() }
}

pub(crate) fn kind() -> Query {
    Query::Kind { name: model() }
}

pub(crate) fn kinds() -> Query {
    Query::Kinds { page: page() }
}

pub(crate) fn policy() -> Query {
    Query::Policy { kind: model() }
}

pub(crate) fn admitted() -> Query {
    Query::Admitted {
        kind: model(),
        page: page(),
    }
}

pub(crate) fn offer_query() -> Query {
    Query::Offer { offer: account() }
}

pub(crate) fn offers() -> Query {
    Query::Offers {
        kind: model(),
        page: page(),
    }
}

pub(crate) fn request_query() -> Query {
    Query::Request { id: 7 }
}

pub(crate) fn queued() -> Query {
    Query::Queued {
        kind: model(),
        page: page(),
    }
}

pub(crate) fn assignment() -> Query {
    Query::Assignment {
        request: 7,
        attempt: 1,
    }
}

pub(crate) fn holds() -> Query {
    Query::Holds {
        offer: account(),
        page: page(),
    }
}

pub(crate) fn usage() -> Query {
    Query::Usage {
        request: 7,
        attempt: 1,
    }
}

pub(crate) fn ledger() -> Query {
    Query::Ledger {
        kind: model(),
        group: "reviewers".into(),
        page: page(),
    }
}

pub(crate) fn record() -> Query {
    Query::Record { offer: account() }
}

pub(crate) fn why() -> Query {
    Query::Why { request: 7 }
}

pub(crate) fn passed_over() -> Query {
    Query::PassedOver {
        request: 7,
        page: page(),
    }
}
