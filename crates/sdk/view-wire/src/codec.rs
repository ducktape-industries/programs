use crate::*;
use serde::{Deserialize, Serialize};

pub(crate) mod bin;
mod read;
mod variants;
mod write;
pub(crate) use write::SPARSE;

/// Bounds what a decode may descend into, since decoding is recursive: a
/// [`Node`] holds its children, a tooltip's content and a patch's subtree,
/// and serde builds them from the inside out, so a chain of nodes is a
/// chain of stack frames. The tree the host walks afterwards —
/// [`sanitize`], the renderer, `Drop` — recurses the same way, which is why
/// the limit sits at decode rather than in each walk, and on [`Node`]
/// itself rather than on the fields that happen to hold one.
///
/// It also keeps where the decode is, so a decode that fails inside a node
/// names that node: by its child indices from the top of the tree it
/// crossed in (a frame's root, a patch's node, a tooltip's content), as a
/// [`Patch`] path counts them. A view built against another wire than the
/// host's fails at the first node the two read differently.
mod budget {
    use std::cell::RefCell;

    use super::MAX_DEPTH;

    thread_local! {
        /// Per node being decoded, outermost first: how many nodes it has
        /// opened inside it so far. Its length is the depth.
        static OPEN: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
        /// Where the failing decode failed: the path of the innermost node
        /// the error passed through.
        static FAILED: RefCell<Option<Vec<u32>>> = const { RefCell::new(None) };
    }

    pub(super) struct Node(());

    impl Node {
        pub(super) fn enter() -> Result<Self, &'static str> {
            OPEN.with_borrow_mut(|open| {
                // The nodes this one sits inside: a decode's outermost node
                // is at 0, as `sanitize` counts depth.
                if open.len() > MAX_DEPTH {
                    return Err("a tree deeper than the host renders");
                }
                match open.last_mut() {
                    Some(opened) => *opened += 1,
                    // a decode left over by a panic says nothing about this one
                    None => FAILED.set(None),
                }
                open.push(0);
                Ok(Self(()))
            })
        }

        /// `error` came out of this node: the innermost node it came out of
        /// is the one named, and the outermost names it in the message.
        pub(super) fn failed<E: serde::de::Error>(&self, error: E) -> E {
            let outermost = OPEN.with_borrow(|open| {
                FAILED.with_borrow_mut(|failed| {
                    if failed.is_none() {
                        // each node's index is one less than the count its
                        // parent had reached when it opened
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

/// `Node`'s derived shape (`#[serde(remote = "Self")]`).
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

/// Decodes a sequence of at most `limit` elements and refuses a longer one
/// as `message`: a length header past the limit is refused before any
/// element is read, and nothing is reserved from the header.
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

/// Marks a derived struct as sparse: most of its fields say nothing most of
/// the time, each with a `skip_serializing_if`, and on the wire it is the
/// fields that say something, each keyed by its declaration index.
///
/// The struct derives with `#[serde(remote = "Self")]`, and this writes the
/// two trait impls over that derived pair, as `Node`'s are written by hand
/// above.
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

// The enums a fallback stands in for: where the reader meets a variant it
// does not know inside one, it steps over that enum's whole value and fails
// it with [`FELL`] and what it was, and the type's `Deserialize` answers its
// `Unknown` in its place ([`fell`]). By name, so no other deserializer (the
// goldens' JSON, the schema's tracer) sees anything but the derived shape.
pub(crate) const FALLBACKS: [&str; 2] = ["Node", "Event"];
pub(crate) const FELL: &str = "$view_wire::fell: ";

// What a fallback's value held that this build does not know, where `error`
// is the reader's [`FELL`]; `None` for any other failure.
pub(crate) fn fell(error: &impl std::fmt::Display) -> Option<String> {
    error.to_string().strip_prefix(FELL).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    /// A `Serialize` written by hand that refuses.
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

    /// A struct crosses as its fields in order and a variant as its index:
    /// no name is in the bytes.
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
        // {1: [7, nil]}
        assert_eq!(super::encode(&pair), [0x81, 0x01, 0x92, 0x07, 0xc0]);
        assert_eq!(super::encode(&Kind::Plain), [0x00]);
        // {2: "a"}
        assert_eq!(
            super::encode(&Kind::One("a".into())),
            [0x81, 0x02, 0xa1, b'a']
        );
        for kind in [pair, Kind::Plain, Kind::One("a".into())] {
            assert_eq!(super::decode::<Kind>(&super::encode(&kind)).unwrap(), kind);
        }
    }

    /// A decode that fails inside a node names that node by its child
    /// indices from the top of its tree, and the next decode starts clean.
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
        // the inner container's second child becomes a kind no one knows
        let at = bytes
            .windows(3)
            .rposition(|bytes| bytes == [0x92, space, space])
            .expect("the inner container's children")
            + 2;
        // a kind a later build added stands in as unknown, its siblings read
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
        // bytes that are no variant at all fail, naming the node
        bytes[at] = 0xc1;
        let error = super::decode::<Node>(&bytes).unwrap_err();
        assert!(error.starts_with("node at [1, 1]: "), "{error}");
        bytes[at] = space;
        assert_eq!(super::decode::<Node>(&bytes), Ok(tree));
    }

    /// A map or a sequence that does not say its length up front (gpui's
    /// `FontFeatures` writes one) is counted as it is written.
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

    /// A struct that leaves a field out, and never said it is sparse, would
    /// shift every field after it: the writer refuses it.
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
