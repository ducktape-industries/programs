//! Bytes on the wire: one MessagePack `bin`, its length and then the bytes
//! as they are. A byte field of a wire type says so with
//! `#[serde(with = "crate::codec::bin")]`; one that does not would cross as
//! an array of integers, a marker and a decode step for every byte, which
//! `schema.txt`'s test refuses by name.
//!
//! Written with one copy and read with one: [`decode`](crate::decode) lends
//! the `bin` from the bytes it was given, and the length is checked against
//! the field's bound before the copy that keeps it is made.

use serde::de::{Deserialize, Deserializer, Error, Visitor};
use serde::ser::{Serialize, Serializer};

pub(crate) trait Bin: Sized {
    fn write<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error>;
    fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error>;
}

pub(crate) fn serialize<T: Bin, S: Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value.write(serializer)
}

pub(crate) fn deserialize<'de, T: Bin, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<T, D::Error> {
    T::read(deserializer)
}

pub(crate) fn bounded<'de, D: Deserializer<'de>>(
    deserializer: D,
    limit: usize,
    message: &'static str,
) -> Result<Vec<u8>, D::Error> {
    struct Bytes(usize, &'static str);
    impl Visitor<'_> for Bytes {
        type Value = Vec<u8>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("bytes")
        }
        fn visit_bytes<E: Error>(self, bytes: &[u8]) -> Result<Vec<u8>, E> {
            if bytes.len() > self.0 {
                return Err(E::custom(self.1));
            }
            Ok(bytes.to_vec())
        }
    }
    match deserializer.is_human_readable() {
        true => crate::bounded_vec(deserializer, limit, message),
        false => deserializer.deserialize_bytes(Bytes(limit, message)),
    }
}

impl Bin for Vec<u8> {
    fn write<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self)
    }
    fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        bounded(deserializer, usize::MAX, "")
    }
}

pub(crate) fn serialize_fixed<const N: usize, S: Serializer>(
    bytes: &[u8; N],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_bytes(bytes)
}

pub(crate) fn fixed<const N: usize, E: Error>(bytes: Vec<u8>) -> Result<[u8; N], E> {
    <[u8; N]>::try_from(bytes)
        .map_err(|bytes| E::invalid_length(bytes.len(), &format!("{N} bytes").as_str()))
}

struct Inner<T>(T);

impl<T: Bin> Serialize for Inner<&T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.write(serializer)
    }
}

impl<'de, T: Bin> Deserialize<'de> for Inner<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::read(deserializer).map(Inner)
    }
}

impl<T: Bin> Bin for Option<T> {
    fn write<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().map(Inner).serialize(serializer)
    }
    fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<Inner<T>>::deserialize(deserializer)?.map(|inner| inner.0))
    }
}

impl<T: Bin, E: Serialize + for<'de> Deserialize<'de>> Bin for Result<T, E> {
    fn write<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.as_ref().map(Inner).serialize(serializer)
    }
    fn read<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Result::<Inner<T>, E>::deserialize(deserializer)?.map(|inner| inner.0))
    }
}
