// The program natively over `program::MockHost`, with a real valset seated as the validators role.

use ::program::{Env, Error, HashKind, MockHost, Origin, Program, code};
use abi::BlobId;
use valset::{Genesis, Member, Valset};

use crate::{Blobs, Bundle, Custody, Entry, Holding, Op, PROGRAM, Query, Reply};

fn key(n: u8) -> Vec<u8> {
    vec![n; 32]
}

fn seated(validators: &[u8]) -> MockHost {
    let valset = MockHost::default();
    let genesis = Genesis {
        validators: validators
            .iter()
            .map(|n| Member {
                key: key(*n),
                address: None,
                halo: format!("node-{n}.example.org:9100"),
            })
            .collect(),
        member_cap: 16,
        queries: abi::role::validators::QueryPolicy::Signed,
    };
    Valset::init(
        &valset.exec(MockHost::env(valset::PROGRAM).root()),
        &abi::encode(&genesis),
    )
    .unwrap();
    valset
}

fn host(validators: &[u8]) -> MockHost {
    let host = MockHost::default();
    host.sibling::<Valset>(valset::PROGRAM, &seated(validators));
    host
}

fn entry(kind: &str, body: &[u8]) -> Entry {
    let framed = blob::frame(kind, body).unwrap();
    Entry::of(&framed).unwrap()
}

fn at(height: u64) -> Env {
    Env {
        height,
        ..MockHost::env(PROGRAM)
    }
}

fn put(host: &MockHost, blobs: Vec<Entry>) -> Result<u64, Error> {
    host.attempt(|| Blobs::execute(&host.exec(at(5)), Op::Put { blobs }))?;
    Ok(abi::decode(&host.take_output()).unwrap())
}

fn receipt(host: &MockHost, by: u8, bundle: u64) -> Result<(), Error> {
    let env = at(7).signed(key(by), None);
    host.attempt(|| Blobs::execute(&host.exec(env), Op::Receipt { bundle }))
}

fn ask(host: &MockHost, query: Query) -> Reply {
    Blobs::query(&host.query(MockHost::env(PROGRAM)), query).unwrap()
}

fn held(host: &MockHost, blob: BlobId) -> Option<Holding> {
    let Reply::Held(holding) = ask(host, Query::Held { blob }) else {
        panic!("Held answers Held");
    };
    holding
}

fn bundle(host: &MockHost, n: u64) -> Option<Bundle> {
    let Reply::Bundle(bundle) = ask(host, Query::Bundle { n }) else {
        panic!("Bundle answers Bundle");
    };
    bundle
}

#[test]
fn an_account_puts_numbered_bundles_and_a_bare_key_or_an_empty_put_is_refused() {
    let host = host(&[1]);
    let pack = entry("pack", b"PACK");
    let code_blob = entry("program", b"\0asm");
    assert_eq!(put(&host, vec![pack.clone(), code_blob.clone()]), Ok(0));
    assert_eq!(put(&host, vec![pack.clone()]), Ok(1));
    let first = bundle(&host, 0).unwrap();
    assert_eq!(first.putter, 1);
    assert_eq!(first.height, 5);
    assert_eq!(first.blobs, vec![pack.clone(), code_blob]);
    assert_eq!(first.custody, Custody::Receipted(Vec::new()));

    let bare = at(5).signed(key(9), None);
    let refused = host.refused(|| {
        Blobs::execute(
            &host.exec(bare),
            Op::Put {
                blobs: vec![pack.clone()],
            },
        )
    });
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert_eq!(
        put(&host, Vec::new()).unwrap_err().code,
        code::INVALID_INPUT
    );
    assert_eq!(bundle(&host, 2), None);
}

#[test]
fn a_bundle_is_held_once_a_quorum_of_current_validators_receipts_it() {
    let host = host(&[1, 2, 3, 4]);
    let pack = entry("pack", b"PACK");
    let n = put(&host, vec![pack.clone()]).unwrap();

    assert_eq!(receipt(&host, 9, n).unwrap_err().code, code::UNAUTHORIZED);
    receipt(&host, 1, n).unwrap();
    assert_eq!(receipt(&host, 1, n).unwrap_err().code, code::ALREADY_EXISTS);
    receipt(&host, 2, n).unwrap();
    assert_eq!(
        bundle(&host, n).unwrap().custody,
        Custody::Receipted(vec![key(1), key(2)])
    );
    assert_eq!(held(&host, pack.id), None);

    receipt(&host, 3, n).unwrap();
    assert_eq!(
        bundle(&host, n).unwrap().custody,
        Custody::Held { since: 7 }
    );
    assert_eq!(
        held(&host, pack.id),
        Some(Holding {
            bundle: n,
            len: pack.len,
            kind: "pack".into(),
            since: 7,
        })
    );
    assert_eq!(receipt(&host, 4, n).unwrap_err().code, code::WRONG_STATE);
    assert_eq!(receipt(&host, 1, 9).unwrap_err().code, code::NOT_FOUND);
    assert_eq!(held(&host, entry("pack", b"OTHER").id), None);
}

#[test]
fn a_receipt_from_a_key_no_longer_seated_does_not_count_toward_the_quorum() {
    let host = host(&[1, 2, 3, 4]);
    let n = put(&host, vec![entry("pack", b"PACK")]).unwrap();
    receipt(&host, 1, n).unwrap();
    receipt(&host, 2, n).unwrap();
    host.sibling::<Valset>(valset::PROGRAM, &seated(&[2, 3, 4, 5]));
    receipt(&host, 3, n).unwrap();
    assert!(!bundle(&host, n).unwrap().held());
    receipt(&host, 4, n).unwrap();
    assert!(bundle(&host, n).unwrap().held());
}

#[test]
fn a_blob_put_twice_is_held_by_whichever_bundle_reached_its_quorum() {
    let host = host(&[1]);
    let pack = entry("pack", b"PACK");
    let lone = put(&host, vec![pack.clone()]).unwrap();
    let later = put(&host, vec![entry("pack", b"OTHER"), pack.clone()]).unwrap();
    receipt(&host, 1, later).unwrap();
    assert_eq!(
        held(&host, pack.id).map(|holding| holding.bundle),
        Some(later)
    );
    assert!(!bundle(&host, lone).unwrap().held());
}

#[test]
fn a_put_naming_a_blob_a_program_put_with_another_kind_or_length_is_refused() {
    let host = host(&[1]);
    let wasm = b"\0asm".to_vec();
    let put_by_a_program = host.exec(at(4));
    let id = put_by_a_program
        .blob_put(HashKind::Sha256, "program", wasm.clone())
        .unwrap();
    let named = entry("program", &wasm);
    assert_eq!(named.id, id);
    let unframed = Entry {
        len: wasm.len() as u64,
        ..named.clone()
    };
    let other_kind = Entry {
        kind: "part".into(),
        ..named.clone()
    };
    for wrong in [unframed, other_kind] {
        let refused = host.refused(|| {
            Blobs::execute(
                &host.exec(at(5)),
                Op::Put {
                    blobs: vec![named.clone(), wrong],
                },
            )
        });
        assert_eq!(refused.code, code::INVALID_INPUT);
        assert!(refused.message.contains("never be held"), "{refused:?}");
    }
    assert_eq!(bundle(&host, 0), None);
    assert_eq!(put(&host, vec![named.clone()]), Ok(0));
    let uploaded = entry("pack", b"NOT PUT BY A PROGRAM");
    assert_eq!(put(&host, vec![uploaded]), Ok(1));
}

#[test]
fn a_frame_from_a_program_or_the_chain_does_not_receipt() {
    let host = host(&[1]);
    let n = put(&host, vec![entry("pack", b"PACK")]).unwrap();
    for origin in [Origin::Program("forge".into()), Origin::Root] {
        let env = Env { origin, ..at(7) };
        let refused = host.refused(|| Blobs::execute(&host.exec(env), Op::Receipt { bundle: n }));
        assert_eq!(refused.code, code::UNAUTHORIZED);
    }
}
