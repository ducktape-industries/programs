//! A serializable root view and small loading conveniences.
use crate::host::Error;
use crate::methods::Method;
use crate::{Context, IntoElement, Task, Window};
use futures::StreamExt;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::rc::Rc;

pub trait Render: 'static + Sized {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement;
}
// A root view: everything the host reads about it (the manifest
// `export_view!` writes: [`NAME`](View::NAME), [`DESCRIPTION`](View::DESCRIPTION),
// [`CAPABILITIES`](View::CAPABILITIES), [`MIN_WINDOW_WIDTH`](View::MIN_WINDOW_WIDTH),
// [`WINDOW_WIDTH`](View::WINDOW_WIDTH), [`ICON`](View::ICON),
// [`TINT`](View::TINT)), how it is built
// ([`new`](View::new)) and how it joins the host ([`attach`](View::attach)).
pub trait View: Render + Serialize + DeserializeOwned + Default {
    /// The name on the tab and in the catalog, `1..=64` bytes.
    const NAME: &'static str;
    /// One line for the catalog, at most 256 bytes.
    const DESCRIPTION: &'static str = "";
    /// The view's icon where the app shows the view small (a tab on a
    /// folded bar), named as `svg().path(..)` names an icon the app bundles
    /// (`"icons/hammer.svg"`), at most 64 bytes: the app bundles the Lucide
    /// icons as `icons/<lucide name>.svg`. With none, or with a name the
    /// app does not bundle, the app shows the name's initial. A path that
    /// could leave the app's bundled set (a leading `/`, a `\` or `:`, an
    /// empty, `.` or `..` segment) does not compile:
    ///
    /// ```compile_fail,E0080
    /// # use serde::{Deserialize, Serialize};
    /// # use view::{View, prelude::*};
    /// #[derive(Default, Serialize, Deserialize)]
    /// struct Outside;
    /// impl View for Outside {
    ///     const NAME: &'static str = "Outside";
    ///     const ICON: &'static str = "../icons/hammer.svg";
    /// }
    /// # impl Render for Outside {
    /// #     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    /// #         div()
    /// #     }
    /// # }
    /// view::export_view!(Outside);
    /// # fn main() {}
    /// ```
    const ICON: &'static str = "";
    /// The colour of the view's tile where the app lists it (a phone's
    /// Home): a [`Tint`](crate::Tint) by name (`Tint::Blue`), whose light
    /// and dark values are the app's, so every tile reads beside the
    /// others. With none, the tile is the app's neutral one.
    const TINT: crate::Tint = crate::Tint::None;
    /// The `<capability>` halves of the method kinds this view asks
    /// through, and the only ones the host lets it reach: a method whose
    /// capability is not here is refused `undeclared_capability`, by the
    /// app and by `TestAppContext` alike.
    const CAPABILITIES: &'static [crate::methods::Capability] = &[];
    /// The narrowest pointer window, in logical px, at which every
    /// essential element is visible and nothing is clipped: on a desk the
    /// app never lays the view out narrower, and never sizes a window
    /// holding it narrower unless the desk or screen itself is narrower. It
    /// is not a phone's width: a touch screen seats the view at the
    /// screen's own width whatever this says, so lowering it "for phones"
    /// only lets desktop windows shrink. `export_view!` writes it into the
    /// manifest:
    ///
    /// ```
    /// # use serde::{Deserialize, Serialize};
    /// # use view::{View, prelude::*};
    /// #[derive(Default, Serialize, Deserialize)]
    /// struct Wide;
    /// impl View for Wide {
    ///     const NAME: &'static str = "Wide";
    ///     const MIN_WINDOW_WIDTH: u32 = 640;
    /// }
    /// # impl Render for Wide {
    /// #     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    /// #         div()
    /// #     }
    /// # }
    /// view::export_view!(Wide);
    /// # fn main() {}
    /// ```
    ///
    /// ```compile_fail,E0080
    /// # use serde::{Deserialize, Serialize};
    /// # use view::{View, prelude::*};
    /// #[derive(Default, Serialize, Deserialize)]
    /// struct Zero;
    /// impl View for Zero {
    ///     const NAME: &'static str = "Zero";
    ///     const MIN_WINDOW_WIDTH: u32 = 0;
    /// }
    /// # impl Render for Zero {
    /// #     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    /// #         div()
    /// #     }
    /// # }
    /// view::export_view!(Zero);
    /// # fn main() {}
    /// ```
    const MIN_WINDOW_WIDTH: u32 = 480;
    /// The width, in logical px, a pointer window holding the view opens
    /// at, for a view that wants more room than the app's own share of the
    /// desk: a desk opens it at the wider of the two, never wider than the
    /// desk itself, and leaves a window the person has moved or sized as
    /// they left it. `None`, the default, is the app's share. Like
    /// [`MIN_WINDOW_WIDTH`](View::MIN_WINDOW_WIDTH) it is not a phone's: a
    /// touch screen seats the view at the screen's own width. It is never
    /// under `MIN_WINDOW_WIDTH`, or it does not compile:
    ///
    /// ```compile_fail,E0080
    /// # use serde::{Deserialize, Serialize};
    /// # use view::{View, prelude::*};
    /// #[derive(Default, Serialize, Deserialize)]
    /// struct Cramped;
    /// impl View for Cramped {
    ///     const NAME: &'static str = "Cramped";
    ///     const MIN_WINDOW_WIDTH: u32 = 640;
    ///     const WINDOW_WIDTH: Option<u32> = Some(600);
    /// }
    /// # impl Render for Cramped {
    /// #     fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
    /// #         div()
    /// #     }
    /// # }
    /// view::export_view!(Cramped);
    /// # fn main() {}
    /// ```
    const WINDOW_WIDTH: Option<u32> = None;
    /// The view's state on first mount, before it is attached. State only:
    /// what it follows and asks starts in [`attach`](View::attach).
    fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self::default()
    }
    /// Joins the host: every subscription, follower and read the view keeps
    /// running. Called once per mount, after [`new`](View::new) on a first
    /// mount and after the snapshot is decoded on a restore, so what is
    /// started here survives a redeploy. It also repairs what a snapshot
    /// cannot carry (a request in flight, a menu half open).
    fn attach(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
}
/// Data the view asked for, in the states it can be in. `Loading` and
/// `Reloading` carry the read's abort handle: dropping or replacing the
/// slot cancels it. Serialises `Loading` as `Idle` and `Reloading` as the
/// value it shows, so a restored view reads them again.
#[derive(Default)]
pub enum Loadable<T> {
    #[default]
    Idle,
    Loading(Task<()>),
    Ready(T),
    /// The value on screen while [`Context::load`] reads it again.
    Reloading(T, Task<()>),
    Failed(Error),
}

impl<T> Loadable<T> {
    /// The value on screen: ready, or ready while it is read again.
    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(value) | Self::Reloading(value, _) => Some(value),
            _ => None,
        }
    }
    pub fn ready_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Ready(value) | Self::Reloading(value, _) => Some(value),
            _ => None,
        }
    }
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading(_))
    }
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
    pub fn failed(&self) -> Option<&Error> {
        match self {
            Self::Failed(refusal) => Some(refusal),
            _ => None,
        }
    }
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LoadableSnapshot<T> {
    Idle,
    Ready(T),
    Failed { code: String, message: String },
}

/// A finished load: its value, or the refusal in its place.
impl<T> From<Result<T, Error>> for Loadable<T> {
    fn from(result: Result<T, Error>) -> Self {
        match result {
            Ok(value) => Self::Ready(value),
            Err(refusal) => Self::Failed(refusal),
        }
    }
}

impl<T: Serialize> Serialize for Loadable<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Idle | Self::Loading(_) => LoadableSnapshot::<&T>::Idle,
            Self::Ready(value) | Self::Reloading(value, _) => LoadableSnapshot::Ready(value),
            Self::Failed(refusal) => LoadableSnapshot::Failed {
                code: refusal.code.clone(),
                message: refusal.message.clone(),
            },
        }
        .serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Loadable<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match LoadableSnapshot::deserialize(deserializer)? {
            LoadableSnapshot::Idle => Self::Idle,
            LoadableSnapshot::Ready(value) => Self::Ready(value),
            LoadableSnapshot::Failed { code, message } => Self::Failed(Error::new(code, message)),
        })
    }
}

// A `Loadable` the view reads again whenever what it shows may have moved
// (`Context::reread`): asked for while a read of it is out, the read out is kept and lands what
// it read, and one more read follows it. A read replaced for every write heard while it runs
// never lands on a network that writes every block.
pub struct Reread<T> {
    shown: Loadable<T>,
    again: bool,
}

impl<T> Default for Reread<T> {
    fn default() -> Self {
        Self {
            shown: Loadable::Idle,
            again: false,
        }
    }
}

impl<T> std::ops::Deref for Reread<T> {
    type Target = Loadable<T>;
    fn deref(&self) -> &Loadable<T> {
        &self.shown
    }
}

impl<T> std::ops::DerefMut for Reread<T> {
    fn deref_mut(&mut self) -> &mut Loadable<T> {
        &mut self.shown
    }
}

impl<T: Serialize> Serialize for Reread<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.shown.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Reread<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self {
            shown: Loadable::deserialize(deserializer)?,
            again: false,
        })
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Reread<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.shown.fmt(f)
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Loadable<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => f.write_str("Idle"),
            Self::Loading(_) => f.write_str("Loading"),
            Self::Ready(value) => f.debug_tuple("Ready").field(value).finish(),
            Self::Reloading(value, _) => f.debug_tuple("Reloading").field(value).finish(),
            Self::Failed(refusal) => f.debug_tuple("Failed").field(refusal).finish(),
        }
    }
}

impl<V: 'static> Context<'_, V> {
    /// Reads `work` into the slot `at` finds in `view`: `cx.load(self,
    /// work, |view| &mut view.slot)`. The one rule: what the slot shows
    /// stays until the answer lands. A value already there stays on screen
    /// (`Reloading`); a slot with nothing to show (`Idle`, `Failed`) shows
    /// `Loading`. A value lands `Ready`, a refusal `Failed`. It notifies
    /// when, and only when, it changes what the slot shows: a read that
    /// lands the value already there draws nothing.
    ///
    /// A read that must blank what is shown says so in a line of its own
    /// first: `self.slot = Loadable::Idle;`.
    ///
    /// The read lives in the slot: dropping or replacing the slot cancels
    /// it, so a newer `load` of the slot supersedes this one and an older
    /// answer never lands over a newer one.
    pub fn load<T: PartialEq + 'static>(
        &mut self,
        view: &mut V,
        work: impl Future<Output = Result<T, Error>> + 'static,
        at: impl Fn(&mut V) -> &mut Loadable<T> + 'static,
    ) {
        self.load_then(view, work, at, |_, _| {});
    }

    // Reads into the `Reread` slot `at` finds, as `load` does, unless a
    // read of it is out: that read is kept, and once it has landed
    // `read_again` runs, once, however many asks came in meanwhile.
    // `read_again` is the view's own read (`Self::read_roster`), so the
    // read that follows asks for what the view wants by then.
    pub fn reread<T, F>(
        &mut self,
        view: &mut V,
        at: impl Fn(&mut V) -> &mut Reread<T> + 'static,
        work: impl FnOnce(&mut V, &mut Context<V>) -> F,
        read_again: impl FnOnce(&mut V, &mut Context<V>) + 'static,
    ) where
        T: PartialEq + 'static,
        F: Future<Output = Result<T, Error>> + 'static,
    {
        let slot = at(view);
        if matches!(slot.shown, Loadable::Loading(_) | Loadable::Reloading(..)) {
            slot.again = true;
            return;
        }
        slot.again = false;
        let work = work(view, self);
        let at = Rc::new(at);
        let land = at.clone();
        self.load_then(
            view,
            work,
            move |view| &mut land(view).shown,
            move |view, cx| {
                if std::mem::take(&mut at(view).again) {
                    read_again(view, cx);
                }
            },
        );
    }

    // `load`, and `then` once the answer has landed in the slot.
    fn load_then<T: PartialEq + 'static>(
        &mut self,
        view: &mut V,
        work: impl Future<Output = Result<T, Error>> + 'static,
        at: impl Fn(&mut V) -> &mut Loadable<T> + 'static,
        then: impl FnOnce(&mut V, &mut Context<V>) + 'static,
    ) {
        // the slot as it is now; `at` then moves into the task, which finds
        // the slot again when the answer lands
        let slot = at(view);
        let task = self.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |view, cx| {
                let slot = at(view);
                match (slot.take(), result) {
                    (Loadable::Reloading(shown, _), Ok(value)) if shown == value => {
                        *slot = Loadable::Ready(shown)
                    }
                    (_, result) => {
                        *slot = Loadable::from(result);
                        cx.notify();
                    }
                }
                then(view, cx);
            });
        });
        *slot = match slot.take() {
            Loadable::Ready(shown) | Loadable::Reloading(shown, _) => {
                Loadable::Reloading(shown, task)
            }
            Loadable::Loading(_) => Loadable::Loading(task),
            Loadable::Idle | Loadable::Failed(_) => {
                self.notify();
                Loadable::Loading(task)
            }
        };
    }

    /// Follows a host stream: subscribes to `D` with `request` and runs
    /// `each` on every item, in order, until the stream ends or the view is
    /// gone. `each` decides what the item means: it calls `cx.notify()`
    /// when the item changed what the view shows, and an item that changes
    /// nothing draws nothing. A refused item is handed to `each` like any
    /// other, so each follower says what a refusal means to it; a refusal
    /// the host sends as the stream's last item ends the stream.
    ///
    /// The task is the subscription, as gpui's `Subscription` is: keep it
    /// to end the stream early (dropping it unsubscribes), or `.detach()`
    /// it to follow for as long as the view runs.
    pub fn follow<D: Method>(
        &mut self,
        request: D::Request,
        mut each: impl FnMut(&mut V, Result<D::Reply, Error>, &mut Context<V>) + 'static,
    ) -> Task<()> {
        let mut stream = self.host().subscribe::<D>(request);
        self.spawn(async move |this, cx| {
            while let Some(item) = stream.next().await {
                if this.update(cx, |view, cx| each(view, item, cx)).is_err() {
                    break;
                }
            }
        })
    }

    /// Runs `work` and hands its answer, the value or the refusal, to
    /// `land`, and does nothing else: `land` changes what it changes and
    /// calls `cx.notify()` if that moved what the view shows. For a write,
    /// or a read that does more than fill a slot. In gpui's words it is
    /// `cx.spawn(async move |this, cx| { let answer = work.await;
    /// this.update(cx, |view, cx| ..) })` in one line.
    ///
    /// The task is the wait: `.detach()` it, or keep it beside the value it
    /// reads, where a newer one stored in its place cancels this one, so
    /// an older answer never lands over a newer one, and dropping it
    /// cancels the wait. A value kept whole in a [`Loadable`] is
    /// [`load`](Context::load)'s.
    pub fn land<T: 'static>(
        &mut self,
        work: impl Future<Output = Result<T, Error>> + 'static,
        land: impl FnOnce(&mut V, Result<T, Error>, &mut Context<V>) + 'static,
    ) -> Task<()> {
        self.spawn(async move |this, cx| {
            let result = work.await;
            // the view is gone: nothing is waiting for the answer
            let _ = this.update(cx, |view, cx| land(view, result, cx));
        })
    }
}

impl<V: View> Context<'_, V> {
    /// A refusal nothing on screen waits for, kept in the host's log under
    /// the view's [`NAME`](View::NAME): `<name>: <what> refused: <refusal>`.
    pub fn log_refused(&self, what: &str, refusal: &Error) {
        self.host()
            .log(format!("{}: {what} refused: {refusal}", V::NAME));
    }
}

#[cfg(test)]
mod follow_tests {
    use crate::methods::{self, Capability, Change, Changes, Program, Query};
    use crate::testing::{Probe, TestAppContext};
    use crate::wire::Event;
    use crate::{
        Context, IntoElement, Loadable, ParentElement, Render, Reread, Task, View, Window,
    };
    use serde::{Deserialize, Serialize};
    use std::{cell::Cell, rc::Rc};

    /// A block at `height` that wrote nothing a query declares.
    fn block(height: u64) -> Option<Change> {
        Some(Change {
            height,
            keys: Vec::new(),
        })
    }

    /// Counts the heads a program's live stream announces, for as long as
    /// the view runs.
    #[derive(Default, Serialize, Deserialize)]
    struct Heads {
        seen: usize,
    }
    impl View for Heads {
        const NAME: &'static str = "Heads";
        const CAPABILITIES: &'static [Capability] = &[Capability::Program, Capability::Host];
        fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            cx.follow::<Changes<Probe>>((), |view: &mut Heads, change, cx| match change {
                Ok(_) => {
                    view.seen += 1;
                    cx.notify();
                }
                Err(refusal) => cx.log_refused("the changes", &refusal),
            })
            .detach();
        }
    }
    impl Render for Heads {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::div().child(self.seen.to_string())
        }
    }

    /// Nothing in the view holds the follower: `detach` keeps it for as
    /// long as the view runs, and releasing the view unsubscribes.
    #[test]
    fn a_detached_follower_hears_every_item_until_the_view_is_released() {
        let mut cx = TestAppContext::new();
        let feed = cx.host().stream::<Changes<Probe>>();
        let view = cx.open::<Heads>();
        cx.run_until_parked();
        feed.send(None);
        feed.send(None);
        cx.run_until_parked();
        view.read(|heads| assert_eq!(heads.seen, 2));
        assert!(cx.has_text("2"), "the item that notifies is drawn");
        assert!(feed.subscribed(), "nothing dropped the stream");

        let mut driver = crate::Driver::<Heads>::new();
        let host = driver.app.host();
        driver.tick_with(Vec::new(), |_| ());
        assert!(host.drain_cancels().is_empty());
        drop(driver);
        assert_eq!(
            host.drain_cancels().len(),
            1,
            "releasing the view unsubscribed"
        );
    }

    /// Follows the same stream with the task kept, so the view can end it.
    #[derive(Default, Serialize, Deserialize)]
    struct Kept {
        seen: usize,
        #[serde(skip)]
        live: Option<Task<()>>,
    }
    impl View for Kept {
        const NAME: &'static str = "Kept";
        const CAPABILITIES: &'static [Capability] = &[Capability::Program];
        fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            self.live = Some(cx.follow::<Changes<Probe>>((), |view: &mut Kept, _, cx| {
                view.seen += 1;
                cx.notify();
            }));
        }
    }
    impl Render for Kept {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::div().child(self.seen.to_string())
        }
    }

    #[test]
    fn a_kept_follower_hears_every_item_until_its_task_is_dropped() {
        let mut cx = TestAppContext::new();
        let feed = cx.host().stream::<Changes<Probe>>();
        let view = cx.open::<Kept>();
        cx.run_until_parked();
        feed.send(None);
        feed.send(None);
        cx.run_until_parked();
        view.read(|heads| assert_eq!(heads.seen, 2));
        assert!(cx.has_text("2"), "the item that notifies is drawn");
        cx.update(&view, |heads, _, _| heads.live = None);
        cx.run_until_parked();
        assert!(!feed.subscribed(), "dropping the task unsubscribed");
    }

    /// The log line carries the view's own `NAME`: no call site spells it.
    #[test]
    fn log_refused_names_the_view() {
        let mut cx = TestAppContext::new();
        cx.host()
            .refuse::<Changes<Probe>>("gone", "the program left");
        cx.open::<Heads>();
        cx.run_until_parked();
        assert_eq!(
            cx.host().requests::<methods::HostLog>(),
            ["Heads: the changes refused: gone: the program left"]
        );
    }

    /// A program that keeps one number.
    struct Counter;
    impl Program for Counter {
        const NAME: &'static str = "counter";
        type Op = ();
        type Query = ();
        type Reply = u64;

        fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
            unreachable!()
        }

        fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
            unreachable!()
        }
    }

    /// Shows the counter, read again on each of its heads.
    #[derive(Default, Serialize, Deserialize)]
    struct Count {
        value: Loadable<u64>,
        #[serde(skip)]
        live: Option<Task<()>>,
        #[serde(skip)]
        read: Option<Task<()>>,
    }
    impl View for Count {
        const NAME: &'static str = "Count";
        const CAPABILITIES: &'static [Capability] = &[Capability::Program];
        fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            self.live =
                Some(cx.follow::<Changes<Counter>>((), |view: &mut Count, _, cx| view.read(cx)));
            self.read(cx);
        }
    }
    impl Count {
        fn read(&mut self, cx: &mut Context<Self>) {
            let ask = cx.host().ask::<Query<Counter>>(());
            cx.load(self, ask, |view| &mut view.value);
        }
    }
    impl Render for Count {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::div().child(match &self.value {
                Loadable::Ready(value) | Loadable::Reloading(value, _) => value.to_string(),
                Loadable::Failed(refusal) => refusal.message.clone(),
                Loadable::Idle | Loadable::Loading(_) => "…".to_owned(),
            })
        }
    }

    /// A head whose re-read lands the number already on screen draws
    /// nothing: neither the item nor the landing renders. A head that moves
    /// the number draws it once.
    #[test]
    fn a_head_whose_reread_lands_the_same_value_draws_nothing() {
        let mut cx = TestAppContext::new();
        let feed = cx.host().stream::<Changes<Counter>>();
        let count = Rc::new(Cell::new(5u64));
        let answer = count.clone();
        cx.host()
            .handle::<Query<Counter>>(move |()| Ok(answer.get()));
        cx.open::<Count>();
        assert!(cx.has_text("5"), "{:?}", cx.texts());
        let renders = cx.renders();
        feed.send(block(1));
        cx.run_until_parked();
        assert_eq!(
            cx.host().requests::<Query<Counter>>().len(),
            2,
            "the head re-read"
        );
        assert_eq!(cx.renders() - renders, 0, "the same count drew nothing");
        count.set(6);
        feed.send(block(2));
        cx.run_until_parked();
        assert!(cx.has_text("6"), "{:?}", cx.texts());
        assert_eq!(cx.renders() - renders, 1, "a moved count draws once");
    }

    /// The id of the counter read the last frame asked.
    fn asked(cx: &TestAppContext) -> u64 {
        let asks = cx.last_frame().requests.iter();
        let mut reads = asks.filter(|request| request.kind == "program.query");
        reads.next().expect("the frame asked the counter").id
    }

    fn answer(id: u64, value: u64) -> Event {
        Event::Response {
            id,
            result: Ok(methods::encode(&value)),
            done: true,
        }
    }

    /// The one rule of `load`: a value on screen stays there, `Reloading`,
    /// until the answer lands, and asking again draws nothing.
    #[test]
    fn a_load_on_a_ready_slot_keeps_it_shown_until_the_answer_lands() {
        let mut cx = TestAppContext::new();
        cx.host().handle::<Query<Counter>>(|()| Ok(5));
        let view = cx.open::<Count>();
        assert!(cx.has_text("5"), "{:?}", cx.texts());
        cx.host().never::<Query<Counter>>();
        let renders = cx.renders();
        cx.update(&view, |view, _, cx| view.read(cx));
        cx.tick(vec![]);
        view.read(|view| {
            assert!(
                matches!(view.value, Loadable::Reloading(5, _)),
                "{:?}",
                view.value
            )
        });
        assert!(cx.has_text("5"), "the value stays on screen");
        assert_eq!(cx.renders(), renders, "asking again drew nothing");
        let id = asked(&cx);
        cx.tick(vec![answer(id, 6)]);
        view.read(|view| assert!(matches!(view.value, Loadable::Ready(6)), "{:?}", view.value));
        assert!(cx.has_text("6"), "{:?}", cx.texts());
        assert_eq!(cx.renders(), renders + 1, "the new value draws once");
    }

    /// A slot with nothing to show says it is loading, and the view renders
    /// for that once: the caller writes no `cx.notify()` beside the `load`.
    /// A slot the view blanks first (`Loadable::Idle`, a line of its own)
    /// is such a slot.
    #[test]
    fn a_load_on_an_idle_slot_shows_loading_and_notifies_once() {
        let mut cx = TestAppContext::new();
        cx.host().handle::<Query<Counter>>(|()| Ok(5));
        let view = cx.open::<Count>();
        assert!(cx.has_text("5"), "{:?}", cx.texts());
        cx.host().never::<Query<Counter>>();
        let renders = cx.renders();
        cx.update(&view, |view, _, cx| {
            view.value = Loadable::Idle;
            view.read(cx);
        });
        cx.tick(vec![]);
        view.read(|view| assert!(view.value.is_loading(), "{:?}", view.value));
        assert!(cx.has_text("…"), "{:?}", cx.texts());
        assert_eq!(cx.renders(), renders + 1, "the blanked slot drew once");
        let id = asked(&cx);
        cx.tick(vec![answer(id, 5)]);
        assert!(cx.has_text("5"), "{:?}", cx.texts());
        assert_eq!(cx.renders(), renders + 2, "and its answer once");
    }

    /// The view declares no `host`: a refused re-read reaches the view
    /// itself, in the slot it re-read, not a log it cannot write.
    #[test]
    fn a_refused_load_lands_its_refusal_in_the_view() {
        let mut cx = TestAppContext::new();
        let feed = cx.host().stream::<Changes<Counter>>();
        cx.host().handle::<Query<Counter>>(|()| Ok(5));
        let view = cx.open::<Count>();
        cx.host()
            .refuse::<Query<Counter>>("stale", "the counter moved on");
        feed.send(block(1));
        cx.run_until_parked();
        view.read(|view| {
            let refusal = view.value.failed().expect("the refusal landed");
            assert_eq!(refusal.message, "the counter moved on");
        });
        assert!(cx.has_text("the counter moved on"), "{:?}", cx.texts());
    }

    /// Two reads of one value are out; the host answers the newer first
    /// (the older was retrying a transport failure). The older answer must
    /// not land over the newer one: in a `load` slot the newer read
    /// cancelled it, and so does a `land` task stored in its place.
    #[test]
    fn a_late_answer_never_lands_over_a_newer_one() {
        let mut cx = TestAppContext::new();
        cx.host().never::<Query<Counter>>();
        let entity = cx.open::<Count>();
        let query = |cx: &TestAppContext| -> Vec<u64> {
            cx.last_frame()
                .requests
                .iter()
                .filter(|request| request.kind == "program.query")
                .map(|request| request.id)
                .collect()
        };
        let answer = |id: u64, value: u64| Event::Response {
            id,
            result: Ok(methods::encode(&value)),
            done: true,
        };
        let first = query(&cx);
        cx.tick(vec![answer(first[0], 0)]);
        // a load and a land go out, then a newer pair replaces them
        let read = |cx: &mut TestAppContext| {
            cx.update(&entity, |view, _, cx| {
                view.read(cx);
                let ask = cx.host().ask::<Query<Counter>>(());
                view.read = Some(cx.land(ask, |view, value, cx| {
                    view.value = Loadable::from(value.map(|value| value * 10));
                    cx.notify();
                }));
            });
            cx.tick(vec![]);
            query(cx)
        };
        let older = read(&mut cx);
        let newer = read(&mut cx);
        assert_eq!((older.len(), newer.len()), (2, 2), "{older:?} {newer:?}");
        // the loads: the newer lands, then the older
        cx.tick(vec![answer(newer[0], 2), answer(older[0], 1)]);
        entity.read(|view| assert_eq!(view.value.ready(), Some(&2), "{:?}", view.value));
        // the lands: the newer lands, then the older
        cx.tick(vec![answer(newer[1], 4), answer(older[1], 3)]);
        entity.read(|view| assert_eq!(view.value.ready(), Some(&40), "{:?}", view.value));
    }

    // Shows the counter in a `Reread` slot, read again whenever the view hears it may have
    // moved.
    #[derive(Default, Serialize, Deserialize)]
    struct Recount {
        value: Reread<u64>,
    }
    impl View for Recount {
        const NAME: &'static str = "Recount";
        const CAPABILITIES: &'static [Capability] = &[Capability::Program];
        fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
            self.read(cx);
        }
    }
    impl Recount {
        fn read(&mut self, cx: &mut Context<Self>) {
            cx.reread(
                self,
                |view| &mut view.value,
                |_, cx| cx.host().ask::<Query<Counter>>(()),
                Self::read,
            );
        }
    }
    impl Render for Recount {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            crate::div().child(match self.value.ready() {
                Some(value) => value.to_string(),
                None => "…".to_owned(),
            })
        }
    }

    // A network that writes every block asks for a read while one is out, again and again: the
    // read out lands what it read, and one more read follows it, however many asks came in
    // meanwhile. A read replaced for each ask would never land.
    #[test]
    fn a_reread_asked_while_one_is_out_lands_it_then_reads_once_more() {
        let mut cx = TestAppContext::new();
        cx.host().never::<Query<Counter>>();
        let view = cx.open::<Recount>();
        let first = asked(&cx);
        for _ in 0..3 {
            cx.update(&view, |view, _, cx| view.read(cx));
            cx.tick(vec![]);
        }
        assert_eq!(
            cx.host().requests::<Query<Counter>>().len(),
            1,
            "the read out is kept"
        );
        cx.host().handle::<Query<Counter>>(|()| Ok(6));
        cx.tick(vec![answer(first, 5)]);
        view.read(|view| assert_eq!(view.value.ready(), Some(&5), "{:?}", view.value));
        cx.run_until_parked();
        assert!(cx.has_text("6"), "{:?}", cx.texts());
        assert_eq!(
            cx.host().requests::<Query<Counter>>().len(),
            2,
            "one more read, and no other"
        );
    }
}
