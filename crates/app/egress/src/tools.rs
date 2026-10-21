//! What egress tells an agent of its ops and queries. Its `Op` is the egress role's, defined in abi,
//! so a marker carries the ops' words and a working value of each; `Query` derives its own.
use ::program::tools::{Described, Tool, example};

use crate::{Answer, Delivery, Event, Op, PageRequest, Query, Request, StreamEnd, http};

pub(crate) struct Ops;

fn key() -> Vec<u8> {
    vec![7; 32]
}

fn page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

fn op(variant: &'static str, description: &'static str, value: Op) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: "",
    }
}

impl Described for Ops {
    const ABOUT: &'static str = "Calls from programs to the world outside the chain: the public internet (service http) or a service registered here, each answered by a member node elected among those serving it, whole or as streamed events, or expired once its wait runs out.";

    fn tools() -> Vec<Tool> {
        vec![
            op(
                "Register",
                "Register a service by name (lowercase letters, digits, -, _ and .), owned by your account. http and relay are egress's own and never registered; a name taken is refused.",
                Op::Register {
                    service: "pods".into(),
                },
            ),
            op(
                "Assign",
                "Set the member nodes, by node key, that may serve a registered service, replacing its list: a registered service is served only by the nodes it names. Its owner, or whom the service's policy admits.",
                Op::Assign {
                    service: "pods".into(),
                    nodes: vec![key()],
                },
            ),
            op(
                "Serve",
                "Signed by a member node's own key: the services this node serves, replacing its list; an empty list stops it serving.",
                Op::Serve {
                    services: vec!["pods".into()],
                },
            ),
            op(
                "Request",
                "From a program, as a call it awaits (network_call sends it so): an HTTP request to the public internet (service http, a full http:// or https:// target) or to a registered service (a target path), its body parts blobs put first. The elected node answers whole, or as events egress delivers to the caller after the bytes of deliver. wait is how many blocks it waits before it expires, 64 if null.",
                Op::Request {
                    request: Request::Http {
                        service: "pods".into(),
                        head: http::Head {
                            method: http::Method::Get,
                            target: "/v1/pods".into(),
                            headers: Vec::new(),
                        },
                        body: Vec::new(),
                    },
                    delivery: Delivery::Whole,
                    wait: None,
                },
            ),
            op(
                "Answer",
                "Signed by the node a request was elected to, in a block after the one that made it: the request's whole answer, its output or why it failed.",
                Op::Answer {
                    id: 1,
                    answer: Answer::Output(b"{}".to_vec()),
                },
            ),
            op(
                "Events",
                "Signed by the node a streamed request was elected to: its events, each with its seq; egress delivers them to the caller in seq order, and each block that carries one moves the request's expiry on by its wait.",
                Op::Events(vec![Event {
                    id: 1,
                    seq: 0,
                    data: b"data: hello".to_vec(),
                }]),
            ),
            op(
                "EndStream",
                "Signed by the node a streamed request was elected to: the stream ends after events events, complete with its HTTP status and headers, or cut with a reason.",
                Op::EndStream {
                    id: 1,
                    events: 1,
                    end: StreamEnd::Complete {
                        status: 200,
                        headers: Vec::new(),
                    },
                },
            ),
            op(
                "Expire",
                "egress's own scheduled run alone: a request unanswered when its wait runs out ends, refused to its caller.",
                Op::Expire { id: 1 },
            ),
        ]
    }
}

pub(crate) fn assigned() -> Query {
    Query::Assigned { key: key() }
}

pub(crate) fn node() -> Query {
    Query::Node { key: key() }
}

pub(crate) fn serving() -> Query {
    Query::Serving {
        service: "pods".into(),
    }
}

pub(crate) fn request() -> Query {
    Query::Request { id: 1 }
}

pub(crate) fn nodes() -> Query {
    Query::Nodes { page: page() }
}

pub(crate) fn service() -> Query {
    Query::Service {
        name: "pods".into(),
    }
}

pub(crate) fn services() -> Query {
    Query::Services { page: page() }
}

pub(crate) fn sender() -> Query {
    Query::Sender { id: 1 }
}
