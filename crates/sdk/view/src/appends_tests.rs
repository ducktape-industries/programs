//! A view of this build beside a host of a later one (J-085): an event it
//! cannot read is dropped and the rest of the tick runs, and `host.methods`
//! says what the host answers, or nothing on a host before it.
use super::*;
use crate::methods::{Capability, HostMethods, Hosted, Method};
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
