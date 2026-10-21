// Server flow tests: pack admission with closure checks, ref command decisions, and a full push.

#[path = "common/mod.rs"]
mod common;

use common::{blob, commit, file_tree, fixture, no_base, oid_list, oid_text, sha1, signature};
use gitcore::server::{
    Policy, RefUpdate, Refusal, admit_pack, apply_commands, push, valid_ref_name,
};
use gitcore::wire::pktline::{self, Pkt, Reader};
use gitcore::wire::receive::{RefCommand, advertise_refs};
use gitcore::{Error, Hash, Kind, Limits, MemoryObjects, Object, Objects, Oid, Tag, pack};
use std::collections::BTreeMap;

fn objects_of(pack_bytes: &[u8]) -> Vec<Object> {
    pack::read(pack_bytes, Hash::Sha1, &Limits::generous(), no_base)
        .unwrap()
        .into_iter()
        .map(|(_, object)| object)
        .collect()
}

fn base_store() -> MemoryObjects {
    let mut store = MemoryObjects::new(Hash::Sha1);
    admit_pack(
        &mut store,
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    store
}

#[test]
fn admit_stores_a_closed_pack() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let stored = admit_pack(
        &mut store,
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    let expected = oid_list(fixture!("base.oids"), Hash::Sha1);
    let mut sorted = stored;
    sorted.sort();
    assert_eq!(sorted, expected);
    assert_eq!(store.count(), expected.len());
}

#[test]
fn admit_rejects_a_pack_whose_closure_is_missing() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let result = admit_pack(
        &mut store,
        fixture!("ofs.pack"),
        Hash::Sha1,
        &Limits::generous(),
    );
    let Err(Error::MissingObject(_)) = result else {
        panic!("expected MissingObject, got {result:?}");
    };
    assert_eq!(store.count(), 0);
}

#[test]
fn admit_accepts_a_thin_pack_over_the_base() {
    let mut store = base_store();
    let before = store.count();
    let stored = admit_pack(
        &mut store,
        fixture!("thin.pack"),
        Hash::Sha1,
        &Limits::generous(),
    )
    .unwrap();
    assert_eq!(
        stored.len(),
        oid_list(fixture!("delta.oids"), Hash::Sha1).len()
    );
    assert_eq!(store.count(), before + stored.len());
    let tip = oid_text(fixture!("tip.oid"), Hash::Sha1);
    assert!(store.has(&tip).unwrap());
}

#[test]
fn admit_rejects_a_pack_of_the_other_hash() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let result = admit_pack(
        &mut store,
        fixture!("sha256.pack"),
        Hash::Sha1,
        &Limits::generous(),
    );
    assert_eq!(result, Err(Error::BadChecksum));
    let mut sha256 = MemoryObjects::new(Hash::Sha256);
    let stored = admit_pack(
        &mut sha256,
        fixture!("sha256.pack"),
        Hash::Sha256,
        &Limits::generous(),
    )
    .unwrap();
    assert!(stored.iter().all(|id| id.hash() == Hash::Sha256));
}

#[test]
fn admit_rejects_a_tree_pointing_at_a_missing_blob() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let mut scratch = MemoryObjects::new(Hash::Sha1);
    let missing = blob(&mut scratch, b"never pushed\n");
    let tree = gitcore::Tree {
        entries: vec![gitcore::TreeEntry {
            mode: gitcore::Mode::Regular,
            name: b"f".to_vec(),
            id: missing,
        }],
    };
    let pack_bytes =
        pack::write(vec![Object::new(Kind::Tree, tree.serialize())], Hash::Sha1).unwrap();
    assert_eq!(
        admit_pack(&mut store, &pack_bytes, Hash::Sha1, &Limits::generous()),
        Err(Error::MissingObject(missing))
    );
}

fn command(old: Oid, new: Oid, name: &str) -> RefCommand {
    RefCommand {
        old,
        new,
        name: name.as_bytes().to_vec(),
    }
}

#[test]
fn ref_command_decisions() {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let t = file_tree(&mut store, &[("f", "a\n")]);
    let root = commit(&mut store, t, &[], 1, "root");
    let child = commit(&mut store, t, &[root], 2, "child");
    let other = commit(&mut store, t, &[], 3, "other");
    let tag = Tag {
        object: child,
        kind: Kind::Commit,
        name: b"v1".to_vec(),
        tagger: Some(signature(4)),
        message: b"v1\n".to_vec(),
    };
    let tag_id = store.put(Kind::Tag, &tag.serialize()).unwrap();
    let zero = Hash::Sha1.zero();
    let unknown = sha1("9999999999999999999999999999999999999999");
    let mut refs = BTreeMap::new();
    refs.insert(b"refs/heads/main".to_vec(), root);
    let policy = Policy::default();

    let decided = apply_commands(
        &store,
        &refs,
        &[
            command(root, child, "refs/heads/main"),
            command(zero, other, "refs/heads/feature"),
            command(zero, tag_id, "refs/tags/v1"),
            command(zero, t, "refs/tags/tree"),
        ],
        &policy,
        100,
    )
    .unwrap();
    assert_eq!(decided[0].1, Ok(RefUpdate::Set(child)));
    assert_eq!(decided[1].1, Ok(RefUpdate::Set(other)));
    assert_eq!(decided[2].1, Ok(RefUpdate::Set(tag_id)));
    assert_eq!(decided[3].1, Ok(RefUpdate::Set(t)));

    let cases = [
        (command(child, other, "refs/heads/main"), Refusal::StaleOld),
        (
            command(root, other, "refs/heads/main"),
            Refusal::NonFastForward,
        ),
        (command(zero, other, "refs/heads/main"), Refusal::StaleOld),
        (command(root, unknown, "refs/heads/main"), Refusal::NotHeld),
        (
            command(root, zero, "refs/heads/main"),
            Refusal::DeleteRefused,
        ),
        (command(zero, t, "refs/heads/tree"), Refusal::NotACommit),
        (
            command(zero, tag_id, "refs/heads/tagged"),
            Refusal::NotACommit,
        ),
        (command(zero, zero, "refs/heads/nothing"), Refusal::Invalid),
        (
            command(zero, child, "refs/heads/bad..name"),
            Refusal::BadName,
        ),
        (command(zero, child, "HEAD"), Refusal::BadName),
    ];
    for (case, expected) in cases {
        let name = String::from_utf8_lossy(&case.name).into_owned();
        let decided = apply_commands(&store, &refs, &[case], &policy, 100).unwrap();
        assert_eq!(decided[0].1, Err(expected), "{name}");
    }
    let twice = apply_commands(
        &store,
        &refs,
        &[
            command(root, child, "refs/heads/main"),
            command(root, other, "refs/heads/main"),
        ],
        &policy,
        100,
    )
    .unwrap();
    assert_eq!(twice[1].1, Err(Refusal::Duplicate));

    let permissive = Policy {
        allow_force: true,
        allow_delete: true,
    };
    let forced = apply_commands(
        &store,
        &refs,
        &[
            command(root, other, "refs/heads/main"),
            command(zero, tag_id, "refs/heads/tagged"),
        ],
        &permissive,
        100,
    )
    .unwrap();
    assert_eq!(forced[0].1, Ok(RefUpdate::Set(other)));
    assert_eq!(forced[1].1, Err(Refusal::NotACommit));
    let deleted = apply_commands(
        &store,
        &refs,
        &[command(root, zero, "refs/heads/main")],
        &permissive,
        100,
    )
    .unwrap();
    assert_eq!(deleted[0].1, Ok(RefUpdate::Delete));
    let capped = apply_commands(
        &store,
        &refs,
        &[command(root, child, "refs/heads/main")],
        &policy,
        0,
    )
    .unwrap();
    assert_eq!(capped[0].1, Err(Refusal::CapReached));
}

#[test]
fn ref_name_rules() {
    assert!(valid_ref_name(b"refs/heads/main"));
    assert!(valid_ref_name(b"refs/tags/v1.0"));
    assert!(valid_ref_name(b"refs/heads/feature/x-y_z"));
    for bad in [
        &b"refs/heads/"[..],
        b"refs",
        b"refs/",
        b"main",
        b"refs/heads/.hidden",
        b"refs/heads/a..b",
        b"refs/heads/a.lock",
        b"refs/heads/a b",
        b"refs/heads/a~b",
        b"refs/heads/a^b",
        b"refs/heads/a:b",
        b"refs/heads/a?b",
        b"refs/heads/a*b",
        b"refs/heads/a[b",
        b"refs/heads/a\\b",
        b"refs/heads/a@{b",
        b"refs/heads/a//b",
        b"refs/heads/a.",
        b"refs/heads/a\x01b",
        b"refs/heads/a\x7fb",
    ] {
        assert!(!valid_ref_name(bad), "{}", String::from_utf8_lossy(bad));
    }
}

fn push_request(commands: &[(Oid, Oid, &str)], caps: &str) -> Vec<u8> {
    let mut request = Vec::new();
    for (index, (old, new, name)) in commands.iter().enumerate() {
        let mut line = format!("{old} {new} {name}").into_bytes();
        let first = index == 0;
        if first {
            line.push(0);
            line.extend_from_slice(caps.as_bytes());
        }
        pktline::push(&mut request, &line);
    }
    request.extend_from_slice(pktline::flush());
    request
}

fn report_lines(report: &[u8], sideband: bool) -> Vec<String> {
    let inner: Vec<u8> = if sideband {
        let mut reader = Reader::new(report);
        let Pkt::Data(data) = reader.next().unwrap().unwrap() else {
            panic!();
        };
        assert_eq!(data[0], 1);
        assert_eq!(reader.next().unwrap().unwrap(), Pkt::Flush);
        data[1..].to_vec()
    } else {
        report.to_vec()
    };
    Reader::new(&inner)
        .map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => String::from_utf8_lossy(pktline::strip_newline(data)).into_owned(),
            other => format!("{other:?}"),
        })
        .collect()
}

#[test]
fn full_push_flow_against_memory_objects() {
    let hash = Hash::Sha1;
    let mut store = MemoryObjects::new(hash);
    let mut refs: BTreeMap<Vec<u8>, Oid> = BTreeMap::new();
    let limits = Limits::generous();
    let policy = Policy::default();
    let zero = hash.zero();
    let base = oid_text(fixture!("base.oid"), hash);
    let tip = oid_text(fixture!("tip.oid"), hash);

    let advertised = advertise_refs(
        &refs,
        hash,
        &[b"report-status", b"side-band-64k", b"delete-refs"],
    );
    assert!(advertised.ends_with(
        b"capabilities^{}\0report-status side-band-64k delete-refs object-format=sha1\n0000"
    ));

    let caps = "report-status side-band-64k object-format=sha1 agent=git/2.55";
    let create = push_request(&[(zero, base, "refs/heads/main")], caps);
    let mut inline = create.clone();
    inline.extend_from_slice(fixture!("base.pack"));
    assert_eq!(
        push(
            &mut store, &refs, &inline, b"", hash, &limits, &policy, 1000
        ),
        Err(Error::BadRequest),
        "a pack rides beside the request, never inside it"
    );
    assert_eq!(store.count(), 0);
    let outcome = push(
        &mut store,
        &refs,
        &create,
        fixture!("base.pack"),
        hash,
        &limits,
        &policy,
        1000,
    )
    .unwrap();
    assert_eq!(outcome.unpack_error, None);
    assert_eq!(
        outcome.moves,
        vec![(b"refs/heads/main".to_vec(), RefUpdate::Set(base))]
    );
    assert_eq!(
        outcome.stored.len(),
        oid_list(fixture!("base.oids"), hash).len()
    );
    assert_eq!(
        report_lines(&outcome.report, true),
        ["unpack ok", "ok refs/heads/main", "Flush"]
    );
    for (name, update) in outcome.moves {
        let RefUpdate::Set(id) = update else { panic!() };
        refs.insert(name, id);
    }

    let fast_forward = push_request(&[(base, tip, "refs/heads/main")], caps);
    let outcome = push(
        &mut store,
        &refs,
        &fast_forward,
        fixture!("thin.pack"),
        hash,
        &limits,
        &policy,
        1000,
    )
    .unwrap();
    assert_eq!(
        outcome.moves,
        vec![(b"refs/heads/main".to_vec(), RefUpdate::Set(tip))]
    );
    assert_eq!(
        report_lines(&outcome.report, true),
        ["unpack ok", "ok refs/heads/main", "Flush"]
    );
    refs.insert(b"refs/heads/main".to_vec(), tip);

    let stale = push_request(&[(base, base, "refs/heads/main")], "report-status");
    let outcome = push(&mut store, &refs, &stale, b"", hash, &limits, &policy, 1000).unwrap();
    assert!(outcome.moves.is_empty());
    assert_eq!(
        outcome.refusals,
        vec![(b"refs/heads/main".to_vec(), Refusal::StaleOld)]
    );
    assert_eq!(
        report_lines(&outcome.report, false),
        ["unpack ok", "ng refs/heads/main stale info", "Flush"]
    );

    let rewind = push_request(&[(tip, base, "refs/heads/main")], "report-status");
    let outcome = push(
        &mut store, &refs, &rewind, b"", hash, &limits, &policy, 1000,
    )
    .unwrap();
    assert_eq!(
        outcome.refusals,
        vec![(b"refs/heads/main".to_vec(), Refusal::NonFastForward)]
    );

    let orphan_pack = pack::write(objects_of(fixture!("ofs.pack")), hash).unwrap();
    let mut empty = MemoryObjects::new(hash);
    let broken = push_request(&[(zero, tip, "refs/heads/main")], caps);
    let outcome = push(
        &mut empty,
        &BTreeMap::new(),
        &broken,
        &orphan_pack,
        hash,
        &limits,
        &policy,
        1000,
    )
    .unwrap();
    assert!(matches!(
        outcome.unpack_error,
        Some(Error::MissingObject(_))
    ));
    assert!(outcome.moves.is_empty());
    let lines = report_lines(&outcome.report, true);
    assert!(lines[0].starts_with("unpack missing object "));
    assert_eq!(lines[1], "ng refs/heads/main n/a (unpacker error)");
    assert_eq!(empty.count(), 0);

    let silent = push_request(&[(tip, zero, "refs/heads/main")], "");
    let outcome = push(
        &mut store,
        &refs,
        &silent,
        b"",
        hash,
        &limits,
        &Policy {
            allow_force: false,
            allow_delete: true,
        },
        1000,
    )
    .unwrap();
    assert_eq!(
        outcome.moves,
        vec![(b"refs/heads/main".to_vec(), RefUpdate::Delete)]
    );
    assert!(outcome.report.is_empty());

    assert_eq!(
        push(
            &mut store, &refs, b"garbage", b"", hash, &limits, &policy, 1000
        ),
        Err(Error::BadPktLine)
    );
}

#[test]
fn a_push_from_a_shallow_clone_moves_the_ref() {
    let hash = Hash::Sha1;
    let mut store = base_store();
    let base = oid_text(fixture!("base.oid"), hash);
    let tip = oid_text(fixture!("tip.oid"), hash);
    let refs = BTreeMap::from([(b"refs/heads/main".to_vec(), base)]);
    let mut request = Vec::new();
    pktline::push(&mut request, format!("shallow {base}\n").as_bytes());
    request.extend_from_slice(&push_request(
        &[(base, tip, "refs/heads/main")],
        "report-status",
    ));
    let outcome = push(
        &mut store,
        &refs,
        &request,
        fixture!("thin.pack"),
        hash,
        &Limits::generous(),
        &Policy::default(),
        1000,
    )
    .unwrap();
    assert_eq!(
        outcome.moves,
        vec![(b"refs/heads/main".to_vec(), RefUpdate::Set(tip))]
    );
    assert_eq!(
        report_lines(&outcome.report, false),
        ["unpack ok", "ok refs/heads/main", "Flush"]
    );
}
