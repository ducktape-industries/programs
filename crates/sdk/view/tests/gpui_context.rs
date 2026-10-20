use serde::{Deserialize, Serialize};
use view::prelude::*;
use view::{View, testing::TestAppContext, wire};

#[derive(Default, Serialize, Deserialize)]
struct Counter {
    clicks: usize,
}
impl View for Counter {
    const NAME: &'static str = "Counter";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for Counter {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("button")
            .role(Role::Button)
            .focusable()
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.clicks += 1;
                cx.notify();
            }))
            .child("Click")
    }
}
fn opened<V: View>() -> (TestAppContext, view::Entity<V>) {
    let mut cx = TestAppContext::new();
    let view = cx.open::<V>();
    (cx, view)
}

#[derive(Default, Serialize, Deserialize)]
struct PointerSurface {
    seen: Vec<(usize, bool, f32)>,
}
impl View for PointerSurface {
    const NAME: &'static str = "PointerSurface";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self::default()
    }
}
impl Render for PointerSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("surface")
            .role(Role::Group)
            .aria_label("Surface")
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.seen.push((
                        event.click_count,
                        event.modifiers.shift,
                        event.position.x.as_f32(),
                    ));
                    cx.notify();
                }),
            )
    }
}

#[test]
fn pointer_listener_preserves_payload_and_routes_after_frame_reset() {
    let (mut cx, view) = opened::<PointerSurface>();
    let (mut other, other_view) = opened::<PointerSurface>();
    let handler = cx
        .interactivity("surface")
        .on_mouse_down
        .expect("mouse route");
    assert_eq!(other.interactivity("surface").on_mouse_down, Some(handler));
    let event = MouseDownEvent {
        button: MouseButton::Right,
        position: gpui::point(gpui::px(12.5), gpui::px(7.0)),
        modifiers: gpui::Modifiers {
            shift: true,
            ..Default::default()
        },
        click_count: 2,
        first_mouse: true,
    };
    cx.simulate_mouse_down("surface", event.clone());
    view.read(|view| {
        assert_eq!(view.seen, vec![(2, true, 12.5)]);
    });
    other.simulate_event(wire::Event::MouseDown {
        handler: handler + 1,
        phase: wire::DispatchPhase::Bubble,
        event: (&event).into(),
    });
    other_view.read(|view| assert!(view.seen.is_empty()));
    assert_eq!(cx.interactivity("surface").on_mouse_down, Some(handler));
}

#[derive(Default, Serialize, Deserialize)]
struct TooltipSurface;
impl View for TooltipSurface {
    const NAME: &'static str = "TooltipSurface";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for TooltipSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("target")
            .tooltip_show_delay(std::time::Duration::from_millis(250))
            .hoverable_tooltip(|_, cx| cx.new(|_| TooltipContent).into())
    }
}

struct TooltipContent;
impl Render for TooltipContent {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id("tip").child("Help")
    }
}

#[test]
fn tooltip_delay_is_order_independent_and_builder_runs_only_after_request() {
    let (mut cx, _) = opened::<TooltipSurface>();
    let interactivity = cx.interactivity("target");
    let tooltip = interactivity.tooltip.clone().expect("tooltip recipe");
    assert!(tooltip.hoverable);
    assert_eq!(tooltip.delay_ms, 250);
    cx.simulate_hover("target", true);
    let response = cx.last_frame();
    let [response] = response.tooltip_responses.as_slice() else {
        panic!("one tooltip response")
    };
    assert_eq!(response.request, tooltip.request);
    assert_eq!(response.character_index, None);
    let Some(content) = response.content.as_deref() else {
        panic!("ordinary tooltip has content")
    };
    let wire::Node::Container(view::wire::ContainerNode { id, children, .. }) = content else {
        panic!("tooltip content is a lowered container")
    };
    assert_eq!(id, &Some(wire::ElementIdWire::Name("tip".into())));
    assert_eq!(children.len(), 1);
}

#[derive(Default, Serialize, Deserialize)]
struct RichTooltipSurface;
impl View for RichTooltipSurface {
    const NAME: &'static str = "RichTooltipSurface";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for RichTooltipSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        InteractiveText::new("rich-tip", StyledText::new("alpha beta"))
            .tooltip(|index, _, cx| (index == 6).then(|| cx.new(|_| TooltipContent).into()))
    }
}

#[test]
fn rich_text_tooltip_routes_character_index_and_explicit_none() {
    let (mut cx, _) = opened::<RichTooltipSurface>();
    cx.simulate_rich_hover("rich-tip", Some(6));
    let [some] = cx.last_frame().tooltip_responses.as_slice() else {
        panic!("one tooltip response")
    };
    assert_eq!(some.character_index, Some(6));
    assert!(some.content.is_some());

    cx.simulate_rich_hover("rich-tip", Some(0));
    let [none] = cx.last_frame().tooltip_responses.as_slice() else {
        panic!("one explicit empty tooltip response")
    };
    assert_eq!(none.character_index, Some(0));
    assert!(none.content.is_none());
}

#[derive(Default, Serialize, Deserialize)]
struct FocusSurface;
impl View for FocusSurface {
    const NAME: &'static str = "FocusSurface";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
fn focused(key: &'static str, handle: &FocusHandle) -> view::Stateful<view::Div> {
    div()
        .id(key)
        .role(Role::Button)
        .aria_label(key)
        .track_focus(handle)
}

impl Render for FocusSurface {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let first = cx.focus_handle();
        let same = first.clone();
        let unrelated = cx.focus_handle();
        div()
            .child(focused("first", &first))
            .child(focused("same", &same))
            .child(focused("other", &unrelated))
    }
}

#[test]
fn opaque_focus_allocations_share_only_through_clone() {
    let (cx, _) = opened::<FocusSurface>();
    let ids: Vec<_> = ["first", "same", "other"]
        .map(|key| cx.interactivity(key).focus_handle.unwrap())
        .to_vec();
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
}

#[test]
fn click_routes_are_frame_owned_and_isolated_per_view() {
    let (mut first, first_view) = opened::<Counter>();
    let (mut second, second_view) = opened::<Counter>();
    let route = |cx: &TestAppContext| cx.interactivity("button").on_click.unwrap();
    let first_id = route(&first);
    assert_eq!(first_id, route(&second));
    for expected in 1..=20 {
        first.simulate_click("button");
        assert_eq!(
            route(&first),
            first_id,
            "reset must release previous frame routes"
        );
        first_view.read(|view| assert_eq!(view.clicks, expected));
        second_view.read(|view| assert_eq!(view.clicks, 0));
    }
    second.simulate_click("button");
    second_view.read(|view| assert_eq!(view.clicks, 1));
    first.simulate_event(wire::Event::Message(first_id));
    first_view.read(|view| assert_eq!(view.clicks, 20, "message and click routes differ"));
}

#[test]
fn listeners_use_weak_entities() {
    let mut first = TestAppContext::new();
    let entity = first.open::<Counter>();
    let listener = first.update(&entity, |_, _, cx| {
        cx.listener(|_: &mut Counter, _: &ClickEvent, _, _| panic!("released listener ran"))
    });
    drop(entity);
    drop(first);
    let mut second = TestAppContext::new();
    let other = second.open::<Counter>();
    second.update(&other, |_, window, cx| {
        listener(&ClickEvent::default(), window, cx)
    });
}

#[derive(Default, Serialize, Deserialize)]
struct GlobalReader {
    initial: usize,
}
struct Configuration(usize);
impl Global for Configuration {}
impl View for GlobalReader {
    const NAME: &'static str = "GlobalReader";
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.initial = cx.global::<Configuration>().0;
    }
}
impl Render for GlobalReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(cx.global::<Configuration>().0.to_string())
    }
}
#[test]
fn test_globals_are_available_during_creation_and_restore_without_clone() {
    let mut cx = TestAppContext::new();
    cx.set_global(Configuration(13));
    let entity = cx.open::<GlobalReader>();
    entity.read(|view| assert_eq!(view.initial, 13));
    let snapshot = cx.snapshot().unwrap();
    cx.set_global(Configuration(21));
    let restored = cx.restore::<GlobalReader>(&snapshot).unwrap();
    restored.read(|view| assert_eq!(view.initial, 21));
    let second = cx.open::<GlobalReader>();
    second.read(|view| assert_eq!(view.initial, 21));
}

#[derive(Default, Serialize, Deserialize)]
struct ThemeReader;
impl View for ThemeReader {
    const NAME: &'static str = "ThemeReader";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for ThemeReader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().bg(cx.global::<Theme>().surface)
    }
}
#[test]
fn host_theme_events_update_the_global_and_emit_style_patches() {
    let (mut cx, _) = opened::<ThemeReader>();
    let background =
        |cx: &TestAppContext| cx.styles()[cx.root().style().unwrap()].background.clone();
    cx.simulate_theme(true);
    assert!(matches!(
        cx.last_frame().patches.as_slice(),
        [wire::Patch::Props { .. }]
    ));
    assert_eq!(background(&cx), Some(Theme::dark().surface.into()));
    assert_ne!(Theme::dark().surface, Theme::light().surface);

    cx.simulate_theme(false);
    assert!(matches!(
        cx.last_frame().patches.as_slice(),
        [wire::Patch::Props { .. }]
    ));
    assert_eq!(background(&cx), Some(Theme::light().surface.into()));
}
