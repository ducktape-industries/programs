//! The `egress` program, bound to the egress role (`abi::role::egress`): a program's call to a
//! registered service or the public internet, answered whole or as `text/event-stream` events by
//! the elected member node, or expired by its own scheduled run. Its first queries are the role's.
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;
mod tools;

pub use ::program::{BlobId, ProgramId};
pub use abi::role::egress::{
    Answer, CROSSPOND, DEFAULT_WAIT, Delivery, Event, HTTP, NodeRow, RELAY, RESERVED, Request,
    RequestRow, ServiceRow, StreamEnd, http,
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

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Register {
        service: String,
    },
    Assign {
        service: String,
        nodes: Vec<Vec<u8>>,
    },
    Serve {
        services: Vec<String>,
    },
    Request {
        request: Request,
        delivery: Delivery,
        wait: Option<u64>,
    },
    Answer {
        id: u64,
        answer: Answer,
    },
    Events(Vec<Event>),
    EndStream {
        id: u64,
        events: u64,
        end: StreamEnd,
    },
    Expire {
        id: u64,
    },
    Unregister {
        service: String,
    },
    Migrate {
        height: u64,
        first: Option<u64>,
        last: Option<u64>,
    },
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
#[ask(Egress)]
pub enum Query {
    #[ask(Reply::Requests(Vec<RequestRow>))]
    #[reads(state::ASSIGNED, state::REQUESTS)]
    #[tool_query(description = "The open requests elected to a node, by its key.", example = crate::tools::assigned)]
    Assigned { key: Vec<u8> },
    #[ask(Reply::Node(Option<NodeRow>))]
    #[reads(state::NODES)]
    #[tool_query(description = "A node's row, by its key: the services it serves and the block it began, or null.", example = crate::tools::node)]
    Node { key: Vec<u8> },
    #[ask(Reply::Serving(Vec<Vec<u8>>))]
    #[reads(state::SERVING, state::SERVICES)]
    #[tool_query(description = "The member nodes that may serve a service now: serving it, and named by its Assign where it is registered.", example = crate::tools::serving)]
    Serving { service: String },
    #[ask(Reply::Request(Option<RequestRow>))]
    #[reads(state::REQUESTS)]
    #[tool_query(description = "One open request, or null once it is answered or expired.", example = crate::tools::request)]
    Request { id: u64 },
    #[ask(Reply::Nodes(PageResponse<NodeRow>))]
    #[reads(state::NODES)]
    #[tool_query(description = "Every serving node, a page at a time.", example = crate::tools::nodes)]
    Nodes { page: PageRequest },
    #[ask(Reply::Service(Option<ServiceRow>))]
    #[reads(state::SERVICES)]
    #[tool_query(description = "One registered service: its owner, the nodes assigned to it and the block it was registered, or null.", example = crate::tools::service)]
    Service { name: String },
    #[ask(Reply::Services(PageResponse<ServiceRow>))]
    #[reads(state::SERVICES)]
    #[tool_query(description = "Every registered service, a page at a time.", example = crate::tools::services)]
    Services { page: PageRequest },
    #[ask(Reply::Sender(Option<AccountNumber>))]
    #[reads(state::SENDERS)]
    #[tool_query(description = "The account an open request was sent as, or null.", example = crate::tools::sender)]
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
            request: Request::Crosspond { link },
            delivery,
            wait,
        } => (
            "Request a crosspond relay".to_owned(),
            vec![
                field("network", Value::Hash(link.to_vec())),
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
        Op::Unregister { service } => (format!("Unregister service {service}"), Vec::new()),
        Op::Migrate {
            height,
            first,
            last,
        } => {
            let bound = |bound: &Option<u64>, open: &str| {
                bound.map_or_else(|| Value::text(open), Value::count)
            };
            (
                "Migrate".to_owned(),
                vec![
                    field("height", Value::count(*height)),
                    field("first", bound(first, "the first")),
                    field("last", bound(last, "the last")),
                ],
            )
        }
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
            "Expire",
            "Unregister",
            "Migrate"
        ]
    );
}

#[test]
fn every_op_of_the_egress_role_meets_egresss_own_at_one_tag() {
    use abi::role::egress as role;
    let in_the_role = |op: &role::Op| match op {
        role::Op::Register { .. }
        | role::Op::Assign { .. }
        | role::Op::Serve { .. }
        | role::Op::Request { .. }
        | role::Op::Answer { .. }
        | role::Op::Events(_)
        | role::Op::EndStream { .. }
        | role::Op::Expire { .. }
        | role::Op::Unregister { .. } => {}
    };
    let request = Request::Relay {
        note: b"ping".to_vec(),
    };
    let end = StreamEnd::Cut {
        reason: "gone".into(),
    };
    let event = Event {
        id: 1,
        seq: 0,
        data: b"data".to_vec(),
    };
    let ops = [
        (
            role::Op::Register {
                service: "s".into(),
            },
            Op::Register {
                service: "s".into(),
            },
        ),
        (
            role::Op::Assign {
                service: "s".into(),
                nodes: vec![vec![7]],
            },
            Op::Assign {
                service: "s".into(),
                nodes: vec![vec![7]],
            },
        ),
        (
            role::Op::Serve {
                services: vec!["s".into()],
            },
            Op::Serve {
                services: vec!["s".into()],
            },
        ),
        (
            role::Op::Request {
                request: request.clone(),
                delivery: Delivery::Whole,
                wait: Some(3),
            },
            Op::Request {
                request,
                delivery: Delivery::Whole,
                wait: Some(3),
            },
        ),
        (
            role::Op::Answer {
                id: 1,
                answer: Answer::Output(b"pong".to_vec()),
            },
            Op::Answer {
                id: 1,
                answer: Answer::Output(b"pong".to_vec()),
            },
        ),
        (
            role::Op::Events(vec![event.clone()]),
            Op::Events(vec![event]),
        ),
        (
            role::Op::EndStream {
                id: 1,
                events: 2,
                end: end.clone(),
            },
            Op::EndStream {
                id: 1,
                events: 2,
                end,
            },
        ),
        (role::Op::Expire { id: 1 }, Op::Expire { id: 1 }),
        (
            role::Op::Unregister {
                service: "s".into(),
            },
            Op::Unregister {
                service: "s".into(),
            },
        ),
    ];
    for (sent, own) in ops {
        in_the_role(&sent);
        assert_eq!(abi::decode::<Op>(&abi::encode(&sent)).unwrap(), own);
        assert_eq!(abi::decode::<role::Op>(&abi::encode(&own)).unwrap(), sent);
    }
}
