use super::FakeHost;
use crate::wire::{Event, Frame, notifier::Answer};
use crate::{Context, Entity, Notifier, NotifierDriver};

/// One [`Notifier`] and its [`FakeHost`], driven as the app drives it:
/// [`start`](Self::start) runs it as a connect does, every tick hands it
/// what the host owes, and [`stop`](Self::stop) ends it as a disconnect or
/// a quit does. Its requests are held to its manifest, as a view's are.
pub struct TestNotifier<N: Notifier> {
    host: FakeHost,
    driver: Option<NotifierDriver<N>>,
    ticks: u64,
}

impl<N: Notifier> Default for TestNotifier<N> {
    fn default() -> Self {
        Self {
            host: FakeHost::default(),
            driver: None,
            ticks: 0,
        }
    }
}

impl<N: Notifier> TestNotifier<N> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn host(&self) -> FakeHost {
        self.host.clone()
    }

    /// Starts a new run of the notifier, as the app does when a network is
    /// connected (a run before it ended first), and ticks until it parks.
    pub fn start(&mut self) -> Entity<N> {
        self.stop();
        self.host.declare(N::CAPABILITIES);
        let driver = NotifierDriver::<N>::new();
        let entity = driver.entity.clone();
        self.driver = Some(driver);
        self.ticks = 0;
        self.run_until_parked();
        entity
    }

    // Ends the run, as a disconnect, a new deployment or a quit does: what
    // it followed is unsubscribed, and what the host owed it is dropped.
    // Its store is the host's (its own, or the test's handlers), which
    // outlives it.
    pub fn stop(&mut self) {
        if self.driver.take().is_some() {
            self.host.reset_connection();
        }
    }

    /// How many ticks this run took.
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Ticks until the notifier parks: no answer owed and nothing ready.
    pub fn run_until_parked(&mut self) {
        loop {
            let owed: Vec<Answer> = self
                .host
                .take_events()
                .into_iter()
                .filter_map(|event| match event {
                    Event::Response { id, result, done } => Some(Answer { id, result, done }),
                    _ => None,
                })
                .collect();
            let driver = self.driver.as_mut().expect("start the notifier first");
            let turn = driver.tick(owed);
            self.ticks += 1;
            let frame = Frame {
                requests: turn.requests,
                cancels: turn.cancels,
                busy: turn.busy,
                unchanged: true,
                ..Frame::default()
            };
            self.host.accept(&frame, &driver.app.host(), None);
            if !turn.busy && !self.host.owes_events() {
                return;
            }
        }
    }

    /// Runs `f` on the notifier between ticks; what it asks crosses on the
    /// next tick, which this does not run.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut N, &mut Context<N>) -> R) -> R {
        let driver = self.driver.as_mut().expect("start the notifier first");
        let entity = driver.entity.clone();
        entity.update(&mut driver.app, f)
    }
}
