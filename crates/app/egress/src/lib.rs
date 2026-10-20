//! The `egress` program, bound to the egress role (`abi::role::egress`): a program's call to a
//! registered service or the public internet, answered whole or as `text/event-stream` events by
//! the elected member node, or expired by its own scheduled run. Its first queries are the role's.
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use ::program::{BlobId, ProgramId};
pub use abi::role::egress::{
    Answer, DEFAULT_WAIT, Delivery, Event, HTTP, NodeRow, Op, RELAY, RESERVED, Request, RequestRow,
    ServiceRow, StreamEnd, http,
};
pub use abi::role::identity::AccountNumber;
pub use program::Egress;
pub use rules::service_key;
pub use store::{PageRequest, PageResponse};

// The table of the services egress claims, for what reads its owned kinds.
pub mod tables {
    pub use crate::state::SERVICES;
}

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "egress";

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Egress)]
pub enum Query {
    #[ask(Reply::Requests(Vec<RequestRow>))]
    #[reads(state::ASSIGNED, state::REQUESTS)]
    Assigned { key: Vec<u8> },
    #[ask(Reply::Node(Option<NodeRow>))]
    #[reads(state::NODES)]
    Node { key: Vec<u8> },
    #[ask(Reply::Serving(Vec<Vec<u8>>))]
    #[reads(state::SERVING, state::SERVICES)]
    Serving { service: String },
    #[ask(Reply::Request(Option<RequestRow>))]
    #[reads(state::REQUESTS)]
    Request { id: u64 },
    #[ask(Reply::Nodes(PageResponse<NodeRow>))]
    #[reads(state::NODES)]
    Nodes { page: PageRequest },
    #[ask(Reply::Service(Option<ServiceRow>))]
    #[reads(state::SERVICES)]
    Service { name: String },
    #[ask(Reply::Services(PageResponse<ServiceRow>))]
    #[reads(state::SERVICES)]
    Services { page: PageRequest },
    #[ask(Reply::Sender(Option<AccountNumber>))]
    #[reads(state::SENDERS)]
    Sender { id: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Requests(Vec<RequestRow>),
    Node(Option<NodeRow>),
    Serving(Vec<Vec<u8>>),
    Request(Option<RequestRow>),
    Nodes(PageResponse<NodeRow>),
    Service(Option<ServiceRow>),
    Services(PageResponse<ServiceRow>),
    Sender(Option<AccountNumber>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let names = |names: &[String]| Value::List(names.iter().map(Value::text).collect());
    let (title, fields) = match op {
        Op::Register { service } => (format!("Register service {service}"), Vec::new()),
        Op::Assign { service, nodes } => (
            format!("Assign service {service}"),
            vec![field("nodes", Value::count(nodes.len() as u64))],
        ),
        Op::Serve { services } if services.is_empty() => ("Stop serving".to_owned(), Vec::new()),
        Op::Serve { services } => ("Serve".to_owned(), vec![field("services", names(services))]),
        Op::Request {
            request: Request::Relay { note },
            delivery,
            wait,
        } => (
            "Request a relay".to_owned(),
            vec![
                field("note", Value::count(note.len() as u64)),
                field("delivery", Value::text(delivered(delivery))),
                field("wait", Value::count(wait.unwrap_or(DEFAULT_WAIT))),
            ],
        ),
        Op::Request {
            request:
                Request::Http {
                    service,
                    head,
                    body,
                },
            delivery,
            wait,
        } => (
            format!("Request {service}"),
            vec![
                field("method", Value::text(head.method.as_str())),
                field("target", Value::text(&head.target)),
                field("body", Value::count(body.len() as u64)),
                field("delivery", Value::text(delivered(delivery))),
                field("wait", Value::count(wait.unwrap_or(DEFAULT_WAIT))),
            ],
        ),
        Op::Answer { id, answer } => {
            let field = match answer {
                Answer::Output(output) => field("output", Value::count(output.len() as u64)),
                Answer::Failed(reason) => field("failed", Value::text(reason)),
            };
            (format!("Answer request {id}"), vec![field])
        }
        Op::Events(events) => {
            let requests: std::collections::BTreeSet<u64> =
                events.iter().map(|event| event.id).collect();
            let named: Vec<String> = requests.iter().map(u64::to_string).collect();
            let data: usize = events.iter().map(|event| event.data.len()).sum();
            (
                format!("{} streamed events", events.len()),
                vec![
                    field("requests", Value::text(named.join(", "))),
                    field("data", Value::count(data as u64)),
                ],
            )
        }
        Op::EndStream { id, events, end } => {
            let end = match end {
                StreamEnd::Complete { status, .. } => {
                    field("status", Value::count(u64::from(*status)))
                }
                StreamEnd::Cut { reason } => field("cut", Value::text(reason)),
            };
            (
                format!("End request {id}'s stream"),
                vec![field("events", Value::count(*events)), end],
            )
        }
        Op::Expire { id } => (format!("Expire request {id}"), Vec::new()),
    };
    describe::Description { title, fields }
}

fn delivered(delivery: &Delivery) -> &'static str {
    match delivery {
        Delivery::Whole => "whole",
        Delivery::Events { .. } => "events",
    }
}

describe::export!(Op, describe);

#[test]
fn the_roles_queries_lead_and_encode_as_the_role_asks() {
    use abi::role::egress as role;
    let key = vec![7];
    let pairs = [
        (
            abi::encode(&Query::Assigned { key: key.clone() }),
            abi::encode(&role::Query::Assigned { key: key.clone() }),
        ),
        (
            abi::encode(&Query::Node { key: key.clone() }),
            abi::encode(&role::Query::Node { key }),
        ),
        (
            abi::encode(&Query::Serving {
                service: "s".into(),
            }),
            abi::encode(&role::Query::Serving {
                service: "s".into(),
            }),
        ),
        (
            abi::encode(&Reply::Serving(Vec::new())),
            abi::encode(&role::Reply::Serving(Vec::new())),
        ),
        (
            abi::encode(&Reply::Requests(Vec::new())),
            abi::encode(&role::Reply::Requests(Vec::new())),
        ),
        (
            abi::encode(&Reply::Node(None)),
            abi::encode(&role::Reply::Node(None)),
        ),
    ];
    for (ours, roles) in pairs {
        assert_eq!(ours, roles);
    }
}

#[test]
fn op_variants_follow_the_roles_order() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Register",
            "Assign",
            "Serve",
            "Request",
            "Answer",
            "Events",
            "EndStream",
            "Expire"
        ]
    );
}
