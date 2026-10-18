use crate::testing::TestAppContext;
use crate::{Context, InteractiveElement, ParentElement, Render, Task, View, Window};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::methods::{Change, Changes};
use crate::testing::Probe;

#[derive(Default, Serialize, Deserialize)]
struct Streams {
    values: Vec<u64>,
    pending_after_item: bool,
    #[serde(skip)]
    /// Keeps the stream task alive.
    _task: Option<Task<()>>,
}
impl View for Streams {
    const NAME: &'static str = "Streams";
    const CAPABILITIES: &'static [crate::methods::Capability] =
        &[crate::methods::Capability::Program];
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut stream = cx.host().subscribe::<Changes<Probe>>(());
        let task = cx.spawn(async move |this, cx| {
            while let Some(value) = stream.next().await {
                let pending = this
                    .update(cx, |view, cx| {
                        view.values.push(value.unwrap().unwrap().height);
                        cx.notify();
                        view.pending_after_item
                    })
                    .unwrap();
                if pending {
                    futures::future::pending::<()>().await;
                }
            }
        });
        Self {
            _task: Some(task),
            ..Self::default()
        }
    }
}
impl Render for Streams {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
            .id("values")
            .child(self.values.len().to_string())
    }
}

fn opened() -> (TestAppContext, crate::Entity<Streams>) {
    let mut cx = TestAppContext::new();
    let streams = cx.open::<Streams>();
    (cx, streams)
}

#[test]
fn snapshots_allow_parked_streams_and_reject_ordinary_pending_futures() {
    let (mut cx, _) = opened();
    assert!(cx.snapshot().is_ok());
    let task = cx
        .app_mut()
        .spawn(async |_| futures::future::pending::<()>().await);
    assert!(cx.snapshot().is_err(), "unpolled work is not quiescent");
    cx.tick(vec![]);
    assert!(cx.snapshot().is_err());
    drop(task);
    cx.tick(vec![]);
    assert!(cx.snapshot().is_ok());
    assert!(cx.snapshot().is_ok());
}

#[test]
fn a_stream_task_awaiting_other_work_is_not_safe_to_snapshot() {
    let (mut cx, streams) = opened();
    let feed = cx.host().stream::<Changes<Probe>>();
    cx.update(&streams, |view, _, cx| {
        view.pending_after_item = true;
        cx.notify();
    });
    feed.send(Some(Change {
        height: 1,
        keys: Vec::new(),
    }));
    cx.tick(vec![]);
    streams.read(|view| assert_eq!(view.values, [1]));
    assert!(cx.snapshot().is_err());
}

#[test]
fn a_hot_stream_delivers_every_queued_item_in_order_in_one_tick() {
    let (mut cx, streams) = opened();
    let id = cx.last_frame().requests[0].id;
    let host = cx.app_mut().host();
    for value in 0..1000u64 {
        let block = Some(Change {
            height: value,
            keys: Vec::new(),
        });
        host.fulfill(id, Ok(crate::methods::encode(&block)), false);
    }
    assert!(!cx.tick(vec![]).busy);
    streams.read(|view| assert_eq!(view.values, (0..1000).collect::<Vec<_>>()));
    assert!(cx.snapshot().is_ok());
    host.close_stream(id);
    cx.tick(vec![]);
    assert!(cx.app_mut().inner.tasks.borrow().is_empty());
}
