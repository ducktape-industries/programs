use crate::accesskit::{Action, ActionData, AriaCurrent, HasPopup, Invalid, Live};
use crate::prelude::*;
use crate::testing::TestAppContext;
use crate::{Lowering, View, wire};

fn lower(element: impl IntoElement) -> wire::Node {
    let mut app = App::for_driver();
    let mut window = app.window();
    Lowering::new(&mut window, &mut app).lower(element)
}

fn aria(node: &wire::Node) -> &wire::Aria {
    &interactivity(node).aria
}

fn interactivity(node: &wire::Node) -> &wire::Interactivity {
    node.interactivity()
        .unwrap_or_else(|| panic!("no interactivity: {node:?}"))
}

#[test]
fn a_focusable_node_is_a_tab_stop_unless_it_says_otherwise() {
    let stop = lower(div().id("open").focusable().on_click(|_, _, _| {}));
    assert_eq!(interactivity(&stop).tab_stop, Some(true));
    let skipped = lower(div().id("busy").focusable().tab_stop(false));
    assert_eq!(interactivity(&skipped).tab_stop, Some(false));
    let plain = lower(div().id("box"));
    assert_eq!(plain.interactivity(), None);
    let mut app = App::for_driver();
    let handle = app.focus_handle();
    let mut window = app.window();
    let tracked = Lowering::new(&mut window, &mut app).lower(div().id("menu").track_focus(&handle));
    assert_eq!(interactivity(&tracked).tab_stop, Some(true));
}

#[test]
fn a_div_that_declares_no_interaction_carries_no_interactivity() {
    let plain = [
        lower(div()),
        lower(div().flex().child("text")),
        lower(div().id("scroller").overflow_y_scroll()),
    ];
    for node in plain {
        let wire::Node::Container(wire::ContainerNode { interactivity, .. }) = node else {
            panic!("a div is a container")
        };
        assert_eq!(interactivity, None);
    }
    for declared in [
        lower(div().id("open").on_click(|_, _, _| {})),
        lower(div().id("title").role(gpui::Role::Heading)),
        lower(div().hover(|style| style.opacity(0.5))),
    ] {
        assert!(declared.interactivity().is_some(), "{declared:?}");
    }
}

fn named(names: &[&'static str]) -> Vec<wire::ElementIdWire> {
    names
        .iter()
        .map(|name| wire::ElementIdWire::Name((*name).into()))
        .collect()
}

#[test]
fn each_phase_two_setter_lowers_into_its_one_wire_field() {
    let node = lower(
        div()
            .id("field")
            .aria_live(Live::Polite)
            .aria_busy(true)
            .aria_required(true)
            .aria_invalid(Invalid::True)
            .aria_read_only(true)
            .aria_has_popup(HasPopup::Listbox)
            .aria_current(AriaCurrent::Page)
            .custom_action(3, "Pin"),
    );
    assert_eq!(
        *aria(&node),
        wire::Aria {
            live: Some(Live::Polite),
            busy: true,
            required: true,
            invalid: Some(Invalid::True),
            read_only: true,
            has_popup: Some(HasPopup::Listbox),
            current: Some(AriaCurrent::Page),
            custom_actions: vec![(3, "Pin".into())],
            ..Default::default()
        }
    );
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Stepper {
    count: i32,
    data: Option<ActionData>,
}

impl View for Stepper {
    const NAME: &'static str = "Stepper";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for Stepper {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("stepper")
            .role(Role::SpinButton)
            .aria_label("Count")
            .aria_numeric_value(f64::from(self.count))
            .on_a11y_action(Action::Increment, {
                let step = cx.listener(|view: &mut Self, data: &Option<ActionData>, _, cx| {
                    view.count += 1;
                    view.data = data.clone();
                    cx.notify();
                });
                let mut asked = Vec::new();
                move |data: Option<&ActionData>, window: &mut Window, app: &mut App| {
                    asked.push(data.cloned());
                    step(asked.last().expect("just asked"), window, app)
                }
            })
    }
}

#[test]
fn an_a11y_action_reaches_the_listener_its_route_names_with_its_data() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Stepper>();
    let &[(Action::Increment, _)] = aria(cx.root()).actions.as_slice() else {
        panic!("one Increment route: {:?}", aria(cx.root()).actions);
    };
    let data = Some(ActionData::Value("2".into()));
    cx.simulate_a11y_action("stepper", Action::Increment, data.clone());
    view.read(|view| {
        assert_eq!(view.count, 1);
        assert_eq!(view.data, data);
    });
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Pinned {
    heard: Vec<i32>,
}

impl View for Pinned {
    const NAME: &'static str = "Pinned";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}

impl Render for Pinned {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let heard = cx.listener(|view: &mut Self, id: &i32, _, cx| {
            view.heard.push(*id);
            cx.notify();
        });
        div()
            .id("message")
            .role(Role::Article)
            .aria_label("Message")
            .custom_action(1, "Pin")
            .on_a11y_action(Action::CustomAction, move |data, window, app| {
                if let Some(ActionData::CustomAction(id)) = data {
                    heard(id, window, app)
                }
            })
    }
}

#[test]
fn a_custom_action_the_node_does_not_offer_is_not_heard() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<Pinned>();
    let &[(Action::CustomAction, _)] = aria(cx.root()).actions.as_slice() else {
        panic!("one custom route: {:?}", aria(cx.root()).actions);
    };
    for id in [2, 1] {
        cx.simulate_a11y_action(
            "message",
            Action::CustomAction,
            Some(ActionData::CustomAction(id)),
        );
    }
    view.read(|view| assert_eq!(view.heard, [1]));
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Log {
    #[serde(skip)]
    scroll: ScrollHandle,
}
impl View for Log {
    const NAME: &'static str = "Log";
    const CAPABILITIES: &'static [crate::methods::Capability] = &[crate::methods::Capability::Host];
}
impl Render for Log {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let press = |id: &'static str| div().id(id).role(Role::Button).focusable().child(id);
        div()
            .id("pane")
            .child(
                div()
                    .id("log")
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child("line"),
            )
            .child(
                press("end")
                    .on_click(cx.listener(|log: &mut Self, _, _, _| log.scroll.scroll_to_bottom())),
            )
            .child(
                press("top").on_click(cx.listener(|log: &mut Self, _, _, _| {
                    log.scroll.set_offset(gpui::point(px(0.), px(-40.)))
                })),
            )
    }
}

#[test]
fn a_scroll_handle_moves_the_div_it_tracks() {
    let mut cx = TestAppContext::new();
    cx.open::<Log>();
    let log = named(&["pane", "log"]);
    cx.simulate_click("end");
    cx.simulate_click("top");
    assert_eq!(
        cx.host().requests::<crate::methods::HostWidget>(),
        [
            wire::WidgetCommand::SnapEnd {
                target: log.clone()
            },
            wire::WidgetCommand::ScrollTo {
                target: log,
                x: 0.,
                y: 40.,
            },
        ]
    );
}
