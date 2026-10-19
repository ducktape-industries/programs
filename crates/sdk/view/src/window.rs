//! The view's single window: what the host says about it, and the
//! platform work it delegates to the host.
use crate::context::AppState;
use crate::{ElementId, slots, wire};
use gpui::{Pixels, Size};
use std::rc::Rc;
pub struct Window {
    app: Rc<AppState>,
}
impl Window {
    pub(crate) fn new(app: Rc<AppState>) -> Self {
        Self { app }
    }
    /// The size the view is laid out in, in logical pixels: its pane's
    /// body, as the host knows it before the frame is drawn
    /// ([`Event::Viewport`](wire::Event::Viewport)). It is in hand on the
    /// first tick, moves ahead of the draw that shows a resize, and a
    /// replacement has it before its first tree, so a layout decided from
    /// it is right in the first frame it draws.
    pub fn viewport_size(&self) -> Size<Pixels> {
        self.app.viewport.get()
    }
    /// The device's primary pointer is a finger (a phone), or not
    /// ([`Event::Pointer`](wire::Event::Pointer)): in hand on the first
    /// tick like the viewport, so `design::ladder(window.touch())` is right
    /// in the first frame it draws.
    pub fn touch(&self) -> bool {
        self.app.touch.get()
    }
    /// Gives the keyboard to the element `id` names in the frame this tick
    /// renders. `id` is the element's own: the SDK, which lowered the ids
    /// above it, sends the host the element's whole path. An id two scopes
    /// hold names neither, and the host refuses it; [`focus_path`]
    /// names one.
    ///
    /// [`focus_path`]: Self::focus_path
    pub fn focus(&mut self, id: impl Into<ElementId>) {
        self.focus_path([id]);
    }
    /// Gives the keyboard to the element whose path ends with `path`: the
    /// ids above it that tell it apart, its own last (`["reply",
    /// "input"]`).
    pub fn focus_path<I: Into<ElementId>>(&mut self, path: impl IntoIterator<Item = I>) {
        let target = path
            .into_iter()
            .map(|id| crate::element::wire_id(id.into()))
            .collect();
        self.dispatch(wire::WidgetCommand::Focus { target });
    }
    /// Moves the keyboard to the next Tab stop, as Tab does: round the open
    /// dialog that holds it, never out of it.
    pub fn focus_next(&mut self) {
        self.dispatch(wire::WidgetCommand::FocusNext);
    }
    /// Moves the keyboard to the previous Tab stop, as Shift-Tab does:
    /// round the open dialog that holds it, never out of it.
    pub fn focus_prev(&mut self) {
        self.dispatch(wire::WidgetCommand::FocusPrevious);
    }
    /// Asks the host for `command` with the frame this tick renders: what
    /// a [`TextField`](crate::TextField)'s `replace` asks is sent here. A
    /// target the command names by an element's own id goes out as that
    /// element's whole path, as [`focus`](Self::focus)'s does.
    pub fn dispatch(&mut self, command: wire::WidgetCommand) {
        slots::widget(&self.app.slots, command);
    }
}

/// Sends the widget commands this tick asked for, in order, each target
/// named by the path `root` (the tree this frame carries) gives it: the
/// host matches whole paths. A view names a target by the ids it holds
/// ([`Window::focus`]); the frame it renders is the first that holds an
/// element it just opened, so the path is read there, not earlier.
pub(crate) fn send_widgets(
    context: &slots::Context,
    root: Option<&wire::Node>,
    behind: Option<&wire::Node>,
) {
    let host = slots::host(context);
    for mut command in slots::take_widgets(context) {
        if let (Some(root), Some(target)) = (root, target_mut(&mut command)) {
            // `behind` holds the subtrees `root` keeps from the last frame
            let found =
                resolve(root, target).or_else(|| behind.and_then(|behind| resolve(behind, target)));
            if let Some(path) = found {
                *target = path;
            }
        }
        host.notify::<crate::methods::HostWidget>(command);
    }
}

fn target_mut(command: &mut wire::WidgetCommand) -> Option<&mut wire::WidgetTarget> {
    use wire::WidgetCommand as C;
    match command {
        C::FocusPrevious | C::FocusNext | C::FocusHandle { .. } => None,
        C::Replace { target, .. }
        | C::Focus { target }
        | C::CursorFront { target }
        | C::CursorEnd { target }
        | C::Cursor { target, .. }
        | C::SelectAll { target }
        | C::Select { target, .. }
        | C::Snap { target, .. }
        | C::SnapEnd { target }
        | C::ScrollTo { target, .. }
        | C::ScrollBy { target, .. } => Some(target),
    }
}

/// The whole path of the one element `target` names in `root`: the one at
/// exactly that path, else the one whose path ends with it. None when it
/// is a whole path already, or when no element or more than one ends with
/// it: it goes as named, and the host refuses a path it does not mount.
fn resolve(root: &wire::Node, target: &[wire::ElementIdWire]) -> Option<wire::WidgetTarget> {
    fn walk(
        node: &wire::Node,
        row: Option<usize>,
        path: &mut Vec<wire::ElementIdWire>,
        visit: &mut impl FnMut(&[wire::ElementIdWire]),
    ) {
        let segment = wire::identity::segment(node.identity().cloned(), row);
        let entered = segment.is_some();
        if let Some(segment) = segment {
            path.push(segment);
            visit(path);
        }
        for (at, child) in node.children().iter().enumerate() {
            walk(child, wire::identity::row(node, at), path, visit);
        }
        if entered {
            path.pop();
        }
    }
    let (mut whole, mut ending) = (false, Vec::new());
    walk(root, None, &mut Vec::new(), &mut |path| {
        whole |= path == target;
        if path.ends_with(target) {
            ending.push(path.to_vec());
        }
    });
    match (whole, <[_; 1]>::try_from(ending)) {
        (false, Ok([path])) => Some(path),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use crate::methods::{Capability, HostWidget, Method};
    use crate::testing::TestAppContext;
    use crate::{Context, ElementId, Host, Input, Render, View, Window, host, wire};
    use serde::{Deserialize, Serialize};

    fn target(name: &str) -> wire::WidgetTarget {
        vec![wire::ElementIdWire::Name(name.into())]
    }

    async fn perform(host: Host, command: wire::WidgetCommand) -> Result<Vec<u8>, host::Error> {
        host.request(HostWidget::KIND, &wire::encode(&command))
            .await
    }

    #[derive(Default, Serialize, Deserialize)]
    struct WidgetView(bool);
    impl View for WidgetView {
        const NAME: &'static str = "WidgetView";
        const CAPABILITIES: &'static [Capability] = &[Capability::Host];
        fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
            let host = cx.host();
            cx.spawn(async move |this, cx| {
                perform(
                    host,
                    wire::WidgetCommand::Focus {
                        target: target("App/draft"),
                    },
                )
                .await
                .unwrap();
                this.update(cx, |view, cx| {
                    view.0 = true;
                    cx.notify();
                })
                .unwrap();
            })
            .detach();
            Self(false)
        }
    }
    impl Render for WidgetView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl crate::IntoElement {
            Input::new(
                ElementId::Name("App/draft".into()),
                &crate::TextField::default(),
                "Draft",
            )
            .on_change(cx.listener(|_, _: &wire::TextChange, _, _| {}))
        }
    }

    #[test]
    fn widget_futures_wait_for_the_hosts_acknowledgment() {
        let mut cx = TestAppContext::new();
        cx.host().never::<HostWidget>();
        let view = cx.open::<WidgetView>();
        let frame = cx.last_frame();
        let [focus] = frame.requests.as_slice() else {
            panic!("one focus request: {:?}", frame.requests)
        };
        assert_eq!(focus.kind, HostWidget::KIND);
        assert_eq!(
            cx.host().requests::<HostWidget>(),
            [wire::WidgetCommand::Focus {
                target: target("App/draft")
            }]
        );
        let id = focus.id;
        cx.tick(vec![]);
        view.read(|view| assert!(!view.0, "the future waits for the answer"));
        cx.simulate_event(wire::Event::Response {
            id,
            result: Ok(wire::encode(&())),
            done: true,
        });
        view.read(|view| assert!(view.0));
    }

    #[test]
    fn widget_commands_go_out_with_the_frame_in_order() {
        let app = crate::App::for_driver();
        let host = app.host();
        let mut window = app.window();
        window.focus("first");
        window.focus_next();
        window.focus("second");
        assert!(
            host.drain_outbox().is_empty(),
            "nothing goes out before the frame"
        );
        super::send_widgets(&app.inner.slots, None, None);
        let sent: Vec<_> = host
            .drain_outbox()
            .iter()
            .map(|request| wire::decode::<wire::WidgetCommand>(&request.payload).unwrap())
            .collect();
        assert_eq!(
            sent,
            [
                wire::WidgetCommand::Focus {
                    target: target("first")
                },
                wire::WidgetCommand::FocusNext,
                wire::WidgetCommand::Focus {
                    target: target("second")
                },
            ]
        );
    }

    #[test]
    fn focus_handle_schedules_its_opaque_host_command() {
        let mut app = crate::App::for_driver();
        let handle = app.focus_handle();
        let mut window = app.window();
        handle.focus(&mut window, &mut app);
        super::send_widgets(&app.inner.slots, None, None);
        let requests = app.host().drain_outbox();
        let [request] = requests.as_slice() else {
            panic!("one focus command")
        };
        assert_eq!(
            wire::decode::<wire::WidgetCommand>(&request.payload).unwrap(),
            wire::WidgetCommand::FocusHandle { handle: 0 }
        );
    }

    /// Two forms, each with a field named "input"; one press asks for the
    /// right one by the ids that tell it apart, the other by the bare id
    /// both hold.
    #[derive(Default, Serialize, Deserialize)]
    struct TwoForms;
    impl View for TwoForms {
        const NAME: &'static str = "TwoForms";
        const CAPABILITIES: &'static [Capability] = &[Capability::Host];
    }
    impl Render for TwoForms {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
            use crate::{InteractiveElement, ParentElement, StatefulInteractiveElement, div};
            let form = |id: &'static str| {
                div()
                    .id(id)
                    .child(Input::new("input", &crate::TextField::default(), id))
            };
            let press = |id: &'static str, focus: fn(&mut Window)| {
                div()
                    .id(id)
                    .role(gpui::Role::Button)
                    .focusable()
                    .on_click(move |_, window, _| focus(window))
                    .child(id)
            };
            div()
                .id("forms")
                .child(form("left"))
                .child(form("right"))
                .child(press("right-input", |window| {
                    window.focus_path(["right", "input"])
                }))
                .child(press("any-input", |window| window.focus("input")))
        }
    }

    fn path(names: &[&str]) -> wire::WidgetTarget {
        names
            .iter()
            .map(|name| wire::ElementIdWire::Name((*name).into()))
            .collect()
    }

    /// The host matches a target's whole path, so the SDK sends the one
    /// the frame lowered: the right form's field, never the first that
    /// ends with "input".
    #[test]
    fn a_focus_names_the_one_element_it_asks_for_by_its_whole_path() {
        let mut cx = TestAppContext::new();
        cx.open::<TwoForms>();
        cx.simulate_click("right-input");
        assert_eq!(
            cx.host().requests::<HostWidget>().last(),
            Some(&wire::WidgetCommand::Focus {
                target: path(&["forms", "right", "input"])
            })
        );
        let focused = cx.focused().expect("a field has the keys");
        assert!(std::ptr::eq(
            focused,
            &cx.root().children()[1].children()[0]
        ));
    }

    /// An id two scopes hold names neither: it goes out as named, and the
    /// host refuses it, where it once focused the first in the tree.
    #[test]
    #[should_panic(expected = "the host refuses to focus")]
    fn an_id_two_scopes_hold_focuses_neither() {
        let mut cx = TestAppContext::new();
        cx.open::<TwoForms>();
        cx.simulate_click("any-input");
    }
}
