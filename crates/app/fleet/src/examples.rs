//! A working value of each fleet op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use abi::role::egress::http;

use crate::{
    Affinity, Asks, Event, Input, Kind, MachineId, Notify, OnLoss, Op, Out, PageRequest, Query,
    Request, Serves, Spec, State,
};

fn page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

fn desktop() -> MachineId {
    MachineId {
        account: 50,
        name: "desktop".into(),
    }
}

pub(crate) fn ask() -> Op {
    let spec = Spec {
        kind: Kind::Job,
        image: None,
        command: vec![
            "sh".into(),
            "/var/tmp/warm/build.sh".into(),
            "programs".into(),
            "dev".into(),
            "lane".into(),
        ],
        env: vec![("JOB".into(), "j-129".into())],
        workdir: None,
        inputs: vec![Input::Forge {
            path: "/work/programs".into(),
            repo: "programs".into(),
            commit: "0".repeat(40),
        }],
        asks: Asks::default(),
        on: Some(Affinity::Labels(vec!["warm".into()])),
        out: None,
        on_loss: OnLoss::Retry,
        notify: Some(Notify::Chat {
            channel: "j-129".into(),
        }),
    };
    Op::Ask { spec }
}

pub(crate) fn ask_in() -> Op {
    let Op::Ask { spec } = ask() else {
        unreachable!("ask() answers an Ask")
    };
    Op::AskIn {
        spec,
        kind: Some("fleet.build".into()),
        class: "review".into(),
        limit: Some(1200),
    }
}

pub(crate) fn ready_in() -> Op {
    Op::ReadyIn {
        name: "desktop".into(),
        labels: vec!["heavy".into()],
        slots: 2,
        serves: Serves::Anyone,
        kind: Some("fleet.build".into()),
    }
}

pub(crate) fn promote() -> Op {
    Op::Promote {
        run: 7,
        class: "front".into(),
    }
}

pub(crate) fn walk() -> Op {
    Op::Walk {
        kind: "fleet.build".into(),
    }
}

pub(crate) fn expire() -> Op {
    Op::Expire { run: 7, attempt: 1 }
}

pub(crate) fn filed() -> Query {
    Query::Filed { run: 7 }
}

pub(crate) fn kind_of() -> Query {
    Query::KindOf { machine: desktop() }
}

pub(crate) fn ledger() -> Query {
    Query::Ledger {
        kind: "fleet.build".into(),
        group: "tech".into(),
        page: page(),
    }
}

pub(crate) fn ready() -> Op {
    Op::Ready {
        name: "desktop".into(),
        labels: vec!["oci".into(), "warm".into()],
        slots: 2,
        serves: Serves::Anyone,
    }
}

pub(crate) fn claim() -> Op {
    Op::Claim { run: 7, attempt: 1 }
}

pub(crate) fn beat() -> Op {
    Op::Beat {
        name: "desktop".into(),
    }
}

pub(crate) fn report() -> Op {
    Op::Report {
        run: 7,
        attempt: 1,
        state: State::Passed,
        summary: "GATES ok".into(),
        out: Some(Out {
            path: "/shared/fleet/7/".into(),
            snapshot: ::program::BlobId::Sha256([0; 32]),
        }),
    }
}

pub(crate) fn decline() -> Op {
    Op::Decline {
        run: 7,
        attempt: 1,
        why: "the image does not pull here".into(),
    }
}

pub(crate) fn cancel() -> Op {
    Op::Cancel { run: 7 }
}

pub(crate) fn requeue() -> Op {
    Op::Requeue { run: 7 }
}

pub(crate) fn lapse() -> Op {
    Op::Lapse { machine: desktop() }
}

pub(crate) fn unclaimed() -> Op {
    Op::Unclaimed { run: 7, attempt: 1 }
}

pub(crate) fn place() -> Op {
    Op::Place { run: 7 }
}

pub(crate) fn call() -> Op {
    let head = http::Head {
        method: http::Method::Get,
        target: "/v1/pods/duckpond-7/wait?every=30".into(),
        headers: Vec::new(),
    };
    Op::Call {
        run: 7,
        attempt: 1,
        call: 2,
        request: Request::Http {
            service: "pods".into(),
            head,
            body: Vec::new(),
        },
        stream: true,
        wait: None,
    }
}

pub(crate) fn heard() -> Op {
    Op::Heard {
        run: 7,
        attempt: 1,
        call: 2,
        event: Event {
            id: 40,
            seq: 0,
            data: b"{}".to_vec(),
        },
    }
}

pub(crate) fn workload() -> Query {
    Query::Workload { id: 7 }
}

pub(crate) fn run() -> Query {
    Query::Run { id: 7 }
}

pub(crate) fn queued() -> Query {
    Query::Queued { page: page() }
}

pub(crate) fn machine() -> Query {
    Query::Machine { machine: desktop() }
}

pub(crate) fn machines() -> Query {
    Query::Machines { page: page() }
}

pub(crate) fn holds() -> Query {
    Query::Holds {
        machine: desktop(),
        page: page(),
    }
}

pub(crate) fn pieces() -> Query {
    Query::Pieces {
        run: 7,
        attempt: 1,
        call: 2,
        page: page(),
    }
}
