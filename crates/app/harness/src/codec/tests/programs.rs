use ::program::schema::interface;
use ::program::{BlobId, Cause, MessageId, Outcome, Ownership, Party, Principal, Scheme};
use abi::role::identity::{Category, Kind, Profile, Script, Standing};
use serde_json::json;
use store::{PageRequest, PageResponse};

use super::*;

fn page<T>(items: Vec<T>) -> PageResponse<T> {
    PageResponse {
        height: 12,
        items,
        next: Some(vec![1, 2]),
    }
}

fn first() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

fn blob(byte: u8) -> BlobId {
    BlobId::Sha256([byte; 32])
}

#[test]
fn chat_round_trips() {
    let chat = roots(interface::<chat::Chat>());
    let posted = chat.op(&chat::Op::PostMessage {
        channel_id: "general".into(),
        message_id: "m1".into(),
        blocks: chat::parse_message("hi **all** <@3>\n\n- one\n- two"),
        thread: Some(4),
    });
    assert_eq!(posted["PostMessage"]["channel_id"], "general");
    assert_eq!(posted["PostMessage"]["thread"], 4);
    chat.op(&chat::Op::CreateChannel {
        channel_id: "c".into(),
        name: "c".into(),
        post_policy: chat::PostPolicy::MembersOnly,
    });
    chat.op(&chat::Op::SetMembership {
        channel_id: "c".into(),
        principal: Principal::Account(3),
        member: true,
    });
    let asked = chat.query(&chat::Query::Search {
        text: "hi".into(),
        viewer: vec![Principal::Account(3), Principal::Root],
        channel_id: None,
        page: first(),
    });
    assert_eq!(
        asked,
        json!({ "Search": {
            "text": "hi",
            "viewer": [{ "Account": 3 }, "Root"],
            "channel_id": null,
            "page": { "after": null, "limit": 20 },
        } })
    );
    chat.query(&chat::Query::Channels { page: first() });
    chat.reply(&chat::Reply::Channel(None));
    chat.reply(&chat::Reply::Accounts(page(vec![Profile {
        number: 3,
        name: "Scout".into(),
        kind: Kind::Program {
            manager: 1,
            category: Category::Agent,
            standing: Standing::Active,
            script: Some(Script {
                calls: "harness".into(),
                config: "/home/Ada/scout.md".into(),
            }),
        },
    }])));
    chat.shapes();
}

#[test]
fn access_round_trips() {
    let access = roots(interface::<access::Access>());
    let set = access.op(&access::Op::Set {
        program: "files".into(),
        key: "/shared".into(),
        list: Some(Ownership::Listed(vec![
            Party::Account(2),
            Party::Program("harness".into()),
        ])),
    });
    assert_eq!(
        set["Set"]["list"],
        json!({ "Listed": [{ "Account": 2 }, { "Program": "harness" }] })
    );
    access.op(&access::Op::Set {
        program: "harness".into(),
        key: "agents/4".into(),
        list: Some(Ownership::Anyone),
    });
    access.query(&access::Query::Writers {
        program: "files".into(),
        key: "/shared/a.md".into(),
    });
    access.reply(&access::Reply::List(Some(Ownership::Owner)));
    access.shapes();
}

#[test]
fn files_round_trips() {
    let files = roots(interface::<files::Files>());
    let commit = files.op(&files::Op::Commit {
        base: Some(blob(7)),
        message: "add".into(),
        changes: vec![
            files::Change::Put {
                path: "/shared/a.md".into(),
                exec: false,
                meta: [("mime".to_owned(), "text/markdown".to_owned())].into(),
                content: files::Content::Chunks {
                    size: 5,
                    chunks: vec![blob(8)],
                },
            },
            files::Change::Mkdir {
                path: "/shared/b".into(),
            },
            files::Change::Remove {
                path: "/tmp".into(),
            },
        ],
    });
    assert_eq!(
        commit["Commit"]["base"],
        json!({ "Sha256": "07".repeat(32) })
    );
    assert_eq!(
        commit["Commit"]["changes"][0]["Put"]["meta"],
        json!([["mime", "text/markdown"]])
    );
    assert_eq!(
        commit["Commit"]["changes"][1]["Mkdir"]["path"],
        json!("/shared/b")
    );
    files.op(&files::Op::Unpin { name: "v1".into() });
    files.query(&files::Query::Read {
        path: "/shared/a.md".into(),
        at: None,
        offset: 0,
        len: 5,
    });
    files.query(&files::Query::Head);
    files.reply(&files::Reply::Pin(None));
    files.reply(&files::Reply::Diff(Vec::new()));
    files.shapes();
}

#[test]
fn forge_round_trips() {
    let forge = roots(interface::<forge::Forge>());
    forge.op(&forge::Op::Create {
        repo: "core".into(),
        hash: abi::HashKind::Sha1,
    });
    forge.op(&forge::Op::Grant {
        repo: "core".into(),
        principal: Principal::Account(2),
    });
    forge.query(&forge::Query::Repos { page: first() });
    forge.query(&forge::Query::Blob {
        repo: "core".into(),
        oid: "ab".into(),
        range: None,
    });
    forge.reply(&forge::Reply::Activity {
        height: 3,
        last_height: 2,
    });
    forge.shapes();
}

#[test]
fn identity_round_trips() {
    let identity = roots(interface::<identity::Identity>());
    identity.op(&identity::Op::Create {
        name: "Ada".into(),
        scheme: Scheme::Ed25519,
    });
    identity.op(&identity::Op::SetScript {
        account: 4,
        script: Some(Script {
            calls: "harness".into(),
            config: "/a.md".into(),
        }),
    });
    identity.query(&identity::Query::Profiles {
        after: Some(3),
        limit: 10,
    });
    identity.reply(&identity::Reply::Profiles {
        profiles: vec![Profile {
            number: 1,
            name: "Ada".into(),
            kind: Kind::User,
        }],
        next: None,
    });
    identity.shapes();
}

#[test]
fn programs_round_trips() {
    let programs = roots(interface::<::programs::Programs>());
    let entry = ::programs::Entry {
        program: "chat".into(),
        code: blob(1),
        params: vec![9],
    };
    programs.op(&::programs::Op::Schedule(::programs::Scheduled {
        height: 100,
        change: ::programs::Change::Set(entry.clone()),
    }));
    programs.op(&::programs::Op::Grant(::programs::Grant {
        scope: ::programs::Scope::All,
        account: 2,
    }));
    assert_eq!(programs.query(&::programs::Query::Roster), json!("Roster"));
    programs.query(&::programs::Query::Program("chat".into()));
    programs.reply(&::programs::Reply::Roles(::program::MockHost::roles()));
    programs.reply(&::programs::Reply::Roster {
        height: 5,
        programs: vec![entry],
        views: Vec::new(),
    });
    programs.shapes();
}

#[test]
fn valset_round_trips() {
    let valset = roots(interface::<valset::Valset>());
    valset.op(&valset::Op::Enroll {
        address: None,
        halo: "node.example.org:9100".into(),
        invite: None,
    });
    valset.op(&valset::Op::Vote(valset::Motion::Door { open: true }));
    valset.query(&valset::Query::Validators);
    valset.reply(&valset::Reply::Validators(vec![vec![1; 32]]));
    valset.shapes();
}

#[test]
fn blobs_round_trips() {
    let blobs = roots(interface::<blobs::Blobs>());
    let entry = blobs::Entry {
        id: blob(2),
        len: 10,
        kind: "chunk".into(),
    };
    blobs.op(&blobs::Op::Put {
        blobs: vec![entry.clone()],
    });
    blobs.op(&blobs::Op::Receipt { bundle: 3 });
    blobs.query(&blobs::Query::Held { blob: blob(2) });
    blobs.query(&blobs::Query::Bundle { n: 3 });
    blobs.reply(&blobs::Reply::Held(Some(blobs::Holding {
        bundle: 3,
        len: 10,
        kind: "chunk".into(),
        since: 4,
    })));
    blobs.reply(&blobs::Reply::Bundle(Some(blobs::Bundle {
        putter: 2,
        height: 3,
        blobs: vec![entry.clone()],
        custody: blobs::Custody::Receipted(vec![vec![5; 32]]),
    })));
    blobs.reply(&blobs::Reply::Bundle(Some(blobs::Bundle {
        putter: 2,
        height: 3,
        blobs: vec![entry],
        custody: blobs::Custody::Held { since: 9 },
    })));
    blobs.reply(&blobs::Reply::Held(None));
    blobs.shapes();
}

#[test]
fn awaits_round_trips() {
    let awaits = roots(interface::<awaits::Awaits>());
    awaits.op(&awaits::Op::Resolve {
        ticket: 2,
        answer: Err(abi::Refusal::new(abi::reason::INVALID_INPUT, "no")),
    });
    awaits.op(&awaits::Op::Park {
        frame: 1,
        program: "harness".into(),
        calls: vec![awaits::Call {
            ticket: 1,
            target: "egress".into(),
        }],
    });
    awaits.query(&awaits::Query::Owed {
        target: "egress".into(),
        after: None,
        limit: 5,
    });
    awaits.reply(&awaits::Reply::Ticket(None));
    awaits.shapes();
}

#[test]
fn halo_round_trips() {
    let halo = roots(interface::<halo::Halo>());
    halo.op(&halo::Op::Open {
        name: "ducktape/live".into(),
        cap: halo::Class::Client,
    });
    halo.query(&halo::Query::Lanes);
    halo.reply(&halo::Reply::Lane(Some(halo::Lane {
        name: "ducktape/blobs".into(),
        id: 0,
        cap: halo::Class::Critical,
        enabled: true,
        pinned: true,
        since: 1,
    })));
    halo.shapes();
}

#[test]
fn scheduler_round_trips() {
    let scheduler = roots(interface::<scheduler::Scheduler>());
    scheduler.op(&scheduler::Op::Schedule {
        height: 9,
        target: "harness".into(),
        payload: vec![1, 2, 3],
    });
    scheduler.query(&scheduler::Query::Due {
        height: 9,
        after: None,
        limit: Some(4),
    });
    scheduler.reply(&scheduler::Reply::Entry(Some(scheduler::Entry {
        id: 1,
        scheduler: "scheduler".into(),
        height: 9,
        target: "harness".into(),
        payload: Vec::new(),
        receipt: None,
    })));
    scheduler.shapes();
}

#[test]
fn activity_round_trips() {
    let activity = roots(interface::<activity::Activity>());
    assert_eq!(activity.op(&()), json!(null));
    activity.query(&activity::Query::Acted {
        account: 3,
        page: first(),
    });
    activity.reply(&activity::Reply::Frames(page(vec![activity::Frame {
        height: 1,
        position: 0,
        time: 5,
        signer: vec![1; 32],
        target: "chat".into(),
        applied: true,
    }])));
    activity.shapes();
}

#[test]
fn egress_round_trips() {
    let egress = roots(interface::<egress::Egress>());
    egress.op(&egress::Op::Request {
        request: egress::Request::Http {
            service: "ccs".into(),
            head: egress::http::Head {
                method: egress::http::Method::Post,
                target: "/v1/messages".into(),
                headers: vec![("content-type".into(), "application/json".into())],
            },
            body: vec![blob(3)],
        },
        delivery: egress::Delivery::Events { deliver: vec![1] },
        wait: Some(64),
    });
    egress.op(&egress::Op::EndStream {
        id: 1,
        events: 2,
        end: egress::StreamEnd::Complete {
            status: 200,
            headers: Vec::new(),
        },
    });
    egress.query(&egress::Query::Services { page: first() });
    egress.reply(&egress::Reply::Serving(vec![vec![1; 32]]));
    egress.shapes();
}

#[test]
fn attribution_round_trips() {
    let attribution = roots(interface::<attribution::Attribution>());
    let source = attribution::Source {
        program: "chat".into(),
        object: attribution::Object {
            kind: "message".into(),
            id: "general/1".into(),
        },
    };
    attribution.op(&attribution::Op::Attribute(attribution::Update {
        object: source.object.clone(),
        revision: 1,
        actor: attribution::Actor::Account(1),
        relations: vec![attribution::Relation {
            recipient: 3,
            reason: attribution::Reason::Defined("review".into()),
            detail: Vec::new(),
        }],
        transfers: Vec::new(),
    }));
    attribution.query(&attribution::Query::ChangesOf {
        source: source.clone(),
        page: first(),
    });
    attribution.reply(&attribution::Reply::Changes(page(vec![
        attribution::Change {
            seq: 1,
            source,
            revision: 1,
            recipient: 3,
            reason: attribution::Reason::Mention,
            kind: attribution::Kind::TransferredIn { from: 2 },
            detail: vec![7],
            actor: attribution::Actor::System,
            cause: Cause::Reply {
                id: MessageId {
                    program: "chat".into(),
                    seq: 4,
                },
                outcome: Outcome::Rejected(::program::Error::new("invalid_input", "no")),
            },
            height: 6,
        },
    ])));
    attribution.shapes();
}

#[test]
fn harness_round_trips() {
    let harness = roots(interface::<crate::Harness>());
    harness.op(&crate::Op::SetModel {
        id: "claude".into(),
        wire: crate::Wire::AnthropicMessages,
        service: "ccs".into(),
        name: "claude-test".into(),
    });
    harness.op(&crate::Op::Streamed {
        thread: 1,
        event: crate::Event {
            id: 1,
            seq: 0,
            data: b"data".to_vec(),
        },
    });
    harness.query(&crate::Query::Tools);
    harness.query(&crate::Query::Threads {
        agent: 3,
        page: first(),
    });
    let tools = harness.reply(&crate::Reply::Tools(crate::tools::infos()));
    assert_eq!(
        tools["Tools"].as_array().map(Vec::len),
        Some(crate::tools::CATALOG.len())
    );
    harness.shapes();
}
