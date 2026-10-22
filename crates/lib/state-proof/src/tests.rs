use std::collections::BTreeMap;

use commonware_runtime::{Runner as _, Supervisor as _, deterministic};
use commonware_storage::merkle::Location;
use commonware_utils::NZU64;
use state::{Commitment, Slot};

use super::*;

type Ctx = deterministic::Context;

const KEY: &[u8] = b"out/packet";
const VALUE: &[u8] = b"the packet's bytes";

fn writes(rows: &[(&[u8], Option<&[u8]>)]) -> BTreeMap<Vec<u8>, Slot> {
    rows.iter()
        .map(|(key, value)| (key.to_vec(), value.map(<[u8]>::to_vec)))
        .collect()
}

fn first_rows() -> BTreeMap<Vec<u8>, Slot> {
    writes(&[
        (b"a", Some(b"first")),
        (KEY, Some(VALUE)),
        (b"z", Some(b"last")),
    ])
}

async fn location_of(commitment: &Commitment<Ctx>, key: &[u8], value: &[u8]) -> u64 {
    let db = commitment.db().unwrap();
    let wanted = update(key, value).unwrap();
    let bounds = db.bounds();
    for location in *bounds.start..*bounds.end {
        let (_, operations) = db.proof(Location::new(location), NZU64!(1)).await.unwrap();
        if operations[0].encode().as_ref() == wanted.as_slice() {
            return location;
        }
    }
    panic!("the state crate committed no such write")
}

struct Proven {
    root: [u8; 32],
    location: u64,
    proof: Vec<u8>,
}

async fn proven(commitment: &Commitment<Ctx>, size: Option<u64>) -> Proven {
    let location = location_of(commitment, KEY, VALUE).await;
    let db = commitment.db().unwrap();
    let (proof, operations) = match size {
        Some(size) => db
            .historical_proof(Location::new(size), Location::new(location), NZU64!(1))
            .await
            .unwrap(),
        None => db.proof(Location::new(location), NZU64!(1)).await.unwrap(),
    };
    assert_eq!(
        operations[0].encode().as_ref(),
        update(KEY, VALUE).unwrap().as_slice()
    );
    Proven {
        root: commitment.root().unwrap().0,
        location,
        proof: proof.encode().to_vec(),
    }
}

#[test]
fn a_write_the_state_crate_commits_is_held_at_its_location_under_its_root() {
    deterministic::Runner::default().start(|context| async move {
        let mut commitment = Commitment::open(context.child("a"), "net-crosspond", 8)
            .await
            .unwrap();
        commitment.apply(1, &first_rows()).await.unwrap();
        let Proven {
            root,
            location,
            proof,
        } = proven(&commitment, None).await;
        assert!(holds(&root, location, &proof, KEY, VALUE));
    });
}

#[test]
fn another_value_key_location_or_root_is_not_held() {
    deterministic::Runner::default().start(|context| async move {
        let mut commitment = Commitment::open(context.child("a"), "net-crosspond", 8)
            .await
            .unwrap();
        commitment.apply(1, &first_rows()).await.unwrap();
        let Proven {
            root,
            location,
            proof,
        } = proven(&commitment, None).await;
        assert!(!holds(&root, location, &proof, KEY, b"other bytes"));
        assert!(!holds(&root, location, &proof, b"out/other", VALUE));
        assert!(!holds(&root, location + 1, &proof, KEY, VALUE));
        let mut other = root;
        other[0] ^= 1;
        assert!(!holds(&other, location, &proof, KEY, VALUE));
    });
}

#[test]
fn bytes_that_are_not_one_proof_hold_nothing() {
    deterministic::Runner::default().start(|context| async move {
        let mut commitment = Commitment::open(context.child("a"), "net-crosspond", 8)
            .await
            .unwrap();
        commitment.apply(1, &first_rows()).await.unwrap();
        let Proven {
            root,
            location,
            proof,
        } = proven(&commitment, None).await;
        let cut = &proof[..proof.len() - 1];
        let padded = [proof.as_slice(), &[0]].concat();
        for bytes in [cut, padded.as_slice(), &[]] {
            assert!(!holds(&root, location, bytes, KEY, VALUE));
        }
    });
}

#[test]
fn a_proof_at_an_earlier_size_holds_against_that_sizes_root_after_the_key_is_deleted() {
    deterministic::Runner::default().start(|context| async move {
        let mut commitment = Commitment::open(context.child("a"), "net-crosspond", 8)
            .await
            .unwrap();
        commitment.apply(1, &first_rows()).await.unwrap();
        let then = commitment.root().unwrap().0;
        let size = *commitment.db().unwrap().bounds().end;
        let later = writes(&[(KEY, None), (b"m", Some(b"middle"))]);
        commitment.apply(2, &later).await.unwrap();
        let Proven {
            root,
            location,
            proof,
        } = proven(&commitment, Some(size)).await;
        assert_ne!(root, then);
        assert!(holds(&then, location, &proof, KEY, VALUE));
        assert!(!holds(&root, location, &proof, KEY, VALUE));
    });
}
