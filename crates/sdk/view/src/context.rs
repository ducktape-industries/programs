//! Contexts and handles for the entity graph: the root view and the child
//! entities built with [`App::new`] (`cx.new`). The root is the one entity
//! the snapshot carries; whoever builds a child builds it again on a
//! restore.
use crate::{FocusHandle, Host, Task, Window, executor, slots};
use gpui::{EventEmitter, Subscription};
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::ops::{Deref, DerefMut};
use std::rc::{Rc, Weak};

mod events;

/// A step to run on the view: an editor route hands one back, and the
/// driver runs it after the event.
pub type Callback<V> = Rc<dyn Fn(&mut V, &mut Window, &mut Context<V>)>;

pub(crate) type Globals = std::collections::HashMap<TypeId, Rc<dyn Any>>;

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
pub(crate) type Root = (u64, fn(&dyn Any) -> Vec<u8>);

pub struct App {
    pub(crate) inner: Rc<AppState>,
    globals: Rc<Globals>,
}
pub(crate) struct AppState {
    pub host: Host,
    pub slots: slots::Context,
    /// Every distinct style the view's trees name, by the id a node
    /// carries: the view's half of the frame's style table.
    pub styles: RefCell<crate::wire::Interner>,
    pub tasks: RefCell<Vec<executor::Running>>,
    pub generation: Cell<u64>,
    pub next_focus_id: Cell<u64>,
    pub next_entity_id: Cell<u64>,
    pub events: events::Events,
    pub dirty: Cell<bool>,
    pub alive: Cell<bool>,
    pub globals: RefCell<Rc<Globals>>,
    pub uniform_lists: crate::element::UniformLists,
    // The subtrees kept from the last frame, and each entity's notifies.
    pub memos: RefCell<crate::memo::Cache>,
    // Whether a kept memo is lowered again and held to what it kept.
    pub memo_checks: Cell<bool>,
    /// The ids each press [`design::dense`](crate::design::dense) marked is
    /// filed under, in the tree last lowered: the presses
    /// [`assert_fits`](crate::testing::assert_fits) lets stay under a
    /// finger's height. The mark is the view's and is never sent.
    pub dense: RefCell<Vec<Vec<crate::wire::ElementIdWire>>>,
    #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
    pub root: Cell<Option<Root>>,
    /// The size the view is laid out in, as the host last said
    /// (`Event::Viewport`): what [`Window::viewport_size`] reads.
    pub viewport: Cell<gpui::Size<gpui::Pixels>>,
    /// The device's primary pointer is a finger, as the host last said
    /// (`Event::Pointer`): what [`Window::touch`] reads.
    pub touch: Cell<bool>,
}
impl App {
    pub(crate) fn styles(&self) -> crate::wire::Styles {
        let mut styles = crate::wire::Styles::default();
        styles
            .extend(self.inner.styles.borrow_mut().unsent())
            .expect("the host takes the styles");
        styles
    }

    pub(crate) fn for_driver() -> Self {
        let host = Host::default();
        let slots = slots::Context::with_host(host.clone());
        let mut globals = std::collections::HashMap::new();
        globals.insert(
            TypeId::of::<crate::Theme>(),
            Rc::new(crate::Theme::default()) as Rc<dyn Any>,
        );
        let globals = Rc::new(globals);
        Self {
            globals: globals.clone(),
            inner: Rc::new(AppState {
                host,
                slots,
                styles: RefCell::default(),
                tasks: RefCell::default(),
                generation: Cell::new(0),
                next_focus_id: Cell::new(0),
                next_entity_id: Cell::new(0),
                events: Default::default(),
                dirty: Cell::new(true),
                alive: Cell::new(true),
                globals: RefCell::new(globals),
                uniform_lists: Default::default(),
                memos: RefCell::default(),
                memo_checks: Cell::new(cfg!(debug_assertions)),
                dense: RefCell::default(),
                #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
                root: Cell::new(None),
                viewport: Cell::default(),
                touch: Cell::default(),
            }),
        }
    }
    pub fn host(&self) -> Host {
        self.inner.host.clone()
    }
    // What the view keeps on this device (`store`): `cx.store().get(key)`.
    pub fn store(&self) -> crate::store::Store {
        crate::store::Store(self.host())
    }
    pub fn spawn<R: 'static>(&self, f: impl AsyncFnOnce(&mut AsyncApp) -> R + 'static) -> Task<R> {
        let mut cx = AsyncApp {
            inner: Rc::downgrade(&self.inner),
        };
        let (task, running) = executor::task(async move { f(&mut cx).await });
        self.inner.tasks.borrow_mut().push(running);
        task
    }
    pub(crate) fn window(&self) -> Window {
        Window::new(self.inner.clone())
    }
    pub(crate) fn settle(&mut self) -> bool {
        loop {
            let mut tasks = std::mem::take(&mut *self.inner.tasks.borrow_mut());
            let still_woken = executor::poll(&mut tasks);
            self.refresh_globals();
            let spawned = !self.inner.tasks.borrow().is_empty();
            tasks.append(&mut self.inner.tasks.borrow_mut());
            *self.inner.tasks.borrow_mut() = tasks;
            if !spawned {
                return still_woken;
            }
        }
    }
    pub(crate) fn notify(&self) {
        self.inner.dirty.set(true);
        self.inner
            .generation
            .set(self.inner.generation.get().wrapping_add(1));
    }
    pub(crate) fn from_state(inner: Rc<AppState>) -> Self {
        let globals = inner.globals.borrow().clone();
        Self { inner, globals }
    }
    pub(crate) fn refresh_globals(&mut self) {
        self.globals = self.inner.globals.borrow().clone();
    }
    pub fn set_global<G: gpui::Global>(&mut self, global: G) {
        self.set_shared_global(TypeId::of::<G>(), Rc::new(global));
    }
    pub(crate) fn set_shared_global(&mut self, kind: TypeId, global: Rc<dyn Any>) {
        self.inner.memos.borrow_mut().forget();
        self.refresh_globals();
        Rc::make_mut(&mut self.globals).insert(kind, global);
        *self.inner.globals.borrow_mut() = self.globals.clone();
    }
    pub fn global<G: gpui::Global>(&self) -> &G {
        self.globals
            .get(&TypeId::of::<G>())
            .and_then(|global| global.downcast_ref::<G>())
            .expect("global is not initialized")
    }

    pub fn focus_handle(&mut self) -> FocusHandle {
        let id = self.inner.next_focus_id.get();
        self.inner
            .next_focus_id
            .set(id.checked_add(1).expect("focus handle space exhausted"));
        FocusHandle::new(id)
    }

    /// A child entity: any `'static` state, rendered where its handle is a
    /// child (`.child(entity.clone())`) when it implements [`Render`], and
    /// updated through the handle (`entity.update(cx, |state, cx| ..)`).
    /// The snapshot carries only the root view, so whoever builds a child
    /// builds it in [`View::attach`](crate::View::attach), which runs on a
    /// first mount and on a restore alike.
    ///
    /// ```
    /// # use serde::{Deserialize, Serialize};
    /// use view::{View, prelude::*, testing::TestAppContext};
    ///
    /// struct Counter {
    ///     count: usize,
    /// }
    /// impl Render for Counter {
    ///     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    ///         div().id("count").child(self.count.to_string())
    ///     }
    /// }
    ///
    /// #[derive(Default, Serialize, Deserialize)]
    /// struct Page {
    ///     #[serde(skip)]
    ///     counter: Option<Entity<Counter>>,
    /// }
    /// impl View for Page {
    ///     const NAME: &'static str = "Page";
    ///     fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
    ///         self.counter = Some(cx.new(|_| Counter { count: 0 }));
    ///     }
    /// }
    /// impl Render for Page {
    ///     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    ///         let counter = self.counter.clone().expect("attach built it");
    ///         let bump = div().id("bump").role(Role::Button).focusable().child("Bump");
    ///         div().child(counter.clone()).child(bump.on_click(move |_, _, cx| {
    ///             counter.update(cx, |counter, cx| {
    ///                 counter.count += 1;
    ///                 cx.notify();
    ///             })
    ///         }))
    ///     }
    /// }
    ///
    /// let mut cx = TestAppContext::new();
    /// cx.open::<Page>();
    /// cx.simulate_click("Bump");
    /// assert!(cx.has_text("1"));
    /// ```
    ///
    /// [`Render`]: crate::Render
    #[expect(clippy::new_ret_no_self, reason = "matches GPUI's App::new entity API")]
    pub fn new<T: 'static>(&mut self, build: impl FnOnce(&mut Context<T>) -> T) -> Entity<T> {
        let entity = Entity::reserve(self);
        self.update(|app| {
            let value = build(&mut Context {
                app,
                entity: entity.clone(),
            });
            *entity.value.borrow_mut() = Some(value);
        });
        entity
    }
}

/// What an entity is reached through, as in gpui: the [`App`] a listener
/// gets, an entity's [`Context`], or a task's [`AsyncApp`].
pub trait AppContext {
    /// Runs `f` on the app; [`Released`] once the view is gone, which only
    /// a task's context outlives.
    fn with_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Result<R, Released>;
}
impl AppContext for App {
    fn with_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Result<R, Released> {
        Ok(f(self))
    }
}
impl<T> AppContext for Context<'_, T> {
    fn with_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Result<R, Released> {
        Ok(f(self.app))
    }
}
impl AppContext for AsyncApp {
    fn with_app<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> Result<R, Released> {
        let state = self.inner.upgrade().ok_or(Released)?;
        Ok(f(&mut App::from_state(state)))
    }
}

#[derive(Clone)]
pub struct AsyncApp {
    inner: Weak<AppState>,
}
impl AsyncApp {
    pub fn host(&self) -> Host {
        self.inner.upgrade().expect("app released").host.clone()
    }
    pub fn store(&self) -> crate::store::Store {
        crate::store::Store(self.host())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Released;
impl std::fmt::Display for Released {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("entity released")
    }
}
impl std::error::Error for Released {}

/// A handle to an entity: the root view, or a child built with
/// [`App::new`]. Clones share the state; it lives while a handle does.
pub struct Entity<T> {
    pub(crate) value: Rc<RefCell<Option<T>>>,
    pub(crate) id: u64,
    app: Weak<AppState>,
}
pub struct WeakEntity<T> {
    value: Weak<RefCell<Option<T>>>,
    id: u64,
    app: Weak<AppState>,
}
impl<T> Clone for Entity<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            id: self.id,
            app: self.app.clone(),
        }
    }
}
impl<T> Clone for WeakEntity<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            id: self.id,
            app: self.app.clone(),
        }
    }
}
impl<T> Entity<T> {
    pub(crate) fn reserve(app: &App) -> Self {
        let id = app.inner.next_entity_id.get();
        app.inner.next_entity_id.set(id + 1);
        Self {
            value: Rc::default(),
            id,
            app: Rc::downgrade(&app.inner),
        }
    }
    pub fn downgrade(&self) -> WeakEntity<T> {
        WeakEntity {
            value: Rc::downgrade(&self.value),
            id: self.id,
            app: self.app.clone(),
        }
    }
    pub fn read<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        f(self.value.borrow().as_ref().expect("entity initialized"))
    }
}
impl<T: 'static> Entity<T> {
    /// Runs `f` on the entity's state: from a listener (`cx` is its `App`
    /// or an entity's `Context`) or a task (its `AsyncApp`). What `f`
    /// changes renders once it calls `cx.notify()`.
    pub fn update<C: AppContext, R>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut T, &mut Context<T>) -> R,
    ) -> R {
        cx.with_app(|app| {
            let mut window = app.window();
            self.update_in_window(app, &mut window, |state, _, cx| f(state, cx))
        })
        .expect("the view this entity belongs to is gone")
    }
    pub(crate) fn update_in_window<R>(
        &self,
        app: &mut App,
        window: &mut Window,
        f: impl FnOnce(&mut T, &mut Window, &mut Context<T>) -> R,
    ) -> R {
        assert!(
            self.app.ptr_eq(&Rc::downgrade(&app.inner)),
            "entity belongs to another app"
        );
        app.update(|app| {
            app.refresh_globals();
            let mut value = self.value.borrow_mut();
            let state = value.as_mut().expect("entity initialized");
            #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
            let check = app
                .inner
                .root
                .get()
                .filter(|(root, _)| *root == self.id)
                .map(|(_, encode)| (encode, encode(&*state), app.inner.generation.get()));
            let mut cx = Context {
                app,
                entity: self.clone(),
            };
            let result = f(state, window, &mut cx);
            #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
            if let Some((encode, before, notified)) = check {
                assert!(
                    before == encode(&*state) || notified != cx.app.inner.generation.get(),
                    "state changed without cx.notify()"
                );
            }
            result
        })
    }
}
impl<T: 'static> WeakEntity<T> {
    pub fn upgrade(&self) -> Option<Entity<T>> {
        if !self.app.upgrade()?.alive.get() {
            return None;
        }
        Some(Entity {
            value: self.value.upgrade()?,
            id: self.id,
            app: self.app.clone(),
        })
    }
    /// [`Entity::update`], or [`Released`] once the entity is gone.
    pub fn update<C: AppContext, R>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut T, &mut Context<T>) -> R,
    ) -> Result<R, Released> {
        self.update_in(cx, |state, _, cx| f(state, cx))
    }
    pub fn update_in<C: AppContext, R>(
        &self,
        cx: &mut C,
        f: impl FnOnce(&mut T, &mut Window, &mut Context<T>) -> R,
    ) -> Result<R, Released> {
        let entity = self.upgrade().ok_or(Released)?;
        cx.with_app(|app| {
            let mut window = app.window();
            entity.update_in_window(app, &mut window, f)
        })
    }
}
pub struct Context<'a, V> {
    pub(crate) app: &'a mut App,
    pub(crate) entity: Entity<V>,
}
impl<V> Deref for Context<'_, V> {
    type Target = App;
    fn deref(&self) -> &App {
        self.app
    }
}
impl<V> DerefMut for Context<'_, V> {
    fn deref_mut(&mut self) -> &mut App {
        self.app
    }
}
impl<V> Context<'_, V> {
    pub fn entity(&self) -> Entity<V> {
        self.entity.clone()
    }
    pub fn weak_entity(&self) -> WeakEntity<V> {
        self.entity.downgrade()
    }
}
impl<V: 'static> Context<'_, V> {
    // [`memo`](crate::memo), built by a method of this entity: `build`
    // runs on it, as a listener does, only when the memo is lowered anew.
    pub fn memo<D: PartialEq + 'static, E: crate::IntoElement>(
        &self,
        id: impl Into<gpui::ElementId>,
        deps: D,
        build: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> E + 'static,
    ) -> crate::Memo {
        let entity = self.entity();
        crate::memo(id, deps, move |window, app| {
            entity.update_in_window(app, window, |view, window, cx| {
                crate::IntoElement::into_any_element(build(view, window, cx))
            })
        })
    }
    /// The view renders on the next tick, whichever entity notified; and
    /// what [`observe`](Self::observe)s this entity hears it.
    pub fn notify(&mut self) {
        self.app.notify();
        let id = self.entity.id;
        self.app.inner.memos.borrow_mut().notified(id);
        self.app
            .inner
            .events
            .raise(self.entity.id, None, || Box::new(()));
    }
    /// Tells what [`subscribe`](Self::subscribe)s to this entity, once the
    /// update that emits is done.
    pub fn emit<E: 'static>(&mut self, event: E)
    where
        V: EventEmitter<E>,
    {
        let kind = Some(TypeId::of::<E>());
        self.app
            .inner
            .events
            .raise(self.entity.id, kind, || Box::new(event));
    }
    /// Runs `on_event` on this entity for each `E` that `entity` emits,
    /// while the subscription is kept: store it, or `detach` it.
    pub fn subscribe<W: EventEmitter<E>, E: 'static>(
        &mut self,
        entity: &Entity<W>,
        mut on_event: impl FnMut(&mut V, Entity<W>, &E, &mut Context<V>) + 'static,
    ) -> Subscription {
        let (this, emitter) = (self.weak_entity(), entity.downgrade());
        let kind = Some(TypeId::of::<E>());
        self.listen(entity.id, kind, move |event, app| {
            let (Some(this), Some(emitter)) = (this.upgrade(), emitter.upgrade()) else {
                return;
            };
            let event = event.downcast_ref::<E>().expect("raised as an E");
            this.update(app, |this, cx| on_event(this, emitter, event, cx));
        })
    }
    /// Runs `on_notify` on this entity each time `entity` calls
    /// `cx.notify()`, while the subscription is kept.
    pub fn observe<W: 'static>(
        &mut self,
        entity: &Entity<W>,
        mut on_notify: impl FnMut(&mut V, Entity<W>, &mut Context<V>) + 'static,
    ) -> Subscription {
        let (this, observed) = (self.weak_entity(), entity.downgrade());
        self.listen(entity.id, None, move |_, app| {
            let (Some(this), Some(observed)) = (this.upgrade(), observed.upgrade()) else {
                return;
            };
            this.update(app, |this, cx| on_notify(this, observed, cx));
        })
    }
    fn listen(
        &self,
        emitter: u64,
        kind: events::Kind,
        hear: impl FnMut(&dyn Any, &mut App) + 'static,
    ) -> Subscription {
        let id = self.app.inner.events.listen(emitter, kind, hear);
        let app = Rc::downgrade(&self.app.inner);
        Subscription::new(move || {
            if let Some(app) = app.upgrade() {
                app.events.forget(id);
            }
        })
    }
    pub fn listener<E: ?Sized, F: Fn(&mut V, &E, &mut Window, &mut Context<V>) + 'static>(
        &self,
        f: F,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static + use<E, F, V> {
        let entity = self.weak_entity();
        move |event, window, app| {
            if let Some(entity) = entity.upgrade() {
                entity.update_in_window(app, window, |view, window, cx| f(view, event, window, cx));
            }
        }
    }
    pub fn processor<E, R, F: Fn(&mut V, E, &mut Window, &mut Context<V>) -> R + 'static>(
        &self,
        f: F,
    ) -> impl Fn(E, &mut Window, &mut App) -> R + 'static + use<E, F, R, V> {
        let entity = self.entity();
        move |event, window, app| {
            entity.update_in_window(app, window, |view, window, cx| f(view, event, window, cx))
        }
    }
    pub fn spawn<R: 'static>(
        &self,
        f: impl AsyncFnOnce(WeakEntity<V>, &mut AsyncApp) -> R + 'static,
    ) -> Task<R> {
        let entity = self.weak_entity();
        self.app.spawn(async move |cx| f(entity, cx).await)
    }
}

#[cfg(test)]
mod global_tests {
    use super::*;

    struct Counter(usize);
    impl gpui::Global for Counter {}

    #[test]
    fn globals_need_not_clone_and_app_snapshots_refresh_safely() {
        let mut driver = App::for_driver();
        driver.set_global(Counter(1));
        let mut task = App::from_state(driver.inner.clone());
        let old = driver.global::<Counter>();
        task.set_global(Counter(2));
        assert_eq!(old.0, 1);
        driver.refresh_globals();
        assert_eq!(driver.global::<Counter>().0, 2);
    }

    #[test]
    fn updating_a_global_preserves_other_tasks_updates() {
        struct Other(usize);
        impl gpui::Global for Other {}
        let mut driver = App::for_driver();
        let mut task = App::from_state(driver.inner.clone());
        task.set_global(Other(7));
        driver.set_global(Counter(3));
        assert_eq!(driver.global::<Other>().0, 7);
        assert_eq!(driver.global::<Counter>().0, 3);
    }
}
