use std::collections::BTreeMap;

use ::program::{HashKind, MockHost, code};
use borsh::BorshSerialize;
use borsh::schema::Definition;

use super::{ada, ask, head, host, text};
use crate::*;

const LIVE_OPS: [(&str, i64); 9] = [
    ("Commit", 1),
    ("Pin", 2),
    ("Unpin", 3),
    ("Watch", 4),
    ("Guard", 5),
    ("Unguard", 6),
    ("AdoptOwners", 7),
    ("Vacate", 8),
    ("HoldHead", 9),
];

const LIVE_CHANGES: [(&str, i64); 6] = [
    ("Put", 0),
    ("Mkdir", 1),
    ("Remove", 2),
    ("Move", 3),
    ("Symlink", 4),
    ("SetOwnership", 5),
];

const STAGE_TAG: u8 = 0;
const INLINE_TAG: u8 = 0;
const COMMIT_TAG: u8 = 1;
const PIN_TAG: u8 = 2;
const PUT_TAG: u8 = 0;
const CHUNKS_TAG: u8 = 1;

fn encoded<T: BorshSerialize>(value: &T) -> Vec<u8> {
    borsh::to_vec(value).unwrap()
}

fn tagged(tag: u8, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![tag];
    for field in fields {
        bytes.extend_from_slice(field);
    }
    bytes
}

fn tags_of(declaration: &str) -> Vec<(String, i64)> {
    let schema = ::program::schema::interface::<Files>();
    let Some(Definition::Enum { variants, .. }) = schema.get_definition(declaration) else {
        panic!("{declaration} is an enum in files' schema");
    };
    variants
        .iter()
        .map(|(tag, name, _)| (name.clone(), *tag))
        .collect()
}

fn put_bytes(path: &str, content: Vec<u8>) -> Vec<u8> {
    let meta: BTreeMap<String, String> = BTreeMap::new();
    tagged(
        PUT_TAG,
        &[
            encoded(&path.to_owned()),
            encoded(&false),
            encoded(&meta),
            content,
        ],
    )
}

fn commit_bytes(put: Vec<u8>) -> Vec<u8> {
    let base: Option<BlobId> = None;
    tagged(
        COMMIT_TAG,
        &[
            encoded(&base),
            encoded(&"by hand".to_owned()),
            encoded(&1u32),
            put,
        ],
    )
}

fn sent(host: &MockHost, bytes: &[u8]) -> Result<(), ::program::Error> {
    host.attempt(|| ::program::execute::<Files>(&host.exec(ada()), bytes))
}

#[test]
fn each_live_op_change_and_content_keeps_its_tag_in_the_schema() {
    let ops = tags_of("Op");
    for (name, tag) in LIVE_OPS {
        assert!(
            ops.contains(&(name.to_owned(), tag)),
            "{name} {tag} in {ops:?}"
        );
    }
    let changes = tags_of("Change");
    for (name, tag) in LIVE_CHANGES {
        let kept = changes.contains(&(name.to_owned(), tag));
        assert!(kept, "{name} {tag} in {changes:?}");
    }
    let contents = tags_of("Content");
    assert!(
        contents.contains(&("Chunks".to_owned(), i64::from(CHUNKS_TAG))),
        "{contents:?}"
    );
}

#[test]
fn a_commit_and_a_pin_written_as_bytes_land_and_read_back() {
    let host = host();
    let chunk = host
        .exec(ada())
        .blob_put(HashKind::Sha256, CHUNK, b"hi")
        .unwrap();
    let content = tagged(CHUNKS_TAG, &[encoded(&2u64), encoded(&vec![chunk])]);
    sent(&host, &commit_bytes(put_bytes("/shared/hi", content))).unwrap();
    assert_eq!(text(&host, "/shared/hi", None).unwrap(), "hi");
    let snapshot = head(&host).unwrap();
    let pin = tagged(
        PIN_TAG,
        &[encoded(&"by-hand".to_owned()), encoded(&snapshot)],
    );
    sent(&host, &pin).unwrap();
    let asked = Query::Pin {
        name: "by-hand".into(),
    };
    let Reply::Pin(Some(row)) = ask(&host, asked).unwrap() else {
        panic!("the pin reads back");
    };
    assert_eq!(row.snapshot, snapshot);
}

#[test]
fn a_stage_or_an_inline_content_is_refused_and_writes_nothing() {
    let host = host();
    let stage = tagged(STAGE_TAG, &[encoded(&b"x".to_vec())]);
    let inline_content = tagged(INLINE_TAG, &[encoded(&b"x".to_vec())]);
    let inline = commit_bytes(put_bytes("/shared/x", inline_content));
    for bytes in [stage, inline] {
        let refused = host.refused(|| ::program::execute::<Files>(&host.exec(ada()), &bytes));
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
        let undecoded = refused.message.starts_with("files: Op did not decode: ");
        assert!(undecoded, "{refused}");
    }
    assert_eq!(head(&host), None);
}
