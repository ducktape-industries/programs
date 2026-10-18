//! The widget tree itself: the [`Node`] enum every frame carries, and the
//! walks over it a host and the differ share.

use crate::*;
use serde::{Deserialize, Serialize};

/// The reference point used by the native GPUI anchored element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    TopCenter,
    BottomCenter,
    LeftCenter,
    RightCenter,
}

/// How an anchored child is kept inside the host viewport.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AnchoredFitMode {
    SnapToWindow,
    SnapToWindowWithMargin([f32; 4]),
    SwitchAnchor,
}

/// Coordinate space for an anchored position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnchoredPositionMode {
    Window,
    Local,
}

/// How a picture fills its box, an image's ([`ImageStyle`]) or a
/// camera's ([`Node::Camera`]): gpui's `ObjectFit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageObjectFit {
    Fill,
    Contain,
    Cover,
    ScaleDown,
    None,
}

impl From<gpui::ObjectFit> for ImageObjectFit {
    fn from(fit: gpui::ObjectFit) -> Self {
        match fit {
            gpui::ObjectFit::Fill => Self::Fill,
            gpui::ObjectFit::Contain => Self::Contain,
            gpui::ObjectFit::Cover => Self::Cover,
            gpui::ObjectFit::ScaleDown => Self::ScaleDown,
            gpui::ObjectFit::None => Self::None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImageStyle {
    pub grayscale: bool,
    pub object_fit: ImageObjectFit,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SvgSource {
    None,
    Data {
        hash: u64,
        #[serde(with = "crate::codec::bin")]
        bytes: Option<Vec<u8>>,
    },
    /// A file of the set the host bundles (`icons/hammer.svg`); a host
    /// draws it only if it is a [`safe_asset_path`].
    Asset(String),
    External(String),
}

/// Whether `path` names a file inside the set a host bundles
/// (`icons/hammer.svg`) and nothing outside it: not empty, no leading `/`,
/// no `\`, no `:`, and no empty, `.` or `..` segment. The one rule for an
/// [`SvgSource::Asset`] and a manifest's `icon`.
pub const fn safe_asset_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    // where the segment being read starts
    let mut start = 0;
    let mut i = 0;
    while i <= bytes.len() {
        if i == bytes.len() || bytes[i] == b'/' {
            // an empty path and a leading, doubled or trailing `/` are each
            // an empty segment
            let outside = match i - start {
                0 => true,
                1 => bytes[start] == b'.',
                2 => bytes[start] == b'.' && bytes[start + 1] == b'.',
                _ => false,
            };
            if outside {
                return false;
            }
            start = i + 1;
        } else if matches!(bytes[i], b'\\' | b':') {
            return false;
        }
        i += 1;
    }
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SvgTransformation {
    pub scale: [f32; 2],
    pub translate: [f32; 2],
    pub rotate: f32,
}

/// One widget. Retained elements carry their native typed identity across
/// frames for host state, focus, and accessibility ancestry.
///
/// The shape is derived; `Serialize` and `Deserialize` are the codec's
/// (`codec.rs`), which decodes every node inside the frame's budget. A
/// node's variant name, for a report, is `<&str>::from(&node)`.
#[derive(Clone, Debug, PartialEq, Deserialize, strum::IntoStaticStr, strum::EnumDiscriminants)]
#[serde(remote = "Self")]
#[strum_discriminants(vis(pub(crate)))]
pub enum Node {
    /// One native GPUI paragraph with optional interactive byte ranges.
    RichText {
        id: Option<ElementIdWire>,
        style: StyleId,
        text: String,
        runs: RichTextRuns,
        font_family_overrides: Vec<(std::ops::Range<usize>, gpui::SharedString)>,
        clickable_ranges: Vec<std::ops::Range<usize>>,
        on_click: Option<u32>,
        on_hover: Option<u32>,
        /// The route the host asks for the tooltip over a character
        /// ([`crate::Event::TooltipRequest`] with its index).
        tooltip: Option<u32>,
    },
    /// A native GPUI anchored element. The host owns fitting and clipping.
    Anchored {
        anchor: Anchor,
        fit: AnchoredFitMode,
        position: Option<[f32; 2]>,
        position_mode: AnchoredPositionMode,
        offset: Option<[f32; 2]>,
        children: Vec<Node>,
    },
    /// A GPUI uniform-height list. The host owns the native viewport; the
    /// view carries the rows of the window it lowered: a viewport-sized
    /// window around the rows the host shows, and the measurement row.
    UniformList {
        id: ElementIdWire,
        path: Vec<ElementIdWire>,
        route: u32,
        style: StyleId,
        #[serde(with = "crate::style::optional")]
        interactivity: Option<Box<Interactivity>>,
        count: usize,
        measure_index: usize,
        sizing: crate::list::UniformListSizing,
        horizontal_sizing: crate::list::UniformListHorizontalSizing,
        y_flipped: bool,
        /// A scroll the view asked for, once: the host applies it when
        /// `revision` moves, never again for the frames that carry it on.
        scroll_request: Option<crate::list::UniformListScrollRequest>,
        revision: u64,
        indices: Vec<u32>,
        children: Vec<Node>,
    },
    List {
        /// The id its author gave it: the list is a scope of its own, and
        /// the host keeps its state and draws its rows under it.
        id: ElementIdWire,
        /// The path the list is filed under, ending in `id`.
        path: Vec<ElementIdWire>,
        item_count: usize,
        alignment: ListAlignment,
        overdraw: f32,
        sizing: ListSizingBehavior,
        following_tail: bool,
        revision: u64,
        commands: Vec<ListCommand>,
        request_handler: u32,
        scroll_handler: Option<u32>,
        range_start: usize,
        style: StyleId,
        #[serde(with = "crate::style::optional")]
        interactivity: Option<Box<Interactivity>>,
        children: Vec<Node>,
    },
    Container(ContainerNode),
    /// A grabbed divider: local movement deltas and native cursor; one child.
    ResizeHandle {
        id: ElementIdWire,
        style: StyleId,
        #[serde(with = "crate::style::optional")]
        interactivity: Option<Box<Interactivity>>,
        on_press: Option<u32>,
        on_release: Option<u32>,
        on_drag: Option<u32>,
        cursor: Option<mouse::Cursor>,
        content: Box<Node>,
    },
    /// A deferred draw, painted after everything in the frame that is not.
    Deferred {
        priority: usize,
        content: Box<Node>,
    },
    /// Watches where layout put its child. `on_bounds` hears the child's
    /// bounds when it comes into view and whenever they differ from the
    /// last it heard, as [`Event::Bounds`].
    Sensor {
        id: ElementIdWire,
        style: StyleId,
        on_bounds: Option<u32>,
        child: Box<Node>,
    },
    Text(TextNode),
    /// A raster picture sent once per typed content hash.
    Image {
        id: Option<ElementIdWire>,
        hash: u64,
        data: Option<ImageData>,
        label: Option<String>,
        image_style: ImageStyle,
        loading: bool,
        fallback: bool,
        state_children: Vec<Node>,
        style: StyleId,
        #[serde(with = "crate::style::optional")]
        interactivity: Option<Box<Interactivity>>,
    },
    /// A vector picture. Its bytes cross ONCE: the frame that first shows a
    /// picture carries them under `hash`, and every frame after — a changed
    /// tree re-sends every node — names the hash alone. The host keeps what
    /// it decoded by hash for as long as the view runs; a hash it has not
    /// seen draws as empty space of the node's size.
    Svg {
        id: Option<ElementIdWire>,
        source: SvgSource,
        transformation: SvgTransformation,
        label: Option<String>,
        style: StyleId,
        #[serde(with = "crate::style::optional")]
        interactivity: Option<Box<Interactivity>>,
    },
    /// A text field, one line or many, whose text the host's editing engine
    /// owns. `value`, `cursor` and `tokens` are the view's copy: the host
    /// adopts them on a fresh mount and whenever `generation` moves (a reset
    /// the view means), and otherwise reports its own text through
    /// `on_change` as [`Event::Text`]. `revision` is the host's, as of the
    /// last change the view applied, so the host can let go of the edits
    /// before it. A key in `claims` reaches the view as [`Event::KeyDown`]
    /// on `on_key` instead of editing; the engine's own keys cannot be
    /// claimed ([`KeyClaim::engine_owned`]). Enter in a one-line field is
    /// `on_submit`, as [`Event::Message`].
    Field {
        id: ElementIdWire,
        multiline: bool,
        value: String,
        cursor: TextRange,
        generation: u64,
        revision: u64,
        tokens: Box<[TextToken]>,
        claims: Box<[KeyClaim]>,
        options: Box<InputOptions>,
        placeholder: String,
        secure: bool,
        on_change: Option<u32>,
        on_key: Option<u32>,
        on_submit: Option<u32>,
        style: StyleId,
    },
    /// Nothing: what an empty view renders as, and what stands in for a
    /// subtree the host cut.
    Space,
    /// A base plus an optional modal layer. Closing removes the second child.
    Overlay {
        id: ElementIdWire,
        /// The accessible name of the dialog; the variant is its role.
        label: Option<String>,
        style: StyleId,
        on_dismiss: Option<u32>,
        children: Vec<Node>,
    },
    /// A panel over a scrim, holding one child the view owns. Under a
    /// Compact body (`design::width::COMPACT_BELOW`) the host docks it to
    /// the bottom edge, opened at the first of `detents` (fractions of the
    /// body's height, ascending, each in `0..=1`), dragged between them and
    /// down to dismiss; with no `detents` the panel is as tall as its child
    /// (the host lays it out), at most the body, and rests there. At a
    /// wider body it is a centred modal, as [`Node::Overlay`]'s second
    /// child. The host owns the drag, the scrim
    /// (an accessible "Close" control), the snap and the keyboard lift; the
    /// view owns what is inside. `label` is the dialog's name. A sheet
    /// inside a sheet is refused.
    Sheet {
        id: ElementIdWire,
        label: Option<String>,
        style: StyleId,
        detents: Vec<f32>,
        on_dismiss: Option<u32>,
        child: Box<Node>,
    },
    /// Bounded geometry painted by the host, in widget-local coordinates.
    Canvas {
        style: StyleId,
        commands: Vec<CanvasCommand>,
    },
    /// A web page, drawn by the host's browser engine in the node's box.
    /// Every press, key and scroll on it is the page's, so it carries no
    /// listeners, and no label or role: the page is its own content
    /// (deliberate; an accessible name can come later with the page's own
    /// accessibility tree). Only a view whose manifest declares the `web`
    /// capability may send one; a host refuses the frame of any other
    /// ([`Frame::undeclared`]).
    ///
    /// The page opens on `url` when the host first sees the node (under its
    /// path) and is closed when a frame no longer has it. The view moves it
    /// with `commands`, applied in order, once, when `revision` moves (a
    /// [`Node::List`]'s pattern); in the frame that first carries the node
    /// they apply after the page is asked to open `url` (a `Load` there
    /// opens its address instead). `on_state` hears where the page is as
    /// [`Event::Web`].
    ///
    /// A host loads only an `https://` page, or an `http://` one on this
    /// machine, with no user name or password in the address: every
    /// `url`, every `Load`, and every top-level navigation the page makes
    /// itself (a link, a script, a redirect). Frames inside the page are
    /// the page's own and are not held to the rule.
    Web {
        id: ElementIdWire,
        url: String,
        style: StyleId,
        revision: u64,
        commands: Vec<crate::web::WebCommand>,
        on_state: Option<u32>,
    },
    /// A camera's picture, drawn by the host in the node's box as an
    /// image's is by `object_fit` (`Contain`: the whole picture, centred,
    /// the box's background on the sides it does not fill; `Cover`: the box
    /// covered, the picture cut centred), turned upright for the display's
    /// orientation, and flipped left to right when `mirror` (a person
    /// checking themselves expects a mirror). No picture crosses to the
    /// view. Only a view whose manifest declares the `camera` capability
    /// may send one; a host refuses the frame of any other
    /// ([`Frame::undeclared`]).
    ///
    /// The camera opens when the host first sees the node (under its
    /// path), the system's permission asked first where it asks; it opens
    /// again when `device` changes (`None`: the system's default camera,
    /// else an id `camera.devices` listed: the host opens only an id it
    /// listed, and a piece naming another reports
    /// [`crate::MediaStatus::NoDevice`]), and is released (its light off)
    /// when a frame no longer has the node or its window closes. A view
    /// turns its camera off by not placing the piece. `on_state` hears
    /// where the camera is as [`Event::Media`].
    ///
    /// With `publish`, the host also encodes what it captures and
    /// publishes it into that track of the view's room (stream mode,
    /// priority 128: the kind decides, not the view) for as long as the
    /// piece is placed, and [`crate::MediaState::track`] says where the
    /// track stands. The track goes up whatever the camera's status. The
    /// published picture is never mirrored: `mirror` and `object_fit` are
    /// the preview's. A view that publishes also declares the `live`
    /// capability.
    Camera {
        id: ElementIdWire,
        style: StyleId,
        device: Option<String>,
        mirror: bool,
        object_fit: ImageObjectFit,
        on_state: Option<u32>,
        publish: Option<crate::methods::LiveTrack>,
    },
    /// A microphone's level, drawn by the host as a meter in the node's
    /// box: a bar from the left in the style's text colour over its
    /// background. The level never crosses: it moves faster than a view
    /// should draw. Only a view whose manifest declares the `microphone`
    /// capability may send one; a host refuses the frame of any other
    /// ([`Frame::undeclared`]).
    ///
    /// The microphone opens as a [`Node::Camera`]'s camera does, by
    /// `device` (an input `microphone.devices` listed), and is released
    /// when a frame no longer has the node; a view mutes by not placing
    /// it. `output` is the speaker (an output `microphone.devices` listed;
    /// `None`: the default) a recording plays on and the monitor loops to.
    /// `monitor` plays what the microphone hears straight back while set:
    /// through a speaker ([`crate::methods::AudioKind::Speaker`]) it howls,
    /// so a view sets it only on the person's explicit choice, with
    /// headphones on, and the host stops it when the sound falls to one
    /// ([`crate::MediaState::monitor_stopped`]). `commands`
    /// apply in order, once, when `revision` moves (a [`Node::List`]'s
    /// pattern): [`crate::MicCommand::Record`] records, then plays back.
    /// `on_state` hears the microphone and the recording as
    /// [`Event::Media`].
    ///
    /// With `publish`, the host also encodes what it hears and publishes
    /// it into that track of the view's room (datagram mode, priority 64,
    /// one 20 ms frame per object) for as long as the piece is placed; the
    /// track goes up whatever the microphone's status, so a person with no
    /// working microphone is still in the room. `muted` (with `publish`)
    /// releases the device (the system's mark off), keeps the track up
    /// and sends nothing on it: a muted person stays in the room. Without
    /// `publish`, `muted` is ignored: a view mutes a plain piece by not
    /// placing it. A view that publishes also declares the `live`
    /// capability.
    Microphone {
        id: ElementIdWire,
        style: StyleId,
        device: Option<String>,
        output: Option<String>,
        monitor: bool,
        revision: u64,
        commands: Vec<crate::media::MicCommand>,
        on_state: Option<u32>,
        publish: Option<crate::methods::LiveTrack>,
        muted: bool,
    },
    /// A person's track from the view's room, played by the host: a
    /// picture drawn in the node's box ([`RemoteMedia::Video`]), or a
    /// voice on a speaker ([`RemoteMedia::Audio`]). Nothing of it crosses
    /// to the view. Only a view whose manifest declares the `live`
    /// capability may send one; a host refuses the frame of any other
    /// ([`Frame::undeclared`]).
    ///
    /// The host subscribes to `track` when it first sees the node (under
    /// its path): a picture from the group the node holds (its keyframe),
    /// a voice from now on. It lets go when a frame no longer has the
    /// node. `on_state` hears it as [`Event::Media`]: `status` is
    /// `Starting` until the first object, `Live` while they flow, `Gone`
    /// when the track ended, `Unsupported` where this build decodes no
    /// video (a voice still plays); [`crate::MediaState::track`] says why,
    /// and a voice's `speaking` says the person speaks.
    ///
    /// The host keeps its own cap on how many remote pieces one view
    /// places, apart from its own devices'.
    Remote {
        id: ElementIdWire,
        style: StyleId,
        track: crate::methods::LiveTrack,
        media: RemoteMedia,
        on_state: Option<u32>,
    },
}

/// What a [`Node::Remote`] plays of its track.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemoteMedia {
    /// A picture, fitting the node's box as an image's does by
    /// `object_fit`.
    Video { object_fit: ImageObjectFit },
    /// A voice, mixed with the room's others on `output` (an output
    /// `microphone.devices` listed; `None`: the default). It draws
    /// nothing.
    Audio { output: Option<String> },
}

mod impls;
