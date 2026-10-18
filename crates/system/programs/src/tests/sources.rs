// `Query::Sources`: what each running program's code says it was built from.

use ::program::section::{CUSTOM, MODULE};
use ::program::source::SECTION;
use ::program::{BlobId, MockChain, Roles};

use crate::{CODE_KIND, Entry, Genesis, PROGRAM, Programs, Provenance, Query, Reply, Source};

fn module(customs: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = MODULE.to_vec();
    out.extend([1, 1, 0]);
    for (name, payload) in customs {
        let mut body = vec![name.len() as u8];
        body.extend(name.as_bytes());
        body.extend(*payload);
        assert!(body.len() < 0x80, "one byte of LEB128 holds the size");
        out.push(CUSTOM);
        out.push(body.len() as u8);
        out.extend(body);
    }
    out
}

#[test]
fn each_program_says_the_commit_its_code_was_built_from() {
    let mut chain = MockChain::default();
    chain.seat::<Programs>(PROGRAM);
    let source = Source {
        repo: "programs".into(),
        commit: "4cd69803b2faa3996ce1ddadbbc9b1d6e7641133".into(),
    };
    let encoded = abi::encode(&source);
    let stamped = module(&[
        ("ducktape.view", b"view"),
        (SECTION, &encoded),
        ("ducktape.schema", b"after"),
    ]);
    let codes = [
        ("chat", Some(stamped)),
        ("plain", Some(module(&[("ducktape.view", b"view")]))),
        ("text", Some(b"not a module".to_vec())),
        ("broken", Some(module(&[(SECTION, b"\x05")]))),
        ("gone", None),
    ];
    let programs = codes
        .into_iter()
        .map(|(program, code)| Entry {
            program: program.into(),
            code: match code {
                Some(code) => chain.put_blob(CODE_KIND, code).unwrap(),
                None => BlobId::Sha256([9; 32]),
            },
            params: Vec::new(),
        })
        .collect();
    let genesis = Genesis {
        programs,
        views: Vec::new(),
        hooks: Vec::new(),
        roles: Roles::default(),
    };
    chain.init(PROGRAM, &genesis).unwrap();
    let Reply::Sources { programs, .. } = chain.query(PROGRAM, &Query::Sources).unwrap() else {
        panic!("Sources answers Sources")
    };
    let read: Vec<(String, Provenance)> = programs
        .into_iter()
        .map(|built| (built.program, built.source))
        .collect();
    let unread = "its ducktape.source section is no repository and commit";
    assert_eq!(
        read[0],
        ("broken".into(), Provenance::Unread(unread.into()))
    );
    assert_eq!(read[1], ("chat".into(), Provenance::Commit(source)));
    assert_eq!(read[2].0, "gone");
    assert!(
        matches!(&read[2].1, Provenance::Unread(why) if why.contains("is not held here")),
        "{read:?}"
    );
    assert_eq!(read[3], ("plain".into(), Provenance::Unstamped));
    assert_eq!(read[4], ("text".into(), Provenance::Unstamped));
}

#[test]
fn sources_is_asked_after_every_older_query() {
    assert_eq!(abi::encode(&Query::Roster), [6]);
    assert_eq!(abi::encode(&Query::Sources), [7]);
    let answered = Reply::Sources {
        height: 3,
        programs: Vec::new(),
    };
    assert_eq!(abi::encode(&answered)[0], 7);
}
