//! A byte range of a blob over `Bounds.blob_bytes`: read alone and judged text or binary on its
//! own bytes, while the blob read whole stays `Oversize`.
mod common;
use common::*;
use forge::{BlobView, ByteRange, Content};

fn seeded(content: &[u8]) -> (MemorySandbox, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let id = blob(&mut store, content);
    let head = file_commit(&mut store, &[], 1, &[("big", content)]);
    let sandbox = MemorySandbox::default();
    let narrow = Bounds {
        blob_bytes: 64,
        ..bounds()
    };
    sandbox.chain.init("forge", &narrow).unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let refs = [(Hash::Sha1.zero(), head, "refs/heads/main")];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    (sandbox, id)
}

fn query(id: Oid, range: Option<ByteRange>) -> Query {
    Query::Blob {
        repo: "project".into(),
        oid: id.to_hex(),
        range,
    }
}

fn answer(sandbox: &MemorySandbox, id: Oid, range: Option<ByteRange>) -> BlobView {
    match abi::decode(&ask(sandbox, &query(id, range)).unwrap()).unwrap() {
        Reply::Blob { blob, .. } => blob,
        other => panic!("{other:?}"),
    }
}

fn read(sandbox: &MemorySandbox, id: Oid, offset: u64, len: u64) -> BlobView {
    answer(sandbox, id, Some(ByteRange { offset, len }))
}

#[test]
fn a_range_of_a_blob_over_blob_bytes_reads_that_range_alone() {
    let text: Vec<u8> = (0..200u8).map(|i| b'a' + i % 26).collect();
    let (sandbox, id) = seeded(&text);
    let whole = answer(&sandbox, id, None);
    assert_eq!(whole.content, Content::Oversize);
    assert!(whole.bytes.is_empty());
    let middle = read(&sandbox, id, 100, 64);
    assert_eq!(middle.content, Content::Text);
    assert_eq!(middle.bytes, text[100..164]);
    assert_eq!(
        middle.range,
        ByteRange {
            offset: 100,
            len: 64
        }
    );
    let tail = read(&sandbox, id, 180, 64);
    assert_eq!(tail.bytes, text[180..]);
    assert_eq!(tail.range.len, 20);
    let long = ask(&sandbox, &query(id, Some(ByteRange { offset: 0, len: 65 })));
    assert_eq!(long.unwrap_err().code, code::INVALID_INPUT);
}

#[test]
fn a_range_is_text_or_binary_on_its_own_bytes() {
    let mut mixed = "é".repeat(50).into_bytes();
    mixed.extend_from_slice(&[0; 10]);
    mixed.extend_from_slice(&[b'z'; 90]);
    mixed[150] = 0xff;
    let (sandbox, id) = seeded(&mixed);
    let cut = read(&sandbox, id, 1, 64);
    assert_eq!(cut.content, Content::Text, "a code point cut at an edge");
    assert_eq!(cut.bytes, mixed[1..65]);
    let nul = read(&sandbox, id, 90, 64);
    assert_eq!(nul.content, Content::Binary);
    assert!(nul.bytes.is_empty());
    assert_eq!(
        nul.range,
        ByteRange {
            offset: 90,
            len: 64
        }
    );
    assert_eq!(read(&sandbox, id, 140, 20).content, Content::Binary);
    let after = read(&sandbox, id, 160, 64);
    assert_eq!(
        (after.content, after.bytes),
        (Content::Text, vec![b'z'; 40])
    );
}
