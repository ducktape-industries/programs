use crate::*;
use serde::{Deserialize, Serialize};

pub(crate) mod bin;
mod read;
mod variants;
mod write;
pub(crate) use write::SPARSE;

mod budget {
    use std::cell::RefCell;

    use super::MAX_DEPTH;

    thread_local! {
        static OPEN: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
        static FAILED: RefCell<Option<Vec<u32>>> = const { RefCell::new(None) };
    }

    pub(super) struct Node(());

    impl Node {
        pub(super) fn enter() -> Result<Self, &'static str> {
            OPEN.with_borrow_mut(|open| {
                if open.len() > MAX_DEPTH {
                    return Err("a tree deeper than the host renders");
                }
                match open.last_mut() {
                    Some(opened) => *opened += 1,
                    None => FAILED.set(None),
                }
                open.push(0);
                Ok(Self(()))
            })
        }

        pub(super) fn failed<E: serde::de::Error>(&self, error: E) -> E {
            let outermost = OPEN.with_borrow(|open| {
                FAILED.with_borrow_mut(|failed| {
                    if failed.is_none() {
                        let path = open[..open.len() - 1].iter().map(|opened| opened - 1);
                        *failed = Some(path.collect());
                    }
                });
                open.len() == 1
            });
            match outermost {
                true => E::custom(format_args!(
                    "node at {:?}: {error}",
                    FAILED.take().unwrap_or_default()
                )),
                false => error,
            }
        }
    }

    impl Drop for Node {
        fn drop(&mut self) {
            OPEN.with_borrow_mut(|open| open.pop());
        }
    }
}

impl Serialize for Node {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        variants::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let node = budget::Node::enter().map_err(serde::de::Error::custom)?;
        Node::deserialize(deserializer).or_else(|error| match fell(&error) {
            Some(what) => Ok(Node::Unknown { what }),
            None => Err(node.failed(error)),
        })
    }
}

pub(crate) fn bounded_vec<'de, D, T>(
    deserializer: D,
    limit: usize,
    message: &'static str,
) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Values<T>(usize, &'static str, std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Values<T> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str(self.1)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            if seq.size_hint().is_some_and(|len| len > self.0) {
                return Err(serde::de::Error::custom(self.1));
            }
            let mut values = Vec::new();
            while let Some(value) = seq.next_element()? {
                if values.len() == self.0 {
                    return Err(serde::de::Error::custom(self.1));
                }
                values.push(value);
            }
            Ok(values)
        }
    }
    deserializer.deserialize_seq(Values(limit, message, std::marker::PhantomData))
}

/// The bytes a wire value crosses as: see [`write`] for the encoding.
pub fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    value
        .serialize(write::Writer::new(&mut bytes))
        .expect("wire types are plain data");
    bytes
}

/// The bytes of a value whose serde is not the wire's own: a view's state,
/// whatever it derives or writes by hand. It is written with its field
/// names, so the build that reads it back may have added a field, and a
/// `Serialize` that refuses (a custom error) is an answer, not a panic
/// inside the view.
pub fn try_encode<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    value
        .serialize(&mut rmp_serde::Serializer::new(&mut bytes).with_struct_map())
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

macro_rules! sparse {
    ($type:ident) => {
        impl serde::Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                struct Fields<'a>(&'a $type);
                impl serde::Serialize for Fields<'_> {
                    fn serialize<S: serde::Serializer>(
                        &self,
                        serializer: S,
                    ) -> Result<S::Ok, S::Error> {
                        $type::serialize(self.0, serializer)
                    }
                }
                serializer.serialize_newtype_struct($crate::codec::SPARSE, &Fields(self))
            }
        }
        impl<'de> serde::Deserialize<'de> for $type {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                $type::deserialize(deserializer)
            }
        }
    };
}
pub(crate) use sparse;

// Reads what `encode` wrote, from this build or a later one (the
// appends below). The reader works over the slice itself: a string or a
// `bin` is lent from `bytes` and copied once, into the value that keeps it.
//
// APPENDS. A later build of this crate only appends: a field at its
// struct's end, with `#[serde(default)]`; a variant at its enum's end. So
// this reader takes a struct's fields it knows and steps over the rest, a
// sparse struct's keys it knows and steps over the rest, and fills a field
// it was not sent with its default. A variant this build does not know
// fails the value it is in, up to the nearest fallback ([`fallback`]):
// [`Node`] and [`Event`], which stand in `Unknown` for the value and say
// what was not known. A host or a view of an earlier build reads a later
// one's frame with no tree node or event lost but those.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, String> {
    let mut reader = read::Reader::new(bytes);
    let value = T::deserialize(&mut reader).map_err(|error| error.to_string())?;
    if reader.rest() != 0 {
        return Err("trailing MessagePack bytes".into());
    }
    Ok(value)
}

pub(crate) const FALLBACKS: [&str; 2] = ["Node", "Event"];
pub(crate) const FELL: &str = "$view_wire::fell: ";

pub(crate) fn fell(error: &impl std::fmt::Display) -> Option<String> {
    error.to_string().strip_prefix(FELL).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    struct Refusing;

    impl serde::Serialize for Refusing {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("not while a transfer is open"))
        }
    }

    #[test]
    fn a_views_own_serde_answers_instead_of_panicking() {
        assert_eq!(
            super::try_encode(&Refusing).unwrap_err(),
            "not while a transfer is open"
        );
        assert_eq!(super::try_encode(&7u8).unwrap(), super::encode(&7u8));
    }

    #[test]
    fn a_struct_is_its_fields_and_a_variant_is_its_index() {
        #[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
        enum Kind {
            Plain,
            Pair { left: u8, right: Option<bool> },
            One(String),
        }
        let pair = Kind::Pair {
            left: 7,
            right: None,
        };
        assert_eq!(super::encode(&pair), [0x81, 0x01, 0x92, 0x07, 0xc0]);
        assert_eq!(super::encode(&Kind::Plain), [0x00]);
        assert_eq!(
            super::encode(&Kind::One("a".into())),
            [0x81, 0x02, 0xa1, b'a']
        );
        for kind in [pair, Kind::Plain, Kind::One("a".into())] {
            assert_eq!(super::decode::<Kind>(&super::encode(&kind)).unwrap(), kind);
        }
    }

    #[test]
    fn a_decode_that_fails_inside_a_node_names_the_node() {
        use crate::{ContainerNode, Node, StyleId};
        let boxed = |children| {
            Node::Container(ContainerNode {
                id: None,
                style: StyleId(0),
                interactivity: Default::default(),
                children,
            })
        };
        let space = super::encode(&Node::Space)[0];
        let tree = boxed(vec![Node::Space, boxed(vec![Node::Space, Node::Space])]);
        let mut bytes = super::encode(&tree);
        let at = bytes
            .windows(3)
            .rposition(|bytes| bytes == [0x92, space, space])
            .expect("the inner container's children")
            + 2;
        bytes[at] = 0x7f;
        let Node::Container(ContainerNode { children, .. }) =
            super::decode::<Node>(&bytes).unwrap()
        else {
            panic!("the root reads")
        };
        assert!(
            matches!(children[1].children()[1], Node::Unknown { .. }),
            "{children:?}"
        );
        bytes[at] = 0xc1;
        let error = super::decode::<Node>(&bytes).unwrap_err();
        assert!(error.starts_with("node at [1, 1]: "), "{error}");
        bytes[at] = space;
        assert_eq!(super::decode::<Node>(&bytes), Ok(tree));
    }

    #[test]
    fn a_map_of_unknown_length_is_counted_as_it_is_written() {
        let features = gpui::FontFeatures(std::sync::Arc::new(vec![
            ("calt".into(), 1),
            ("liga".into(), 0),
        ]));
        let bytes = super::encode(&features);
        assert_eq!(bytes[..5], [0xdf, 0, 0, 0, 2]);
        assert_eq!(
            super::decode::<gpui::FontFeatures>(&bytes).unwrap(),
            features
        );

        struct Unsized;
        impl serde::Serialize for Unsized {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_seq((0..3u8).filter(|_| true))
            }
        }
        assert_eq!(super::encode(&Unsized), [0xdd, 0, 0, 0, 3, 0, 1, 2]);
    }

    #[test]
    #[should_panic(expected = "wire types are plain data")]
    fn a_struct_that_skips_a_field_must_be_sparse() {
        #[derive(serde::Serialize)]
        struct Skips {
            #[serde(skip_serializing_if = "Option::is_none")]
            first: Option<u8>,
            second: u8,
        }
        super::encode(&Skips {
            first: None,
            second: 1,
        });
    }
}
