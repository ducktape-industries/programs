use super::focus::{self, Focus};
use super::{
    FakeHost, assert_frame_accessible, assert_taken_whole, button, chain_to, input, keys, texts,
};
use std::collections::HashMap;

use crate::{
    App, Context, Driver, Entity, View, Window,
    host::Host,
    wire::{self, DispatchPhase, Event, Frame, Interactivity, Node},
};

trait TestDriver {
    fn tick(&mut self, events: Vec<Event>) -> (Frame, usize);
    fn app_mut(&mut self) -> &mut App;
    fn host(&self) -> Host;
    fn snapshot(&self) -> Result<Vec<u8>, String>;
    fn renders(&self) -> u64;
}
impl<V: View> TestDriver for Driver<V> {
    /// The frame the host gets, and its size on the wire.
    fn tick(&mut self, events: Vec<Event>) -> (Frame, usize) {
        self.tick_with(events, |frame| (frame.clone(), wire::encode(frame).len()))
    }
    fn app_mut(&mut self) -> &mut App {
        self.app_mut()
    }
    fn host(&self) -> Host {
        self.host()
    }
    fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.snapshot()
    }
    fn renders(&self) -> u64 {
        self.renders
    }
}

/// What one tick cost: what the view did and what crossed to the host.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    /// The view rendered: it called `cx.notify()`, or the host had no tree.
    pub rendered: bool,
    /// Nodes in the tree the host shows after the tick.
    pub nodes: usize,
    /// Patches the frame carried; none when it sent the tree whole or
    /// nothing changed.
    pub patches: usize,
    /// The frame's encoded size: the bytes the host decodes for the tick.
    pub bytes: usize,
    pub requests: usize,
    pub busy: bool,
}

/// One view and its [`FakeHost`], driven as the app drives it: every
/// `simulate_*` sends what the host sends for that input and ticks until the
/// view parks. Each frame is held to the host's rules: its sanitizer, the
/// accessibility audit, the request budget, and keys delivered only along
/// the focus path.
#[derive(Default)]
pub struct TestAppContext {
    host: FakeHost,
    driver: Option<Box<dyn TestDriver>>,
    /// The last frame, as the host received it.
    frame: Frame,
    /// The tree the host holds: the last one sent whole, patched since.
    tree: Option<Node>,
    /// The style table of that tree, as the host holds it: a whole frame's
    /// entries, and each one a later frame brought.
    styles: wire::Styles,
    /// The presses [`design::dense`](crate::design::dense) marked in the
    /// tree the view last lowered, as its [`App`] keeps them.
    dense: Vec<Vec<wire::ElementIdWire>>,
    reports: Vec<TickReport>,
    focus: Option<Focus>,
    globals: crate::context::Globals,
    ticks: u64,
    /// The text of every field in the tree, as the host's engine holds it,
    /// by the ids the field is filed under.
    fields: HashMap<Vec<wire::ElementIdWire>, HostField>,
    /// The host engine's revision of the fields' text, one count for all,
    /// and every edit it made, at the revision that made it.
    text_revision: u64,
    edits: Vec<(u64, wire::Edit)>,
    /// The size the host lays the view out in, sent ahead of its first
    /// tick as the app does ([`VIEWPORT`] until [`simulate_resize`]).
    ///
    /// [`simulate_resize`]: Self::simulate_resize
    viewport: Option<(f32, f32)>,
    /// The bounds each sensor in the tree was last told, by the ids it is
    /// filed under: the host tells them again only when they differ.
    sensors: HashMap<Vec<wire::ElementIdWire>, [f32; 4]>,
    /// The device as last told through [`simulate_touch`](Self::simulate_touch):
    /// a replacement hears it before its first tree, as the app tells it.
    touch: bool,
    /// Set by [`sweep`](super::sweep): every frame is held to
    /// [`assert_fits`](super::assert_fits) at this width under this ladder.
    pub(super) fits: Option<(f32, crate::design::Ladder)>,
}

/// One field's text in the host's engine: the engine owns it, and the view's
/// copy in a frame only follows. `generation` is the view's document the
/// engine took its text from.
///
/// Narrower than the app's host: it tells the view of a text it adopted
/// only when the text differs (the app also when the cursor or the preedit
/// does), keeps one revision and one edit log for all fields, forgets every
/// field on a restore, and has no IME.
struct HostField {
    generation: u64,
    text: String,
    tokens: Vec<wire::TextToken>,
}

/// The window a test host opens a view in, until a test resizes it: a
/// desk's 60% × 70% of 1280 × 764 less the pane's chrome, about what the
/// console gives a new window.
pub const VIEWPORT: (f32, f32) = (766., 501.);

impl TestAppContext {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn host(&self) -> FakeHost {
        self.host.clone()
    }
    pub fn open<V: View>(&mut self) -> Entity<V> {
        self.host.declare(V::CAPABILITIES, V::TARGETS);
        let driver = Driver::<V>::initialize_in(self.fresh_app(), None);
        let entity = driver.entity();
        self.mount(Box::new(driver));
        entity
    }
    pub fn snapshot(&self) -> Result<Vec<u8>, String> {
        self.driver.as_ref().expect("open a view first").snapshot()
    }
    /// A redeploy: the view restored from `bytes` in a fresh instance, as
    /// the app swaps it in. The host keeps its focus where it was.
    pub fn restore<V: View>(&mut self, bytes: &[u8]) -> Result<Entity<V>, String> {
        self.host.declare(V::CAPABILITIES, V::TARGETS);
        let driver = Driver::<V>::from_snapshot_in(self.fresh_app(), bytes)?;
        let entity = driver.entity();
        self.mount(Box::new(driver));
        Ok(entity)
    }
    /// Seats `driver` as the app seats a view: the host's facts (the
    /// viewport, the reader's offset) reach it on its first tick.
    fn mount(&mut self, driver: Box<dyn TestDriver>) {
        self.host.reset_connection();
        self.driver = Some(driver);
        self.frame = Frame::default();
        self.tree = None;
        self.fields.clear();
        self.sensors.clear();
        self.styles = wire::Styles::default();
        self.ticks = 0;
        let (width, height) = self.viewport.unwrap_or(VIEWPORT);
        self.run(vec![
            Event::Viewport { width, height },
            Event::Pointer { touch: self.touch },
            Event::Offset { minutes: 0 },
        ]);
    }
    /// How many times the open view rendered since it was opened or
    /// restored. A view renders on a tick after it called `cx.notify()`,
    /// and only then (or when the host lost its tree): read it before and
    /// after an event to pin what the event costs.
    pub fn renders(&self) -> u64 {
        self.driver.as_ref().expect("open a view first").renders()
    }
    /// How many ticks the open view ran since it was opened or restored:
    /// one per batch of host events, and again while it says it is busy.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }
    /// The ticks the last `simulate_*`, `run_until_parked` or `tick` ran,
    /// in order.
    pub fn reports(&self) -> &[TickReport] {
        &self.reports
    }
    pub(crate) fn app_mut(&mut self) -> &mut App {
        self.driver.as_mut().expect("open a view first").app_mut()
    }
    /// Runs `f` on `entity` (the open view, or a child it built) between
    /// ticks, as a listener would. It does not tick: what it changes renders
    /// on the next tick, after a `cx.notify()`.
    pub fn update<T: 'static, R>(
        &mut self,
        entity: &Entity<T>,
        f: impl FnOnce(&mut T, &mut Window, &mut Context<T>) -> R,
    ) -> R {
        let app = self.app_mut();
        let mut window = app.window();
        entity.update_in_window(app, &mut window, f)
    }
    fn fresh_app(&self) -> App {
        let mut app = App::for_driver();
        for (kind, value) in &self.globals {
            app.set_shared_global(*kind, value.clone());
        }
        app
    }
    /// Set a global before opening a view, or rerender the current view with it.
    pub fn set_global<G: gpui::Global>(&mut self, global: G) {
        let kind = std::any::TypeId::of::<G>();
        let global: std::rc::Rc<dyn std::any::Any> = std::rc::Rc::new(global);
        self.globals.insert(kind, global.clone());
        if let Some(driver) = &mut self.driver {
            let app = driver.app_mut();
            app.set_shared_global(kind, global);
            app.notify();
            self.run_until_parked();
        }
    }
    pub fn run_until_parked(&mut self) {
        self.run(Vec::new());
    }
    /// Sends `event` as the host would and ticks until the view parks: the
    /// floor every `simulate_*` stands on.
    pub fn simulate_event(&mut self, event: Event) {
        self.run(vec![event]);
    }
    /// Exactly one tick: `events`, after the answers the host owes from the
    /// last one.
    pub fn tick(&mut self, events: Vec<Event>) -> TickReport {
        let report = self.step(events);
        self.reports = vec![report];
        report
    }
    /// Ticks until the view parks: no answer owed and nothing left to do.
    fn run(&mut self, mut events: Vec<Event>) {
        self.reports.clear();
        loop {
            let report = self.step(std::mem::take(&mut events));
            self.reports.push(report);
            if !report.busy && !self.host.owes_events() {
                return;
            }
        }
    }
    /// One tick, taken in as the host takes it: the frame's requests
    /// answered, its tree kept or patched, sanitized and audited, and focus
    /// moved where the host would move it.
    fn step(&mut self, events: Vec<Event>) -> TickReport {
        let mut owed = self.host.take_events();
        owed.extend(events);
        let driver = self.driver.as_mut().expect("open a view first");
        let renders = driver.renders();
        let (frame, bytes) = driver.tick(owed);
        let rendered = driver.renders() > renders;
        self.dense = driver.app_mut().inner.dense.borrow().clone();
        self.ticks += 1;
        let host = driver.host();
        let dialogs = self.tree.as_ref().map(focus::open_dialogs);
        // the frame as the host takes it: its style entries join the table
        // (a whole tree's replace it), and every style a node names is one
        // the table holds
        let mut hosted = Frame {
            styles: frame.styles.clone(),
            root: frame.root.clone(),
            tooltip_responses: frame.tooltip_responses.clone(),
            ..Frame::default()
        };
        assert_taken_whole(wire::sanitize(&mut hosted, &mut self.styles));
        if let Some(root) = &frame.root {
            self.tree = Some(root.clone());
        } else if !frame.patches.is_empty() {
            assert_taken_whole(wire::apply(
                self.tree.as_mut().expect("patch needs previous tree"),
                frame.patches.clone(),
                &self.styles,
            ));
        }
        // its requests answered, and what the tree places kept, as the app
        // keeps its pieces
        self.host.accept(&frame, &host, self.tree.as_ref());
        assert_frame_accessible(self.tree.as_ref(), &frame.tooltip_responses);
        if let (Some((width, ladder)), Some(tree)) = (&self.fits, &self.tree) {
            super::assert_fits(tree, &self.styles, &self.dense, *width, ladder);
        }
        let report = TickReport {
            rendered,
            nodes: self.tree.as_ref().map_or(0, Node::count),
            patches: frame.patches.len(),
            bytes,
            requests: frame.requests.len(),
            busy: frame.busy,
        };
        self.move_focus(&frame, dialogs.unwrap_or_default());
        let mut told = self.adopt();
        told.extend(self.replace(&frame));
        self.forget_sensors();
        self.host.owe(told);
        self.frame = frame;
        report
    }
    /// What the host's engine does with the fields of the tree it now
    /// holds. A field new to the tree starts with the frame's text. A field
    /// whose generation moved is a new document: the engine's text becomes
    /// the frame's, whatever was typed into the old one, and the view is
    /// told when that changed it, with no edit, since it edits nothing. A
    /// field at the generation the engine holds keeps the engine's text: the
    /// frame's is the view's copy of it. One that left the tree is forgotten.
    fn adopt(&mut self) -> Vec<Event> {
        let mut shown = Vec::new();
        if let Some(root) = &self.tree {
            super::chain(root, &mut |chain| {
                if let Node::Field {
                    generation,
                    value,
                    cursor,
                    tokens,
                    on_change,
                    ..
                } = chain.last().unwrap()
                {
                    let frame = HostField {
                        generation: *generation,
                        text: value.clone(),
                        tokens: tokens.to_vec(),
                    };
                    shown.push((super::authored_path(chain), frame, *cursor, *on_change));
                }
                false
            });
        }
        let mut held = std::mem::take(&mut self.fields);
        let mut told = Vec::new();
        for (path, frame, cursor, on_change) in shown {
            let field = match held.remove(&path) {
                Some(held) if held.generation == frame.generation => held,
                Some(held) => {
                    if let Some(handler) = on_change.filter(|_| held.text != frame.text) {
                        let (text, tokens) = (frame.text.clone(), frame.tokens.clone());
                        let change = self.text_change(frame.generation, None, text, cursor, tokens);
                        told.push(Event::Text { handler, change });
                    }
                    frame
                }
                None => frame,
            };
            self.fields.insert(path, field);
        }
        told
    }
    /// A sensor that left the tree is forgotten, as the app forgets it: back
    /// in the tree, it is told its bounds again.
    fn forget_sensors(&mut self) {
        if self.sensors.is_empty() {
            return;
        }
        let mut shown = std::collections::HashSet::new();
        if let Some(root) = &self.tree {
            super::chain(root, &mut |chain| {
                if matches!(chain.last(), Some(Node::Sensor { .. })) {
                    shown.insert(super::authored_path(chain));
                }
                false
            });
        }
        self.sensors.retain(|path, _| shown.contains(path));
    }
    /// The view's own focus moves, then a dialog that opened this frame
    /// takes the keyboard; focus on a node that left the tree is gone.
    fn move_focus(&mut self, frame: &Frame, open_before: Vec<Vec<wire::ElementIdWire>>) {
        let Some(root) = self.tree.as_ref() else {
            return;
        };
        for request in &frame.requests {
            if request.kind == <crate::methods::HostWidget as crate::methods::Method>::KIND
                && let Ok(command) = wire::decode::<wire::WidgetCommand>(&request.payload)
                && let Some(focus) = Focus::moved_by(&command, root, self.focus.as_ref())
            {
                self.focus = Some(focus);
            }
        }
        for dialog in focus::open_dialogs(root) {
            if !open_before.contains(&dialog)
                && let Some(entered) = focus::entered(root, &dialog, self.focus.as_ref())
            {
                self.focus = Some(entered);
            }
        }
        if self.focus.as_ref().is_some_and(|f| f.chain(root).is_none()) {
            self.focus = None;
        }
    }

    /// The last frame, as the host received it: no tree when it sent
    /// patches or nothing changed ([`root`](Self::root) is the tree).
    pub fn last_frame(&self) -> &Frame {
        &self.frame
    }
    pub fn texts(&self) -> Vec<String> {
        texts(self.tree.as_ref())
    }
    pub fn has_text(&self, text: &str) -> bool {
        self.texts().iter().any(|shown| shown == text)
    }
    /// The style table of the tree the host holds: `cx.styles()[id]` is
    /// the style a node, or one of its conditional styles, names.
    pub fn styles(&self) -> &wire::Styles {
        &self.styles
    }
    /// The ids each press [`design::dense`](crate::design::dense) marked is
    /// filed under, in the tree the view last lowered: what
    /// [`assert_fits`](super::assert_fits) takes beside [`Self::root`] and
    /// [`Self::styles`].
    pub fn dense(&self) -> &[Vec<wire::ElementIdWire>] {
        &self.dense
    }
    /// The base style of the node under `key`, which must be in the tree
    /// and carry one.
    pub fn style(&self, key: &str) -> &gpui::StyleRefinement {
        let id = self
            .node(key)
            .style()
            .unwrap_or_else(|| panic!("{key:?} carries no style"));
        &self.styles[id]
    }
    pub fn find(&self, key: &str) -> Option<&Node> {
        chain_to(self.tree.as_ref()?, key).and_then(|chain| chain.last().copied())
    }
    /// The node under `key`, which must be in the tree; read it through
    /// its accessors (`style()`, `interactivity()`, `text()`,
    /// `children()`).
    pub fn node(&self, key: &str) -> &Node {
        self.find(key)
            .unwrap_or_else(|| panic!("no node {key:?} in {:?}", self.keys()))
    }
    /// The role, aria, focus and routes of the node `key` names, which
    /// must be one of the kinds that carry them.
    pub fn interactivity(&self, key: &str) -> &Interactivity {
        match self.node(key) {
            node @ (Node::Container(_)
            | Node::UniformList { .. }
            | Node::List { .. }
            | Node::ResizeHandle { .. }
            | Node::Image { .. }
            | Node::Svg { .. }) => node.interactivity().unwrap_or(Interactivity::none()),
            _ => panic!("{key:?} carries no interactivity"),
        }
    }
    /// The node that holds the keyboard, if any does.
    pub fn focused(&self) -> Option<&Node> {
        let chain = self.focus.as_ref()?.chain(self.tree.as_ref()?)?;
        chain.last().copied()
    }
    /// The whole tree the host shows.
    pub fn root(&self) -> &Node {
        self.tree.as_ref().expect("view has a tree")
    }
    fn keys(&self) -> Vec<String> {
        keys(self.tree.as_ref())
    }
    /// The chain from the root down to the node under `key`.
    fn chain(&self, key: &str) -> Vec<&Node> {
        chain_to(self.root(), key).unwrap_or_else(|| panic!("no node {key:?} in {:?}", self.keys()))
    }
    /// The tree the host shows and the styles it names, as the one frame a
    /// host takes on a fresh mount. A screen export writes it as JSON,
    /// which `ducktape-app --render-tree` draws.
    pub fn whole_frame(&self) -> Frame {
        let mut root = self.root().clone();
        // the styles the tree names, numbered from the first it names
        let mut styles = wire::Interner::default();
        root.for_each_mut(&mut |node| {
            node.styles_mut(&mut |id| *id = styles.intern(&self.styles[*id]))
        });
        Frame {
            styles: styles.unsent(),
            root: Some(root),
            ..Frame::default()
        }
    }
    /// [`whole_frame`](Self::whole_frame) in encoded bytes: the proxy a
    /// native test has for the fuel a render spends. (Every tick already
    /// holds the tree to the host's budgets.)
    pub fn frame_bytes(&self) -> usize {
        crate::wire::encode(&self.whole_frame()).len()
    }

    // Focus and keys.

    /// Puts the keyboard on `key`, as a person does by Tab or click: the
    /// node must be able to hold it (`focusable`, a tracked focus handle, a
    /// text field).
    pub fn simulate_focus(&mut self, key: &str) {
        let chain = self.chain(key);
        assert!(
            focus::holds_focus(chain.last().unwrap()),
            "{key:?} cannot hold focus: it is not focusable, tracks no focus handle, and is \
             no text field"
        );
        self.focus = Some(Focus::at(&chain));
    }
    /// Tab (`forward`) or Shift-Tab, as the host moves the keyboard for
    /// it: to the next Tab stop in document order, kept inside an open
    /// dialog.
    pub fn simulate_tab(&mut self, forward: bool) {
        self.focus = focus::tab(self.root(), self.focus.as_ref(), forward);
    }
    /// `keystroke` (gpui's words: `"shift-left"`) goes down while `key`
    /// holds the keyboard or contains the node that does. The host sends
    /// it down the focus path to every capture listener, root first, then
    /// up it to every key listener, focused node first, until a node
    /// consumes it ([`consumed_at`]): an open overlay's layer turns Escape
    /// into its dismiss there. A node off the focus path never hears it,
    /// and the test fails instead.
    pub fn simulate_key_down(&mut self, key: &str, keystroke: &str) {
        let event = gpui::KeyDownEvent {
            keystroke: keystroke_of(keystroke),
            is_held: false,
            prefer_character_input: false,
        };
        let events = self.along_focus(
            key,
            |i| i.capture_key_down,
            |i| i.on_key_down,
            |handler, phase| Event::KeyDown {
                handler,
                phase,
                event: (&event).into(),
            },
            |chain| consumed_at(chain, &event.keystroke),
        );
        self.run(events);
    }
    /// `keystroke` comes up, down the same path as
    /// [`simulate_key_down`](Self::simulate_key_down).
    pub fn simulate_key_up(&mut self, key: &str, keystroke: &str) {
        let event = gpui::KeyUpEvent {
            keystroke: keystroke_of(keystroke),
        };
        let events = self.along_focus(
            key,
            |i| i.capture_key_up,
            |i| i.on_key_up,
            |handler, phase| Event::KeyUp {
                handler,
                phase,
                event: (&event).into(),
            },
            |_| None,
        );
        self.run(events);
    }
    /// The modifier keys change while `key` holds the keyboard or contains
    /// the node that does: every listener up the focus path hears it.
    pub fn simulate_modifiers_changed(&mut self, key: &str, event: gpui::ModifiersChangedEvent) {
        let events = self.along_focus(
            key,
            |_| None,
            |i| i.on_modifiers_changed,
            |handler, _| Event::ModifiersChanged {
                handler,
                event: (&event).into(),
            },
            |_| None,
        );
        self.run(events);
    }
    /// The events a key-like input on the focus path through `key` makes:
    /// `reach` says from which node of the chain up it still bubbles, and
    /// what the node that stopped it makes of it.
    fn along_focus(
        &self,
        key: &str,
        capture: fn(&Interactivity) -> Option<u32>,
        bubble: fn(&Interactivity) -> Option<u32>,
        event: impl Fn(u32, DispatchPhase) -> Event,
        reach: impl Fn(&[&Node]) -> Option<(usize, Option<Event>)>,
    ) -> Vec<Event> {
        let root = self.root();
        let focused = self.focus.as_ref().and_then(|focus| focus.chain(root));
        let Some(chain) = focused
            .as_ref()
            .filter(|chain| chain.iter().any(|node| node.key() == Some(key)))
        else {
            panic!(
                "the host refuses this key: {key:?} is not on the focus path ({}); keys reach \
                 only the focused node and its ancestors: `simulate_focus` it or click it first",
                match focused.as_ref().map(|chain| chain.last().unwrap()) {
                    Some(node) =>
                        format!("focus is at {:?}", node.key().unwrap_or("a nameless node")),
                    None => "nothing has focus".into(),
                }
            )
        };
        let stopped = reach(chain);
        let (from, made) = stopped.clone().unwrap_or((0, None));
        let mut events = along(chain, from, capture, bubble, event);
        events.extend(made);
        assert!(
            !events.is_empty() || stopped.is_some(),
            "nothing on the focus path through {key:?} listens for it"
        );
        events
    }

    // Pointer.

    /// A press of the button whose key, label or accessible name is `name`:
    /// its click and, as gpui passes a click out, the click of every node
    /// around it up to where the host stops it ([`click_stops_at`]), after
    /// the press put the keyboard on it or on the innermost node around it
    /// that can hold it.
    pub fn simulate_click(&mut self, name: &str) {
        let Some(chain) = button(self.root(), name) else {
            panic!("no button {name:?} in {:?}", self.texts());
        };
        let events = along(
            &chain,
            click_stops_at(&chain),
            |_| None,
            |i| i.on_click,
            |handler, _| Event::Click {
                handler,
                event: (&gpui::ClickEvent::default()).into(),
            },
        );
        let pressed = focus::pressed(&chain);
        self.press(pressed);
        self.run(events);
    }
    /// A press of a mouse button other than the primary on `key`.
    pub fn simulate_aux_click(&mut self, key: &str) {
        let chain = self.chain(key);
        let handler = route(&chain, key, "aux click", |i| i.on_aux_click);
        let event = Event::AuxClick {
            handler,
            event: (&gpui::ClickEvent::default()).into(),
        };
        let pressed = focus::pressed(&chain);
        self.press(pressed);
        self.run(vec![event]);
    }
    /// A mouse button goes down on `key`: down the chain to it and back up,
    /// from the deepest node that occludes what is behind it (gpui's hit
    /// test ends there, so the nodes around it are not under the pointer),
    /// and to every node outside it that listens for a press outside. The
    /// press puts the keyboard where [`simulate_click`](Self::simulate_click)
    /// does.
    pub fn simulate_mouse_down(&mut self, key: &str, event: gpui::MouseDownEvent) {
        let chain = self.chain(key);
        let wire_event = wire::interactivity::MouseDown::from(&event);
        let mut events = along(
            &chain[press_stops_at(&chain)..],
            0,
            |i| i.capture_mouse_down,
            |i| i.on_mouse_down,
            |handler, phase| Event::MouseDown {
                handler,
                phase,
                event: wire_event,
            },
        );
        events.extend(
            outside(self.root(), &chain, |i| i.on_mouse_down_out).map(|handler| {
                Event::MouseDownOut {
                    handler,
                    event: wire_event,
                }
            }),
        );
        let pressed = focus::pressed(&chain);
        self.press(pressed);
        self.run(events);
    }
    /// A mouse button comes up on `key`, as
    /// [`simulate_mouse_down`](Self::simulate_mouse_down) went down.
    pub fn simulate_mouse_up(&mut self, key: &str, event: gpui::MouseUpEvent) {
        let chain = self.chain(key);
        let wire_event = wire::interactivity::MouseUp::from(&event);
        let mut events = along(
            &chain,
            0,
            |i| i.capture_mouse_up,
            |i| i.on_mouse_up,
            |handler, phase| Event::MouseUp {
                handler,
                phase,
                event: wire_event,
            },
        );
        events.extend(
            outside(self.root(), &chain, |i| i.on_mouse_up_out).map(|handler| Event::MouseUpOut {
                handler,
                event: wire_event,
            }),
        );
        self.run(events);
    }
    /// The pointer moves over `key`: every listener up the chain hears it.
    pub fn simulate_mouse_move(&mut self, key: &str, event: gpui::MouseMoveEvent) {
        let event = wire::interactivity::MouseMove::from(&event);
        let events = self.along_chain(
            key,
            |_| None,
            |i| i.on_mouse_move,
            |handler, phase| Event::MouseMove {
                handler,
                phase,
                event,
            },
        );
        self.run(events);
    }
    /// The pointer leaves `key`.
    pub fn simulate_mouse_exit(&mut self, key: &str, event: gpui::MouseExitEvent) {
        let chain = self.chain(key);
        let event = Event::MouseExit {
            handler: route(&chain, key, "mouse exit", |i| i.on_mouse_exit),
            phase: DispatchPhase::Bubble,
            event: (&event).into(),
        };
        self.run(vec![event]);
    }
    /// A force press on `key`, down the chain and back up.
    pub fn simulate_mouse_pressure(&mut self, key: &str, event: gpui::MousePressureEvent) {
        let event = wire::interactivity::MousePressure::from(&event);
        let events = self.along_chain(
            key,
            |i| i.capture_mouse_pressure,
            |i| i.on_mouse_pressure,
            |handler, phase| Event::MousePressure {
                handler,
                phase,
                event,
            },
        );
        self.run(events);
    }
    /// A wheel or trackpad scroll over `key`: every listener up the chain.
    pub fn simulate_scroll_wheel(&mut self, key: &str, event: gpui::ScrollWheelEvent) {
        let event = wire::interactivity::ScrollWheel::from(&event);
        let events = self.along_chain(
            key,
            |_| None,
            |i| i.on_scroll_wheel,
            |handler, phase| Event::ScrollWheel {
                handler,
                phase,
                event,
            },
        );
        self.run(events);
    }
    /// A pinch over `key`, down the chain and back up.
    pub fn simulate_pinch(&mut self, key: &str, event: gpui::PinchEvent) {
        let event = wire::interactivity::Pinch::from(&event);
        let events = self.along_chain(
            key,
            |i| i.capture_pinch,
            |i| i.on_pinch,
            |handler, phase| Event::Pinch {
                handler,
                phase,
                event,
            },
        );
        self.run(events);
    }
    /// The pointer enters (`true`) or leaves `key`: its hover listener, and
    /// on entering, its tooltip's request.
    pub fn simulate_hover(&mut self, key: &str, hovered: bool) {
        let interactivity = self.interactivity(key);
        let mut events: Vec<Event> = interactivity
            .on_hover
            .map(|handler| Event::Hover { handler, hovered })
            .into_iter()
            .collect();
        if hovered && let Some(tooltip) = &interactivity.tooltip {
            events.push(Event::TooltipRequest {
                request: tooltip.request,
                character_index: None,
            });
        }
        assert!(
            !events.is_empty(),
            "{key:?} has no hover route and no tooltip"
        );
        self.run(events);
    }
    /// A dragged file leaves `key`.
    pub fn simulate_file_drop_exit(&mut self, key: &str) {
        let handler = route(&self.chain(key), key, "file drop exit", |i| {
            i.on_file_drop_exit
        });
        self.run(vec![Event::FileDropExit { handler }]);
    }
    /// The events down the chain to `key` and back up it.
    fn along_chain(
        &self,
        key: &str,
        capture: fn(&Interactivity) -> Option<u32>,
        bubble: fn(&Interactivity) -> Option<u32>,
        event: impl Fn(u32, DispatchPhase) -> Event,
    ) -> Vec<Event> {
        let events = along(&self.chain(key), 0, capture, bubble, event);
        assert!(
            !events.is_empty(),
            "nothing on the chain to {key:?} listens for it"
        );
        events
    }
    /// A press moves the keyboard where it lands, if anything there can
    /// hold it.
    fn press(&mut self, pressed: Option<Focus>) {
        if pressed.is_some() {
            self.focus = pressed;
        }
    }

    // Text, pictures and other widgets.

    /// The field with key or placeholder `name` now reads `text`: the
    /// host's engine took the typing into the text it holds and says so,
    /// caret at the end.
    pub fn simulate_input(&mut self, name: &str, text: &str) {
        let Some(chain) = input(self.root(), name) else {
            panic!("no input {name:?} in {:?}", self.texts());
        };
        let Some(Node::Field { on_change, .. }) = chain.last() else {
            unreachable!()
        };
        let Some(handler) = *on_change else {
            panic!("field {name:?} hears no change");
        };
        let held = self
            .fields
            .get_mut(&super::authored_path(&chain))
            .expect("the host holds every field of its tree");
        let edit = wire::changed_span(&held.text, text).map(|(range, text)| wire::Edit {
            range,
            len: text.len() as u32,
        });
        held.text = text.to_owned();
        held.tokens.clear();
        let (generation, caret) = (held.generation, wire::TextRange::caret(text.len()));
        let event = Event::Text {
            handler,
            change: self.text_change(generation, edit, text.to_owned(), caret, Vec::new()),
        };
        self.run(vec![event]);
    }
    /// The host's word on a field's text at its next revision, `edit` being
    /// what changed it, logged for the asks that read an older text.
    fn text_change(
        &mut self,
        generation: u64,
        edit: Option<wire::Edit>,
        text: String,
        cursor: wire::TextRange,
        tokens: Vec<wire::TextToken>,
    ) -> wire::TextChange {
        self.text_revision += 1;
        if let Some(edit) = edit {
            self.edits.push((self.text_revision, edit));
        }
        wire::TextChange {
            generation,
            revision: self.text_revision,
            edit,
            text,
            cursor,
            preedit: None,
            tokens,
        }
    }
    /// What the host's engine does with the `Replace`s a frame asks for, in
    /// order: each is carried over the edits made since the revision it
    /// read (`wire::rebase`, the host's own rule; an earlier ask in the same
    /// frame is one), applied to the text the engine holds, and comes back
    /// as the next change. An ask on a document the field has since left
    /// (its generation moved) edits nothing: the frame's field is the
    /// document.
    fn replace(&mut self, frame: &Frame) -> Vec<Event> {
        let Some(root) = self.tree.clone() else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for request in &frame.requests {
            if request.kind != <crate::methods::HostWidget as crate::methods::Method>::KIND {
                continue;
            }
            let Ok(wire::WidgetCommand::Replace {
                target,
                generation,
                revision,
                range,
                text,
                token,
                cursor,
            }) = wire::decode::<wire::WidgetCommand>(&request.payload)
            else {
                continue;
            };
            let Some(chain) = Focus::Path(target.clone()).chain(&root) else {
                panic!("a Replace on a field that is not in the tree: {target:?}");
            };
            let Some(Node::Field {
                on_change: Some(handler),
                ..
            }) = chain.last()
            else {
                panic!("a Replace on no field that hears changes: {target:?}");
            };
            let held = self
                .fields
                .get_mut(&target)
                .expect("the host holds every field of its tree");
            if generation != held.generation {
                continue;
            }
            let (value, tokens) = (&mut held.text, &mut held.tokens);
            let since: Vec<wire::Edit> = self
                .edits
                .iter()
                .filter(|(at, _)| *at > revision)
                .map(|(_, edit)| *edit)
                .collect();
            let rebased = wire::rebase(range, text.len(), cursor, since);
            let range = rebased.range.range();
            let replacement = rebased.replacement(value, &text);
            let delta = replacement.len() as i64 - range.len() as i64;
            // the engine's spans: one an edit touches goes, one after it moves
            tokens.retain(|token| {
                token.range.end as usize <= range.start || token.range.start as usize >= range.end
            });
            for token in tokens.iter_mut() {
                if token.range.start as usize >= range.end {
                    token.range.start = (token.range.start as i64 + delta) as u32;
                    token.range.end = (token.range.end as i64 + delta) as u32;
                }
            }
            // one span only when the replacement is the view's text alone
            if let Some(id) = token.filter(|_| replacement == text) {
                tokens.push(wire::TextToken {
                    range: wire::TextRange::from(range.start..range.start + text.len()),
                    id,
                });
                tokens.sort_by_key(|token| token.range.start);
            }
            value.replace_range(range.clone(), &replacement);
            let caret = wire::TextRange::caret((rebased.cursor.start as usize).min(value.len()));
            let edit = wire::Edit {
                range: range.into(),
                len: replacement.len() as u32,
            };
            let (value, tokens) = (value.clone(), tokens.clone());
            let change = self.text_change(generation, Some(edit), value, caret, tokens);
            events.push(Event::Text {
                handler: *handler,
                change,
            });
        }
        events
    }
    /// The field with key or placeholder `name` is submitted (Enter).
    pub fn simulate_submit(&mut self, name: &str) {
        let Some(Node::Field { on_submit, .. }) = self.input(name) else {
            unreachable!()
        };
        let Some(message) = on_submit else {
            panic!("input {name:?} has no submit route");
        };
        self.run(vec![Event::Message(*message)]);
    }
    fn input(&self, name: &str) -> Option<&Node> {
        match input(self.root(), name) {
            Some(chain) => chain.last().copied(),
            None => panic!("no input {name:?} in {:?}", self.texts()),
        }
    }
    /// The clickable range at `index` of the rich text `key` is pressed.
    pub fn simulate_rich_click(&mut self, key: &str, index: usize) {
        let Node::RichText {
            on_click: Some(handler),
            clickable_ranges,
            ..
        } = self.node(key)
        else {
            panic!("{key} is not interactive rich text");
        };
        assert!(
            index < clickable_ranges.len(),
            "rich text click index out of bounds"
        );
        let event = Event::Select {
            handler: *handler,
            index: u32::try_from(index).expect("rich text click index fits the wire"),
        };
        self.run(vec![event]);
    }
    /// The pointer moves to character `index` of the rich text `key`
    /// (`None`: off its text): its hover route, and over a character, its
    /// tooltip's request for that character.
    pub fn simulate_rich_hover(&mut self, key: &str, index: Option<u32>) {
        let Node::RichText {
            on_hover, tooltip, ..
        } = self.node(key)
        else {
            panic!("{key:?} is no rich text");
        };
        let mut events: Vec<Event> = on_hover
            .map(|handler| Event::RichTextHover {
                handler,
                event: wire::RichTextHover {
                    index,
                    position: gpui::Point::default(),
                    pressed_button: None,
                    modifiers: gpui::Modifiers::default(),
                },
            })
            .into_iter()
            .collect();
        if let (Some(request), Some(index)) = (tooltip, index) {
            events.push(Event::TooltipRequest {
                request: *request,
                character_index: Some(index),
            });
        }
        assert!(
            !events.is_empty(),
            "{key:?} has no hover route and no tooltip"
        );
        self.run(events);
    }
    /// The host lays the child of the sensor `name` out at `origin`, in
    /// the window's pixels, `size` wide and tall, in view. The sensor hears
    /// it as the app tells it: the first time, and when the origin or the
    /// size differs from the last it was told; the same bounds again are no
    /// event and no tick. A sensor that left the tree is forgotten, so it
    /// hears its bounds again when it is back. A test that never gives a
    /// sensor its bounds reads the frame the app draws before layout.
    pub fn simulate_bounds(&mut self, name: &str, origin: (f32, f32), size: (f32, f32)) {
        let chain = self.chain(name);
        let Some(Node::Sensor { on_bounds, .. }) = chain.last() else {
            panic!("{name:?} is no sensor");
        };
        let Some(handler) = *on_bounds else {
            panic!("sensor {name:?} has no bounds route");
        };
        let bounds = [origin.0, origin.1, size.0, size.1];
        if self.sensors.insert(super::authored_path(&chain), bounds) == Some(bounds) {
            return;
        }
        let [x, y, width, height] = bounds;
        self.run(vec![Event::Bounds {
            handler,
            x,
            y,
            width,
            height,
        }]);
    }
    /// The resize handle `name` is dragged by `dx`, `dy`.
    pub fn simulate_drag(&mut self, name: &str, dx: f64, dy: f64) {
        let Node::ResizeHandle {
            on_drag: Some(handler),
            ..
        } = self.node(name)
        else {
            panic!("{name:?} is no resize handle with a drag route");
        };
        let handler = *handler;
        self.run(vec![Event::Drag { handler, dx, dy }]);
    }
    /// The modal backdrop of the overlay `name` dismisses it; for a sheet,
    /// its scrim pressed or the panel dragged down.
    pub fn simulate_dismiss(&mut self, name: &str) {
        let (Node::Overlay {
            on_dismiss: Some(message),
            ..
        }
        | Node::Sheet {
            on_dismiss: Some(message),
            ..
        }) = self.node(name)
        else {
            panic!("{name:?} is no overlay or sheet with a dismiss route");
        };
        let message = *message;
        self.run(vec![Event::Message(message)]);
    }
    /// The phone's Back (its gesture or its bar), to the root's `on_back`
    /// in the tree the host holds: the view must have somewhere to go
    /// back to, else the host would have left the program and this panics.
    pub fn simulate_back(&mut self) {
        let Some(handler) = self.back_handler() else {
            panic!("the root has no on_back: the host leaves the program on Back");
        };
        self.run(vec![Event::Back { handler }]);
    }
    /// The root's `on_back` route, when the view has somewhere to go back to.
    pub fn back_handler(&self) -> Option<u32> {
        self.root().interactivity()?.on_back
    }
    /// The device's primary pointer is a finger, or not
    /// (`Event::Pointer`): the view reads it as `window.touch()` from the
    /// frame this draws, and a replacement hears it before its first tree.
    pub fn simulate_touch(&mut self, touch: bool) {
        self.touch = touch;
        self.run(vec![Event::Pointer { touch }]);
    }
    /// Assistive technology asks the node `key` for `action`, which it
    /// advertises.
    pub fn simulate_a11y_action(
        &mut self,
        key: &str,
        action: wire::Action,
        data: Option<wire::ActionData>,
    ) {
        let Some(&(_, handler)) = self
            .interactivity(key)
            .aria
            .actions
            .iter()
            .find(|(advertised, _)| *advertised == action)
        else {
            panic!("{key:?} does not advertise {action:?}");
        };
        self.run(vec![Event::A11yAction { handler, data }]);
    }
    /// A key the field `key` claimed goes down in it: the host hands it to
    /// the view instead of editing.
    pub fn simulate_field_key(&mut self, key: &str, keystroke: &str) {
        let Node::Field {
            on_key: Some(handler),
            ..
        } = self.node(key)
        else {
            panic!("{key:?} is no field that hears keys");
        };
        let event = Event::KeyDown {
            handler: *handler,
            phase: wire::DispatchPhase::Bubble,
            event: wire::interactivity::KeyDown {
                state: (&keystroke_of(keystroke)).into(),
                repeat: false,
                prefer_character_input: false,
            },
        };
        self.run(vec![event]);
    }
    /// The host switches the theme.
    pub fn simulate_theme(&mut self, dark: bool) {
        self.run(vec![Event::Theme { dark }]);
    }
    /// The pane is `width` by `height`: what `Window::viewport_size` reads
    /// from the next frame on, sent ahead of the draw as the host sends it
    /// (a resize, a restore into another window). Before a view is opened,
    /// the pane it opens in ([`VIEWPORT`] otherwise).
    pub fn simulate_resize(&mut self, width: f32, height: f32) {
        self.viewport = Some((width, height));
        if self.driver.is_some() {
            self.run(vec![Event::Viewport { width, height }]);
        }
    }
    /// The reader's UTC offset is `minutes` now: the dates the view writes
    /// read in that zone from the next frame on.
    pub fn simulate_offset(&mut self, minutes: i32) {
        self.run(vec![Event::Offset { minutes }]);
    }
    /// The host lost the tree and asks for it whole.
    pub fn simulate_resync(&mut self) {
        self.run(vec![Event::Resync]);
    }

    // Lists.

    /// A pane `rows` rows tall: every list in the tree is asked for the
    /// rows it would show, as the host asks after it lays the frame out.
    /// A list anchored at its end shows its last rows, a uniform list
    /// scrolled to a row shows that row; every other list its first rows.
    /// Before this (or [`simulate_range`](Self::simulate_range)) a list
    /// holds the window it sized from its viewport, as the first frame on
    /// the host does; rows the window holds change nothing.
    pub fn simulate_viewport(&mut self, rows: usize) {
        let mut events = Vec::new();
        super::chain(self.root(), &mut |chain| {
            let node = chain.last().unwrap();
            if let Some(range) = shown(node, rows) {
                events.extend(range_events(node, range));
            }
            false
        });
        assert!(!events.is_empty(), "no list in {:?}", self.keys());
        self.run(events);
    }
    /// The list at or inside `key` scrolled to show `range`.
    pub fn simulate_range(&mut self, key: &str, range: std::ops::Range<usize>) {
        let mut events = None;
        super::chain(self.node(key), &mut |chain| {
            let node = chain.last().unwrap();
            if matches!(node, Node::UniformList { .. } | Node::List { .. }) {
                events = Some(range_events(node, range.clone()));
            }
            events.is_some()
        });
        let Some(events) = events else {
            panic!("no list at or inside {key:?}");
        };
        self.run(events);
    }
}

fn keystroke_of(keystroke: &str) -> gpui::Keystroke {
    gpui::Keystroke::parse(keystroke).expect("a keystroke gpui reads")
}

/// The one route the last node of `chain` has for `what`.
fn route(chain: &[&Node], key: &str, what: &str, pick: fn(&Interactivity) -> Option<u32>) -> u32 {
    chain
        .last()
        .and_then(|node| node.interactivity())
        .and_then(pick)
        .unwrap_or_else(|| panic!("{key:?} has no {what} route"))
}

/// An input dispatched along `chain` as gpui dispatches it: every capture
/// route from the root down, then every bubble route from the last node up.
fn along(
    chain: &[&Node],
    from: usize,
    capture: fn(&Interactivity) -> Option<u32>,
    bubble: fn(&Interactivity) -> Option<u32>,
    event: impl Fn(u32, DispatchPhase) -> Event,
) -> Vec<Event> {
    let routes = |nodes: &[&Node], pick: fn(&Interactivity) -> Option<u32>| {
        nodes
            .iter()
            .filter_map(move |node| node.interactivity().and_then(pick))
            .collect::<Vec<_>>()
    };
    let captured = routes(chain, capture)
        .into_iter()
        .map(|handler| event(handler, DispatchPhase::Capture));
    let bubbled = routes(&chain[from..], bubble)
        .into_iter()
        .rev()
        .map(|handler| event(handler, DispatchPhase::Bubble));
    captured.chain(bubbled).collect()
}

/// Where a key going down the focus `chain` stops, as the host stops it:
/// the deepest node that consumes it (its `consumes_keys`) hears it last,
/// and an open overlay's layer turns Escape into the overlay's dismiss and
/// stops it.
/// Answers, for a key that stops, the first node of the chain that still
/// hears it and the dismiss; `None` for a key that bubbles to the root.
fn consumed_at(chain: &[&Node], keystroke: &gpui::Keystroke) -> Option<(usize, Option<Event>)> {
    let plain = !keystroke.modifiers.modified();
    for (at, node) in chain.iter().enumerate().rev() {
        if let Node::Overlay {
            on_dismiss: Some(message),
            children,
            ..
        } = node
            && plain
            && keystroke.key == "escape"
            && chain.get(at + 1).is_some_and(|inside| {
                children
                    .get(1)
                    .is_some_and(|layer| std::ptr::eq(layer, *inside))
            })
        {
            return Some((at + 1, Some(Event::Message(*message))));
        }
        if let Node::Sheet {
            on_dismiss: Some(message),
            child,
            ..
        } = node
            && plain
            && keystroke.key == "escape"
            && chain
                .get(at + 1)
                .is_some_and(|inside| std::ptr::eq(&**child, *inside))
        {
            return Some((at + 1, Some(Event::Message(*message))));
        }
        let Some(interactivity) = node.interactivity() else {
            continue;
        };
        let consumes = interactivity.consumes_keys.iter().any(|consumed| {
            gpui::Keystroke::parse(consumed).is_ok_and(|consumed| {
                consumed.key == keystroke.key && consumed.modifiers == keystroke.modifiers
            })
        });
        if consumes {
            return Some((at, None));
        }
    }
    None
}

/// Where a pointer's click on the last node of `chain` stops going out, as
/// the host stops it: at the deepest node that consumes it
/// (`consumes_click`) or blocks the pointer from what is behind it, or at
/// the top of an open overlay's layer, whose content keeps the press from
/// what is under the overlay. Answers the first node of the chain that
/// still hears it.
fn click_stops_at(chain: &[&Node]) -> usize {
    stops_at(chain, |i| i.consumes_click || blocks(i))
}

/// Where a press on the last node of `chain` stops going out: at the
/// deepest node that blocks the pointer from what is behind it, or at the
/// top of an open overlay's layer. A click's `consumes_click` stops the
/// click, not the press under it.
fn press_stops_at(chain: &[&Node]) -> usize {
    stops_at(chain, blocks)
}

/// Whether a node keeps the pointer from what is behind it, the nodes
/// around it included: it occludes them, or blocks all but their scroll.
/// gpui's hit test leaves those unhovered, so none of them hears a press.
fn blocks(i: &Interactivity) -> bool {
    i.occlude || i.block_mouse_except_scroll
}

fn stops_at(chain: &[&Node], stops: fn(&Interactivity) -> bool) -> usize {
    for (at, node) in chain.iter().enumerate().rev() {
        if let Node::Overlay { children, .. } = node
            && chain.get(at + 1).is_some_and(|inside| {
                children
                    .get(1)
                    .is_some_and(|layer| std::ptr::eq(layer, *inside))
            })
        {
            return at + 1;
        }
        // a sheet's panel keeps the press from what is under its scrim
        if let Node::Sheet { child, .. } = node
            && chain
                .get(at + 1)
                .is_some_and(|inside| std::ptr::eq(&**child, *inside))
        {
            return at + 1;
        }
        if node.interactivity().is_some_and(stops) {
            return at;
        }
    }
    0
}

/// The `pick` route of every node in `root` off `chain`: what hears a press
/// outside it.
fn outside<'a>(
    root: &'a Node,
    chain: &[&Node],
    pick: fn(&Interactivity) -> Option<u32>,
) -> impl Iterator<Item = u32> + 'a {
    let on: Vec<*const Node> = chain.iter().map(|node| *node as *const Node).collect();
    let mut out = Vec::new();
    super::chain(root, &mut |nodes| {
        let node = *nodes.last().unwrap();
        if !on.contains(&(node as *const Node))
            && let Some(handler) = node.interactivity().and_then(pick)
        {
            out.push(handler);
        }
        false
    });
    out.into_iter()
}

/// The rows a list shows in a pane `rows` rows tall, when `node` is one.
fn shown(node: &Node, rows: usize) -> Option<std::ops::Range<usize>> {
    use wire::list::{ListAlignment, UniformListScrollStrategy as Strategy};
    let (count, rows, start) = match node {
        Node::UniformList {
            count,
            scroll_request,
            ..
        } => {
            let start = scroll_request.map_or(0, |request| match request.strategy {
                Strategy::Top => request.index,
                Strategy::Center => request.index.saturating_sub(rows / 2),
                Strategy::Bottom => (request.index + 1).saturating_sub(rows),
                Strategy::Nearest if request.index < rows => 0,
                Strategy::Nearest => request.index + 1 - rows,
            });
            (*count, rows, start)
        }
        Node::List {
            item_count,
            alignment,
            following_tail,
            ..
        } => {
            let tail = *alignment == ListAlignment::Bottom || *following_tail;
            (*item_count, rows, if tail { usize::MAX } else { 0 })
        }
        _ => return None,
    };
    let start = start.min(count.saturating_sub(rows));
    Some(start..(start + rows).min(count))
}

/// What the host sends a list it shows `range` of.
fn range_events(node: &Node, range: std::ops::Range<usize>) -> Vec<Event> {
    match node {
        Node::UniformList {
            path, route, count, ..
        } => vec![
            Event::UniformListRange {
                path: path.clone(),
                route: *route,
                start: range.start as u32,
                end: range.end as u32,
                item_height: f32::from(crate::design::size::ROW),
            },
            Event::UniformListState {
                path: path.clone(),
                route: *route,
                top_index: range.start as u32,
                scrollable: range.len() < *count,
                scrolled_to_end: Some(range.end >= *count),
            },
        ],
        Node::List {
            item_count,
            following_tail,
            request_handler,
            scroll_handler,
            ..
        } => {
            let mut events = vec![Event::ListRequest {
                handler: *request_handler,
                request: wire::ListRequest {
                    start: range.start,
                    end: range.end,
                },
            }];
            events.extend(scroll_handler.map(|handler| Event::ListScroll {
                handler,
                event: wire::ListScroll {
                    visible_start: range.start,
                    visible_end: range.end,
                    count: *item_count,
                    is_scrolled: false,
                    is_following_tail: *following_tail,
                    offset: wire::ListOffset {
                        item_ix: range.start,
                        offset_in_item: 0.,
                    },
                },
            }));
            events
        }
        _ => unreachable!("a list"),
    }
}

#[cfg(test)]
mod tests;
