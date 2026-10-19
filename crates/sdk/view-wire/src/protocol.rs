use crate::methods::Capability;
use crate::*;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RichTextHover {
    pub index: Option<u32>,
    pub position: gpui::Point<gpui::Pixels>,
    pub pressed_button: Option<click::MouseButton>,
    pub modifiers: gpui::Modifiers,
}

/// Something the host tells the view.
///
/// A `handler` (a `request` for a tooltip) is a route: the number the view
/// gave a listener in the tree it sent. A route is the listener's authored
/// identity, not its position in the frame: the view hands a number out
/// the first time it lowers that listener (the path of the element carrying
/// it, what the listener is for, and which of that kind under that path) and
/// keeps it while the element is lowered with it, so an event the host took
/// from the frame it last painted runs the same listener in the frame after
/// a structural change. A frame lowered without the listener frees its
/// route, and a freed route names nothing again.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(remote = "Self")]
pub enum Event {
    /// The user activated the widget the view gave this message route to
    /// (a button press, an input submit).
    Message(u32),
    /// A GPUI click, kept distinct from message routes and carrying its input data.
    Click {
        handler: u32,
        event: click::Click,
    },
    /// GPUI element listener payloads.
    MouseDown {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseDown,
    },
    MouseUp {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseUp,
    },
    MouseDownOut {
        handler: u32,
        event: interactivity::MouseDown,
    },
    MouseUpOut {
        handler: u32,
        event: interactivity::MouseUp,
    },
    MousePressure {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MousePressure,
    },
    MouseMove {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseMove,
    },
    MouseExit {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::MouseExit,
    },
    ScrollWheel {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::ScrollWheel,
    },
    Pinch {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::Pinch,
    },
    KeyDown {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::KeyDown,
    },
    KeyUp {
        handler: u32,
        phase: interactivity::DispatchPhase,
        event: interactivity::KeyUp,
    },
    ModifiersChanged {
        handler: u32,
        event: interactivity::ModifiersChanged,
    },
    Hover {
        handler: u32,
        hovered: bool,
    },
    FileDropExit {
        handler: u32,
    },
    AuxClick {
        handler: u32,
        event: click::Click,
    },
    /// Native hover asked the view to build a tooltip for the displayed frame.
    TooltipRequest {
        request: u32,
        /// Native InteractiveText byte index; ordinary tooltips use None.
        character_index: Option<u32>,
    },
    /// A field's text moved under the host's engine (typed, pasted, undone,
    /// a view `Replace` landed, a composition advanced): the whole text as
    /// the host now holds it, at its `revision`.
    Text {
        handler: u32,
        change: TextChange,
    },
    /// The host theme changed; the view stores the corresponding `Theme` global.
    Theme {
        dark: bool,
    },
    /// The device's primary pointer is a finger (a phone), or not: a device
    /// fact like the theme, said from what the device is, never from the
    /// last input. Delivered ahead of the first tick, on change, and to a
    /// replacement before its first tree, so the view reads it as
    /// `Window::touch()` and its first frame is at the right ladder
    /// (`design::ladder`). Under touch a view shows what hover would have
    /// revealed and treats a long press as the context gesture.
    Pointer {
        touch: bool,
    },
    /// The view's viewport: the body its root is laid out in, in logical
    /// pixels, as the host knows it before the frame is drawn. Delivered
    /// ahead of the first tick, on every change ahead of the draw that
    /// shows it, and to a replacement before its first tree. The view
    /// reads it as `Window::viewport_size()`; a list sizes its first
    /// window from it.
    Viewport {
        width: f32,
        height: f32,
    },
    /// The reader's UTC offset in minutes (`540` in Seoul, `-300` in New
    /// York in winter): ahead of the first tick and on every change, so
    /// every time a view writes is in the reader's own zone from its first
    /// frame.
    Offset {
        minutes: i32,
    },
    /// A rich text's clickable range at `index`.
    Select {
        handler: u32,
        index: u32,
    },
    /// A native interactive-text hover changed character index.
    RichTextHover {
        handler: u32,
        event: RichTextHover,
    },
    /// A [`Node::Sensor`]'s child was measured: its bounds, `width` by
    /// `height` with the top left corner at `x`, `y`, in the window's
    /// logical pixels — the pixels a mouse event's `position` is in, so a
    /// view places a pointer inside the child by subtracting the corner.
    /// `handler` is the node's `on_bounds`. Sent when the child comes into
    /// view, and whenever its bounds, the corner or the size, differ from
    /// the last sent while it is in view; out of view the host forgets
    /// them, so the next sight sends them again.
    ///
    /// Delivered after layout, like a DOM `ResizeObserver`: the host lays
    /// the tree out, the sensor reads its child's bounds, and the event
    /// goes to the view on the next tick. A view whose answer changes the
    /// tree so the child measures differently again is told again, one tick
    /// and one frame for each draw, for as long as it does: the host does
    /// not stop it.
    Bounds {
        handler: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    /// Accumulated logical-pixel movement of a grabbed resize handle.
    Drag {
        handler: u32,
        dx: f64,
        dy: f64,
    },
    /// A scrollable's content offset in logical pixels and anchor-relative
    /// fractions, emitted only when its native viewport changes. It carries
    /// no window coordinates.
    ScrollOffset {
        handler: u32,
        x: f32,
        y: f32,
        relative_x: f32,
        relative_y: f32,
    },
    /// The native uniform-list viewport shows rows `start..end` and needs
    /// them on the next view frame. The route fences a stale request from
    /// an older list instance. `item_height` is the row height the host
    /// measured, in logical pixels: the view sizes the list's window
    /// from it and its viewport.
    UniformListRange {
        path: Vec<ElementIdWire>,
        route: u32,
        start: u32,
        end: u32,
        item_height: f32,
    },
    UniformListState {
        path: Vec<ElementIdWire>,
        route: u32,
        top_index: u32,
        scrollable: bool,
        scrolled_to_end: Option<bool>,
    },
    /// A native variable-height list requested a bounded item window.
    ListRequest {
        handler: u32,
        request: ListRequest,
    },
    /// Settled native list geometry, emitted after layout state is released.
    ListScroll {
        handler: u32,
        event: ListScroll,
    },
    /// One answer to a [`Request`]. A one-shot request gets exactly one with
    /// `done`; a subscription gets many, the last one `done`.
    Response {
        id: u64,
        #[serde(with = "crate::codec::bin")]
        result: Result<Vec<u8>, Error>,
        done: bool,
    },
    /// The host no longer holds the tree the view is patching — a patch it
    /// could not apply, a tree it dropped — and wants the next frame whole.
    Resync,
    /// An assistive-technology action request on the node that advertised
    /// `(action, handler)` in [`Aria::actions`](crate::Aria::actions).
    A11yAction {
        handler: u32,
        data: Option<ActionData>,
    },
    /// The person asked to go back (the phone's gesture or its bar), to the
    /// root's `on_back` ([`Interactivity::on_back`]) in the last frame the
    /// host landed. A view that popped its page in the meantime hears one
    /// it has no use for, and ignores it: the next landed frame has no
    /// `on_back`, and the next press leaves the program.
    Back {
        handler: u32,
    },
    /// Where a [`Node::Web`]'s page is now (its address, title, whether it
    /// loads, whether it can go back or forward), to the node's
    /// `on_state`: sent when the page first reports, and whenever any of it
    /// changes after.
    Web {
        handler: u32,
        state: crate::WebState,
    },
    /// Where a [`Node::Camera`]'s or [`Node::Microphone`]'s device is (and
    /// a microphone's recording), or a [`Node::Remote`]'s track, to the
    /// node's `on_state`: sent when the piece first reports, and whenever
    /// any of it changes after.
    Media {
        handler: u32,
        state: crate::MediaState,
    },
    FileDrop {
        handler: u32,
        files: Vec<crate::methods::Picked>,
    },
    // An event this build cannot read, from a host of a later build: its
    // kind, or a value in it, is a variant this build does not know
    // (`codec.rs`, "Appends"). The view drops it; it never crosses.
    #[serde(skip)]
    Unknown {
        what: String,
    },
}

impl Serialize for Event {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Event::serialize(self, serializer)
    }
}

impl<'de> Deserialize<'de> for Event {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Event::deserialize(deserializer).or_else(|error| match crate::fell(&error) {
            Some(what) => Ok(Event::Unknown { what }),
            None => Err(error),
        })
    }
}

impl Event {
    // The wire level that added the event ([`crate::WIRE_LEVEL`]): a host
    // sends a view only the events its manifest's level reads
    // ([`crate::manifest::Manifest::level`]), since a view built before
    // J-085 cannot step over one it does not know. One arm per variant, so
    // an appended event says its level.
    pub fn level(&self) -> u32 {
        match self {
            Self::Message(..)
            | Self::Click { .. }
            | Self::MouseDown { .. }
            | Self::MouseUp { .. }
            | Self::MouseDownOut { .. }
            | Self::MouseUpOut { .. }
            | Self::MousePressure { .. }
            | Self::MouseMove { .. }
            | Self::MouseExit { .. }
            | Self::ScrollWheel { .. }
            | Self::Pinch { .. }
            | Self::KeyDown { .. }
            | Self::KeyUp { .. }
            | Self::ModifiersChanged { .. }
            | Self::Hover { .. }
            | Self::FileDropExit { .. }
            | Self::AuxClick { .. }
            | Self::TooltipRequest { .. }
            | Self::Text { .. }
            | Self::Theme { .. }
            | Self::Pointer { .. }
            | Self::Viewport { .. }
            | Self::Offset { .. }
            | Self::Select { .. }
            | Self::RichTextHover { .. }
            | Self::Bounds { .. }
            | Self::Drag { .. }
            | Self::ScrollOffset { .. }
            | Self::UniformListRange { .. }
            | Self::UniformListState { .. }
            | Self::ListRequest { .. }
            | Self::ListScroll { .. }
            | Self::Response { .. }
            | Self::Resync
            | Self::A11yAction { .. }
            | Self::Back { .. }
            | Self::Web { .. }
            | Self::Media { .. }
            | Self::FileDrop { .. } => 0,
            Self::Unknown { .. } => u32::MAX,
        }
    }
}

/// Why a request failed, as the view gets it: the one [`Error`] — a stable
/// snake_case [`code`] to branch on and the refusing program's `message`,
/// verbatim — so an error a program wrote and one the host wrote are the same
/// type end to end. `Display` writes `code: message`; a screen that wants the
/// message alone reads it.
pub use ::error::{Error, code};

/// Something the view asked the host for. The view never blocks on it: a
/// future (or stream) inside the view waits for the matching
/// [`Event::Response`]s, which the host delivers on its own schedule.
///
/// `kind` is `<capability>.<operation>`; the host refuses a capability the
/// app's manifest did not declare.
///
/// A view's frame carries it in the tree's MessagePack; a notifier's turn
/// ([`crate::notifier::Turn`]) in borsh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Request {
    pub id: u64,
    pub kind: String,
    #[serde(with = "crate::codec::bin")]
    pub payload: Vec<u8>,
}

/// What one tick of the view produced.
///
/// The tree crosses one of three ways: whole in `root`; not at all, with
/// `unchanged` set; or as `patches` against the tree the host holds, with
/// `root` empty and `unchanged` clear.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Tooltip subtrees built only after a native hover request.
    pub tooltip_responses: Vec<TooltipResponse>,
    /// The style table's entries this frame brings: all of them beside a
    /// `root`, which replaces the host's table with its tree, and otherwise
    /// the ones the host does not hold yet, in id order after the ones it
    /// does.
    pub styles: Vec<Style>,
    /// The tree to show. `None` with `unchanged` set means "what you have";
    /// `None` otherwise means "what you have, with `patches` applied".
    pub root: Option<Node>,
    /// Edits to the tree the host holds, in order, when `root` is `None`
    /// and `unchanged` is clear. The host applies them with [`apply`].
    pub patches: Vec<Patch>,
    /// What the view asked for while producing this frame.
    pub requests: Vec<Request>,
    /// Requests the view stopped waiting on — a dropped future or stream.
    /// The host frees whatever it kept for them and sends no more answers.
    pub cancels: Vec<u64>,
    /// `root` is `None` because the tree is the one the view sent last:
    /// the host keeps what it has instead of decoding it again. Requests and
    /// cancels still cross.
    pub unchanged: bool,
    /// The view ran out of tick budget with work still ready — a task that
    /// yields more than one tick runs, a handler chain longer than one
    /// round — and wants the next tick now, not at the next event or answer.
    pub busy: bool,
}

impl Frame {
    /// A capability that a node the frame carries needs ([`Node::capabilities`])
    /// and `declared` leaves out, looked for in the tree, in every patch and
    /// in every tooltip's content; `None` when the view declared each. A
    /// host refuses such a frame and ends the view: sent again, it would be
    /// refused again.
    pub fn undeclared(&self, declared: &[Capability]) -> Option<Capability> {
        fn find(node: &Node, declared: &[Capability]) -> Option<Capability> {
            node.capabilities()
                .iter()
                .find(|capability| !declared.contains(capability))
                .copied()
                .or_else(|| {
                    node.children()
                        .iter()
                        .find_map(|child| find(child, declared))
                })
        }
        let patched = self.patches.iter().filter_map(|patch| match patch {
            Patch::Replace { node, .. }
            | Patch::Props { node, .. }
            | Patch::Insert { node, .. } => Some(node),
            Patch::Remove { .. } | Patch::Move { .. } => None,
        });
        let tooltips = self
            .tooltip_responses
            .iter()
            .filter_map(|response| response.content.as_deref());
        self.root
            .iter()
            .chain(patched)
            .chain(tooltips)
            .find_map(|node| find(node, declared))
    }
}

/// Copied input accessibility and native layout options.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputOptions {
    pub label: String,
    pub description: Option<String>,
    pub disabled: bool,
    /// The value is wrong; `description` says why (AX-108).
    pub invalid: Option<Invalid>,
    pub required: bool,
    pub read_only: bool,
    /// What a software keyboard is told about the field.
    pub text_input: TextInput,
}

/// gpui's `TextInputConfiguration`, field for field but `secure`: what a
/// software keyboard shows and does for the field. `secure` is not here:
/// it is the field's masked flag ([`crate::Node::Field`]'s `secure`), which
/// the host passes to [`TextInput::configuration`]. The defaults are
/// gpui's: no assistance, no particular key. A desktop engine ignores what
/// its OS has no use for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextInput {
    /// The platform may correct what is typed.
    pub autocorrect: bool,
    pub autocapitalize: Autocapitalize,
    /// The keyboard may offer words and spellcheck.
    pub suggestions: bool,
    /// The label of the keyboard's confirm key; pressing it is still input.
    pub input_action: TextInputAction,
    pub virtual_keyboard: VirtualKeyboard,
}

/// gpui's `Autocapitalize`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Autocapitalize {
    #[default]
    None,
    Words,
    Sentences,
    Characters,
}

/// gpui's `TextInputAction`: HTML's `enterkeyhint` values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextInputAction {
    #[default]
    Unspecified,
    Enter,
    Done,
    Go,
    Next,
    Previous,
    Search,
    Send,
}

/// gpui's `VirtualKeyboard`: the keyboard a software keyboard opens as,
/// HTML's `inputmode` values (iOS `keyboardType`, Android `inputType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirtualKeyboard {
    #[default]
    Text,
    Numeric,
    Decimal,
    Tel,
    Email,
    Url,
    Search,
}

impl From<gpui::Autocapitalize> for Autocapitalize {
    fn from(value: gpui::Autocapitalize) -> Self {
        match value {
            gpui::Autocapitalize::None => Self::None,
            gpui::Autocapitalize::Words => Self::Words,
            gpui::Autocapitalize::Sentences => Self::Sentences,
            gpui::Autocapitalize::Characters => Self::Characters,
        }
    }
}

impl From<Autocapitalize> for gpui::Autocapitalize {
    fn from(value: Autocapitalize) -> Self {
        match value {
            Autocapitalize::None => Self::None,
            Autocapitalize::Words => Self::Words,
            Autocapitalize::Sentences => Self::Sentences,
            Autocapitalize::Characters => Self::Characters,
        }
    }
}

impl From<gpui::TextInputAction> for TextInputAction {
    fn from(value: gpui::TextInputAction) -> Self {
        match value {
            gpui::TextInputAction::Unspecified => Self::Unspecified,
            gpui::TextInputAction::Enter => Self::Enter,
            gpui::TextInputAction::Done => Self::Done,
            gpui::TextInputAction::Go => Self::Go,
            gpui::TextInputAction::Next => Self::Next,
            gpui::TextInputAction::Previous => Self::Previous,
            gpui::TextInputAction::Search => Self::Search,
            gpui::TextInputAction::Send => Self::Send,
        }
    }
}

impl From<TextInputAction> for gpui::TextInputAction {
    fn from(value: TextInputAction) -> Self {
        match value {
            TextInputAction::Unspecified => Self::Unspecified,
            TextInputAction::Enter => Self::Enter,
            TextInputAction::Done => Self::Done,
            TextInputAction::Go => Self::Go,
            TextInputAction::Next => Self::Next,
            TextInputAction::Previous => Self::Previous,
            TextInputAction::Search => Self::Search,
            TextInputAction::Send => Self::Send,
        }
    }
}

impl From<gpui::VirtualKeyboard> for VirtualKeyboard {
    fn from(value: gpui::VirtualKeyboard) -> Self {
        match value {
            gpui::VirtualKeyboard::Text => Self::Text,
            gpui::VirtualKeyboard::Numeric => Self::Numeric,
            gpui::VirtualKeyboard::Decimal => Self::Decimal,
            gpui::VirtualKeyboard::Tel => Self::Tel,
            gpui::VirtualKeyboard::Email => Self::Email,
            gpui::VirtualKeyboard::Url => Self::Url,
            gpui::VirtualKeyboard::Search => Self::Search,
        }
    }
}

impl From<VirtualKeyboard> for gpui::VirtualKeyboard {
    fn from(value: VirtualKeyboard) -> Self {
        match value {
            VirtualKeyboard::Text => Self::Text,
            VirtualKeyboard::Numeric => Self::Numeric,
            VirtualKeyboard::Decimal => Self::Decimal,
            VirtualKeyboard::Tel => Self::Tel,
            VirtualKeyboard::Email => Self::Email,
            VirtualKeyboard::Url => Self::Url,
            VirtualKeyboard::Search => Self::Search,
        }
    }
}

impl TextInput {
    /// The same field's configuration in gpui's type, for a host engine.
    /// `secure` is the field's masked flag ([`crate::Node::Field`]'s
    /// `secure`): a secret field hides what is typed from third-party
    /// keyboards and suggestion services.
    pub fn configuration(self, secure: bool) -> gpui::TextInputConfiguration {
        gpui::TextInputConfiguration {
            autocorrect: self.autocorrect,
            autocapitalize: self.autocapitalize.into(),
            suggestions: self.suggestions,
            input_action: self.input_action.into(),
            virtual_keyboard: self.virtual_keyboard.into(),
            secure,
        }
    }
}
