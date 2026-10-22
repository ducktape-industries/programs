//! A view of this build beside a host of a later one (J-085): an event it
//! cannot read is dropped and the rest of the tick runs, and `host.methods`
//! says what the host answers, or nothing on a host before it.
use super::*;
use crate::methods::{Capability, HostMethods, HostRandom, Hosted, Method};
use crate::testing::TestAppContext;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;

type Seen = Rc<RefCell<Option<Option<Hosted>>>>;

#[derive(Default, Serialize, Deserialize)]
struct Asks {
    #[serde(skip)]
    pressed: u32,
    #[serde(skip)]
    hosted: Seen,
}
impl View for Asks {
    const NAME: &'static str = "Asks";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let hosted = Seen::default();
        let kept = hosted.clone();
        let asked = cx.host().hosted();
        cx.spawn(async move |_, _| {
            *kept.borrow_mut() = Some(asked.await);
        })
        .detach();
        Self { pressed: 0, hosted }
    }
}
impl Render for Asks {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("press")
            .role(Role::Button)
            .focusable()
            .aria_label("Press")
            .on_click(cx.listener(|view, _, _, _| view.pressed += 1))
    }
}

fn press_route(node: &wire::Node) -> Option<u32> {
    if let wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        id,
        ..
    }) = node
        && id.as_ref().and_then(wire::ElementIdWire::name) == Some("press")
    {
        return interactivity.on_click;
    }
    node.children().iter().find_map(press_route)
}

const OLDER: &str = "This app is older than this view. Update the app to see everything.";

#[derive(Default, Serialize, Deserialize)]
struct Draws;
impl View for Draws {
    const NAME: &'static str = "Draws";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for Draws {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("draw")
            .role(Role::Button)
            .focusable()
            .aria_label("Draw")
            .on_click(cx.listener(|_, _, _, cx| {
                let drawn = cx.host().ask::<HostRandom>(4);
                cx.spawn(async move |_, _| drawn.await).detach();
            }))
    }
}

fn hosting_without(lacked: &str) -> Hosted {
    Hosted {
        level: wire::WIRE_LEVEL - 1,
        kinds: methods::ALL
            .iter()
            .filter(|kind| **kind != lacked)
            .map(|kind| (*kind).to_owned())
            .collect(),
    }
}

#[test]
fn a_host_that_says_nothing_of_what_it_answers_is_told_to_update() {
    let mut cx = TestAppContext::new();
    cx.host().refuse::<HostMethods>(
        methods::refusal::UNKNOWN_REQUEST,
        "unknown request `host.methods`",
    );
    cx.host().refuse::<HostRandom>(
        methods::refusal::UNKNOWN_REQUEST,
        "unknown request `host.random`",
    );
    cx.open::<Draws>();
    assert!(!cx.has_text(OLDER), "nothing refused yet: {:?}", cx.texts());
    assert!(cx.host().requests::<HostMethods>().is_empty());
    cx.simulate_click("Draw");
    assert!(cx.has_text(OLDER), "{:?}", cx.texts());
    assert_eq!(cx.interactivity("older-host").role, Some(Role::Status));
    assert_eq!(cx.host().requests::<HostMethods>().len(), 1);
    cx.simulate_click("Draw");
    assert_eq!(cx.host().requests::<HostMethods>().len(), 1);
}

#[test]
fn a_kind_the_host_does_not_list_refused_as_unknown_is_told_to_update() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<HostMethods>(|_| Ok(hosting_without(HostRandom::KIND)));
    cx.host().refuse::<HostRandom>(
        methods::refusal::UNKNOWN_REQUEST,
        "unknown request `host.random`",
    );
    cx.open::<Draws>();
    assert!(
        !cx.has_text(OLDER),
        "a host a level behind says nothing while the view asks for nothing it lacks: {:?}",
        cx.texts()
    );
    cx.simulate_click("Draw");
    assert!(cx.has_text(OLDER), "{:?}", cx.texts());
    cx.simulate_click("Draw");
    assert!(cx.has_text(OLDER), "{:?}", cx.texts());
}

#[test]
fn under_touch_the_line_clears_the_hosts_capsule() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<HostMethods>(|_| Ok(hosting_without(HostRandom::KIND)));
    cx.host().refuse::<HostRandom>(
        methods::refusal::UNKNOWN_REQUEST,
        "unknown request `host.random`",
    );
    cx.open::<Draws>();
    cx.simulate_click("Draw");
    let clearance = |cx: &TestAppContext| cx.style("older-host").padding.right;
    assert_ne!(
        clearance(&cx),
        Some(crate::design::CAPSULE_CLEARANCE.into())
    );
    cx.simulate_touch(true);
    assert!(cx.has_text(OLDER), "{:?}", cx.texts());
    assert_eq!(
        clearance(&cx),
        Some(crate::design::CAPSULE_CLEARANCE.into())
    );
}

#[test]
fn the_host_log_names_each_kind_the_host_lacks() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<HostMethods>(|_| Ok(hosting_without(HostRandom::KIND)));
    cx.host().refuse::<HostRandom>(
        methods::refusal::UNKNOWN_REQUEST,
        "unknown request `host.random`",
    );
    cx.open::<Draws>();
    cx.simulate_click("Draw");
    let named: Vec<String> = cx
        .host()
        .requests::<crate::methods::HostLog>()
        .into_iter()
        .filter(|line| line.contains(HostRandom::KIND))
        .collect();
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(named[0].starts_with(OLDER), "{named:?}");
}

#[test]
fn a_kind_the_host_lists_refused_as_unknown_is_that_requests_own() {
    let mut cx = TestAppContext::new();
    cx.host().refuse::<HostRandom>(
        methods::refusal::UNKNOWN_REQUEST,
        "this device has no random source",
    );
    cx.open::<Draws>();
    cx.simulate_click("Draw");
    assert!(!cx.has_text(OLDER), "{:?}", cx.texts());
}

#[test]
fn an_event_this_build_cannot_read_is_dropped_and_the_tick_goes_on() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Asks>();
    let handler = press_route(cx.root()).expect("a press route");
    cx.tick(vec![
        wire::Event::Unknown {
            what: "Event: a later host's".into(),
        },
        wire::Event::Click {
            handler,
            event: (&gpui::ClickEvent::default()).into(),
        },
    ]);
    assert_eq!(view.read(|view| view.pressed), 1);
}

#[test]
fn a_view_asks_its_host_what_it_answers() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Asks>();
    cx.tick(vec![]);
    let seen = view.read(|view| view.hosted.borrow().clone());
    let hosted = seen.expect("asked").expect("answered");
    assert_eq!(hosted.level, wire::WIRE_LEVEL);
    assert!(hosted.has("link.window") && hosted.has(HostMethods::KIND));
    assert!(!hosted.has("link.teleport"));
}

#[test]
fn a_host_before_host_methods_says_nothing_of_what_it_answers() {
    let mut cx = TestAppContext::new();
    cx.host().refuse::<HostMethods>(
        methods::refusal::UNKNOWN_REQUEST,
        "unknown request `host.methods`",
    );
    let view = cx.open::<Asks>();
    cx.tick(vec![]);
    assert_eq!(view.read(|view| view.hosted.borrow().clone()), Some(None));
}
