//! The view half of the view wire: gpui-shaped contexts and elements lowered
//! to `view_wire::Node`, and the host methods a wasm view asks through.
pub use gpui::prelude::FluentBuilder;
pub use gpui::{AccessibleAction, accesskit};
// `#[derive(IntoElement)]` names this crate by its absolute path; this lets
// that path resolve inside the crate too, so its own components derive.
extern crate self as view;

pub use gpui::{
    Anchor, AnchoredPositionMode, Bounds, ClickEvent, CursorStyle, Edges, ElementId, EventEmitter,
    FileDropEvent, FollowMode, FontStyle, FontWeight, Global, HighlightStyle, HoverListenerMode,
    Hsla, Image, ImageFormat, KeyDownEvent, KeyUpEvent, Keystroke, ListAlignment,
    ListHorizontalSizingBehavior, ListOffset, ListScrollEvent, ListSizingBehavior, Modifiers,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseExitEvent, MouseMoveEvent,
    MousePressureEvent, MouseUpEvent, ObjectFit, PinchEvent, Pixels, Point, Resource, Role,
    ScrollStrategy, ScrollWheelEvent, SharedString, StrikethroughStyle, StyleRefinement, Styled,
    Subscription, TextRun, TextStyle, Toggled, UnderlineStyle, hsla, px, rems, rgb,
};
pub use view_derive::IntoElement;
pub use view_wire as wire;
pub use wire::manifest::Tint;
mod theme;
pub use theme::Theme;
mod behavior;
mod element;
mod interactivity;
mod list;
mod primitives;
mod rich_text;
mod view_element;
pub use behavior::{
    ModalOverlay, ResizeHandle, Sensor, Sheet, modal_overlay, resize_handle, sensor, sheet,
};
mod media;
mod revised;
mod web;
pub use element::{
    AnyElement, Div, Element, Input, IntoElement, Lowering, ParentElement, RenderOnce, Textarea,
    UniformList, UniformListScrollHandle, div, uniform_list,
};
/// What a field tells a software keyboard, in gpui's words
/// (`Input::autocapitalize`, `Input::input_action`, `Input::virtual_keyboard`).
pub use gpui::{Autocapitalize, TextInputAction, VirtualKeyboard};
pub use interactivity::{
    BackEvent, FocusHandle, InteractiveElement, Interactivity, ScrollHandle, Stateful,
    StatefulInteractiveElement,
};
pub use list::{List, ListState, list};
pub use media::{
    Camera, MediaState, MediaStatus, MicHandle, Microphone, Playback, RemoteAudio, RemoteVideo,
    TrackState, camera, microphone, remote_audio, remote_video,
};
pub use primitives::{
    Anchored, Canvas, Deferred, ImageSource, ImageStyle, Img, StyledImage, Svg, Transformation,
    anchored, canvas, deferred, img, svg,
};
pub use rich_text::{InteractiveText, StyledText};
pub use view_element::{AnyView, ViewElement};
mod memo;
pub use memo::{Memo, memo};
pub use web::{Web, WebHandle, WebState, web};

/// Every SDK name a view file writes, in one import:
/// `use view::prelude::*;`. gpui's names (elements, styles,
/// `Render`, `Context`, `Window`), the view's own ([`View`], [`Loadable`],
/// [`Task`], [`Host`], [`Error`](crate::host::Error), `export_view!`), the
/// [`design`](crate::design) program, and the [`methods`](crate::methods)
/// program whole: the method markers (`HostSession`, `Changes`, `Submit`)
/// and the request and reply types beside them. A program's types come
/// from the program's crate; an explicit import of a name wins over this
/// glob (`use forge::Query;`).
pub mod prelude {
    pub use crate::design::TextType;
    pub use crate::host::Error;
    pub use crate::methods::*;
    pub use crate::{
        AnyElement, AnyView, App, Autocapitalize, BackEvent, Bounds, ClickEvent, Context, Element,
        ElementId, Entity, EventEmitter, FileDropEvent, FluentBuilder, FocusHandle, FollowMode,
        Global, Host, HoverListenerMode, Hsla, Image, ImageFormat, Input, InteractiveElement,
        InteractiveText, IntoElement, KeyDownEvent, KeyUpEvent, List, ListAlignment,
        ListHorizontalSizingBehavior, ListOffset, ListScrollEvent, ListSizingBehavior, ListState,
        Loadable, MediaState, MediaStatus, Memo, MicHandle, ModifiersChangedEvent, MouseButton,
        MouseDownEvent, MouseExitEvent, MouseMoveEvent, MousePressureEvent, MouseUpEvent,
        ObjectFit, ParentElement, PinchEvent, Pixels, Playback, Render, RenderOnce, Role,
        ScrollHandle, ScrollStrategy, ScrollWheelEvent, SharedString, StatefulInteractiveElement,
        Styled, StyledImage, StyledText, Subscription, Task, TextField, TextInputAction, Textarea,
        Theme, Tint, TrackState, UniformListScrollHandle, View, VirtualKeyboard, WebHandle,
        WebState, Window, anchored, camera, canvas, deferred, design, div, export_notifier,
        export_view, hsla, img, list, memo, microphone, modal_overlay, px, remote_audio,
        remote_video, rems, resize_handle, rgb, sensor, sheet, svg, uniform_list, web,
    };
}
mod text;
pub use text::TextField;
pub mod borsh_bytes;
pub mod design;
pub mod host;
pub mod live;
pub mod picture;
#[cfg(feature = "rig")]
pub mod rig;
pub mod store;
pub mod testing;
mod window;

mod root;
mod snapshot;
pub use root::{Loadable, Render, View};
mod paged;
pub use paged::Paged;
pub use wire::methods;
mod context;
pub use context::{App, AppContext, AsyncApp, Callback, Context, Entity, Released, WeakEntity};
mod executor;
pub use executor::Task;
pub use host::Host;
pub use window::Window;
mod slots;

mod driver;
pub(crate) use driver::Driver;
mod notifier;
pub use notifier::Notifier;
pub(crate) use notifier::NotifierDriver;

const MANIFEST_HEADER: &str = "ducktape.view.manifest\n";

/// What a manifest says, field by field: a view's from its [`View`] impl, a
/// notifier's from its [`Notifier`] impl (no description, no icon, no
/// tint, a min width of 1 and no window width: it draws nothing). One
/// writer for both, so both read the same.
#[doc(hidden)]
pub struct Manifested {
    pub name: &'static str,
    pub description: &'static str,
    pub capabilities: &'static [crate::methods::Capability],
    pub min_width: u32,
    pub icon: &'static str,
    pub tint: Tint,
    pub width: Option<u32>,
}

impl Manifested {
    pub const fn of_view<V: View>() -> Self {
        Self {
            name: V::NAME,
            description: V::DESCRIPTION,
            capabilities: V::CAPABILITIES,
            min_width: V::MIN_WINDOW_WIDTH,
            icon: V::ICON,
            tint: V::TINT,
            width: V::WINDOW_WIDTH,
        }
    }

    pub const fn of_notifier<N: Notifier>() -> Self {
        Self {
            name: N::NAME,
            description: "",
            capabilities: N::CAPABILITIES,
            min_width: 1,
            icon: "",
            tint: Tint::None,
            width: None,
        }
    }

    /// The length of [`Manifested::bytes`].
    pub const fn encoded_len(&self) -> usize {
        let mut len = MANIFEST_HEADER.len()
            + self.name.len()
            + 1
            + self.description.len()
            + 1
            + 1
            + digits(self.min_width)
            + 1
            + digits(wire::WIRE_LEVEL);
        let mut i = 0;
        while i < self.capabilities.len() {
            len += self.capabilities[i].as_str().len() + 1;
            i += 1;
        }
        len += 1;
        len += 1 + self.icon.len() + 1 + self.tint.as_str().len() + 1;
        match self.width {
            Some(width) => len + digits(width),
            None => len,
        }
    }

    // The manifest text (`view_wire::manifest`: header, name, description,
    // capabilities, min width, [`wire::WIRE_LEVEL`], an empty line 7, icon,
    // tint, width), at compile time: a window width under the min width,
    // or an icon that is not an asset path ([`wire::manifest::is_icon`]),
    // fails the build.
    pub const fn bytes<const N: usize>(&self) -> [u8; N] {
        assert!(self.min_width >= 1, "MIN_WINDOW_WIDTH is at least 1");
        assert!(
            !matches!(self.width, Some(width) if width < self.min_width),
            "WINDOW_WIDTH is at least MIN_WINDOW_WIDTH"
        );
        assert!(
            wire::manifest::is_icon(self.icon),
            "ICON is empty or the path of an asset the app bundles (icons/hammer.svg)"
        );
        let mut out = [0u8; N];
        let mut at = put(&mut out, 0, MANIFEST_HEADER.as_bytes());
        at = put(&mut out, at, self.name.as_bytes());
        at = put(&mut out, at, b"\n");
        at = put(&mut out, at, self.description.as_bytes());
        at = put(&mut out, at, b"\n");
        let mut i = 0;
        while i < self.capabilities.len() {
            at = put(&mut out, at, self.capabilities[i].as_str().as_bytes());
            at = put(&mut out, at, b",");
            i += 1;
        }
        at = put(&mut out, at, b"\n");
        at = put_number(&mut out, at, self.min_width);
        at = put(&mut out, at, b"\n");
        at = put_number(&mut out, at, wire::WIRE_LEVEL);
        at = put(&mut out, at, b"\n");
        // line 7, which views before J-054 listed their targets on
        at = put(&mut out, at, b"\n");
        at = put(&mut out, at, self.icon.as_bytes());
        at = put(&mut out, at, b"\n");
        at = put(&mut out, at, self.tint.as_str().as_bytes());
        at = put(&mut out, at, b"\n");
        if let Some(width) = self.width {
            at = put_number(&mut out, at, width);
        }
        assert!(at == N);
        out
    }
}

/// The length of [`manifest_bytes`] for the same view.
pub const fn manifest_len<V: View>() -> usize {
    Manifested::of_view::<V>().encoded_len()
}

// the lines written above, in this order: a line added to the manifest
// is added here and to `Manifest::parse` together
const _: () = assert!(wire::manifest::LINES.len() == 10);

// The manifest text of a view ([`Manifested::bytes`]), from what its
// [`View`] impl says: [`View::NAME`], [`View::DESCRIPTION`],
// [`View::CAPABILITIES`], [`View::MIN_WINDOW_WIDTH`],
// [`View::ICON`], [`View::TINT`], [`View::WINDOW_WIDTH`].
pub const fn manifest_bytes<V: View, const N: usize>() -> [u8; N] {
    Manifested::of_view::<V>().bytes()
}

const fn put(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let mut i = 0;
    while i < bytes.len() {
        out[at + i] = bytes[i];
        i += 1;
    }
    at + bytes.len()
}

const fn digits(number: u32) -> usize {
    match number.checked_ilog10() {
        Some(log) => log as usize + 1,
        None => 1,
    }
}

const fn put_number(out: &mut [u8], at: usize, mut number: u32) -> usize {
    let end = at + digits(number);
    let mut i = end;
    while i > at {
        i -= 1;
        out[i] = b'0' + (number % 10) as u8;
        number /= 10;
    }
    end
}

/// The manifest section and the wasm32 exports ([`wire::abi`]) for a view,
/// from what its [`View`] impl says: `export_view!(Chat);`.
#[macro_export]
macro_rules! export_view {
    ($app:ty) => {
        const MANIFEST_LEN: usize = $crate::manifest_len::<$app>();

        #[cfg_attr(
            target_arch = "wasm32",
            unsafe(link_section = "ducktape.view.manifest")
        )]
        #[used]
        static MANIFEST_SECTION: [u8; MANIFEST_LEN] =
            $crate::manifest_bytes::<$app, MANIFEST_LEN>();

        #[cfg(target_arch = "wasm32")]
        mod wasm_exports {
            use super::*;

            #[unsafe(export_name = "alloc")]
            extern "C" fn alloc(len: u32) -> u32 {
                $crate::exports::alloc(len)
            }

            #[unsafe(export_name = "init")]
            extern "C" fn init() {
                $crate::exports::init::<$app>()
            }

            #[unsafe(export_name = "tick")]
            extern "C" fn tick(ptr: u32, len: u32) -> u64 {
                $crate::exports::tick::<$app>(ptr, len)
            }

            #[unsafe(export_name = "snapshot")]
            extern "C" fn snapshot() -> u64 {
                $crate::exports::snapshot::<$app>()
            }

            #[unsafe(export_name = "restore")]
            extern "C" fn restore(ptr: u32, len: u32) -> u64 {
                $crate::exports::restore::<$app>(ptr, len)
            }
        }
    };
}

/// The view's half of [`wire::abi`]: what `export_view!` builds the five
/// exports from. A program runs one app, so its driver lives here.
#[cfg(target_arch = "wasm32")]
#[doc(hidden)]
pub mod exports {
    use std::any::Any;
    use std::cell::RefCell;

    use crate::{Driver, Notifier, NotifierDriver, View, wire};

    #[link(wasm_import_module = "ducktape_view")]
    unsafe extern "C" {
        fn panicked(ptr: u32, len: u32);
    }

    thread_local! {
        // The last answer, kept until the next export is entered: the host
        // copies it out before it calls again.
        static ANSWER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static DRIVER: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
        static NOTIFIER: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    }

    pub fn notifier_init<N: Notifier>() {
        install_panic_hook();
        NOTIFIER.set(Some(Box::new(NotifierDriver::<N>::new())));
    }

    pub fn notifier_tick<N: Notifier>(ptr: u32, len: u32) -> u64 {
        let answers: Vec<wire::notifier::Answer> =
            borsh::from_slice(&take(ptr, len)).expect("invalid host answers");
        let turn = NOTIFIER.with_borrow_mut(|driver| {
            driver
                .as_mut()
                .and_then(|driver| driver.downcast_mut::<NotifierDriver<N>>())
                .expect("init first")
                .tick(answers)
        });
        answer(borsh::to_vec(&turn).expect("borsh encodes an in-memory turn"))
    }

    fn driver<A: View, R>(run: impl FnOnce(&mut Driver<A>) -> R) -> R {
        DRIVER.with_borrow_mut(|driver| {
            run(driver
                .as_mut()
                .and_then(|driver| driver.downcast_mut())
                .expect("init or restore first"))
        })
    }

    pub fn init<A: View>() {
        install_panic_hook();
        let driver = Driver::<A>::new();
        DRIVER.set(Some(Box::new(driver)));
    }

    pub fn tick<A: View>(ptr: u32, len: u32) -> u64 {
        let events: Vec<wire::Event> =
            wire::decode(&take(ptr, len)).expect("invalid host event frame");
        // The host keeps the tree it has, or patches it; the whole tree
        // crosses only when neither will do (the driver leaves it out then).
        answer(driver::<A, _>(|driver| {
            driver.tick_with(events, wire::encode)
        }))
    }

    pub fn snapshot<A: View>() -> u64 {
        answer(wire::abi::encode_result(driver::<A, _>(|driver| {
            driver.snapshot()
        })))
    }

    /// A refused state leaves the driver that was there in place.
    pub fn restore<A: View>(ptr: u32, len: u32) -> u64 {
        install_panic_hook();
        let restored = Driver::<A>::from_snapshot_in(crate::App::for_driver(), &take(ptr, len))
            .map(|driver| {
                DRIVER.set(Some(Box::new(driver)));
                Vec::new()
            });
        answer(wire::abi::encode_result(restored))
    }

    /// A buffer the host fills and the next export [`take`]s. `0` for none.
    pub fn alloc(len: u32) -> u32 {
        if len == 0 {
            return 0;
        }
        Box::into_raw(vec![0u8; len as usize].into_boxed_slice()) as *mut u8 as u32
    }

    /// The argument the host wrote into what [`alloc`] gave it.
    fn take(ptr: u32, len: u32) -> Vec<u8> {
        if len == 0 {
            return Vec::new();
        }
        let bytes = std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize);
        unsafe { Box::from_raw(bytes) }.into_vec()
    }

    fn answer(bytes: Vec<u8>) -> u64 {
        ANSWER.with_borrow_mut(|answer| {
            *answer = bytes;
            wire::abi::pack(answer.as_ptr() as u32, answer.len() as u32)
        })
    }

    /// A trapped instance can never be entered again, so the message leaves
    /// through the host's import before the abort that follows the hook.
    fn install_panic_hook() {
        std::panic::set_hook(Box::new(|info| {
            let payload = info.payload();
            let message = payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(|text| text.as_str()))
                .unwrap_or("panicked");
            let at = info
                .location()
                .map(|location| format!("{}:{}", location.file(), location.line()))
                .unwrap_or_else(|| "unknown".into());
            let line = format!("{message} at {at}");
            unsafe { panicked(line.as_ptr() as u32, line.len() as u32) };
        }));
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod stale_routes_tests;

#[cfg(test)]
mod row_identity_tests;

#[cfg(test)]
mod appends_tests;
