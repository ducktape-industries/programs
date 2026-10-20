//! Files over the fake node: the real `files` program seated beside the
//! boot set, its tree seeded through its own commits, the view opened as
//! the dev account (named `dev`, so its home is `/home/dev`). Tests by
//! what they cover: `model` (the rules both designs read), `desktop`
//! (the pages under a pointer), `phone` (the screens under a finger).
mod desktop;
mod model;
mod phone;

use files::{Change, Content, Op, Ownership, Party};
use view::methods::{HostSession, Session};
use view::rig::Network;
use view::testing::TestAppContext;

use crate::Files;

pub(crate) const NOTES: &str = "hello\nfrom dev";
pub(crate) const NOT_TEXT: [u8; 4] = [0xff, 0xfe, 0x00, 0x01];
pub(crate) const PNG: [u8; 67] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
];

pub(crate) const T0: u64 = 1_790_000_000_000;

pub(crate) fn put(net: &Network, path: &str, bytes: &[u8]) -> Change {
    let chunks = bytes
        .chunks(files::CHUNK_SIZE as usize)
        .map(|chunk| {
            net.put_blob(files::CHUNK, chunk.to_vec())
                .expect("a chunk blob")
        })
        .collect();
    Change::Put {
        path: path.into(),
        exec: false,
        meta: Default::default(),
        content: Content::Chunks {
            size: bytes.len() as u64,
            chunks,
        },
    }
}

pub(crate) fn commit(
    net: &Network,
    who: u64,
    message: &str,
    changes: Vec<Change>,
) -> files::BlobId {
    let head = match net.query(files::PROGRAM, &files::Query::Head) {
        Ok(files::Reply::Head(head)) => head,
        other => panic!("files answers its head: {other:?}"),
    };
    let op = Op::Commit {
        base: head.map(|row| row.id),
        message: message.into(),
        changes,
    };
    let output = net
        .submit(who, files::PROGRAM, &op)
        .unwrap_or_else(|refusal| panic!("{message}: {refusal:?}"));
    abi::decode(&output).expect("a commit answers its snapshot")
}

pub(crate) fn seeded() -> Network {
    let net = Network::new();
    net.seat::<files::Files>(files::PROGRAM);
    net.seat::<access::Access>(access::PROGRAM);
    net.seat::<ownership::Ownership>(ownership::PROGRAM);
    let dev = net.me();
    let ada = net.person("ada");
    net.at(T0);
    let first = commit(
        &net,
        dev,
        "Start my notes",
        vec![
            put(&net, "/home/dev/notes.txt", b"hello"),
            put(&net, "/home/dev/docs/plan.md", b"# Plan"),
        ],
    );
    net.at(T0 + 60_000);
    commit(
        &net,
        ada,
        "Share the readme",
        vec![
            put(&net, "/home/ada/diary.txt", b"dear diary"),
            put(&net, "/shared/readme.txt", b"read me"),
            put(&net, "/shared/blob.bin", &NOT_TEXT),
            put(&net, "/shared/dot.png", &PNG),
            Change::Symlink {
                path: "/shared/latest".into(),
                target: "/shared/readme.txt".into(),
            },
        ],
    );
    let only_ada = access::Op::Set {
        program: files::PROGRAM.into(),
        key: "/home/ada".into(),
        list: Some(Ownership::Owner),
    };
    net.submit(ada, access::PROGRAM, &only_ada)
        .expect("ada lists her home for herself");
    let guard = Op::Guard {
        path: "/home/ada".into(),
    };
    net.submit(ada, files::PROGRAM, &guard)
        .expect("ada guards her home");
    let only_me = ownership::Op::Create {
        name: "Only me".into(),
        write: ownership::Writers::Owner,
    };
    let made = net
        .submit(ada, ownership::PROGRAM, &only_me)
        .expect("ada makes a policy");
    let policy: ownership::PolicyId = abi::decode(&made).expect("a policy id");
    let relink = ownership::Op::Relink {
        program: files::PROGRAM.into(),
        key: files::guard_key("/home/ada"),
        policy,
    };
    net.submit(ada, ownership::PROGRAM, &relink)
        .expect("ada's home is hers alone");
    net.at(T0 + 120_000);
    commit(
        &net,
        dev,
        "Say more",
        vec![put(&net, "/home/dev/notes.txt", NOTES.as_bytes())],
    );
    net.at(T0 + 180_000);
    let pin = Op::Pin {
        name: "v1".into(),
        snapshot: first,
    };
    net.submit(dev, files::PROGRAM, &pin)
        .expect("a pin on a minted snapshot");
    net
}

pub(crate) fn list_at(net: &Network, path: &str) -> (Option<Ownership>, Option<Party>) {
    let key = path.to_owned();
    let program = files::PROGRAM.to_owned();
    let asked = access::Query::List {
        program: program.clone(),
        key: key.clone(),
    };
    let Ok(access::Reply::List(list)) = net.query(access::PROGRAM, &asked) else {
        panic!("access answers a path's list")
    };
    let asked = access::Query::Resource { program, key };
    let Ok(access::Reply::Resource(resource)) = net.query(access::PROGRAM, &asked) else {
        panic!("access answers who owns a path")
    };
    (list, resource.owner)
}

pub(crate) fn guard_at(net: &Network, path: &str) -> (Option<String>, Option<ownership::Writers>) {
    let asked = files::Query::Guarded { path: path.into() };
    let Ok(files::Reply::Guarded(at)) = net.query(files::PROGRAM, &asked) else {
        panic!("files answers a path's guard")
    };
    let asked = ownership::Query::Effective {
        program: files::PROGRAM.into(),
        key: files::guard_key(path),
    };
    let Ok(ownership::Reply::Policy(policy)) = net.query(ownership::PROGRAM, &asked) else {
        panic!("ownership answers a guard's policy")
    };
    (at, policy.map(|policy| policy.write))
}

pub(crate) fn file() -> files::Node {
    files::Node::File {
        file: files::BlobId::Sha256([0; 32]),
        size: 0,
        exec: false,
    }
}

pub(crate) fn opened_as(net: &Network, session: Session, touch: bool) -> TestAppContext {
    let mut cx = TestAppContext::new();
    net.back(&cx.host());
    let sessions = cx.host().stream::<HostSession>();
    cx.open::<Files>();
    cx.simulate_touch(touch);
    sessions.send(session);
    cx.run_until_parked();
    cx
}

pub(crate) fn desktop(net: &Network) -> TestAppContext {
    opened_as(net, net.session(), false)
}

pub(crate) fn desktop_at(net: &Network, width: f32) -> TestAppContext {
    let mut cx = desktop(net);
    cx.simulate_resize(width, 900.);
    cx.run_until_parked();
    cx
}

pub(crate) fn phone(net: &Network) -> TestAppContext {
    opened_as(net, net.session(), true)
}

pub(crate) fn shows(cx: &TestAppContext, part: &str) -> bool {
    cx.texts().iter().any(|text| text.contains(part))
}

// A markdown file: a heading, a paragraph with a relative link, a table
// whose first cell is bold, nested lists and a quote.
pub(crate) const GUIDE: &str = "# Guide

Read [the notes](../notes.txt) first.

| step | who |
|---|---|
| **plan** | dev |
| ship | ada |

- one
  - nested
- two

> quoted
";

// The seeded network with `GUIDE` at `/home/dev/docs/guide.md`.
pub(crate) fn with_guide() -> Network {
    let net = seeded();
    commit(
        &net,
        net.me(),
        "Write the guide",
        vec![put(&net, "/home/dev/docs/guide.md", GUIDE.as_bytes())],
    );
    net
}
