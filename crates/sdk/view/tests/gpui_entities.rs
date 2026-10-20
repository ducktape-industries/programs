//! Child entities as a gpui developer composes them: a plain struct built
//! with `cx.new`, updated from a listener, heard through `emit`/`subscribe`
//! and `observe`, a tooltip with no `View` type, and children a restored
//! snapshot builds again.
use serde::{Deserialize, Serialize};
use view::prelude::*;
use view::{Entity, EventEmitter, Subscription, View, testing::TestAppContext, wire};

struct Sidebar {
    selected: Option<usize>,
}

struct Selected(usize);
impl EventEmitter<Selected> for Sidebar {}

impl Sidebar {
    fn new(selected: Option<usize>, _: &mut Context<Self>) -> Self {
        Self { selected }
    }
    fn select(&mut self, row: usize, cx: &mut Context<Self>) {
        self.selected = Some(row);
        cx.emit(Selected(row));
        cx.notify();
    }
}

impl Render for Sidebar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().id("sidebar").children((0..3).map(|row| {
            let label = match self.selected == Some(row) {
                true => format!("row {row} (selected)"),
                false => format!("row {row}"),
            };
            div()
                .id(ElementId::Name(format!("row-{row}").into()))
                .role(Role::Button)
                .focusable()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.select(row, cx)))
                .child(label)
        }))
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Shell {
    picked: Option<usize>,
    #[serde(skip)]
    heard: usize,
    #[serde(skip)]
    sidebar: Option<Entity<Sidebar>>,
    #[serde(skip)]
    subscriptions: Vec<Subscription>,
}

impl View for Shell {
    const NAME: &'static str = "Shell";
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let picked = self.picked;
        let sidebar = cx.new(|cx| Sidebar::new(picked, cx));
        self.subscriptions = vec![
            cx.subscribe(&sidebar, |shell, _, Selected(row): &Selected, cx| {
                shell.picked = Some(*row);
                cx.notify();
            }),
            cx.observe(&sidebar, |shell, _, cx| {
                shell.heard += 1;
                cx.notify();
            }),
        ];
        self.sidebar = Some(sidebar);
    }
}

impl Shell {
    fn sidebar(&self) -> &Entity<Sidebar> {
        self.sidebar.as_ref().expect("attach built it")
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picked = match self.picked {
            Some(row) => format!("picked {row}"),
            None => "picked none".into(),
        };
        div()
            .id("shell")
            .child(div().id("picked").child(picked))
            .child(div().id("heard").child(format!("heard {}", self.heard)))
            .child(
                div()
                    .id("last")
                    .role(Role::Button)
                    .focusable()
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.sidebar()
                            .update(cx, |sidebar, cx| sidebar.select(2, cx));
                    }))
                    .child("Select the last"),
            )
            .child(self.sidebar().clone())
    }
}

fn shell() -> (TestAppContext, Entity<Shell>) {
    let mut cx = TestAppContext::new();
    let shell = cx.open::<Shell>();
    (cx, shell)
}

#[test]
fn a_plain_struct_built_with_cx_new_renders_as_a_child() {
    let (cx, _) = shell();
    assert_eq!(
        cx.texts(),
        [
            "picked none",
            "heard 0",
            "Select the last",
            "row 0",
            "row 1",
            "row 2"
        ]
    );
}

#[test]
fn a_listener_updates_a_child_and_the_view_renders_again() {
    let (mut cx, _) = shell();
    let renders = cx.renders();
    cx.simulate_click("last");
    assert!(
        cx.renders() > renders,
        "the child's notify renders the view"
    );
    assert!(cx.has_text("row 2 (selected)"), "{:?}", cx.texts());
}

#[test]
fn a_child_event_reaches_its_parent() {
    let (mut cx, shell) = shell();
    cx.simulate_click("row-1");
    shell.read(|shell| assert_eq!(shell.picked, Some(1)));
    assert!(cx.has_text("picked 1"), "{:?}", cx.texts());
}

#[test]
fn observe_fires_on_a_child_notify() {
    let (mut cx, _) = shell();
    cx.simulate_click("row-0");
    cx.simulate_click("row-2");
    assert!(cx.has_text("heard 2"), "{:?}", cx.texts());
}

#[test]
fn a_dropped_subscription_hears_nothing() {
    let (mut cx, shell) = shell();
    cx.update(&shell, |shell, _, _| shell.subscriptions.clear());
    cx.simulate_click("row-1");
    assert!(cx.has_text("row 1 (selected)"), "{:?}", cx.texts());
    assert!(cx.has_text("picked none"), "{:?}", cx.texts());
    assert!(cx.has_text("heard 0"), "{:?}", cx.texts());
}

#[test]
fn a_restored_snapshot_builds_the_children_again() {
    let (mut cx, _) = shell();
    cx.simulate_click("row-2");
    assert!(cx.has_text("picked 2"), "{:?}", cx.texts());
    let bytes = cx.snapshot().expect("a settled view snapshots");
    cx.restore::<Shell>(&bytes).expect("the snapshot restores");
    assert!(
        cx.has_text("row 2 (selected)"),
        "attach builds the sidebar from the restored state: {:?}",
        cx.texts()
    );
    cx.simulate_click("row-0");
    assert!(
        cx.has_text("picked 0"),
        "the rebuilt child's events reach the restored root: {:?}",
        cx.texts()
    );
}

struct Tip(&'static str);
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("tip").child(self.0)
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Tipped;
impl View for Tipped {
    const NAME: &'static str = "Tipped";
}
impl Render for Tipped {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("target")
            .child("Target")
            .tooltip(|_, cx| cx.new(|_| Tip("Help")).into())
    }
}

#[test]
fn a_tooltip_needs_no_view_type() {
    let mut cx = TestAppContext::new();
    cx.open::<Tipped>();
    cx.simulate_hover("target", true);
    let [response] = cx.last_frame().tooltip_responses.as_slice() else {
        panic!("one tooltip response")
    };
    let content = response
        .content
        .as_deref()
        .expect("the tooltip has content");
    let [wire::Node::Text(text)] = content.children() else {
        panic!("the tip's one text: {content:?}")
    };
    assert_eq!(text.content, "Help");
}
