//! GPUI-shaped interaction recipes lowered into driver-owned frame routes.

use crate::{
    AnyElement, AnyView, App, Div, Element, IntoElement, Lowering, ParentElement, Window, slots,
    wire,
};
use gpui::{
    ClickEvent, ElementId, FileDropEvent, MouseButton, SharedString, StyleRefinement, Styled,
    accesskit,
};
use std::time::Duration;

mod bindings;
use bindings::ButtonBinding;
pub(crate) use bindings::EventListener;

mod focus;
pub use focus::FocusHandle;
mod scroll;
pub use scroll::ScrollHandle;

struct TooltipBuilder {
    build: slots::TooltipBuilder,
    hoverable: bool,
}

/// The person asked to go back: the phone's gesture or its bar. Shaped as
/// `ClickEvent` listeners are; it carries nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BackEvent;

/// An assistive-technology action's listener, as gpui takes it.
type A11yListener = Box<dyn FnMut(Option<&accesskit::ActionData>, &mut Window, &mut App)>;

/// What a view element declares besides its children: its style, its id
/// and, from the first one it declares, its interactions. gpui's has the
/// same name and holds the same things; here the interactions sit behind a
/// pointer of their own, so an element that declares none carries none.
#[derive(Default)]
pub struct Interactivity {
    pub(crate) base_style: StyleRefinement,
    pub(crate) id: Option<ElementId>,
    pub(crate) interactions: Option<Box<Interactions>>,
}

impl Interactivity {
    /// The element's interactions, there from the first one it declares.
    pub(crate) fn interactions(&mut self) -> &mut Interactions {
        self.interactions.get_or_insert_default()
    }
}

/// What an element answers to and how it presents itself: its role and
/// aria, focus, conditional styles, listeners and tooltip. The node it
/// lowers to carries these as its [`wire::Interactivity`].
#[derive(Default)]
pub(crate) struct Interactions {
    pub(crate) role: Option<gpui::Role>,
    pub(crate) aria: wire::Aria,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: Option<bool>,
    pub(crate) tab_index: Option<i32>,
    pub(crate) tab_group: bool,
    pub(crate) focus: Option<Box<StyleRefinement>>,
    pub(crate) in_focus: Option<Box<StyleRefinement>>,
    pub(crate) focus_visible: Option<Box<StyleRefinement>>,
    pub(crate) focus_handle: Option<FocusHandle>,
    scroll_handle: Option<ScrollHandle>,
    pub(crate) group: Option<SharedString>,
    pub(crate) hover: Option<Box<StyleRefinement>>,
    pub(crate) file_drag_over: Option<Box<StyleRefinement>>,
    pub(crate) active: Option<Box<StyleRefinement>>,
    pub(crate) group_hover: Option<(SharedString, Box<StyleRefinement>)>,
    pub(crate) group_active: Option<(SharedString, Box<StyleRefinement>)>,
    a11y_actions: Vec<(gpui::accesskit::Action, A11yListener)>,
    pub(crate) on_click: Vec<EventListener<ClickEvent>>,
    pub(crate) on_aux_click: Vec<EventListener<ClickEvent>>,
    consumes_click: bool,
    mouse_down: Vec<ButtonBinding<gpui::MouseDownEvent>>,
    capture_mouse_down: Vec<EventListener<gpui::MouseDownEvent>>,
    mouse_down_out: Vec<EventListener<gpui::MouseDownEvent>>,
    mouse_up: Vec<ButtonBinding<gpui::MouseUpEvent>>,
    capture_mouse_up: Vec<EventListener<gpui::MouseUpEvent>>,
    mouse_up_out: Vec<ButtonBinding<gpui::MouseUpEvent>>,
    mouse_pressure: Vec<EventListener<gpui::MousePressureEvent>>,
    capture_mouse_pressure: Vec<EventListener<gpui::MousePressureEvent>>,
    mouse_move: Vec<EventListener<gpui::MouseMoveEvent>>,
    mouse_exit: Vec<EventListener<gpui::MouseExitEvent>>,
    scroll_wheel: Vec<EventListener<gpui::ScrollWheelEvent>>,
    pinch: Vec<EventListener<gpui::PinchEvent>>,
    capture_pinch: Vec<EventListener<gpui::PinchEvent>>,
    key_down: Vec<EventListener<gpui::KeyDownEvent>>,
    capture_key_down: Vec<EventListener<gpui::KeyDownEvent>>,
    key_up: Vec<EventListener<gpui::KeyUpEvent>>,
    capture_key_up: Vec<EventListener<gpui::KeyUpEvent>>,
    modifiers_changed: Vec<EventListener<gpui::ModifiersChangedEvent>>,
    consumes_keys: Vec<SharedString>,
    on_hover: Option<EventListener<bool>>,
    pub(crate) on_back: Option<EventListener<BackEvent>>,
    hover_listener_mode: gpui::HoverListenerMode,
    on_file_drop_exit: Vec<EventListener<FileDropEvent>>,
    on_file_drop: Vec<EventListener<Vec<wire::methods::Picked>>>,
    tooltip: Option<TooltipBuilder>,
    tooltip_show_delay: Option<Duration>,
    occlude: bool,
    block_mouse_except_scroll: bool,
    /// A press in a dense list ([`design::dense`](crate::design::dense)):
    /// nothing drawn or sent, only what the sweep lets press under a
    /// finger's height.
    pub(crate) dense: bool,
    /// The page on top of the view's stack, at this depth: set by
    /// [`design::page`](crate::design::page) alone, from its [`Nav`](crate::design::Nav).
    pub(crate) page: Option<u32>,
}

/// Add GPUI's stateless and stateful interactivity declarations to an element.
pub trait InteractiveElement: Sized {
    fn interactivity(&mut self) -> &mut Interactivity;

    fn id(mut self, id: impl Into<ElementId>) -> Stateful<Self> {
        self.interactivity().id = Some(id.into());
        Stateful { element: self }
    }

    fn track_focus(mut self, focus_handle: &FocusHandle) -> Self {
        self.interactivity().interactions().focusable = true;
        self.interactivity().interactions().focus_handle = Some(focus_handle.clone());
        self
    }

    fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.interactivity().interactions().tab_stop = Some(tab_stop);
        self
    }

    fn tab_index(mut self, index: isize) -> Self {
        self.interactivity().interactions().focusable = true;
        self.interactivity().interactions().tab_index = i32::try_from(index).ok();
        self.interactivity().interactions().tab_stop = Some(true);
        self
    }

    fn tab_group(mut self) -> Self {
        self.interactivity().interactions().tab_group = true;
        if self.interactivity().interactions().tab_index.is_none() {
            self.interactivity().interactions().tab_index = Some(0);
        }
        self
    }

    fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().group = Some(group.into());
        self
    }

    fn hover(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().interactions().hover = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    fn group_hover(
        mut self,
        group: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().interactions().group_hover =
            Some((group.into(), Box::new(f(StyleRefinement::default()))));
        self
    }

    fn on_mouse_down(
        mut self,
        button: MouseButton,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_down
            .push(ButtonBinding {
                button: Some(button),
                listener: Box::new(listener),
            });
        self
    }

    fn capture_any_mouse_down(
        mut self,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .capture_mouse_down
            .push(Box::new(listener));
        self
    }

    fn on_any_mouse_down(
        mut self,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_down
            .push(ButtonBinding {
                button: None,
                listener: Box::new(listener),
            });
        self
    }

    fn on_mouse_down_out(
        mut self,
        listener: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_down_out
            .push(Box::new(listener));
        self
    }

    fn on_mouse_up(
        mut self,
        button: MouseButton,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_up
            .push(ButtonBinding {
                button: Some(button),
                listener: Box::new(listener),
            });
        self
    }

    fn capture_any_mouse_up(
        mut self,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .capture_mouse_up
            .push(Box::new(listener));
        self
    }

    fn on_any_mouse_up(
        mut self,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_up
            .push(ButtonBinding {
                button: None,
                listener: Box::new(listener),
            });
        self
    }

    fn on_mouse_up_out(
        mut self,
        button: MouseButton,
        listener: impl Fn(&gpui::MouseUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_up_out
            .push(ButtonBinding {
                button: Some(button),
                listener: Box::new(listener),
            });
        self
    }

    fn on_mouse_pressure(
        mut self,
        listener: impl Fn(&gpui::MousePressureEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_pressure
            .push(Box::new(listener));
        self
    }

    fn capture_mouse_pressure(
        mut self,
        listener: impl Fn(&gpui::MousePressureEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .capture_mouse_pressure
            .push(Box::new(listener));
        self
    }

    fn on_mouse_move(
        mut self,
        listener: impl Fn(&gpui::MouseMoveEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_move
            .push(Box::new(listener));
        self
    }

    fn on_mouse_exit(
        mut self,
        listener: impl Fn(&gpui::MouseExitEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .mouse_exit
            .push(Box::new(listener));
        self
    }

    fn on_scroll_wheel(
        mut self,
        listener: impl Fn(&gpui::ScrollWheelEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .scroll_wheel
            .push(Box::new(listener));
        self
    }

    fn on_pinch(
        mut self,
        listener: impl Fn(&gpui::PinchEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .pinch
            .push(Box::new(listener));
        self
    }

    fn capture_pinch(
        mut self,
        listener: impl Fn(&gpui::PinchEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .capture_pinch
            .push(Box::new(listener));
        self
    }

    fn on_key_down(
        mut self,
        listener: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .key_down
            .push(Box::new(listener));
        self
    }

    fn capture_key_down(
        mut self,
        listener: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .capture_key_down
            .push(Box::new(listener));
        self
    }

    fn on_key_up(
        mut self,
        listener: impl Fn(&gpui::KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .key_up
            .push(Box::new(listener));
        self
    }

    fn capture_key_up(
        mut self,
        listener: impl Fn(&gpui::KeyUpEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .capture_key_up
            .push(Box::new(listener));
        self
    }

    fn on_modifiers_changed(
        mut self,
        listener: impl Fn(&gpui::ModifiersChangedEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .modifiers_changed
            .push(Box::new(listener));
        self
    }

    /// The keystrokes, in gpui's words (`"escape"`, `"shift-tab"`), this
    /// element takes, with its `on_key_down` listeners or its keyboard
    /// click (Enter, Space): the host stops each one here once they have
    /// heard it, so no element around it hears it too.
    /// gpui decides that inside the listener with `cx.stop_propagation()`;
    /// a view listener runs after the host has dispatched the key, so it
    /// says which keys it takes up front.
    fn consumes_keys<'a>(mut self, keys: impl IntoIterator<Item = &'a str>) -> Self {
        self.interactivity().interactions().consumes_keys.extend(
            keys.into_iter()
                .map(|key| SharedString::from(key.to_owned())),
        );
        self
    }

    fn on_file_drop_exit(
        mut self,
        listener: impl Fn(&FileDropEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .on_file_drop_exit
            .push(Box::new(listener));
        self
    }

    fn on_file_drop(
        mut self,
        listener: impl Fn(&Vec<wire::methods::Picked>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .on_file_drop
            .push(Box::new(listener));
        self
    }

    fn file_drag_over(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().interactions().file_drag_over =
            Some(Box::new(f(StyleRefinement::default())));
        self
    }

    fn occlude(mut self) -> Self {
        self.interactivity().interactions().occlude = true;
        self
    }

    fn block_mouse_except_scroll(mut self) -> Self {
        self.interactivity()
            .interactions()
            .block_mouse_except_scroll = true;
        self
    }

    fn focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().interactions().focus = Some(Box::new(f(StyleRefinement::default())));
        self
    }

    fn in_focus(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().interactions().in_focus =
            Some(Box::new(f(StyleRefinement::default())));
        self
    }

    fn focus_visible(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().interactions().focus_visible =
            Some(Box::new(f(StyleRefinement::default())));
        self
    }
}

impl InteractiveElement for Div {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

mod lowering;

/// The stateful wrapper returned by [`InteractiveElement::id`].
pub struct Stateful<E> {
    pub(crate) element: E,
}

impl<E: Styled> Styled for Stateful<E> {
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<E: Element> Element for Stateful<E> {
    fn id(&self) -> Option<ElementId> {
        self.element.id()
    }

    fn defers(&self) -> bool {
        self.element.defers()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        Element::lower(Box::new(self.element), lowering)
    }
}

impl<E: Element> IntoElement for Stateful<E> {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl<E: ParentElement> ParentElement for Stateful<E> {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.element.extend(elements);
    }
}

impl<E: InteractiveElement> InteractiveElement for Stateful<E> {
    fn interactivity(&mut self) -> &mut Interactivity {
        self.element.interactivity()
    }
}

/// Stateful interaction methods with GPUI's public names and signatures where
/// the value can be represented by the view wire contract.
///
/// A relation to another node (`aria_labelled_by`, `aria_described_by`,
/// `aria_controls`, `aria_error_message`) has no setter: the host can name
/// no node of the frame for one until the fork can (the app's
/// `docs/ax.md`, AX-115), so writing one does not compile instead of doing
/// nothing. Say it in the node's own label or description:
///
/// ```
/// use view::prelude::*;
/// let _ = div().id("field").aria_description("Use your email address");
/// ```
///
/// ```compile_fail
/// use view::prelude::*;
/// let _ = div().id("field").aria_labelled_by("caption");
/// ```
pub trait StatefulInteractiveElement: InteractiveElement {
    fn role(mut self, role: gpui::Role) -> Self {
        self.interactivity().interactions().role = Some(role);
        self
    }
    /// Takes focus, and sits in the Tab order unless `tab_stop(false)`.
    fn focusable(mut self) -> Self {
        self.interactivity().interactions().focusable = true;
        self
    }
    fn accessibility_id(mut self, id: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().aria.author_id = Some(id.into());
        self
    }
    fn aria_label(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().aria.label = Some(value.into());
        self
    }
    fn aria_description(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().aria.description = Some(value.into());
        self
    }
    fn aria_keyshortcuts(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().aria.keyshortcuts = Some(value.into());
        self
    }
    fn aria_active_descendant(mut self) -> Self {
        self.interactivity().interactions().aria.active_descendant = true;
        self
    }
    fn aria_value(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().aria.value = Some(value.into());
        self
    }
    fn aria_placeholder(mut self, value: impl Into<SharedString>) -> Self {
        self.interactivity().interactions().aria.placeholder = Some(value.into());
        self
    }
    fn aria_selected(mut self, value: bool) -> Self {
        self.interactivity().interactions().aria.selected = Some(value);
        self
    }
    fn aria_expanded(mut self, value: bool) -> Self {
        self.interactivity().interactions().aria.expanded = Some(value);
        self
    }
    fn aria_disabled(mut self, value: bool) -> Self {
        self.interactivity().interactions().aria.disabled = Some(value);
        self
    }
    fn aria_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().interactions().aria.numeric_value = Some(value);
        self
    }
    fn aria_numeric_value_step(mut self, value: f64) -> Self {
        self.interactivity().interactions().aria.numeric_value_step = Some(value);
        self
    }
    fn aria_min_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().interactions().aria.min_numeric_value = Some(value);
        self
    }
    fn aria_max_numeric_value(mut self, value: f64) -> Self {
        self.interactivity().interactions().aria.max_numeric_value = Some(value);
        self
    }
    fn aria_level(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.level = Some(value);
        self
    }
    fn aria_position_in_set(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.position_in_set = Some(value);
        self
    }
    fn aria_size_of_set(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.size_of_set = Some(value);
        self
    }
    fn aria_row_index(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.row_index = Some(value);
        self
    }
    fn aria_column_index(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.column_index = Some(value);
        self
    }
    fn aria_row_count(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.row_count = Some(value);
        self
    }
    fn aria_column_count(mut self, value: usize) -> Self {
        self.interactivity().interactions().aria.column_count = Some(value);
        self
    }
    fn aria_toggled(mut self, value: gpui::Toggled) -> Self {
        self.interactivity().interactions().aria.toggled = Some(value);
        self
    }
    fn aria_orientation(mut self, value: gpui::Orientation) -> Self {
        self.interactivity().interactions().aria.orientation = Some(value);
        self
    }
    // `aria_live` through `aria_current` and `custom_action` are the
    // names planned for the fork, which has none of them yet; the host
    // delivers each through its aria patch until it does.
    fn aria_live(mut self, value: accesskit::Live) -> Self {
        self.interactivity().interactions().aria.live = Some(value);
        self
    }
    fn aria_busy(mut self, value: bool) -> Self {
        self.interactivity().interactions().aria.busy = value;
        self
    }
    fn aria_required(mut self, value: bool) -> Self {
        self.interactivity().interactions().aria.required = value;
        self
    }
    fn aria_invalid(mut self, value: accesskit::Invalid) -> Self {
        self.interactivity().interactions().aria.invalid = Some(value);
        self
    }
    fn aria_read_only(mut self, value: bool) -> Self {
        self.interactivity().interactions().aria.read_only = value;
        self
    }
    fn aria_has_popup(mut self, value: accesskit::HasPopup) -> Self {
        self.interactivity().interactions().aria.has_popup = Some(value);
        self
    }
    fn aria_current(mut self, value: accesskit::AriaCurrent) -> Self {
        self.interactivity().interactions().aria.current = Some(value);
        self
    }
    /// A custom action assistive technology offers by `description`. Its
    /// request is [`accesskit::Action::CustomAction`] with
    /// `ActionData::CustomAction(id)`: one [`Self::on_a11y_action`]
    /// handler answers every custom action a node has.
    fn custom_action(mut self, id: i32, description: impl Into<String>) -> Self {
        self.interactivity()
            .interactions()
            .aria
            .custom_actions
            .push((id, description.into()));
        self
    }
    /// Answers `action` when assistive technology requests it: gpui's own
    /// setter and signature. Unlike gpui, the host keeps Click, Focus, Blur,
    /// SetValue, ReplaceSelectedText and SetTextSelection, and hears only
    /// the first listener per action; `view_wire::audit` faults the rest
    /// (ActionUnhandled).
    fn on_a11y_action(
        mut self,
        action: accesskit::Action,
        listener: impl FnMut(Option<&accesskit::ActionData>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .a11y_actions
            .push((action, Box::new(listener)));
        self
    }
    fn overflow_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(gpui::Overflow::Scroll);
        self.interactivity().base_style.overflow.y = Some(gpui::Overflow::Scroll);
        self
    }
    fn overflow_x_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.x = Some(gpui::Overflow::Scroll);
        self
    }
    fn overflow_y_scroll(mut self) -> Self {
        self.interactivity().base_style.overflow.y = Some(gpui::Overflow::Scroll);
        self
    }
    fn restrict_scroll_to_axis(mut self) -> Self {
        self.interactivity().base_style.restrict_scroll_to_axis = Some(true);
        self
    }
    /// The div `handle` moves: it scrolls (`overflow_y_scroll`) and has
    /// an id, as gpui's does.
    fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.interactivity().interactions().scroll_handle = Some(handle.clone());
        self
    }

    fn active(mut self, f: impl FnOnce(StyleRefinement) -> StyleRefinement) -> Self {
        self.interactivity().interactions().active = Some(Box::new(f(StyleRefinement::default())));
        self
    }
    fn group_active(
        mut self,
        group: impl Into<SharedString>,
        f: impl FnOnce(StyleRefinement) -> StyleRefinement,
    ) -> Self {
        self.interactivity().interactions().group_active =
            Some((group.into(), Box::new(f(StyleRefinement::default()))));
        self
    }
    /// Every listener given runs, in order, as gpui's do.
    fn on_click(mut self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.interactivity()
            .interactions()
            .on_click
            .push(Box::new(listener));
        self
    }
    fn on_aux_click(
        mut self,
        listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.interactivity()
            .interactions()
            .on_aux_click
            .push(Box::new(listener));
        self
    }
    /// This element's click consumes its press: the host stops the
    /// pointer's click here, so an element under it (a card's own click)
    /// does not take the same press. gpui calls `cx.stop_propagation()` in
    /// the listener; a view listener runs too late for that, so it says
    /// so up front. A focusable element whose keyboard click must not
    /// reach a composite around it consumes Enter and Space too, with
    /// [`InteractiveElement::consumes_keys`]. It needs an `on_click`, in
    /// whose listener the host stops the press: without one the host
    /// refuses the frame, and the view's test fails.
    fn consumes_click(mut self) -> Self {
        self.interactivity().interactions().consumes_click = true;
        self
    }
    fn on_hover(mut self, listener: impl Fn(&bool, &mut Window, &mut App) + 'static) -> Self {
        self.interactivity().interactions().on_hover = Some(Box::new(listener));
        self
    }
    /// Hears Back (a phone's gesture or its bar) while the view has
    /// somewhere to go back to. On the ROOT element only, and set exactly
    /// when a page sits under the root of the view's [`Pages`] or a sheet
    /// is open (`.when(pages.depth() > 0, |root| root.on_back(..))`):
    /// absent, the host leaves the program instead; below the root the
    /// host refuses the frame. A host with no Back (the desktop) never
    /// sends it.
    ///
    /// [`Pages`]: crate::design::Pages
    fn on_back(mut self, listener: impl Fn(&BackEvent, &mut Window, &mut App) + 'static) -> Self {
        self.interactivity().interactions().on_back = Some(Box::new(listener));
        self
    }
    fn hover_listener_mode(mut self, mode: gpui::HoverListenerMode) -> Self {
        self.interactivity().interactions().hover_listener_mode = mode;
        self
    }
    fn tooltip(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.interactivity().interactions().tooltip = Some(TooltipBuilder {
            build: Box::new(build_tooltip),
            hoverable: false,
        });
        self
    }
    fn hoverable_tooltip(
        mut self,
        build_tooltip: impl Fn(&mut Window, &mut App) -> AnyView + 'static,
    ) -> Self {
        self.interactivity().interactions().tooltip = Some(TooltipBuilder {
            build: Box::new(build_tooltip),
            hoverable: true,
        });
        self
    }
    fn tooltip_show_delay(mut self, delay: Duration) -> Self {
        self.interactivity().interactions().tooltip_show_delay = Some(delay);
        self
    }
}

impl<T: InteractiveElement> StatefulInteractiveElement for Stateful<T> {}

impl<E> gpui::prelude::FluentBuilder for Stateful<E> {}

#[cfg(test)]
mod tests;
