//! View-side GPUI-shaped authoring.
//!
//! The fluent style methods are the real `gpui::Styled` implementation. The
//! element and interaction traits are deliberately local: native GPUI
//! elements require a native layout arena, window, and application, none of
//! which exists in a wasm view. Lowering turns this small recipe into wire
//! data once per frame.

use crate::interactivity::{EventListener, Interactivity};
use crate::{App, Window, slots, wire};
use gpui::{
    ElementId, ListHorizontalSizingBehavior, ListSizingBehavior, Overflow, ScrollStrategy,
    SharedString, StyleRefinement, Styled,
};
use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// A view element that can be lowered by the driver.
///
/// This is intentionally a view-side boundary with the same name as GPUI's
/// native trait. GPUI's real `Element` requires native layout and paint state;
/// a wasm view has neither, so lowering is the only operation it can perform.
pub trait Element: 'static + IntoElement {
    /// The authored identity that enters the typed ancestry while this element lowers.
    fn id(&self) -> Option<ElementId> {
        None
    }

    #[doc(hidden)]
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node;

    /// Whether this element lowers to the node of an element it builds (a
    /// view's render), rather than to a node of its own.
    #[doc(hidden)]
    fn defers(&self) -> bool {
        false
    }

    #[doc(hidden)]
    fn into_any(self) -> AnyElement {
        AnyElement(Box::new(self))
    }
}

/// A value that can be converted into a view element recipe.
pub trait IntoElement: Sized {
    type Element: Element;

    fn into_element(self) -> Self::Element;

    fn into_any_element(self) -> AnyElement {
        self.into_element().into_any()
    }
}

trait ElementObject {
    fn id(&self) -> Option<ElementId>;
    fn defers(&self) -> bool;
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node;
}

impl<T: Element> ElementObject for T {
    fn id(&self) -> Option<ElementId> {
        Element::id(self)
    }

    fn defers(&self) -> bool {
        Element::defers(self)
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        Element::lower(self, lowering)
    }
}

/// A type-erased view element, used for conditional children and components.
pub struct AnyElement(Box<dyn ElementObject>);

impl Element for AnyElement {
    fn id(&self) -> Option<ElementId> {
        self.0.id()
    }

    fn defers(&self) -> bool {
        self.0.defers()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        self.0.lower(lowering)
    }

    fn into_any(self) -> AnyElement {
        self
    }
}

impl IntoElement for AnyElement {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }

    fn into_any_element(self) -> AnyElement {
        self
    }
}

impl gpui::prelude::FluentBuilder for AnyElement {}

/// The explicit lowering context for one driver frame. Every element is
/// filed by the host's own rule ([`wire::identity`]), so the path a route,
/// a list or a tooltip is keyed by here is the path the host files the
/// node under. Lowering claims nothing: whether two ids meet is the host's
/// to say, and its sanitizer says it of every frame, a test's too.
pub struct Lowering<'a> {
    window: &'a mut Window,
    app: &'a mut App,
    /// The path of the element being lowered: its id last, when it has one.
    authored_path: Vec<wire::ElementIdWire>,
    /// The index of the list row whose root lowers next ([`Self::lower_row`]).
    row: Option<usize>,
    /// The state of each list lowered so far, beside that list's path.
    lists: Vec<(crate::ListState, Vec<wire::ElementIdWire>)>,
    // The nodes lowered so far.
    lowered: usize,
    // For each memo being lowered, innermost last: whether it holds a list,
    // whose rows are the list state's now, and so can't be kept.
    opaque: Vec<bool>,
    // The entities whose renders are being lowered, innermost last.
    entities: Vec<u64>,
    // A tooltip's content keeps nothing: it lowers once, when asked.
    tooltip: bool,
}

/// An authored [`ElementId`] as the wire carries it. Every id a view can
/// author fits; the only refusals are ids gpui itself could not name.
pub(crate) fn wire_id(id: ElementId) -> wire::ElementIdWire {
    wire::ElementIdWire::from_gpui(id)
        .expect("element ID must be portable across the view boundary")
}

impl<'a> Lowering<'a> {
    pub(crate) fn new(window: &'a mut Window, app: &'a mut App) -> Self {
        slots::begin_lowering(&app.inner.slots, None);
        // the presses the last tree marked dense are gone with it
        app.inner.dense.borrow_mut().clear();
        Self {
            window,
            app,
            authored_path: Vec::new(),
            row: None,
            lists: Vec::new(),
            lowered: 0,
            opaque: Vec::new(),
            entities: Vec::new(),
            tooltip: false,
        }
    }

    /// A lowering of the content the tooltip route `request` builds: its
    /// listeners are keyed inside that tooltip and live as long as it does.
    pub(crate) fn within_tooltip(window: &'a mut Window, app: &'a mut App, request: u32) -> Self {
        slots::begin_lowering(&app.inner.slots, Some(request));
        Self {
            window,
            app,
            authored_path: Vec::new(),
            row: None,
            lists: Vec::new(),
            lowered: 0,
            opaque: Vec::new(),
            entities: Vec::new(),
            tooltip: true,
        }
    }

    // The nodes this lowering lowered: an element that lowers to a node
    // of its own counts one.
    pub(crate) fn lowered(&self) -> usize {
        self.lowered
    }

    // A list lowered inside a memo: the memo is lowered every frame.
    pub(crate) fn opaque(&mut self) {
        self.opaque.iter_mut().for_each(|opaque| *opaque = true);
    }

    // Lowers what `lower` lowers as the render of `entity`, inside the
    // entity lowered around it: a notify of `entity` is one of each
    // entity around it too.
    pub(crate) fn entity(
        &mut self,
        entity: u64,
        lower: impl FnOnce(&mut Self) -> wire::Node,
    ) -> wire::Node {
        if let Some(&parent) = self.entities.last() {
            let mut memos = self.app.inner.memos.borrow_mut();
            memos.parents.insert(entity, parent);
        }
        self.entities.push(entity);
        let node = lower(self);
        self.entities.pop();
        node
    }

    // The node `memo` stands for: the one kept at its place when its deps
    // are the ones it was kept with and its routes are still there, else
    // the one its build lowers to now, kept for the next frame. With
    // checks on (a debug build) a kept memo is lowered again all the same
    // and fails, naming it, when the two differ.
    pub(crate) fn memo(&mut self, memo: crate::memo::Memo) -> wire::Node {
        let crate::memo::Memo {
            id,
            deps,
            same,
            build,
        } = memo;
        if self.tooltip {
            let element = build(self.window, self.app);
            return self.lower_element(element);
        }
        let key = crate::memo::Key {
            path: self.authored_path.clone(),
            row: self.row,
            id,
        };
        let (kept, frame) = {
            let mut memos = self.app.inner.memos.borrow_mut();
            (memos.entries.remove(&key), memos.frame)
        };
        let mut kept = kept.filter(|kept| same(&*kept.deps, &*deps));
        if let Some(kept) = &kept {
            debug_assert!(
                kept.seen != frame,
                "two memos at {key:?}: give each its own id"
            );
        }
        // a memo whose routes can't be taken again is lowered anew
        if !self.app.inner.memo_checks.get()
            && let Some(entry) = kept.take()
            && slots::replay(self.slots(), &entry.journal)
        {
            self.row.take();
            let dense = entry.dense.iter().cloned();
            self.app.inner.dense.borrow_mut().extend(dense);
            let node = entry.node.clone();
            let entry = crate::memo::Entry {
                seen: frame,
                ..entry
            };
            self.app.inner.memos.borrow_mut().entries.insert(key, entry);
            return node;
        }
        self.app.inner.memos.borrow_mut().anew.push(key.id.clone());
        let dense_from = self.app.inner.dense.borrow().len();
        self.opaque.push(false);
        slots::begin_journal(self.slots());
        let element = build(self.window, self.app);
        let node = self.lower_element(element);
        let journal = slots::end_journal(self.slots());
        let opaque = self.opaque.pop().expect("its own mark");
        assert!(
            node.identity().is_some() || key.row.is_some(),
            "memo {:?} lowers to a node with no id: give the element it builds one",
            key.id
        );
        let mut lowered = node.clone();
        crate::memo::without_pictures(&mut lowered);
        if let Some(kept) = kept {
            let (mut was, mut now) = (kept.node, lowered.clone());
            let patches = wire::diff(&mut was, &mut now);
            assert!(
                patches.is_empty(),
                "memo {:?} at {:?} was kept, but lowering it again draws something else \
                 ({} patches, the first {:?}): its deps leave out something it reads",
                key.id,
                key.path,
                patches.len(),
                patches.first()
            );
        }
        if !opaque {
            let dense = self.app.inner.dense.borrow()[dense_from..].to_vec();
            let entry = crate::memo::Entry {
                deps,
                node: lowered,
                journal,
                dense,
                seen: frame,
            };
            self.app.inner.memos.borrow_mut().entries.insert(key, entry);
        }
        node
    }

    pub(crate) fn slots(&self) -> &slots::Context {
        &self.app.inner.slots
    }

    pub(crate) fn theme(&self) -> crate::Theme {
        *self.app.global::<crate::Theme>()
    }

    /// The id `style` crosses under: its entry in the frame's style table,
    /// one for every node that carries the same style.
    pub(crate) fn style(&self, style: &StyleRefinement) -> wire::StyleId {
        self.app.inner.styles.borrow_mut().intern(style)
    }

    /// The id of the style that sets nothing, which a bare text carries.
    pub(crate) fn no_style(&self) -> wire::StyleId {
        self.app.inner.styles.borrow_mut().intern_empty()
    }

    pub(crate) fn parts(&mut self) -> (&mut Window, &mut App) {
        (self.window, self.app)
    }

    pub(crate) fn render_once(&mut self, component: impl RenderOnce) -> wire::Node {
        let element = component.render(self.window, self.app).into_element();
        self.lower_element(element)
    }

    pub(crate) fn lower<E: IntoElement>(&mut self, element: E) -> wire::Node {
        self.lower_element(element.into_element())
    }

    /// Lowers `element` in its scope: filed under its id (or, as a list
    /// row's root, its index), and everything it lowers under it.
    pub(crate) fn lower_element<E: Element>(&mut self, element: E) -> wire::Node {
        // a row's root is the first element under it that lowers to a node
        // of its own: a deferred view lowers to the one its render returns
        let defers = element.defers();
        let row = if defers { None } else { self.row.take() };
        self.lowered += usize::from(!defers);
        let segment = wire::identity::segment(element.id().map(wire_id), row);
        let entered = segment.is_some();
        if let Some(segment) = segment {
            slots::enter_scope(&self.app.inner.slots, segment.clone());
            self.authored_path.push(segment);
        }
        // the box an `AnyElement` already is, or the one box an element gets
        let node = element.into_any().0.lower(self);
        debug_assert!(
            defers
                || wire::identity::segment(node.identity().cloned(), row).as_ref()
                    == entered.then(|| self.authored_path.last()).flatten(),
            "an element is filed as the host files the node it lowers to"
        );
        if entered {
            self.authored_path.pop();
            slots::leave_scope(&self.app.inner.slots);
        }
        node
    }

    /// Lowers row `index` of a list: filed under its own id, else under
    /// its index, so the ids inside one row never meet another row's.
    pub(crate) fn lower_row<E: Element>(&mut self, index: usize, element: E) -> wire::Node {
        self.row = Some(index);
        let node = self.lower_element(element);
        debug_assert!(self.row.is_none(), "a row lowers to a node");
        node
    }

    /// The list being lowered draws `state`. A state is one list's (its
    /// window of rows, the commands waiting for it), so a second list
    /// drawing it in this frame is refused, naming both.
    pub(crate) fn draws_list(&mut self, state: &crate::ListState) {
        self.opaque();
        if let Some((_, first)) = self.lists.iter().find(|(drawn, _)| drawn.is(state)) {
            panic!(
                "one ListState drawn by two lists: {first:?} and {:?}; each list takes a state \
                 of its own",
                self.authored_path
            );
        }
        self.lists.push((state.clone(), self.authored_path.clone()));
    }

    pub(crate) fn current_path(&self) -> &[wire::ElementIdWire] {
        &self.authored_path
    }

    /// The element being lowered is a dense list's press
    /// ([`design::dense`](crate::design::dense)): its path joins the marks
    /// the [`App`] keeps for the sweep.
    pub(crate) fn mark_dense(&self) {
        self.app
            .inner
            .dense
            .borrow_mut()
            .push(self.authored_path.clone());
    }

    /// The id of the identified element being lowered, as the wire carries
    /// it: the segment it was filed under when its lowering began.
    pub(crate) fn own_id(&self) -> wire::ElementIdWire {
        self.authored_path
            .last()
            .cloned()
            .expect("an identified element lowers inside its authored scope")
    }

    /// A route for a listener of `kind` on the element being lowered.
    pub(crate) fn route<A: 'static>(
        &self,
        kind: slots::Kind,
        listener: impl Fn(&A, &mut Window, &mut App) + 'static,
    ) -> u32 {
        slots::route(&self.app.inner.slots, kind, listener)
    }

    pub(crate) fn picture(&self, bytes: impl AsRef<[u8]>) -> (u64, Option<Vec<u8>>) {
        slots::picture(&self.app.inner.slots, bytes)
    }

    pub(crate) fn image(&self, image: &std::sync::Arc<gpui::Image>) -> (u64, Option<Vec<u8>>) {
        slots::image(&self.app.inner.slots, image)
    }

    pub(crate) fn tooltip(&self, build: slots::TooltipBuilder) -> u32 {
        slots::tooltip(&self.app.inner.slots, build)
    }

    pub(crate) fn rich_text_tooltip(&self, build: slots::RichTextTooltipBuilder) -> u32 {
        slots::rich_text_tooltip(&self.app.inner.slots, build)
    }
}

/// A view container backed by a real GPUI style refinement.
#[derive(Default)]
pub struct Div {
    pub(crate) interactivity: Box<Interactivity>,
    children: Vec<AnyElement>,
}

impl Styled for Div {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl Element for Div {
    fn id(&self) -> Option<ElementId> {
        self.interactivity.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            mut interactivity,
            children,
        } = *self;
        let id = interactivity.id.as_ref().map(|_| lowering.own_id());
        if id.is_some() {
            bar_gutter(&mut interactivity.base_style);
        }
        let style = lowering.style(&interactivity.base_style);
        let wire_interactivity = interactivity.into_wire(lowering);
        let children = children
            .into_iter()
            .map(|child| lowering.lower_element(child))
            .collect();
        wire::Node::Container(crate::wire::ContainerNode {
            id,
            style,
            interactivity: wire_interactivity,
            children,
        })
    }
}

/// A scroller the host gives a vertical bar (one with an id) keeps its
/// right edge for that bar: the bar paints over the scroller, so content
/// inset less than its width would sit under it.
fn bar_gutter(style: &mut StyleRefinement) {
    use gpui::{AbsoluteLength, DefiniteLength};
    if style.overflow.y != Some(Overflow::Scroll) {
        return;
    }
    let bar = crate::design::size::SCROLLBAR;
    let right = &mut style.padding.right;
    match right {
        Some(DefiniteLength::Absolute(AbsoluteLength::Pixels(inset))) if *inset >= bar => {}
        _ => *right = Some(bar.into()),
    }
}

impl IntoElement for Div {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Div {}

/// Construct an empty view container.
pub fn div() -> Div {
    Div::default()
}

/// A text field the host's editing engine owns, shown as the host's native
/// one-line field, bound to a [`TextField`](crate::TextField) of the view:
/// the host adopts the field's text when its generation moves, and whenever
/// the host's text moves it lands in the field and the view renders. Its
/// label is what assistive technology calls it: a field has one from birth.
pub struct Input {
    id: ElementId,
    multiline: bool,
    field: crate::TextField,
    placeholder: String,
    options: wire::InputOptions,
    secure: bool,
    claims: Vec<wire::KeyClaim>,
    style: Box<StyleRefinement>,
    on_change: Option<EventListener<wire::TextChange>>,
    on_key: Option<EventListener<gpui::KeyDownEvent>>,
    on_submit: Option<EventListener<()>>,
}

impl Input {
    /// A field over `field`: `Input::new("name", &self.name, "Your name")`.
    /// What is typed lands in `field`, read with
    /// [`text()`](crate::TextField::text), and renders the view, with no
    /// listener written for it.
    pub fn new(
        id: impl Into<ElementId>,
        field: &crate::TextField,
        label: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            multiline: false,
            field: field.clone(),
            placeholder: String::new(),
            options: wire::InputOptions {
                label: label.into(),
                ..Default::default()
            },
            secure: false,
            claims: Vec::new(),
            style: Box::default(),
            on_change: None,
            on_key: None,
            on_submit: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.options.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.options.disabled = disabled;
        self
    }

    /// The value is wrong; [`Self::description`] says why.
    pub fn invalid(mut self, invalid: gpui::accesskit::Invalid) -> Self {
        self.options.invalid = Some(invalid);
        self
    }

    pub fn required(mut self, required: bool) -> Self {
        self.options.required = required;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.options.read_only = read_only;
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    // What a software keyboard is told (`TextInputConfiguration`'s fields
    // but `secure`, which is `Input::secure`). gpui's default turns every
    // assistance OFF (no autocorrect, no capitalization, no suggestions),
    // so a field of free prose sets them on at the call site, and a field
    // whose text is not prose (a name, a code, an address) says what it is
    // (rule 7).

    /// The platform may correct what is typed: off for a name, a code, an address.
    pub fn autocorrect(mut self, autocorrect: bool) -> Self {
        self.options.text_input.autocorrect = autocorrect;
        self
    }
    pub fn autocapitalize(mut self, autocapitalize: gpui::Autocapitalize) -> Self {
        self.options.text_input.autocapitalize = autocapitalize.into();
        self
    }
    /// The keyboard may offer words and spellcheck.
    pub fn suggestions(mut self, suggestions: bool) -> Self {
        self.options.text_input.suggestions = suggestions;
        self
    }
    /// The label of the keyboard's confirm key (`Go`, `Send`, `Search`);
    /// pressing it is still input: a many-line field's Return inserts a
    /// newline whatever the label, and the view's Send button sends.
    pub fn input_action(mut self, action: gpui::TextInputAction) -> Self {
        self.options.text_input.input_action = action.into();
        self
    }
    /// The keyboard that opens: numbers for a member number, a URL
    /// keyboard for an address.
    pub fn virtual_keyboard(mut self, keyboard: gpui::VirtualKeyboard) -> Self {
        self.options.text_input.virtual_keyboard = keyboard.into();
        self
    }

    /// Hears the host's text whenever it moves: typed, pasted, undone, or
    /// an asked-for edit landed. For a view that does more on a change than
    /// keep the text: the bound field already holds the change, and the
    /// view already renders, when the listener runs.
    pub fn on_change(
        mut self,
        listener: impl Fn(&wire::TextChange, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(listener));
        self
    }

    /// Enter in a one-line field.
    pub fn on_submit(mut self, listener: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_submit = Some(Box::new(listener));
        self
    }

    /// A key the view hears on `on_key` instead of the engine: the arrows
    /// of a filter field over a list. The engine's own keys — Backspace,
    /// Delete, Tab, undo and redo — cannot be claimed.
    pub fn claim(mut self, claim: wire::KeyClaim) -> Self {
        self.claims.push(claim);
        self
    }

    pub fn claims(mut self, claims: impl IntoIterator<Item = wire::KeyClaim>) -> Self {
        self.claims.extend(claims);
        self
    }

    /// Hears every claimed key as the host receives it, text untouched.
    pub fn on_key(
        mut self,
        listener: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_key = Some(Box::new(listener));
        self
    }
}

impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl Element for Input {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let this = *self;
        let id = wire_id(this.id);
        let field = this.field.state().clone();
        crate::text::lowered_generation(field.generation);
        // the binding: the host's word lands in the view's field, the view
        // renders, and then the author's own listener hears it, if any
        let (bound, heard) = (this.field, this.on_change);
        let on_change = Some(lowering.route(
            slots::Kind::Change,
            move |change: &wire::TextChange, window: &mut Window, app: &mut App| {
                bound.apply(change);
                app.notify();
                if let Some(heard) = &heard {
                    heard(change, window, app);
                }
            },
        ));
        let on_key = this
            .on_key
            .map(|listener| lowering.route(slots::Kind::Key, listener));
        let on_submit = this
            .on_submit
            .map(|listener| lowering.route(slots::Kind::Submit, listener));
        let claims = this.claims;
        debug_assert!(
            !claims.iter().any(wire::KeyClaim::engine_owned),
            "Backspace, Delete, Tab and the undo chords are the engine's: the host refuses a claim on them"
        );
        wire::Node::Field {
            id,
            multiline: this.multiline,
            value: field.text,
            cursor: field.cursor,
            generation: field.generation,
            revision: field.revision,
            tokens: field.tokens.into(),
            claims: claims.into(),
            options: this.options.into(),
            placeholder: this.placeholder,
            secure: this.secure,
            on_change,
            on_key,
            on_submit,
            style: lowering.style(&this.style),
        }
    }
}

impl IntoElement for Input {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Input {}

/// The many-line [`Input`]: the host's native text area over a
/// [`TextField`](crate::TextField). Enter is a new line unless the view
/// claims it; a claimed key reaches `on_key` instead of editing.
pub struct Textarea(Input);

impl Textarea {
    pub fn new(
        id: impl Into<ElementId>,
        field: &crate::TextField,
        label: impl Into<String>,
    ) -> Self {
        let mut input = Input::new(id, field, label);
        input.multiline = true;
        Self(input)
    }

    pub fn placeholder(self, placeholder: impl Into<String>) -> Self {
        Self(self.0.placeholder(placeholder))
    }

    pub fn read_only(self, read_only: bool) -> Self {
        Self(self.0.read_only(read_only))
    }

    /// See [`Input::description`].
    pub fn description(self, description: impl Into<String>) -> Self {
        Self(self.0.description(description))
    }

    /// See [`Input::disabled`].
    pub fn disabled(self, disabled: bool) -> Self {
        Self(self.0.disabled(disabled))
    }

    /// See [`Input::invalid`].
    pub fn invalid(self, invalid: gpui::accesskit::Invalid) -> Self {
        Self(self.0.invalid(invalid))
    }

    /// See [`Input::autocorrect`].
    pub fn autocorrect(self, autocorrect: bool) -> Self {
        Self(self.0.autocorrect(autocorrect))
    }
    /// See [`Input::autocapitalize`].
    pub fn autocapitalize(self, autocapitalize: gpui::Autocapitalize) -> Self {
        Self(self.0.autocapitalize(autocapitalize))
    }
    /// See [`Input::suggestions`].
    pub fn suggestions(self, suggestions: bool) -> Self {
        Self(self.0.suggestions(suggestions))
    }
    /// See [`Input::input_action`]: `Send` labels the key; the view's
    /// button sends.
    pub fn input_action(self, action: gpui::TextInputAction) -> Self {
        Self(self.0.input_action(action))
    }
    /// See [`Input::virtual_keyboard`].
    pub fn virtual_keyboard(self, keyboard: gpui::VirtualKeyboard) -> Self {
        Self(self.0.virtual_keyboard(keyboard))
    }

    /// See [`Input::claim`].
    pub fn claim(self, claim: wire::KeyClaim) -> Self {
        Self(self.0.claim(claim))
    }

    pub fn claims(self, claims: impl IntoIterator<Item = wire::KeyClaim>) -> Self {
        Self(self.0.claims(claims))
    }

    /// See [`Input::on_change`].
    pub fn on_change(
        self,
        listener: impl Fn(&wire::TextChange, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self(self.0.on_change(listener))
    }

    /// See [`Input::on_key`].
    pub fn on_key(
        self,
        listener: impl Fn(&gpui::KeyDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self(self.0.on_key(listener))
    }
}

impl Styled for Textarea {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.0.style
    }
}

impl Element for Textarea {
    fn id(&self) -> Option<ElementId> {
        Element::id(&self.0)
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        Element::lower(Box::new(self.0), lowering)
    }
}

impl IntoElement for Textarea {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for Textarea {}

/// Add children to an element recipe.
pub trait ParentElement {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>);

    fn child(mut self, child: impl IntoElement) -> Self
    where
        Self: Sized,
    {
        self.extend(std::iter::once(child.into_any_element()));
        self
    }

    /// Adds each of `children`, as [`Self::child`] would.
    ///
    /// Children are not scopes of their own, as the rows of a `list` or a
    /// `uniform_list` are under their list's id: an id inside one child
    /// meets the same id inside the next. A row of data built here carries
    /// its own id (its item's key), which scopes the ids inside it. Two
    /// children named by one key are refused, naming the id and its scope.
    fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self
    where
        Self: Sized,
    {
        self.extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl ParentElement for Div {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Element for SharedString {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Text(crate::wire::TextNode {
            id: None,
            style: lowering.no_style(),
            content: self.to_string(),
            transform: None,
        })
    }
}

/// Words the host draws in capitals and names as written (CSS
/// `text-transform: uppercase`). Built by [`crate::design::eyebrow`], the
/// one role set in capitals.
pub(crate) struct Capitals(pub(crate) SharedString);

impl Element for Capitals {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        wire::Node::Text(crate::wire::TextNode {
            id: None,
            style: lowering.no_style(),
            content: self.0.to_string(),
            transform: Some(crate::wire::TextTransform::Uppercase),
        })
    }
}

impl IntoElement for Capitals {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for &'static str {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.lower((*self).to_owned())
    }
}

impl IntoElement for String {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
    }
}

impl IntoElement for &'static str {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for SharedString {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl IntoElement for Cow<'static, str> {
    type Element = SharedString;

    fn into_element(self) -> Self::Element {
        self.into()
    }
}

/// A one-shot component with the same call shape as GPUI's `RenderOnce`.
pub trait RenderOnce: 'static {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement;
}

mod uniform_list;
pub(crate) use uniform_list::UniformLists;
pub use uniform_list::{UniformList, UniformListScrollHandle, uniform_list};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod ancestry_tests;

#[cfg(test)]
mod bar_gutter_tests {
    use super::*;
    use crate::prelude::*;
    use gpui::px;

    #[test]
    fn a_scroller_keeps_its_right_edge_for_the_bar() {
        let bar = crate::design::size::SCROLLBAR;
        let mut inset = crate::div().id("s").overflow_y_scroll().p(px(8.));
        let mut style = inset.style().clone();
        bar_gutter(&mut style);
        assert_eq!(style.padding.right, Some(bar.into()));
        assert_eq!(
            style.padding.left,
            Some(px(8.).into()),
            "only the bar's edge"
        );
        let mut wide = crate::div().id("s").overflow_y_scroll().pr(px(24.));
        let mut style = wide.style().clone();
        bar_gutter(&mut style);
        assert_eq!(style.padding.right, Some(px(24.).into()));
        let mut still = crate::div().p(px(8.));
        let mut style = still.style().clone();
        bar_gutter(&mut style);
        assert_eq!(
            style.padding.right,
            Some(px(8.).into()),
            "no scroll, no bar"
        );
    }
}
