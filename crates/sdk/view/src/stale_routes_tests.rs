//! A route is the listener's authored identity, not its position in the
//! frame. The host dispatches a pointer event against the frame it last
//! painted, so a route can arrive one frame late, after a structural
//! change: it runs the pressed node's listener if that node is still there
//! and nothing if it is gone. The counts at the end pin what a renumbering
//! used to cost: every routed node after the change went out again.
use super::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

type Log = Rc<RefCell<Vec<String>>>;

fn tick<V: View>(driver: &mut Driver<V>, events: Vec<wire::Event>) -> wire::Frame {
    driver.tick_with(events, |frame| frame.clone())
}

fn root<V: View>(driver: &Driver<V>) -> wire::Node {
    driver.last_root.clone().expect("a tree")
}

fn find<'a>(node: &'a wire::Node, key: &str) -> Option<&'a wire::Node> {
    if node.key() == Some(key) {
        return Some(node);
    }
    node.children().iter().find_map(|child| find(child, key))
}

fn click_route(root: &wire::Node, key: &str) -> u32 {
    let Some(wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = find(root, key)
    else {
        panic!("no {key}")
    };
    interactivity.on_click.expect("a click route")
}

#[derive(Default, Serialize, Deserialize)]
struct Tip {
    name: String,
}
impl View for Tip {
    const NAME: &'static str = "Tip";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child(self.name.clone())
    }
}

fn button(name: &'static str, log: &Log) -> impl IntoElement {
    let l = |kind: &'static str| {
        let log = log.clone();
        move || log.borrow_mut().push(format!("{name}:{kind}"))
    };
    let (click, md, hover, scroll, key) = (l("click"), l("md"), l("hover"), l("scroll"), l("key"));
    div()
        .id(name)
        .role(Role::Button)
        .focusable()
        .child(name)
        .on_click(move |_, _, _| click())
        .on_mouse_down(MouseButton::Left, move |_, _, _| md())
        .on_hover(move |_, _, _| hover())
        .on_scroll_wheel(move |_, _, _| scroll())
        .on_key_down(move |_, _, _| key())
        .tooltip(move |_, cx| cx.new(|_| Tip { name: name.into() }).into())
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
enum Before {
    #[default]
    Nothing,
    Button,
    HoverOnly,
}

#[derive(Default, Serialize, Deserialize)]
struct Shifting {
    before: Before,
    gone_a: bool,
    #[serde(skip)]
    log: Log,
}
impl View for Shifting {
    const NAME: &'static str = "Shifting";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            before: Before::Nothing,
            gone_a: false,
            log: Default::default(),
        }
    }
}
impl Render for Shifting {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let log = self.log.clone();
        let mut root = div().id("root");
        match self.before {
            Before::Nothing => {}
            Before::Button => root = root.child(button("x", &self.log)),
            Before::HoverOnly => {
                root = root.child(
                    div()
                        .id("x")
                        .child("x")
                        .on_hover(move |_, _, _| log.borrow_mut().push("x:hover".into())),
                )
            }
        }
        if !self.gone_a {
            root = root.child(button("a", &self.log));
        }
        root.child(button("b", &self.log))
            .child(button("after", &self.log))
    }
}

#[derive(Debug, PartialEq, Clone, Copy)]
struct Routes {
    click: u32,
    md: u32,
    hover: u32,
    scroll: u32,
    key: u32,
    tip: u32,
}

fn routes(root: &wire::Node, key: &str) -> Routes {
    let Some(wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = find(root, key)
    else {
        panic!("no {key}")
    };
    Routes {
        click: interactivity.on_click.unwrap(),
        md: interactivity.on_mouse_down.unwrap(),
        hover: interactivity.on_hover.unwrap(),
        scroll: interactivity.on_scroll_wheel.unwrap(),
        key: interactivity.on_key_down.unwrap(),
        tip: interactivity.tooltip.as_ref().unwrap().request,
    }
}

fn mouse_down() -> gpui::MouseDownEvent {
    gpui::MouseDownEvent {
        button: gpui::MouseButton::Left,
        position: Default::default(),
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    }
}

fn scroll() -> gpui::ScrollWheelEvent {
    gpui::ScrollWheelEvent {
        position: Default::default(),
        delta: gpui::ScrollDelta::Pixels(Default::default()),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    }
}

fn painted_events(routes: &Routes) -> Vec<(&'static str, wire::Event)> {
    vec![
        (
            "click",
            wire::Event::Click {
                handler: routes.click,
                event: (&gpui::ClickEvent::default()).into(),
            },
        ),
        (
            "md",
            wire::Event::MouseDown {
                handler: routes.md,
                phase: wire::DispatchPhase::Bubble,
                event: (&mouse_down()).into(),
            },
        ),
        (
            "hover",
            wire::Event::Hover {
                handler: routes.hover,
                hovered: true,
            },
        ),
        (
            "scroll",
            wire::Event::ScrollWheel {
                handler: routes.scroll,
                phase: wire::DispatchPhase::Bubble,
                event: (&scroll()).into(),
            },
        ),
        (
            "key",
            wire::Event::KeyDown {
                handler: routes.key,
                phase: wire::DispatchPhase::Bubble,
                event: (&gpui::KeyDownEvent {
                    keystroke: gpui::Keystroke::parse("x").unwrap(),
                    is_held: false,
                    prefer_character_input: false,
                })
                    .into(),
            },
        ),
    ]
}

fn ran_for(driver: &mut Driver<Shifting>, routes: &Routes) -> Vec<(&'static str, Vec<String>)> {
    let log = driver.entity().read(|v| v.log.clone());
    let mut out = Vec::new();
    for (kind, event) in painted_events(routes) {
        log.borrow_mut().clear();
        tick(driver, vec![event]);
        out.push((kind, log.borrow().clone()));
    }
    out
}

fn painted_tooltip(driver: &mut Driver<Shifting>, request: u32) -> Option<Vec<String>> {
    let frame = tick(
        driver,
        vec![wire::Event::TooltipRequest {
            request,
            character_index: None,
        }],
    );
    frame.tooltip_responses.first().map(|response| {
        assert_eq!(response.request, request);
        testing::texts(response.content.as_deref())
    })
}

fn assert_ran(ran: &[(&'static str, Vec<String>)], who: &str) {
    for (kind, listeners) in ran {
        assert_eq!(
            listeners,
            &vec![format!("{who}:{kind}")],
            "the painted {kind} route ran {listeners:?}"
        );
    }
}

#[test]
fn a_painted_route_crossing_an_insert_runs_the_pressed_nodes_listener() {
    for before in [Before::Button, Before::HoverOnly] {
        let mut driver = Driver::<Shifting>::new();
        tick(&mut driver, vec![]);
        let b_painted = routes(&root(&driver), "b");
        driver.entity().update(driver.app_mut(), |v, cx| {
            v.before = before;
            cx.notify();
        });
        tick(&mut driver, vec![]);
        assert_eq!(
            routes(&root(&driver), "b"),
            b_painted,
            "the insert left b's routes alone"
        );
        assert_ran(&ran_for(&mut driver, &b_painted), "b");
        assert_eq!(
            painted_tooltip(&mut driver, b_painted.tip),
            Some(vec!["b".into()])
        );
    }
}

#[test]
fn a_painted_route_crossing_a_removal_runs_the_pressed_node_or_nothing() {
    let mut driver = Driver::<Shifting>::new();
    tick(&mut driver, vec![]);
    let painted = root(&driver);
    let a_painted = routes(&painted, "a");
    let b_painted = routes(&painted, "b");
    let after_painted = routes(&painted, "after");
    driver.entity().update(driver.app_mut(), |v, cx| {
        v.gone_a = true;
        cx.notify();
    });
    tick(&mut driver, vec![]);
    assert_ran(&ran_for(&mut driver, &b_painted), "b");
    assert_ran(&ran_for(&mut driver, &after_painted), "after");
    for (kind, who) in ran_for(&mut driver, &a_painted) {
        assert!(
            who.is_empty(),
            "the removed node's {kind} route ran {who:?}"
        );
    }
    assert_eq!(
        painted_tooltip(&mut driver, a_painted.tip),
        None,
        "a request for a route that is gone gets no response"
    );
}

#[test]
fn a_node_that_comes_back_takes_a_fresh_route() {
    let mut driver = Driver::<Shifting>::new();
    tick(&mut driver, vec![]);
    let a_painted = routes(&root(&driver), "a");
    for gone in [true, false] {
        driver.entity().update(driver.app_mut(), |v, cx| {
            v.gone_a = gone;
            cx.notify();
        });
        tick(&mut driver, vec![]);
    }
    assert_ne!(routes(&root(&driver), "a").click, a_painted.click);
    for (kind, who) in ran_for(&mut driver, &a_painted) {
        assert!(
            who.is_empty(),
            "a route from before the node left ran {who:?} ({kind})"
        );
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Form {
    extra: bool,
    #[serde(skip)]
    log: Log,
}
impl View for Form {
    const NAME: &'static str = "Form";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for Form {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let field = |name: &'static str| {
            let (change, submit) = (self.log.clone(), self.log.clone());
            Input::new(name, &crate::TextField::default(), name)
                .on_change(move |_, _, _| change.borrow_mut().push(format!("{name}:change")))
                .on_submit(move |_, _, _| submit.borrow_mut().push(format!("{name}:submit")))
        };
        let mut form = div().id("form");
        if self.extra {
            form = form.child(field("extra"));
        }
        form.child(field("first")).child(field("second"))
    }
}

fn field_routes(root: &wire::Node, key: &str) -> (u32, u32) {
    let Some(wire::Node::Field {
        on_change: Some(change),
        on_submit: Some(submit),
        ..
    }) = find(root, key)
    else {
        panic!("no field {key}")
    };
    (*change, *submit)
}

#[test]
fn a_field_inserted_above_leaves_the_fields_below_their_routes() {
    let mut driver = Driver::<Form>::new();
    tick(&mut driver, vec![]);
    let (change, submit) = field_routes(&root(&driver), "second");
    driver.entity().update(driver.app_mut(), |v, cx| {
        v.extra = true;
        cx.notify();
    });
    tick(&mut driver, vec![]);
    assert_eq!(
        field_routes(&root(&driver), "second"),
        (change, submit),
        "the field above renumbered second"
    );
    let log = driver.entity().read(|v| v.log.clone());
    tick(
        &mut driver,
        vec![
            wire::Event::Message(submit),
            wire::Event::Text {
                handler: change,
                change: wire::TextChange {
                    generation: 0,
                    revision: 1,
                    edit: None,
                    text: "x".into(),
                    cursor: wire::TextRange::caret(1),
                    preedit: None,
                    tokens: Default::default(),
                },
            },
        ],
    );
    assert_eq!(
        *log.borrow(),
        vec!["second:submit".to_owned(), "second:change".to_owned()]
    );
}

#[derive(Default, Serialize, Deserialize)]
struct Keyed {
    items: Vec<String>,
    #[serde(skip)]
    state: Option<ListState>,
    #[serde(skip)]
    log: Log,
}
impl View for Keyed {
    const NAME: &'static str = "Keyed";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        let items: Vec<String> = (0..10).map(|i| format!("m{i}")).collect();
        Self {
            state: Some(ListState::new(items.len(), ListAlignment::Bottom, px(40.))),
            items,
            log: Default::default(),
        }
    }
}
impl Render for Keyed {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let items = self.items.clone();
        let log = self.log.clone();
        list("rows", self.state.clone().unwrap(), move |index, _, _| {
            let name = items[index].clone();
            let log = log.clone();
            div()
                .id(SharedString::from(name.clone()))
                .role(Role::Button)
                .focusable()
                .child(name.clone())
                .on_click(move |_, _, _| log.borrow_mut().push(format!("{name}:click")))
                .into_any_element()
        })
    }
}

#[test]
fn a_row_prepended_to_a_list_leaves_the_rows_below_their_routes() {
    let mut driver = Driver::<Keyed>::new();
    tick(&mut driver, vec![]);
    let m9 = click_route(&root(&driver), "m9");
    driver.entity().update(driver.app_mut(), |v, cx| {
        v.items.insert(0, "older".into());
        v.state.as_ref().unwrap().splice(0..0, 1);
        cx.notify();
    });
    tick(&mut driver, vec![]);
    assert_eq!(
        click_route(&root(&driver), "m9"),
        m9,
        "the prepend renumbered m9"
    );
    let log = driver.entity().read(|v| v.log.clone());
    tick(
        &mut driver,
        vec![wire::Event::Click {
            handler: m9,
            event: (&gpui::ClickEvent::default()).into(),
        }],
    );
    assert_eq!(*log.borrow(), vec!["m9:click".to_owned()]);
}

fn tx_row(index: usize) -> impl IntoElement {
    div()
        .id(format!("row-{index}"))
        .role(Role::Button)
        .focusable()
        .child(div().child(format!("tx {index}")))
        .child(div().child("deadbeef"))
        .on_click(|_, _, _| {})
        .on_hover(|_, _, _| {})
}

#[derive(Default, Serialize, Deserialize)]
struct UniformTable {
    footer: String,
}
impl View for UniformTable {
    const NAME: &'static str = "UniformTable";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            footer: "Next".into(),
        }
    }
}
impl Render for UniformTable {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("page")
            .child(uniform_list("rows", 2_000, |range, _, _| {
                range.map(tx_row).collect::<Vec<_>>()
            }))
            .child(
                div()
                    .id("footer")
                    .role(Role::Button)
                    .focusable()
                    .child(self.footer.clone())
                    .on_click(|_, _, _| {}),
            )
    }
}

#[derive(Default, Debug)]
struct Sent {
    whole: bool,
    props: usize,
    inserts: usize,
    removes: usize,
    bytes: usize,
}

fn sent<V: View>(driver: &mut Driver<V>, events: Vec<wire::Event>) -> Sent {
    driver.tick_with(events, |frame| {
        let mut out = Sent {
            whole: frame.root.is_some(),
            bytes: wire::encode(frame).len(),
            ..Default::default()
        };
        for patch in &frame.patches {
            match patch {
                wire::Patch::Props { .. } => out.props += 1,
                wire::Patch::Insert { .. } => out.inserts += 1,
                wire::Patch::Remove { .. } => out.removes += 1,
                _ => {}
            }
        }
        out
    })
}

#[test]
fn scrolling_a_uniform_list_sends_only_the_rows_that_moved_in() {
    use crate::list::MARGIN_ROWS as M;
    let mut driver = Driver::<UniformTable>::new();
    tick(&mut driver, vec![]);
    let first = root(&driver);
    let Some(wire::Node::UniformList { path, route, .. }) = find(&first, "rows") else {
        panic!("no uniform list")
    };
    let (path, route) = (path.clone(), *route);
    let range = |start: u32, end: u32| wire::Event::UniformListRange {
        path: path.clone(),
        route,
        start,
        end,
        item_height: 26.,
    };
    let shown = sent(&mut driver, vec![range(100, 124)]);
    let notch = sent(&mut driver, vec![range(101, 125)]);
    let page = sent(&mut driver, vec![range(137, 161)]);
    eprintln!(
        "UNIFORM shown={shown:?} ({} B) one-notch={notch:?} ({} B) past-margin={page:?} ({} B)",
        shown.bytes, notch.bytes, page.bytes
    );
    assert!(!notch.whole, "{notch:?}");
    assert_eq!(
        (notch.props, notch.inserts, notch.removes),
        (0, 0, 0),
        "a notch inside the window: {notch:?}"
    );
    assert!(!page.whole, "{page:?}");
    let moved = (137 - 100) as usize;
    assert_eq!(
        (page.props, page.inserts, page.removes),
        (1, moved, moved),
        "{page:?} (margin {M})"
    );
}

#[derive(Default, Serialize, Deserialize)]
struct Column {
    rows: Vec<usize>,
}
impl View for Column {
    const NAME: &'static str = "Column";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            rows: (1..=50).collect(),
        }
    }
}
impl Render for Column {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("commits")
            .children(self.rows.iter().map(|index| tx_row(*index)))
    }
}

#[test]
fn a_row_landing_at_the_top_of_a_plain_column_sends_one_insert() {
    let mut driver = Driver::<Column>::new();
    let first = sent(&mut driver, vec![]);
    driver.entity().update(driver.app_mut(), |v, cx| {
        v.rows.insert(0, 0);
        cx.notify();
    });
    let pushed = sent(&mut driver, vec![]);
    eprintln!(
        "COLUMN first={first:?} ({} B) one-row-at-top={pushed:?} ({} B)",
        first.bytes, pushed.bytes
    );
    assert!(!pushed.whole, "{pushed:?}");
    assert_eq!((pushed.props, pushed.inserts), (0, 1), "{pushed:?}");
}
