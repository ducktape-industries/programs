//! A list row is filed under its own id, else under its index, and the ids
//! inside it under the row (`wire::identity`): two rows that author the
//! same id inside them both draw, each pressing its own listener, and a
//! list is a scope of its own, under the id its author gave it. A
//! duplicate an author wrote by hand among siblings fails the view's test,
//! naming the id and its scope: the test host runs the host's sanitizer on
//! every frame, and the words are its.
use super::*;
use crate::methods::{Capability, HostWidget};
use crate::testing::TestAppContext;
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Log = Rc<RefCell<Vec<usize>>>;

fn row(index: usize, log: &Log) -> AnyElement {
    let log = log.clone();
    div()
        .flex()
        .child(format!("Row {index}"))
        .child(
            div()
                .id("open")
                .role(Role::Button)
                .focusable()
                .aria_label(format!("Open {index}"))
                .on_click(move |_, _, _| log.borrow_mut().push(index)),
        )
        .into_any_element()
}

fn opens(node: &wire::Node, out: &mut Vec<u32>) {
    if let wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        id,
        ..
    }) = node
        && id.as_ref().and_then(wire::ElementIdWire::name) == Some("open")
    {
        out.push(interactivity.on_click.expect("a click route"));
    }
    for child in node.children() {
        opens(child, out);
    }
}

fn press_each(cx: &mut TestAppContext) {
    let mut routes = Vec::new();
    opens(cx.root(), &mut routes);
    assert_eq!(routes.len(), 3, "every row drew its button");
    for handler in routes {
        cx.tick(vec![wire::Event::Click {
            handler,
            event: (&gpui::ClickEvent::default()).into(),
        }]);
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Rows {
    #[serde(skip)]
    state: Option<ListState>,
    #[serde(skip)]
    log: Log,
}
impl View for Rows {
    const NAME: &'static str = "Rows";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            state: Some(ListState::new(3, ListAlignment::Top, px(40.))),
            log: Log::default(),
        }
    }
}
impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.clone();
        div().id("rows").size_full().child(list(
            "list",
            self.state.clone().unwrap(),
            move |index, _, _| row(index, &log),
        ))
    }
}

#[test]
fn rows_of_a_list_may_author_the_same_id_inside_them() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Rows>();
    press_each(&mut cx);
    let log = view.read(|view| view.log.clone());
    assert_eq!(*log.borrow(), [0, 1, 2]);
}

#[derive(Default, Serialize, Deserialize)]
struct TwoLists {
    #[serde(skip)]
    states: Vec<ListState>,
    #[serde(skip)]
    log: Log,
}
impl View for TwoLists {
    const NAME: &'static str = "TwoLists";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            states: (0..2)
                .map(|_| ListState::new(3, ListAlignment::Top, px(40.)))
                .collect(),
            log: Log::default(),
        }
    }
}
impl Render for TwoLists {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let lists = self.states.iter().cloned().enumerate().map(|(at, state)| {
            let log = self.log.clone();
            list(["first", "second"][at], state, move |index, _, _| {
                row(at * 10 + index, &log)
            })
        });
        div()
            .id("page")
            .size_full()
            .children(lists)
            .child(div().id(0usize))
    }
}

#[test]
fn two_lists_under_one_parent_each_file_their_own_rows() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<TwoLists>();
    let mut routes = Vec::new();
    opens(cx.root(), &mut routes);
    assert_eq!(routes.len(), 6, "both lists drew their rows");
    cx.update(&view, |view, _, cx| {
        view.states[0].splice(3..3, 1);
        cx.notify();
    });
    cx.run_until_parked();
    let mut after = Vec::new();
    opens(cx.root(), &mut after);
    assert_eq!(after.len(), 7, "the first list grew a row");
    assert_eq!(after[..3], routes[..3], "the first list's rows moved");
    assert_eq!(after[4..], routes[3..], "the second list's rows moved");
    for handler in routes {
        cx.tick(vec![wire::Event::Click {
            handler,
            event: (&gpui::ClickEvent::default()).into(),
        }]);
    }
    let log = view.read(|view| view.log.clone());
    assert_eq!(*log.borrow(), [0, 1, 2, 10, 11, 12]);
}

#[derive(Default, Serialize, Deserialize)]
struct UniformRows {
    #[serde(skip)]
    log: Log,
}
impl View for UniformRows {
    const NAME: &'static str = "UniformRows";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for UniformRows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.clone();
        uniform_list("rows", 3, move |range, _, _| {
            range.map(|index| row(index, &log)).collect::<Vec<_>>()
        })
    }
}

#[test]
fn rows_of_a_uniform_list_may_author_the_same_id_inside_them() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<UniformRows>();
    let wire::Node::UniformList { path, route, .. } = cx.root().clone() else {
        panic!("a uniform list")
    };
    cx.tick(vec![wire::Event::UniformListRange {
        path,
        route,
        start: 0,
        end: 3,
        item_height: f32::from(crate::design::size::ROW),
    }]);
    press_each(&mut cx);
    let log = view.read(|view| view.log.clone());
    assert_eq!(*log.borrow(), [0, 1, 2]);
}

#[derive(Default, Serialize, Deserialize)]
struct Twice {
    second: String,
}
impl View for Twice {
    const NAME: &'static str = "Twice";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self { second: "y".into() }
    }
}
impl Render for Twice {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("page").child(
            div().id("rows").child(div().id("x").child("one")).child(
                div().child(
                    div()
                        .id(SharedString::from(self.second.clone()))
                        .child("two"),
                ),
            ),
        )
    }
}

fn write_twice(cx: &mut TestAppContext, view: &Entity<Twice>) {
    cx.update(view, |view, _, cx| {
        view.second = "x".into();
        cx.notify();
    });
    cx.run_until_parked();
}

fn panic_message(f: impl FnOnce()) -> String {
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .expect_err("the test host refuses a duplicate");
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|text| text.to_string()))
        .unwrap_or_default()
}

fn refusal(mut frame: wire::Frame) -> String {
    let refused =
        wire::sanitize(&mut frame, &mut wire::Styles::default()).expect_err("the host refuses it");
    format!("the host refuses this frame: {refused}")
}

fn rename(frame: &mut wire::Frame, name: &str, to: &str) {
    let root = frame.root.as_mut().expect("a whole frame");
    root.for_each_mut(&mut |node| {
        if let wire::Node::Container(wire::ContainerNode { id: Some(id), .. }) = node
            && id.name() == Some(name)
        {
            *id = wire::ElementIdWire::Name(to.to_owned().into());
        }
    });
}

#[test]
fn a_views_test_fails_a_duplicate_with_the_hosts_refusal() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Twice>();
    let mut sent = cx.whole_frame();
    rename(&mut sent, "y", "x");
    let refused = panic_message(|| write_twice(&mut cx, &view));
    assert_eq!(refused, refusal(sent));
}

#[derive(Default, Serialize, Deserialize)]
struct Keyed {
    keys: Vec<String>,
    #[serde(skip)]
    state: Option<ListState>,
}
impl View for Keyed {
    const NAME: &'static str = "Keyed";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            keys: vec!["m".into(), "n".into()],
            state: Some(ListState::new(2, ListAlignment::Top, px(40.))),
        }
    }
}
impl Render for Keyed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let keys = self.keys.clone();
        div().id("rows").size_full().child(list(
            "list",
            self.state.clone().unwrap(),
            move |index, _, _| {
                div()
                    .id(SharedString::from(keys[index].clone()))
                    .child(keys[index].clone())
                    .into_any_element()
            },
        ))
    }
}

#[test]
fn rows_named_by_one_key_fail_a_views_test_with_the_hosts_refusal() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Keyed>();
    let mut sent = cx.whole_frame();
    rename(&mut sent, "n", "m");
    let refused = panic_message(|| {
        cx.update(&view, |view, _, cx| {
            view.keys[1] = "m".into();
            cx.notify();
        });
        cx.run_until_parked();
    });
    assert_eq!(refused, refusal(sent));
}

thread_local! {
    static THREAD_FIRST: Cell<bool> = const { Cell::new(false) };
}

#[derive(Default, Serialize, Deserialize)]
struct Panes {
    #[serde(skip)]
    states: Vec<ListState>,
}
impl View for Panes {
    const NAME: &'static str = "Panes";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        let mut states: Vec<ListState> = (0..2)
            .map(|_| ListState::new(1, ListAlignment::Top, px(40.)))
            .collect();
        if THREAD_FIRST.get() {
            states.reverse();
        }
        Self { states }
    }
}
impl Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let pane = |id: &'static str, state: ListState| {
            div().id(id).child(list("rows", state, move |_, _, _| {
                div()
                    .id("m")
                    .child(Input::new("edit", &crate::TextField::default(), id))
                    .into_any_element()
            }))
        };
        let focus = |id: &'static str, focus: fn(&mut Window)| {
            div()
                .id(id)
                .role(Role::Button)
                .focusable()
                .on_click(move |_, window, _| focus(window))
                .child(id)
        };
        div()
            .id("page")
            .size_full()
            .child(pane("timeline", self.states[0].clone()))
            .child(pane("thread", self.states[1].clone()))
            .child(focus("focus-timeline", |window| {
                window.focus_path(["timeline", "rows", "m", "edit"])
            }))
            .child(focus("focus-thread", |window| {
                window.focus_path(["thread", "rows", "m", "edit"])
            }))
            .child(focus("focus-row", |window| {
                window.focus_path(["rows", "m", "edit"])
            }))
    }
}

fn edit(pane: &'static str) -> Vec<wire::ElementIdWire> {
    ["page", pane, "rows", "m", "edit"]
        .map(|id| wire::ElementIdWire::Name(id.into()))
        .to_vec()
}

#[test]
fn a_field_in_a_list_row_is_focused_by_a_path_through_its_list() {
    let mut cx = TestAppContext::new();
    cx.open::<Panes>();
    for pane in ["thread", "timeline"] {
        cx.simulate_click(&format!("focus-{pane}"));
        assert_eq!(
            cx.host().requests::<HostWidget>().last(),
            Some(&wire::WidgetCommand::Focus { target: edit(pane) })
        );
        assert!(matches!(cx.focused(), Some(wire::Node::Field { .. })));
    }
    let refused = panic_message(|| cx.simulate_click("focus-row"));
    assert!(refused.contains("2 end with it"), "{refused}");
}

#[test]
fn a_new_instance_files_its_list_rows_under_the_same_paths() {
    let fields = || {
        let mut cx = TestAppContext::new();
        cx.open::<Panes>();
        let mut paths = Vec::new();
        crate::testing::chain(cx.root(), &mut |chain| {
            if matches!(chain.last().unwrap(), wire::Node::Field { .. }) {
                paths.push(crate::testing::authored_path(chain));
            }
            false
        });
        paths
    };
    let first = fields();
    THREAD_FIRST.set(true);
    assert_eq!(first, [edit("timeline"), edit("thread")]);
    assert_eq!(fields(), first);
}
