use crate::context::Callback;
use crate::{
    App, Context, Entity, Host, IntoElement, Lowering, Theme, View, executor, px, slots, wire,
};

/// One view's run loop: the wasm exports drive it on the host, and
/// [`TestAppContext`](crate::testing::TestAppContext) drives it in a test.
pub(crate) struct Driver<V: View> {
    pub(crate) app: App,
    pub(crate) entity: Entity<V>,
    pub(crate) last_root: Option<wire::Node>,
    pub(crate) busy: bool,
    /// How many times the root rendered: what `TestAppContext::renders`
    /// reads.
    pub(crate) renders: u64,
    pub(crate) spent: Spent,
}

// What the ticks so far spent in the view, summed: the time it took to
// render its root into elements, to lower them (a child entity and a list
// row render as they are lowered) and to diff the tree against the last,
// on a native clock, and the nodes it lowered. A test host reads them per
// tick.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Spent {
    pub render: std::time::Duration,
    pub lower: std::time::Duration,
    pub diff: std::time::Duration,
    pub lowered: usize,
}

// A wasm view has no clock to read (`Instant::now` panics there), so it
// times nothing.
#[cfg(not(target_arch = "wasm32"))]
type Clock = std::time::Instant;
#[cfg(target_arch = "wasm32")]
struct Clock;
#[cfg(target_arch = "wasm32")]
impl Clock {
    fn now() -> Self {
        Clock
    }
    fn elapsed(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
}
impl<V: View> Drop for Driver<V> {
    fn drop(&mut self) {
        self.app.inner.alive.set(false);
        self.app.inner.tasks.borrow_mut().clear();
    }
}
impl<V: View> Default for Driver<V> {
    fn default() -> Self {
        Self::new()
    }
}
impl<V: View> Driver<V> {
    pub fn new() -> Self {
        Self::initialize_in(App::for_driver(), None)
    }
    pub(crate) fn initialize_in(mut app: App, restored: Option<V>) -> Self {
        let entity = Entity::reserve(&app);
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        app.inner.root.set(Some((entity.id, encode_root::<V>)));
        app.update(|app| {
            let mut window = app.window();
            let mut cx = Context {
                app,
                entity: entity.clone(),
            };
            let mut value = match restored {
                Some(value) => value,
                None => V::new(&mut window, &mut cx),
            };
            value.attach(&mut window, &mut cx);
            *entity.value.borrow_mut() = Some(value);
        });
        Self {
            app,
            entity,
            last_root: None,
            busy: false,
            renders: 0,
            spent: Spent::default(),
        }
    }
    pub fn entity(&self) -> Entity<V> {
        self.entity.clone()
    }
    pub fn host(&self) -> Host {
        self.app.host()
    }
    pub(crate) fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }
    /// Runs one tick and hands its frame to `send` — the host's encoder —
    /// before putting back into the kept tree what the frame carried out of
    /// it. The tree a frame sends whole, and the subtrees its patches
    /// carry, are moved there and back rather than copied: the frame is
    /// gone once it is encoded, and the kept tree is the next frame's base.
    pub(crate) fn tick_with<R>(
        &mut self,
        events: Vec<wire::Event>,
        send: impl FnOnce(&wire::Frame) -> R,
    ) -> R {
        let frame = self.tick_wire(events);
        let sent = send(&frame);
        self.put_back(frame);
        sent
    }

    fn put_back(&mut self, frame: wire::Frame) {
        let kept = match frame.root {
            Some(root) => self.last_root.insert(root),
            None if frame.patches.is_empty() => return,
            None => {
                let kept = self
                    .last_root
                    .as_mut()
                    .expect("a patch frame has a tree to patch");
                put_back(kept, frame.patches);
                kept
            }
        };
        // Remembered without the picture bytes this frame carried: the
        // next view names those pictures by hash alone, and that is
        // the same tree — and the tree the host keeps, which drops the
        // bytes the same way once it has the pictures.
        crate::memo::without_pictures(kept);
    }

    fn tick_wire(&mut self, events: Vec<wire::Event>) -> wire::Frame {
        self.busy = false;
        executor::begin_tick(&mut self.app.inner.tasks.borrow_mut());
        self.settle();
        for event in events {
            if let Some(callback) = self.dispatch(event) {
                let mut window = self.app.window();
                self.entity
                    .update_in_window(&mut self.app, &mut window, |v, w, cx| {
                        callback(v, w, cx);
                    });
                self.settle();
            }
        }
        self.settle();
        self.frame()
    }

    /// Runs one event: its route or its handler. A handler's message comes
    /// back for the view to run; everything else has already happened.
    fn dispatch(&mut self, event: wire::Event) -> Option<Callback<V>> {
        match event {
            wire::Event::Message(handler) => self.route(handler, &()),
            wire::Event::Click { handler, event } | wire::Event::AuxClick { handler, event } => {
                self.route(handler, &gpui::ClickEvent::from(event))
            }
            wire::Event::MouseDown { handler, event, .. } => {
                self.route(handler, &event.into_gpui())
            }
            wire::Event::MouseUp { handler, event, .. } => self.route(handler, &event.into_gpui()),
            wire::Event::MouseDownOut { handler, event } => self.route(handler, &event.into_gpui()),
            wire::Event::MouseUpOut { handler, event } => self.route(handler, &event.into_gpui()),
            wire::Event::MousePressure { handler, event, .. } => {
                self.route(handler, &event.into_gpui())
            }
            wire::Event::MouseMove { handler, event, .. } => {
                self.route(handler, &event.into_gpui())
            }
            wire::Event::MouseExit { handler, event, .. } => {
                self.route(handler, &event.into_gpui())
            }
            wire::Event::ScrollWheel { handler, event, .. } => {
                self.route(handler, &event.into_gpui())
            }
            wire::Event::Pinch { handler, event, .. } => self.route(handler, &event.into_gpui()),
            wire::Event::KeyDown { handler, event, .. } => self.route(handler, &event.into_gpui()),
            wire::Event::KeyUp { handler, event, .. } => self.route(handler, &event.into_gpui()),
            wire::Event::ModifiersChanged { handler, event } => {
                self.route(handler, &event.into_gpui())
            }
            wire::Event::Hover { handler, hovered } => self.route(handler, &hovered),
            wire::Event::Back { handler } => self.route(handler, &crate::BackEvent),
            wire::Event::FileDropExit { handler } => {
                self.route(handler, &gpui::FileDropEvent::Exited)
            }
            wire::Event::FileDrop { handler, files } => self.route(handler, &files),
            wire::Event::RichTextHover { handler, event } => self.route(handler, &event),
            wire::Event::ListScroll { handler, event } => self.route(handler, &event),
            wire::Event::ListRequest { handler, request } => self.route(handler, &request),
            wire::Event::TooltipRequest {
                request,
                character_index,
            } => {
                self.tooltip(request, character_index);
                None
            }
            wire::Event::Select { handler, index } => self.route(handler, &index),
            wire::Event::Bounds {
                handler,
                x,
                y,
                width,
                height,
            } => {
                let origin = gpui::point(px(x), px(y));
                let bounds = gpui::Bounds::new(origin, gpui::size(px(width), px(height)));
                self.route(handler, &bounds)
            }
            wire::Event::Drag { handler, dx, dy } => {
                self.route(handler, &(px(dx as f32), px(dy as f32)))
            }
            wire::Event::Text { handler, change } => self.route(handler, &change),
            wire::Event::ScrollOffset {
                handler,
                x,
                y,
                relative_x,
                relative_y,
            } => self.route(handler, &(x, y, relative_x, relative_y)),
            wire::Event::UniformListRange {
                path,
                route,
                start,
                end,
                item_height,
            } => {
                if self.app.inner.uniform_lists.request_range(
                    path,
                    route,
                    start as usize,
                    end as usize,
                    item_height,
                ) {
                    self.app.notify();
                }
                None
            }
            wire::Event::UniformListState {
                path,
                route,
                top_index,
                scrollable,
                scrolled_to_end,
            } => {
                self.app.inner.uniform_lists.update_state(
                    &path,
                    route,
                    top_index as usize,
                    scrollable,
                    scrolled_to_end,
                );
                None
            }
            wire::Event::Theme { dark } => {
                self.app
                    .set_global(if dark { Theme::dark() } else { Theme::light() });
                self.app.notify();
                None
            }
            wire::Event::Pointer { touch } => {
                if self.app.inner.touch.replace(touch) != touch {
                    self.app.inner.memos.borrow_mut().forget();
                    self.app.notify();
                }
                None
            }
            wire::Event::Viewport { width, height } => {
                let side = |v: f32| px(if v.is_finite() { v.max(0.) } else { 0. });
                let viewport = gpui::size(side(width), side(height));
                if self.app.inner.viewport.replace(viewport) != viewport {
                    self.app.inner.memos.borrow_mut().forget();
                    self.app.notify();
                }
                None
            }
            wire::Event::Offset { minutes } => {
                crate::design::set_utc_offset(minutes);
                self.app.inner.memos.borrow_mut().forget();
                self.app.notify();
                None
            }
            wire::Event::Response { id, result, done } => {
                self.app.host().fulfill(id, result, done);
                // Response order is semantic: queued hidden data must be
                // applied before a later visibility notification.
                self.settle();
                None
            }
            // The host dropped the tree the patches build on.
            wire::Event::Resync => {
                self.last_root = None;
                slots::clear_pictures(&self.app.inner.slots);
                self.app.inner.memos.borrow_mut().forget();
                self.app.notify();
                None
            }
            wire::Event::A11yAction { handler, data } => self.route(handler, &data),
            wire::Event::Web { handler, state } => self.route(handler, &state),
            wire::Event::Media { handler, state } => self.route(handler, &state),
            // a later host's event this build cannot read: dropped, and
            // the rest of the tick runs
            wire::Event::Unknown { .. } => None,
        }
    }

    fn tooltip(&mut self, request: u32, character_index: Option<u32>) {
        let slots = self.app.inner.slots.clone();
        let mut window = self.app.window();
        let built = slots::build_tooltip(
            &slots,
            request,
            character_index.map(|index| index as usize),
            &mut window,
            &mut self.app,
        );
        if let Some(content) = built {
            let content = content.map(|view| {
                Box::new(Lowering::within_tooltip(&mut window, &mut self.app, request).lower(view))
            });
            slots::tooltip_response(
                &slots,
                wire::TooltipResponse {
                    request,
                    character_index,
                    content,
                },
            );
        }
    }

    fn frame(&mut self) -> wire::Frame {
        let render = self.app.inner.dirty.replace(false) || self.last_root.is_none();
        let mut root = render.then(|| self.render_root());
        let hits = std::mem::take(&mut self.app.inner.memos.borrow_mut().hits);
        // a target inside a kept subtree is found in the last tree, which
        // holds it until the frame takes it
        let behind = match hits.is_empty() {
            true => None,
            false => self.last_root.as_ref(),
        };
        crate::window::send_widgets(
            &self.app.inner.slots,
            root.as_ref().or(self.last_root.as_ref()),
            behind,
        );
        self.busy |= self.app.inner.dirty.get() || executor::ready(&self.app.inner.tasks.borrow());
        // Patches against the last tree, unless there is none — a first
        // frame or a resync. Nothing lowered is nothing changed, and a
        // lowered tree that diffs to no patches is the same tree
        // (`apply(old, diff(old, new))` leaves `old == new`).
        let checks = self.app.inner.memo_checks.get();
        let mut patches = Vec::new();
        let mut lost = false;
        let unchanged = match (&mut root, &mut self.last_root) {
            (None, _) => true,
            (Some(root), Some(last)) => {
                let started = Clock::now();
                match hits.is_empty() {
                    true => patches = wire::diff_taking(last, root),
                    false => match crate::memo::keeping(last, root, &hits, checks) {
                        Some(kept) => patches = kept,
                        None => lost = true,
                    },
                }
                self.spent.diff += started.elapsed();
                patches.is_empty()
            }
            (Some(_), None) => {
                assert!(
                    !checks || hits.is_empty(),
                    "memos kept with no last tree to keep them from"
                );
                lost = !hits.is_empty();
                false
            }
        };
        // A kept memo with no node behind it: nothing is sent, and the next
        // tick draws the tree whole.
        if lost {
            root = None;
            patches.clear();
            self.last_root = None;
            slots::clear_pictures(&self.app.inner.slots);
            self.app.inner.memos.borrow_mut().forget();
            self.app.notify();
            self.busy = true;
        }
        let unchanged = unchanged || lost;
        match root.take() {
            Some(tree) if !unchanged => root = self.keep(tree, &mut patches),
            // the same tree, with the subtrees it kept moved in from the
            // last one: the next frame's base
            Some(tree) => self.last_root = Some(tree),
            None => {}
        }
        let host = self.app.host();
        let requests = host.drain_outbox();
        let cancels = host.drain_cancels();
        let mut tooltip_responses = slots::take_tooltip_responses(&self.app.inner.slots);
        wire::Frame {
            styles: self.styles(root.as_mut(), &mut tooltip_responses),
            tooltip_responses,
            root,
            patches,
            requests,
            cancels,
            unchanged,
            busy: self.busy,
        }
    }

    /// The style entries a frame carries. A tree sent whole starts the
    /// table over with the styles it and the frame's tooltips name, and
    /// carries them all; any other frame carries the entries the host does
    /// not hold yet.
    fn styles(
        &mut self,
        whole: Option<&mut wire::Node>,
        tooltips: &mut [wire::TooltipResponse],
    ) -> Vec<wire::Style> {
        let mut styles = self.app.inner.styles.borrow_mut();
        if let Some(root) = whole {
            // the table starts over, numbered anew: the subtrees kept for
            // the next frame are parts of this tree, renumbered with it
            let mut memos = self.app.inner.memos.borrow_mut();
            styles.retain(|visit| {
                let contents = tooltips
                    .iter_mut()
                    .filter_map(|response| response.content.as_deref_mut());
                for tree in std::iter::once(root).chain(contents) {
                    tree.for_each_mut(&mut |node| node.styles_mut(visit));
                }
                for entry in memos.entries.values_mut() {
                    entry.shell.for_each_mut(&mut |node| node.styles_mut(visit));
                }
            });
        }
        styles.unsent()
    }

    fn render_root(&mut self) -> wire::Node {
        self.renders += 1;
        slots::begin_frame(&self.app.inner.slots);
        self.app.inner.memos.borrow_mut().begin_frame();
        self.app.inner.uniform_lists.begin_frame();
        let (entity, spent) = (&self.entity, &mut self.spent);
        let root = self.app.update(|app| {
            let mut window = app.window();
            let started = Clock::now();
            let element = {
                let mut cx = Context {
                    app,
                    entity: entity.clone(),
                };
                let mut view = entity.value.borrow_mut();
                view.as_mut()
                    .expect("entity initialized")
                    .render(&mut window, &mut cx)
                    .into_element()
            };
            spent.render += started.elapsed();
            let started = Clock::now();
            let mut lowering = Lowering::new(&mut window, app);
            let root = lowering.entity(entity.id, |lowering| lowering.lower_element(element));
            spent.lowered += lowering.lowered();
            spent.lower += started.elapsed();
            root
        });
        slots::end_frame(&self.app.inner.slots);
        self.app.inner.memos.borrow_mut().end_frame();
        self.app.inner.uniform_lists.end_frame();
        root
    }

    /// Keeps a changed tree as the next frame's base and says what to send:
    /// the tree itself (patches cleared), or nothing beside the patches.
    /// The diff took the patches' subtrees out of `tree`; a tree sent whole
    /// gets them back here, a patched one in [`Driver::put_back`] once the
    /// frame is sent.
    fn keep(&mut self, mut tree: wire::Node, patches: &mut Vec<wire::Patch>) -> Option<wire::Node> {
        // Property changes remain patches even for tiny trees; replacing
        // their identity would discard native state. Structural edits
        // use the whole tree when it has no more nodes than the patches
        // carry (counted, not encoded: sizing both by encoding them was
        // most of a frame's cost).
        let only_props = patches
            .iter()
            .all(|patch| matches!(patch, wire::Patch::Props { .. }));
        let carried = || -> usize {
            patches
                .iter()
                .map(|patch| match patch {
                    wire::Patch::Replace { node, .. } | wire::Patch::Insert { node, .. } => {
                        node.count()
                    }
                    _ => 1,
                })
                .sum()
        };
        // The tree's count as rendered: the hollowed tree plus what each
        // taken subtree holds beyond its stand-in.
        let nodes = || -> usize {
            let taken: usize = patches
                .iter()
                .map(|patch| match patch {
                    wire::Patch::Replace { node, .. } | wire::Patch::Insert { node, .. } => {
                        node.count() - 1
                    }
                    _ => 0,
                })
                .sum();
            tree.count() + taken
        };
        if !only_props && carried() >= nodes() {
            put_back(&mut tree, std::mem::take(patches));
        }
        // A frame that carries patches sends no tree, so the tree itself is
        // kept; one that sends the tree has nothing to keep until the tree
        // comes back from the encoder.
        match patches.is_empty() {
            true => {
                self.last_root = None;
                Some(tree)
            }
            false => {
                self.last_root = Some(tree);
                None
            }
        }
    }

    /// Runs a route listener. A routed event carries no message back.
    fn route<E: 'static>(&mut self, handler: u32, event: &E) -> Option<Callback<V>> {
        let slots = self.app.inner.slots.clone();
        let mut window = self.app.window();
        slots::run_route(&slots, handler, event, &mut window, &mut self.app);
        None
    }

    fn settle(&mut self) {
        self.busy |= self.app.settle();
    }
}

/// The root's snapshot bytes, for the debug check that a change to them
/// notified. A view whose serde refuses has none, before and after.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
fn encode_root<V: View>(view: &dyn std::any::Any) -> Vec<u8> {
    wire::try_encode(view.downcast_ref::<V>().expect("the root view")).unwrap_or_default()
}

/// Puts the subtrees `patches` carry back into the tree `wire::diff_taking`
/// took them from, which is `new` as it was: a patch's path and index are
/// the position of its node in `new`. Each subtree goes into its stand-in's
/// slot, never inserted, so the patches' order does not matter. Patches
/// that carry no subtree are skipped.
pub(crate) fn put_back(new: &mut wire::Node, patches: Vec<wire::Patch>) {
    for patch in patches {
        let (path, index, node) = match patch {
            wire::Patch::Replace { path, node } => (path, None, node),
            wire::Patch::Insert { path, index, node } => (path, Some(index), node),
            wire::Patch::Props { .. } | wire::Patch::Remove { .. } | wire::Patch::Move { .. } => {
                continue;
            }
        };
        let mut target = &mut *new;
        for index in path {
            target = &mut target.children_mut()[index as usize];
        }
        match index {
            Some(index) => target.children_mut()[index as usize] = node,
            None => *target = node,
        }
    }
}
