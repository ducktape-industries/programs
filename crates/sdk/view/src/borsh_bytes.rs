//! A borsh value in a view's serde snapshot, as its bytes:
//! `#[serde(with = "view::borsh_bytes")]` on a field whose
//! type is a program's (borsh-only) type, kept as it came.
//!
//! The bytes go as one byte string (`serialize_bytes`): one call, their own
//! length. A `Vec<u8>` would go as serde writes any sequence, one call a byte
//! and up to two bytes each, and a phone snapshots its views every second.
use std::fmt;
use std::marker::PhantomData;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::de::{self, Visitor};
use serde::{Deserializer, Serializer};

pub fn serialize<T: BorshSerialize, S: Serializer>(value: &T, s: S) -> Result<S::Ok, S::Error> {
    let bytes = borsh::to_vec(value).map_err(serde::ser::Error::custom)?;
    s.serialize_bytes(&bytes)
}

pub fn deserialize<'de, T: BorshDeserialize, D: Deserializer<'de>>(d: D) -> Result<T, D::Error> {
    d.deserialize_bytes(Borsh(PhantomData))
}

struct Borsh<T>(PhantomData<T>);

impl<T: BorshDeserialize> Visitor<'_> for Borsh<T> {
    type Value = T;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a borsh value's bytes")
    }

    fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<T, E> {
        borsh::from_slice(bytes).map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Kept {
        #[serde(with = "super")]
        params: Vec<u8>,
    }

    #[test]
    fn a_borsh_value_is_kept_as_one_byte_string() {
        let kept = Kept {
            params: vec![0xff; 1 << 16],
        };
        let bytes = crate::wire::try_encode(&kept).unwrap();
        assert!(bytes.len() < (1 << 16) + 32, "{} bytes", bytes.len());
        assert_eq!(crate::wire::decode::<Kept>(&bytes).unwrap(), kept);
    }
}
