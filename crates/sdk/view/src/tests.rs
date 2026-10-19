use super::*;
use futures::StreamExt;
use gpui::{Image, ImageFormat};
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use crate::testing::TestAppContext;
use crate::wire::Event;

#[derive(Default, Serialize, Deserialize)]
struct Probe {
    received: Vec<String>,
    #[serde(skip)]
    streams: Vec<Task<()>>,
}
impl Probe {
    fn watch(&mut self, cx: &mut Context<Self>, kind: &'static str) {
        let mut stream = cx.host().raw_subscribe(kind, &[]);
        self.streams.push(cx.spawn(async move |this, cx| {
            while let Some(reply) = stream.next().await {
                reply.unwrap();
                if this
                    .update(cx, |view, cx| {
                        view.received.push(kind.into());
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        }));
    }
}
impl View for Probe {
    const NAME: &'static str = "Probe";
    const CAPABILITIES: &'static [wire::methods::Capability] = &[wire::methods::Capability::Host];
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.watch(cx, "visible");
        self.watch(cx, "data");
    }
}
impl Render for Probe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let press = cx.listener(|view, _: &ClickEvent, _, cx| {
            view.received.push("press".into());
            cx.notify();
        });
        div()
            .id("press")
            .role(crate::Role::Button)
            .focusable()
            .on_click(press)
            .child(self.received.len().to_string())
    }
}
fn response(id: u64, done: bool) -> wire::Event {
    wire::Event::Response {
        id,
        result: Ok(Vec::new()),
        done,
    }
}
fn opened<V: View>() -> (TestAppContext, Entity<V>) {
    let mut cx = TestAppContext::new();
    let view = cx.open::<V>();
    (cx, view)
}

#[test]
fn held_streams_remain_open_and_dropping_tasks_cancels_them() {
    let (mut cx, probe) = opened::<Probe>();
    let first = cx.last_frame().requests.clone();
    assert_eq!(first.len(), 2);
    let id = first[0].id;
    cx.tick(vec![]);
    assert!(cx.last_frame().requests.is_empty());
    cx.tick(vec![response(id, false)]);
    probe.read(|view| assert_eq!(view.received, ["visible"]));
    assert!(
        cx.snapshot().is_ok(),
        "held streams can be restored independently"
    );
    cx.update(&probe, |view, _, cx| {
        view.streams.clear();
        view.watch(cx, "replacement");
    });
    cx.tick(vec![]);
    let frame = cx.last_frame();
    assert_eq!(frame.cancels.len(), 2);
    assert!(frame.cancels.contains(&id));
    assert_eq!(frame.requests.len(), 1);
    assert_eq!(frame.requests[0].kind, "replacement");
}

#[test]
fn snapshot_refuses_unsettled_writes_and_failed_restore_keeps_old_view() {
    use wire::methods::{HostId, Method};
    let (mut cx, _) = opened::<Probe>();
    cx.host().never::<HostId>();
    cx.app_mut()
        .spawn(async move |cx| {
            cx.host().ask::<HostId>("write".into()).await.unwrap();
        })
        .detach();
    cx.tick(vec![]);
    assert!(cx.snapshot().is_err());
    let write = cx
        .last_frame()
        .requests
        .iter()
        .find(|request| request.kind == HostId::KIND)
        .unwrap()
        .id;
    cx.tick(vec![Event::Response {
        id: write,
        result: Ok(HostId::encode_reply(&"write-1".into())),
        done: true,
    }]);
    let snapshot = cx.snapshot().unwrap();
    assert!(cx.restore::<Probe>(&[]).is_err());
    assert_eq!(cx.snapshot().unwrap(), snapshot);
    cx.restore::<Probe>(&snapshot).unwrap();
    assert_eq!(cx.last_frame().requests.len(), 2);
}

#[test]
fn stream_updates_follow_response_order_instead_of_spawn_order() {
    let (mut cx, probe) = opened::<Probe>();
    let requests = cx.last_frame().requests.clone();
    for order in [["data", "visible"], ["visible", "data"]] {
        cx.update(&probe, |view, _, cx| {
            view.received.clear();
            cx.notify();
        });
        let events = order
            .iter()
            .map(|kind| {
                response(
                    requests
                        .iter()
                        .find(|request| request.kind == *kind)
                        .unwrap()
                        .id,
                    false,
                )
            })
            .collect();
        cx.tick(events);
        probe.read(|view| assert_eq!(view.received, order));
    }
}

#[test]
fn unchanged_frames_preserve_listener_tables_and_resync_renders() {
    let (mut cx, probe) = opened::<Probe>();
    assert!(cx.last_frame().root.is_some());
    cx.tick(vec![]);
    let renders = cx.renders();
    for _ in 0..3 {
        cx.tick(vec![]);
        assert!(cx.last_frame().unchanged);
    }
    assert_eq!(cx.renders(), renders);
    cx.simulate_click("press");
    assert!(cx.reports()[0].rendered && cx.reports()[0].patches > 0);
    probe.read(|view| assert_eq!(view.received, ["press"]));
    let renders = cx.renders();
    cx.tick(vec![Event::Resync]);
    assert!(cx.last_frame().root.is_some());
    assert_eq!(cx.renders(), renders + 1);
}

#[test]
fn two_views_on_one_thread_have_independent_hosts() {
    let (mut first, first_probe) = opened::<Probe>();
    let (mut second, second_probe) = opened::<Probe>();
    let first_requests = first.last_frame().requests.clone();
    let second_requests = second.last_frame().requests.clone();
    assert_eq!(first_requests.len(), 2);
    assert_eq!(second_requests.len(), 2);
    assert_eq!(
        first_requests[0].id, second_requests[0].id,
        "each host starts its own ID sequence"
    );
    first.tick(vec![response(first_requests[0].id, false)]);
    second.tick(vec![]);
    first_probe.read(|view| assert_eq!(view.received, ["visible"]));
    second_probe.read(|view| assert!(view.received.is_empty()));
    first.app_mut().host().log("only first");
    second.tick(vec![]);
    assert!(second.last_frame().requests.is_empty());
    first.tick(vec![]);
    let frame = first.last_frame();
    assert_eq!(frame.requests.len(), 1);
    assert_eq!(frame.requests[0].payload, methods::encode(&"only first"));
}

#[test]
fn tasks_are_awaitable_drop_cancels_and_detach_runs() {
    let (mut cx, _) = opened::<Probe>();
    let app = cx.app_mut();
    let order = Rc::new(RefCell::new(Vec::new()));
    let child_order = order.clone();
    let child = app.spawn(async move |_| {
        child_order.borrow_mut().push(1);
        3
    });
    let parent_order = order.clone();
    let parent = app.spawn(async move |_| {
        let value = child.await;
        parent_order.borrow_mut().push(value);
        value + 1
    });
    let canceled_order = order.clone();
    drop(app.spawn(async move |_| {
        canceled_order.borrow_mut().push(99);
    }));
    let detached_order = order.clone();
    app.spawn(async move |_| {
        detached_order.borrow_mut().push(5);
    })
    .detach();
    cx.tick(vec![]);
    assert_eq!(futures::executor::block_on(parent), 4);
    assert_eq!(*order.borrow(), vec![1, 3, 5]);
}

#[derive(Default, Serialize, Deserialize)]
struct UniformProbe;

impl View for UniformProbe {
    const NAME: &'static str = "UniformProbe";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for UniformProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        uniform_list("rows", 2_000, |range, _, _| {
            range
                .map(|index| {
                    div()
                        .id(format!("row-{index}"))
                        .child(format!("row {index}"))
                })
                .collect::<Vec<_>>()
        })
    }
}

fn uniform_rows(cx: &TestAppContext) -> (usize, Vec<u32>, usize) {
    match cx.root() {
        wire::Node::UniformList {
            count,
            indices,
            children,
            ..
        } => (*count, indices.clone(), children.len()),
        node => panic!("expected uniform list, got {node:?}"),
    }
}

/// A uniform list's first frame holds a screenful: the rows that fill the
/// viewport the host sent ahead of the tick, at the design system's row
/// height until the host has measured one, and a margin past them. The
/// host's first range, inside that window, changes nothing; a range past
/// it moves the window there, margin and all; a request the window holds
/// draws nothing new.
#[test]
fn uniform_list_lowers_a_screenful_before_the_host_asks_and_grows_to_what_it_shows() {
    let (mut cx, _) = opened::<UniformProbe>();
    let (count, indices, children) = uniform_rows(&cx);
    assert_eq!(count, 2_000);
    let row = f32::from(crate::design::size::ROW);
    let rows = (crate::testing::VIEWPORT.1 / row).ceil() as usize;
    let margin = crate::list::MARGIN_ROWS as u32;
    let first: Vec<u32> = (0..rows as u32 + margin).collect();
    assert_eq!(indices, first, "the viewport's rows and a margin past them");
    assert_eq!(children, indices.len());
    let renders = cx.renders();
    cx.simulate_range("rows", 0..rows);
    assert!(cx.last_frame().unchanged, "the rows shown are held already");
    assert_eq!(cx.renders(), renders);

    cx.simulate_range("rows", 1_000..1_020);
    let (_, indices, children) = uniform_rows(&cx);
    assert_eq!(children, indices.len());
    assert_eq!(
        indices.first(),
        Some(&0),
        "the measurement row is always sent"
    );
    assert_eq!(
        &indices[1..],
        (1_000 - margin..1_020 + margin).collect::<Vec<_>>(),
        "the rows shown with a margin past each edge"
    );

    cx.simulate_range("rows", 1_005..1_025);
    assert!(
        cx.last_frame().unchanged,
        "a range inside the window does not rerender"
    );

    cx.simulate_range("rows", 0..u32::MAX as usize);
    let (_, indices, _) = uniform_rows(&cx);
    assert_eq!(indices.len(), 2_000);
    assert_eq!(
        indices.last().copied(),
        Some(1_999),
        "the window's last row is sent, the measurement row inside it"
    );
}

thread_local! {
    /// The rows [`CountedRows`] painted, in order.
    static PAINTED: std::cell::RefCell<Vec<usize>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Default, Serialize, Deserialize)]
struct CountedRows;

impl View for CountedRows {
    const NAME: &'static str = "CountedRows";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}

impl Render for CountedRows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        uniform_list("rows", 2_000, |range, _, _| {
            range
                .map(|index| {
                    PAINTED.with(|painted| painted.borrow_mut().push(index));
                    div()
                        .id(format!("row-{index}"))
                        .child(format!("row {index}"))
                })
                .collect::<Vec<_>>()
        })
    }
}

/// Each row a frame sends is painted once: the measurement row, which is
/// sent whatever the window, is painted on its own only when the window
/// does not hold it (a list at its top painted its first row twice a
/// render, the copy thrown away).
#[test]
fn a_uniform_list_paints_each_row_it_sends_once() {
    let painted = || PAINTED.with(|painted| std::mem::take(&mut *painted.borrow_mut()));
    painted();
    let (mut cx, _) = opened::<CountedRows>();
    let rows = painted();
    let (_, indices, _) = uniform_rows(&cx);
    assert_eq!(
        rows,
        indices
            .iter()
            .map(|index| *index as usize)
            .collect::<Vec<_>>()
    );
    cx.simulate_range("rows", 1_000..1_020);
    let rows = painted();
    let (_, indices, _) = uniform_rows(&cx);
    assert_eq!(rows.iter().filter(|index| **index == 0).count(), 1);
    assert_eq!(
        rows.len(),
        indices.len(),
        "far down: the window and the measurement row"
    );
}

/// A filter leaves one row and is cleared: the rows that came back are in
/// the frame that shows them, a screenful of them, with no host ask
/// between.
#[test]
fn a_uniform_list_whose_rows_come_back_holds_a_screenful_again() {
    #[derive(Default, Serialize, Deserialize)]
    struct Filtered {
        count: usize,
    }
    impl View for Filtered {
        const NAME: &'static str = "Filtered";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self { count: 300 }
        }
    }
    impl Render for Filtered {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            uniform_list("rows", self.count, |range, _, _| {
                range
                    .map(|index| {
                        div()
                            .id(format!("row-{index}"))
                            .child(format!("row {index}"))
                    })
                    .collect::<Vec<_>>()
            })
        }
    }
    let (mut cx, view) = opened::<Filtered>();
    let (_, before, _) = uniform_rows(&cx);
    assert!(before.len() > 20);
    cx.update(&view, |view, _, cx| {
        view.count = 1;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(uniform_rows(&cx).1, [0]);
    cx.update(&view, |view, _, cx| {
        view.count = 300;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(uniform_rows(&cx).1, before);
}

/// A list of 2,000 rows that tracks its scroll.
#[derive(Default, Serialize, Deserialize)]
struct Scrolled {
    #[serde(skip)]
    scroll: UniformListScrollHandle,
}

impl View for Scrolled {
    const NAME: &'static str = "Scrolled";
}

impl Render for Scrolled {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        uniform_list("rows", 2_000, |range, _, _| {
            range
                .map(|index| {
                    div()
                        .id(format!("row-{index}"))
                        .child(format!("row {index}"))
                })
                .collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll)
    }
}

/// The view scrolls its list to `index`, and the frame that carries the
/// scroll is lowered: the rows in it.
fn scrolled_to(
    cx: &mut TestAppContext,
    view: &Entity<Scrolled>,
    index: usize,
    strategy: gpui::ScrollStrategy,
) -> Vec<u32> {
    cx.update(view, |view, _, cx| {
        view.scroll.scroll_to_item(index, strategy);
        cx.notify();
    });
    cx.run_until_parked();
    uniform_rows(cx).1
}

/// The rows of `shown` the frame does not hold.
fn missing(rows: &[u32], shown: Range<u32>) -> Vec<u32> {
    shown.filter(|row| !rows.contains(row)).collect()
}

/// A row the view scrolls to is in the frame that carries the scroll: the
/// window is anchored on it, so a composite's active row claims in the
/// frame that answers the key, with no claimless frame between.
#[test]
fn a_uniform_list_lowers_the_row_it_is_scrolled_to_in_the_same_frame() {
    let (mut cx, view) = opened::<Scrolled>();
    let (_, before, _) = uniform_rows(&cx);
    assert!(!before.contains(&1_500));
    let indices = scrolled_to(&mut cx, &view, 1_500, gpui::ScrollStrategy::Nearest);
    assert!(indices.contains(&1_500), "{indices:?}");
    let wire::Node::UniformList {
        scroll_request,
        revision,
        ..
    } = cx.root()
    else {
        unreachable!()
    };
    assert_eq!(scroll_request.map(|request| request.index), Some(1_500));
    assert_eq!(*revision, 1);
    // the request crosses once: the next frame carries none, under the
    // same revision, so a host that applied it does not again; the list
    // stays where the scroll left it
    cx.update(&view, |_, _, cx| cx.notify());
    cx.run_until_parked();
    let wire::Node::UniformList {
        scroll_request,
        revision,
        ..
    } = cx.root()
    else {
        unreachable!()
    };
    assert_eq!((scroll_request.is_none(), *revision), (true, 1));
    assert_eq!(uniform_rows(&cx).1, indices, "the window keeps its place");
}

/// The host shows rows 100..120 and the view scrolls to a row among them
/// (`Nearest`, a composite's every arrow): the host does not move for a row
/// on screen, its range stays, and it never asks again, so every row on
/// screen stays in the frame. A row above the shown lands at their top and
/// one below at their bottom, as gpui's own `Nearest` puts them, and the
/// rows on screen until the host has scrolled stay with them.
#[test]
fn a_scroll_to_a_row_on_screen_keeps_the_rows_on_screen() {
    use gpui::ScrollStrategy::{Nearest, Top};
    let margin = crate::list::MARGIN_ROWS as u32;
    let (mut cx, view) = opened::<Scrolled>();
    cx.simulate_range("rows", 100..120);
    let (_, held, _) = uniform_rows(&cx);
    for at in [119, 110, 107, 100] {
        let rows = scrolled_to(&mut cx, &view, at, Nearest);
        assert_eq!(rows, held, "nearest {at}: the list does not move");
    }
    let rows = scrolled_to(&mut cx, &view, 110, Top);
    assert_eq!(rows, held, "a row on screen is left where it is");

    // a row above the rows shown lands at their top: the window grows to
    // it with a margin, and keeps the rows on screen until the host moves
    let rows = scrolled_to(&mut cx, &view, 85, Nearest);
    assert_eq!(rows[1..], (85 - margin..120 + margin).collect::<Vec<u32>>());
    cx.simulate_range("rows", 85..105);
    // a row below them lands at their bottom
    let rows = scrolled_to(&mut cx, &view, 125, Nearest);
    assert_eq!(missing(&rows, 85..126), [0u32; 0], "{rows:?}");
    assert_eq!(rows.last(), Some(&(126 + margin - 1)), "a margin below it");

    // a row far from the rows shown is a jump: the window goes with it
    let rows = scrolled_to(&mut cx, &view, 1_500, Nearest);
    assert_eq!(missing(&rows, 1_481..1_501), [0u32; 0], "{rows:?}");
    assert!(!rows.contains(&125), "the rows left behind are let go");
}

#[test]
fn a_uniform_window_grown_by_a_scroll_is_trimmed_by_the_hosts_range() {
    use gpui::ScrollStrategy::Nearest;
    let margin = crate::list::MARGIN_ROWS as u32;
    let (mut cx, view) = opened::<Scrolled>();
    cx.simulate_range("rows", 100..120);
    let rows = scrolled_to(&mut cx, &view, 80, Nearest);
    assert_eq!(
        (rows[1], rows.last()),
        (80 - margin, Some(&(120 + margin - 1))),
        "grown, not moved"
    );
    cx.simulate_range("rows", 80..100);
    assert!(cx.last_frame().unchanged, "rows it holds: nothing to draw");
    cx.update(&view, |_, _, cx| cx.notify());
    cx.run_until_parked();
    let (_, rows, _) = uniform_rows(&cx);
    assert_eq!(
        (rows[1], rows.last()),
        (80 - margin, Some(&(100 + margin - 1))),
        "{rows:?}"
    );
}

/// A pane resized taller while its list is scrolled shows more rows below
/// the ones it showed: they are in the frame that answers the resize,
/// counted from the rows the host shows, not from the window's first row.
#[test]
fn a_pane_resized_taller_lowers_the_rows_it_now_shows() {
    let (mut cx, _) = opened::<Scrolled>();
    cx.simulate_range("rows", 100..120);
    let row = f32::from(crate::design::size::ROW);
    cx.simulate_resize(766., 501. + 16. * row);
    let (_, rows, _) = uniform_rows(&cx);
    assert_eq!(missing(&rows, 100..136), [0u32; 0], "{rows:?}");
}

#[test]
fn spawning_from_an_entity_update_settles_without_borrowing_the_view() {
    let (mut cx, probe) = opened::<Probe>();
    cx.update(&probe, |_, _, cx| {
        cx.spawn(async move |this, cx| {
            this.update(cx, |view, cx| {
                view.received.push("first".into());
                cx.notify();
                cx.spawn(async move |this, cx| {
                    this.update(cx, |view, cx| {
                        view.received.push("second".into());
                        cx.notify();
                    })
                    .unwrap();
                })
                .detach();
            })
            .unwrap();
        })
        .detach();
    });
    assert!(!cx.tick(vec![]).busy);
    probe.read(|view| assert_eq!(view.received, ["first", "second"]));
}

#[test]
fn weak_entity_updates_fail_after_the_view_is_released() {
    let (mut cx, probe) = opened::<Probe>();
    let weak = probe.downgrade();
    let saved = Rc::new(RefCell::new(None));
    let captured = saved.clone();
    cx.app_mut()
        .spawn(async move |cx| {
            *captured.borrow_mut() = Some(cx.clone());
        })
        .detach();
    cx.tick(vec![]);
    let mut async_cx = saved.borrow_mut().take().unwrap();
    drop(cx);
    assert!(weak.upgrade().is_none());
    assert_eq!(weak.update(&mut async_cx, |_, _| ()), Err(Released));
}

#[test]
fn a_self_waking_future_keeps_the_frame_busy() {
    let (mut cx, _) = opened::<Probe>();
    let polls = Rc::new(Cell::new(0));
    let counted = polls.clone();
    let task = cx.app_mut().spawn(async move |_| {
        std::future::poll_fn(move |cx| {
            counted.set(counted.get() + 1);
            cx.waker().wake_by_ref();
            std::task::Poll::<()>::Pending
        })
        .await;
    });
    assert!(cx.tick(vec![]).busy);
    let first = polls.get();
    assert!(first > 0);
    assert!(cx.tick(vec![]).busy);
    assert!(polls.get() > first, "the next tick polls it again");
    assert!(cx.snapshot().is_err());
    drop(task);
    assert!(!cx.tick(vec![]).busy);
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
#[test]
#[should_panic(expected = "state changed without cx.notify()")]
fn update_guard_detects_missing_notify() {
    let (mut cx, probe) = opened::<Probe>();
    cx.update(&probe, |view, _, _| view.received.push("forgot".into()));
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
#[test]
#[should_panic(expected = "state changed without cx.notify()")]
fn listener_guard_detects_missing_notify() {
    #[derive(Default, Serialize, Deserialize)]
    struct Silent(bool);
    impl View for Silent {
        const NAME: &'static str = "Silent";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(false)
        }
    }
    impl Render for Silent {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let press = cx.listener(|view, _: &ClickEvent, _, _| view.0 = true);
            div()
                .id("silent")
                .role(crate::Role::Button)
                .focusable()
                .on_click(press)
                .child("Silent")
        }
    }
    let (mut cx, _) = opened::<Silent>();
    cx.simulate_click("silent");
}

#[test]
fn messages_and_responses_settle_in_input_order() {
    let (mut cx, probe) = opened::<Probe>();
    let requests = cx.last_frame().requests.clone();
    let press = Event::Click {
        handler: cx.interactivity("press").on_click.unwrap(),
        event: (&ClickEvent::default()).into(),
    };
    cx.tick(vec![press.clone(), response(requests[1].id, false), press]);
    probe.read(|view| assert_eq!(view.received, ["press", "data", "press"]));
}

#[test]
fn chained_spawns_finish_in_one_tick() {
    fn enqueue(cx: &mut Context<Probe>, remaining: usize) {
        cx.spawn(async move |this, cx| {
            this.update(cx, |view, cx| {
                view.received.push("round".into());
                cx.notify();
                if remaining > 1 {
                    enqueue(cx, remaining - 1);
                }
            })
            .unwrap();
        })
        .detach();
    }
    let (mut cx, probe) = opened::<Probe>();
    cx.update(&probe, |_, _, cx| enqueue(cx, 40));
    assert!(!cx.tick(vec![]).busy);
    probe.read(|view| assert_eq!(view.received.len(), 40));
}

mod lifecycle;
mod memo;
mod phone;
mod pictures;
mod primitive_tests;
mod style_table;
mod tick_alloc;

#[test]
fn notifying_during_render_requests_another_frame() {
    #[derive(Default, Serialize, Deserialize)]
    struct Again(bool);
    impl View for Again {
        const NAME: &'static str = "Again";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self(false)
        }
    }
    impl Render for Again {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            if !self.0 {
                self.0 = true;
                cx.notify();
            }
            div()
        }
    }
    let (cx, _) = opened::<Again>();
    let busy: Vec<_> = cx.reports().iter().map(|report| report.busy).collect();
    assert_eq!(busy, [true, false]);
}

#[test]
fn the_manifest_bytes_are_what_the_view_trait_says() {
    use wire::methods::Capability;
    #[derive(Default, Serialize, Deserialize)]
    struct App;
    impl View for App {
        const NAME: &'static str = "App";
        const DESCRIPTION: &'static str = "Words";
        const ICON: &'static str = "icons/hammer.svg";
        const TINT: Tint = Tint::Orange;
        const CAPABILITIES: &'static [Capability] = &[Capability::Clock, Capability::Program];
        const MIN_WINDOW_WIDTH: u32 = 560;
        const WINDOW_WIDTH: Option<u32> = Some(1180);
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            App
        }
    }
    impl Render for App {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }
    let bytes: [u8; manifest_len::<App>()] = manifest_bytes::<App, { manifest_len::<App>() }>();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        format!(
            "ducktape.view.manifest\nApp\nWords\nclock,program,\n560\n{}\n\nicons/hammer.svg\norange\n1180",
            wire::WIRE_LEVEL
        )
    );
    let manifest = wire::manifest::Manifest::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(manifest.min_width, 560);
    assert_eq!(manifest.level(), wire::WIRE_LEVEL);
    assert_eq!(manifest.capabilities, App::CAPABILITIES);
    assert!(manifest.targets.is_empty());
    assert_eq!(manifest.icon, App::ICON);
    assert_eq!(manifest.tint, Tint::Orange);
    assert_eq!(manifest.width, Some(1180));
    assert_eq!((&*manifest.name, &*manifest.description), ("App", "Words"));
    // a view that declares nothing is laid out from 480, opens at the
    // app's share, reaches no method and has no icon and no tint
    assert_eq!(<UniformProbe as View>::MIN_WINDOW_WIDTH, 480);
    assert_eq!(<UniformProbe as View>::WINDOW_WIDTH, None);
    assert_eq!(<UniformProbe as View>::DESCRIPTION, "");
    assert_eq!(<UniformProbe as View>::ICON, "");
    assert!(<UniformProbe as View>::CAPABILITIES.is_empty());
    let bytes: [u8; manifest_len::<UniformProbe>()] =
        manifest_bytes::<UniformProbe, { manifest_len::<UniformProbe>() }>();
    let manifest = wire::manifest::Manifest::parse(std::str::from_utf8(&bytes).unwrap()).unwrap();
    assert_eq!(manifest.min_width, 480);
    assert_eq!(manifest.name, "UniformProbe");
    assert_eq!(manifest.icon, "");
    assert_eq!(manifest.tint, Tint::None);
    assert_eq!(manifest.width, None);
}
