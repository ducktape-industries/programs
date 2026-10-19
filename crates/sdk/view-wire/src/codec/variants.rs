//! `Node`'s `Serialize`: the shape `Deserialize` derives, each variant
//! written by a function of its own.

use serde::Serialize;
use serde::ser::{SerializeStructVariant, Serializer};

use crate::Interactivity;
use crate::node::{Node, NodeDiscriminants};

// A field written through a wrapper, as a derived field's `with` is.
macro_rules! wrapped {
    ($field:ident) => {
        $field
    };
    ($field:ident: $wrap:ident) => {
        &$wrap($field)
    };
}

// A derived `Serialize` is one function over every variant's fields, and
// unoptimized its frame keeps a slot for each (some 30 KB), which a tree
// takes once per level: 64 levels outgrew a test thread's 2 MiB stack.
// Here the match picks the variant and a variant's function holds its own
// fields alone. A variant lists its fields in declaration order: the order
// the wire writes them in and the derived `Deserialize` reads them.
macro_rules! by_variant {
    (
        unit: [$($unit:ident),* $(,)?],
        newtype: [$($newtype:ident),* $(,)?],
        fields: [$($variant:ident { $($field:ident $(: $wrap:ident)?),* $(,)? }),* $(,)?] $(,)?
    ) => {
        pub(super) fn serialize<S: Serializer>(
            node: &Node,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            let index = NodeDiscriminants::from(node) as u32;
            match node {
                $(Node::$unit => serializer.serialize_unit_variant("Node", index, stringify!($unit)),)*
                $(Node::$newtype(inner) => {
                    serializer.serialize_newtype_variant("Node", index, stringify!($newtype), inner)
                })*
                $(Node::$variant { .. } => $variant(node, serializer, index),)*
                Node::Unknown { .. } => Err(serde::ser::Error::custom(
                    "a node this build could not read never crosses",
                )),
            }
        }

        $(
            #[allow(non_snake_case)]
            fn $variant<S: Serializer>(
                node: &Node,
                serializer: S,
                index: u32,
            ) -> Result<S::Ok, S::Error> {
                let Node::$variant { $($field),* } = node else {
                    unreachable!("`serialize` hands a variant to its own function")
                };
                let len = [$(stringify!($field)),*].len();
                let mut out =
                    serializer.serialize_struct_variant("Node", index, stringify!($variant), len)?;
                $(out.serialize_field(stringify!($field), wrapped!($field $(: $wrap)?))?;)*
                out.end()
            }
        )*
    };
}

// An element's interactivity, as `style::optional` writes it.
struct Optional<'a>(&'a Option<Box<Interactivity>>);

impl Serialize for Optional<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        crate::style::optional::serialize(self.0, serializer)
    }
}

by_variant! {
    unit: [Space],
    newtype: [Container, Text],
    fields: [
        RichText {
            id, style, text, runs, font_family_overrides, clickable_ranges, on_click, on_hover,
            tooltip,
        },
        Anchored { anchor, fit, position, position_mode, offset, children },
        UniformList {
            id, path, route, style, interactivity: Optional, count, measure_index, sizing,
            horizontal_sizing, y_flipped, scroll_request, revision, indices, children,
        },
        List {
            id, path, item_count, alignment, overdraw, sizing, following_tail, revision, commands,
            request_handler, scroll_handler, range_start, style, interactivity: Optional,
            children,
        },
        ResizeHandle {
            id, style, interactivity: Optional, on_press, on_release, on_drag, cursor, content,
        },
        Deferred { priority, content },
        Sensor { id, style, on_bounds, child },
        Image {
            id, hash, data, label, image_style, loading, fallback, state_children, style,
            interactivity: Optional,
        },
        Svg { id, source, transformation, label, style, interactivity: Optional },
        Field {
            id, multiline, value, cursor, generation, revision, tokens, claims, options,
            placeholder, secure, on_change, on_key, on_submit, style,
        },
        Overlay { id, label, style, on_dismiss, children },
        Sheet { id, label, style, detents, on_dismiss, child },
        Canvas { style, commands },
        Web { id, url, style, revision, commands, on_state },
        Camera { id, style, device, mirror, object_fit, on_state, publish },
        Microphone {
            id, style, device, output, monitor, revision, commands, on_state, publish, muted,
        },
        Remote { id, style, track, media, on_state },
    ],
}
