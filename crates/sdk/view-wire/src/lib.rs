//! The wire between a host and a view running in wasm.
//!
//! The view ships a WIDGET TREE, not a picture: every tick it returns the
//! [`Node`] its view built, with every value in it (text, colours, sizes),
//! and the host's own toolkit does layout, render, fonts, IME, clipboard and
//! scroll. The view never learns where anything landed, which is the point:
//! there is nothing in it to draw with.
//!
//! A style crosses once. A node names its style by [`StyleId`], an entry of
//! the table its tree crosses with ([`Frame::styles`]): the view's
//! [`Interner`] numbers each distinct style, and the host's [`Styles`] is
//! the table of the tree it holds, which it reads at render.
//!
//! Interaction goes back as MEANING, not input. An element's
//! [`Interactivity`] carries, per listener (`on_click`, `on_mouse_down`,
//! `on_key_down`, ...), the index of the handler the view registered this
//! frame; the host sends [`Event::Click`], [`Event::MouseDown`],
//! [`Event::KeyDown`] and the rest with that index and the view runs its
//! own handler. A text field ([`Node::Field`]) is the host engine's: it
//! sends [`Event::Text`] with the whole text it now holds, and the view
//! asks for an edit with [`WidgetCommand::Replace`]. A rich text's
//! clickable ranges answer with [`Event::Select`] and the index of the
//! range clicked.
//!
//! The types here are the one definition of the format: the view serializes
//! them and the host deserializes the same code, so a field neither side can
//! drop silently. No name crosses: a struct is its fields in order, a
//! variant its index, and a struct that mostly says nothing the fields it
//! sets (`codec/write.rs`). A host that reads a frame from an untrusted
//! module runs [`sanitize`] first.

/// The wire this build speaks, computed by `build.rs` from the committed
/// golden files: the bytes of what the fixtures sample (`frame.bin`,
/// `methods.bin`) and the shape of every type that crosses (`schema.txt`:
/// each field and variant the tree reaches, each method's kind, target,
/// request and reply). No one bumps it by hand: a shape change fails the
/// golden until it is regenerated, and regenerating moves it. A value's
/// grammar (a colour in a text run, a length in a style entry) and a
/// method's encode function count only as far as the fixtures' bytes sample
/// them. A view's manifest carries the
/// id it was built with, and a host refuses any other.
pub const WIRE_ID: &str = include!(concat!(env!("OUT_DIR"), "/wire_id.rs"));

// The wire level this build speaks: 1 since J-085, when every reader
// learned to step over what a later build appends (`codec.rs`, "Appends"),
// and one more with each append after it. A view's manifest carries it
// (line 6); a host reads every level, and sends a view only the events its
// level reads ([`Event::level`]). A view built before J-085 carries a
// [`WIRE_ID`] there, and is level 0. `tests/golden/levels/<level>.bin`
// keeps each level's frame, and every one must still read. Level 2:
// `op.put` takes a blob of any size. Level 3: `file.read`.
pub const WIRE_LEVEL: u32 = 3;

/// For `skip_serializing_if`: a value that says nothing is left out.
pub(crate) fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

pub mod abi;
pub mod manifest;
pub mod methods;
pub mod notifier;

mod sanitization;
pub use sanitization::SanitizeReport;

pub mod text;
pub use text::{
    CHIP, Edit, KeyClaim, TextChange, TextRange, TextToken, changed_span, rebase, validate_field,
};

mod image;
pub use image::ImageData;

mod aria;
pub mod click;
pub use aria::Aria;
/// gpui's own accessibility vocabulary, the one `Aria` and [`Event::A11yAction`] speak.
pub use gpui::accesskit::{Action, ActionData, AriaCurrent, HasPopup, Invalid, Live};
mod element_id;
pub use element_id::{ElementIdAtom, ElementIdWire, MAX_ELEMENT_ID_DEPTH};
mod style;
mod style_sanitize;
pub use style::{GroupRefinement, Interactivity};
mod styles;
pub use styles::{Interner, Style, StyleId, Styles, entry_fields};

mod qr;
pub use qr::{Qr, QrCorrection, QrSize, QrVersion};
mod rich_text;
pub use rich_text::{HighlightStyle as RichTextHighlightStyle, Runs as RichTextRuns};
mod canvas;
pub mod list;
pub use canvas::{
    CanvasCommand, CanvasLineCap, CanvasLineJoin, CanvasSegment, CanvasShape, CanvasStroke,
};
pub use list::{
    ListAlignment, ListCommand, ListKey, ListOffset, ListRequest, ListScroll, ListSizingBehavior,
};
mod widget;
pub use widget::{WidgetCommand, WidgetTarget};
pub mod web;
pub use web::{WebCommand, WebState};
pub mod media;
pub use media::{MediaState, MediaStatus, MicCommand, Playback, TrackState};

mod styled_nodes;
pub use styled_nodes::{ContainerNode, TextNode, TextTransform};
mod node;
pub use node::{
    Anchor, AnchoredFitMode, AnchoredPositionMode, ImageObjectFit, ImageStyle, Node, RemoteMedia,
    SvgSource, SvgTransformation, safe_asset_path,
};
mod accessibility;
pub use accessibility::{Fault, FaultKind, audit};
mod patch;
pub use patch::{Kept, Patch, apply, diff, diff_keeping, diff_taking};

pub mod interactivity;
pub mod keyboard;
pub mod mouse;
pub use interactivity::{DispatchPhase, HoverListenerMode, Tooltip, TooltipResponse};

mod protocol;
pub use protocol::{
    Autocapitalize, Error, Event, Frame, InputOptions, Request, RichTextHover, TextInput,
    TextInputAction, VirtualKeyboard, code,
};

mod frame_sanitize;
pub mod identity;
pub use frame_sanitize::{MAX_DEPTH, Refused, sanitize};
pub(crate) use frame_sanitize::{finite, sanitize_tree};

mod codec;
pub(crate) use codec::bounded_vec;
pub(crate) use codec::fell;
pub use codec::{decode, encode, try_encode};

#[cfg(test)]
mod tests;
