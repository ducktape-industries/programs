//! A program's NOTIFIER: the headless half of its view. The host runs one
//! per program per network, from the moment the network is connected,
//! whether or not a pane shows the view, and never per pane: what becomes a
//! notice, for whom, in what words, under what tag and link, and when it is
//! seen, is the program's to decide here, in its own crate. The host only
//! carries it (`notify.post`, `notify.seen`, `host.badge`).
//!
//! A notifier is the view's own crate built with its `notifier` feature: a
//! wasm module of its own, with no element tree, no layout and no input,
//! which a founding packs into the program's code as the `ducktape.notify`
//! section beside the view. It is written as a view is, on the same
//! [`Context`]: `cx.follow::<Changes<P>>` for what it watches,
//! `cx.host().query(..)` and `cx.land(..)` for what it reads,
//! `store::get`/`store::set` for what it keeps, and `cx.host().notify::<NotifyPost>(..)`
//! for what it posts.
//!
//! ```
//! use view::prelude::*;
//! use view::{Notifier, testing::TestNotifier};
//!
//! /// Posts one notice per session it is handed.
//! struct Hello;
//! impl Notifier for Hello {
//!     const NAME: &'static str = "Hello";
//!     const CAPABILITIES: &'static [Capability] = &[Capability::Host, Capability::Notify];
//!     fn new(cx: &mut Context<Self>) -> Self {
//!         cx.follow::<HostSession>((), |_, session, cx| {
//!             if let Ok(session) = session {
//!                 cx.host().notify::<NotifyPost>(Notification {
//!                     title: format!("hello, {}", session.signer),
//!                     ..Notification::default()
//!                 });
//!             }
//!         })
//!         .detach();
//!         Hello
//!     }
//! }
//! # view::export_notifier!(Hello);
//!
//! let mut cx = TestNotifier::<Hello>::new();
//! let sessions = cx.host().stream::<HostSession>();
//! cx.start();
//! sessions.send(Session { signer: "0102".into(), ..Session::default() });
//! cx.run_until_parked();
//! assert_eq!(cx.host().requests::<NotifyPost>()[0].title, "hello, 0102");
//! ```
use crate::methods::Capability;
use crate::wire::notifier::{Answer, Turn};
use crate::{App, Context, Entity, executor};

// A program's notifier: everything the host reads about it (the manifest
// `export_notifier!` writes: [`NAME`](Notifier::NAME),
// [`CAPABILITIES`](Notifier::CAPABILITIES))
// and how it starts ([`new`](Notifier::new)).
pub trait Notifier: 'static + Sized {
    /// The name its notices are logged under, as a view's tab is named:
    /// the view's own [`View::NAME`](crate::View::NAME), so the person
    /// reads one name for the program.
    const NAME: &'static str;
    /// The `<capability>` halves of the method kinds it asks through, as a
    /// view's [`View::CAPABILITIES`](crate::View::CAPABILITIES). The host
    /// starts no notifier that declares `op`, `invite`, `clipboard`, `link`
    /// or `web`: what runs with no one looking signs nothing and opens
    /// nothing.
    const CAPABILITIES: &'static [Capability] = &[];
    /// Starts it: its state, and every follower and read it keeps running.
    /// Called once per run; a new deployment, a reconnect or a relaunch
    /// starts a new one, which finds what it kept in its store.
    fn new(cx: &mut Context<Self>) -> Self;
}

/// One notifier's run loop: the wasm exports drive it on the host, and
/// [`TestNotifier`](crate::testing::TestNotifier) drives it in a test.
pub(crate) struct NotifierDriver<N: Notifier> {
    pub(crate) app: App,
    pub(crate) entity: Entity<N>,
}

impl<N: Notifier> Drop for NotifierDriver<N> {
    fn drop(&mut self) {
        self.app.inner.alive.set(false);
        self.app.inner.tasks.borrow_mut().clear();
    }
}

impl<N: Notifier> NotifierDriver<N> {
    pub(crate) fn new() -> Self {
        let mut app = App::for_driver();
        let entity = Entity::reserve(&app);
        app.update(|app| {
            let value = N::new(&mut Context {
                app,
                entity: entity.clone(),
            });
            *entity.value.borrow_mut() = Some(value);
        });
        Self { app, entity }
    }

    /// One tick: what the host owes it, in order, each settled before the
    /// next, as a view's tick runs its events; then what it asked.
    pub(crate) fn tick(&mut self, answers: Vec<Answer>) -> Turn {
        executor::begin_tick(&mut self.app.inner.tasks.borrow_mut());
        let mut busy = self.app.settle();
        for Answer { id, result, done } in answers {
            self.app.host().fulfill(id, result, done);
            busy |= self.app.settle();
        }
        busy |= self.app.settle() || executor::ready(&self.app.inner.tasks.borrow());
        let host = self.app.host();
        Turn {
            requests: host.drain_outbox(),
            cancels: host.drain_cancels(),
            busy,
        }
    }
}

/// The manifest section and the wasm32 exports ([`crate::wire::notifier`])
/// for a notifier, from what its [`Notifier`] impl says:
/// `export_notifier!(ChatNotices);`. A crate exports a notifier in the
/// build its `notifier` feature turns on and a view in the other.
#[macro_export]
macro_rules! export_notifier {
    ($notifier:ty) => {
        const NOTIFIER_MANIFEST_LEN: usize =
            $crate::Manifested::of_notifier::<$notifier>().encoded_len();

        #[cfg_attr(
            target_arch = "wasm32",
            unsafe(link_section = "ducktape.view.manifest")
        )]
        #[used]
        static NOTIFIER_MANIFEST: [u8; NOTIFIER_MANIFEST_LEN] =
            $crate::Manifested::of_notifier::<$notifier>().bytes();

        #[cfg(target_arch = "wasm32")]
        mod notifier_exports {
            use super::*;

            #[unsafe(export_name = "alloc")]
            extern "C" fn alloc(len: u32) -> u32 {
                $crate::exports::alloc(len)
            }

            #[unsafe(export_name = "init")]
            extern "C" fn init() {
                $crate::exports::notifier_init::<$notifier>()
            }

            #[unsafe(export_name = "tick")]
            extern "C" fn tick(ptr: u32, len: u32) -> u64 {
                $crate::exports::notifier_tick::<$notifier>(ptr, len)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::prelude::*;
    use crate::testing::TestNotifier;
    use crate::{Manifested, Notifier};

    /// Counts the blocks of a program and posts one notice per block.
    struct Counter {
        seen: u64,
    }
    impl Notifier for Counter {
        const NAME: &'static str = "Counter";
        const CAPABILITIES: &'static [Capability] = &[Capability::Program, Capability::Notify];
        fn new(cx: &mut Context<Self>) -> Self {
            cx.follow::<Changes<crate::testing::Probe>>((), |counter, change, cx| {
                if let Ok(Some(change)) = change {
                    counter.seen += 1;
                    cx.host().notify::<NotifyPost>(Notification {
                        title: format!("block {}", change.height),
                        ..Notification::default()
                    });
                }
            })
            .detach();
            Self { seen: 0 }
        }
    }

    #[test]
    fn a_notifier_follows_what_it_names_and_posts_with_no_tree() {
        let mut cx = TestNotifier::<Counter>::new();
        let blocks = cx.host().stream::<Changes<crate::testing::Probe>>();
        let notices = cx.start();
        for height in [4, 5] {
            blocks.send(Some(Change {
                height,
                keys: Vec::new(),
            }));
        }
        cx.run_until_parked();
        let titles: Vec<String> = cx
            .host()
            .requests::<NotifyPost>()
            .into_iter()
            .map(|post| post.title)
            .collect();
        assert_eq!(titles, ["block 4", "block 5"]);
        notices.read(|counter| assert_eq!(counter.seen, 2));
        // stopped, as on a disconnect: it follows nothing any more
        cx.stop();
        assert!(!blocks.subscribed());
    }

    /// Rings once per session it is handed.
    struct Ring;
    impl Notifier for Ring {
        const NAME: &'static str = "Ring";
        const CAPABILITIES: &'static [Capability] = &[Capability::Host, Capability::Notify];
        fn new(cx: &mut Context<Self>) -> Self {
            cx.follow::<HostSession>((), |_, session, cx| {
                if session.is_ok() {
                    cx.host().notify::<NotifyPost>(ring());
                }
            })
            .detach();
            Ring
        }
    }

    fn ring() -> Notification {
        Notification {
            title: "alice is calling".into(),
            tag: "call/c-1".into(),
            link: "duck://local/huddle/call/c-1".into(),
            kind: NoticeKind::Call,
            until: 1_700_000_045,
            end_link: "duck://local/huddle/call/c-1/leave".into(),
            ..Notification::default()
        }
    }

    /// A ring crosses as it was posted: its kind, its end and its hang-up
    /// link with it.
    #[test]
    fn a_ring_reaches_the_host_with_its_kind_and_its_end() {
        let mut cx = TestNotifier::<Ring>::new();
        let sessions = cx.host().stream::<HostSession>();
        cx.start();
        sessions.send(Session::default());
        cx.run_until_parked();
        assert_eq!(cx.host().requests::<NotifyPost>(), [ring()]);
    }

    // The notifier's manifest is a view's: the host reads its name,
    // capabilities with the same parser, at the wire it speaks.
    #[test]
    fn a_notifiers_manifest_reads_as_a_views() {
        const LEN: usize = Manifested::of_notifier::<Counter>().encoded_len();
        let bytes: [u8; LEN] = Manifested::of_notifier::<Counter>().bytes();
        let text = std::str::from_utf8(&bytes).unwrap();
        let manifest = crate::wire::manifest::Manifest::parse(text).unwrap();
        assert_eq!(manifest.name, "Counter");
        assert_eq!(
            manifest.capabilities,
            [Capability::Program, Capability::Notify]
        );
        assert!(manifest.targets.is_empty());
        assert_eq!(manifest.level(), crate::wire::WIRE_LEVEL);
        // it draws nothing: no tint and no window width, written as such
        assert!(text.ends_with("\nnone\n"), "{text:?}");
        assert_eq!(manifest.tint, crate::Tint::None);
        assert_eq!(manifest.width, None);
    }
}
