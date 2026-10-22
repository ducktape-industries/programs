//! Whether a network's state held a write: a QMDB operation proof checked against one program's
//! root, as ducktape's state crate commits each write. The crosspond program links it for packets
//! and validator sets it takes from another network.
use commonware_codec::{Decode, Encode, varint::UInt};
use commonware_cryptography::sha256::Digest;
use commonware_cryptography::{Hasher, Sha256};
use commonware_storage::merkle::hasher::Standard;
use commonware_storage::merkle::{Bagging, Location, MAX_PROOF_DIGESTS_PER_ELEMENT, Proof, mmr};

const UPDATE: u8 = 0xD2;

pub fn update(key: &[u8], value: &[u8]) -> Option<Vec<u8>> {
    let leaf = abi::encode(&abi::Entry {
        key: key.to_vec(),
        value: value.to_vec(),
    });
    let length = UInt(u32::try_from(leaf.len()).ok()?).encode();
    Some(
        [
            [UPDATE].as_slice(),
            Sha256::hash(&[key]).as_ref(),
            length.as_ref(),
            leaf.as_slice(),
        ]
        .concat(),
    )
}

pub fn holds(root: &[u8; 32], location: u64, proof: &[u8], key: &[u8], value: &[u8]) -> bool {
    let Some(operation) = update(key, value) else {
        return false;
    };
    let Ok(proof) = Proof::<mmr::Family, Digest>::decode_cfg(proof, &MAX_PROOF_DIGESTS_PER_ELEMENT)
    else {
        return false;
    };
    let hasher = Standard::<Sha256>::new(Bagging::BackwardFold);
    proof.verify_range_inclusion(
        &hasher,
        &[operation],
        Location::new(location),
        &Digest(*root),
    )
}

#[cfg(test)]
mod tests;
