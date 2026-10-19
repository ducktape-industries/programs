use super::*;
use crate::methods::Capability;
use crate::{
    ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render,
    Role, StatefulInteractiveElement, Task, Window, methods::Changes, testing::Probe,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct LiveView {
    items: usize,
    #[serde(skip)]
    task: Option<Task<()>>,
}
impl View for LiveView {
    const NAME: &'static str = "LiveView";
    const CAPABILITIES: &'static [Capability] = &[Capability::Program];
    // subscribes in `attach` alone: `new` is the default
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let mut stream = cx.host().subscribe::<Changes<Probe>>(());
        self.task = Some(cx.spawn(async move |this, cx| {
            while let Some(item) = stream.next().await {
                item.unwrap();
                this.update(cx, |view, cx| {
                    view.items += 1;
                    cx.notify();
                })
                .unwrap();
            }
        }));
    }
}
impl Render for LiveView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div().id("items").child(self.items.to_string())
    }
}

/// A view that follows the host in `attach` alone keeps following it
/// after a redeploy restores it from its snapshot, and hears each item
/// once.
#[test]
fn a_view_that_subscribes_in_attach_keeps_its_followers_across_a_restore() {
    let mut cx = TestAppContext::new();
    let feed = cx.host().stream::<Changes<Probe>>();
    cx.open::<LiveView>();
    feed.send(None);
    cx.run_until_parked();
    assert!(cx.has_text("1"));
    let snapshot = cx.snapshot().unwrap();
    assert!(snapshot[0] >= 0x80, "a named MessagePack map, not JSON");
    feed.send(None);
    let restored = cx.restore::<LiveView>(&snapshot).unwrap();
    restored.read(|view| assert_eq!(view.items, 1));
    feed.send(None);
    cx.run_until_parked();
    restored.read(|view| assert_eq!(view.items, 2));
    assert_eq!(cx.host().requests::<Changes<Probe>>().len(), 2);
}

/// Logs through `host`, which its manifest leaves out.
#[derive(Default, Serialize, Deserialize)]
struct Undeclared;
impl View for Undeclared {
    const NAME: &'static str = "Undeclared";
    const CAPABILITIES: &'static [Capability] = &[Capability::Program];
    fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.host().log("hello");
        Self
    }
}
impl Render for Undeclared {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
    }
}

#[test]
#[should_panic(expected = "undeclared_capability")]
fn a_method_the_manifest_leaves_out_fails_the_test() {
    TestAppContext::new().open::<Undeclared>();
}

/// A button with nothing to say for itself.
#[derive(Default, Serialize, Deserialize)]
struct Nameless;
impl View for Nameless {
    const NAME: &'static str = "Nameless";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for Nameless {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
            .id("nameless")
            .role(crate::Role::Button)
            .focusable()
            .on_click(|_, _, _| {})
    }
}

#[test]
#[should_panic(expected = "Unnamed at nameless")]
fn every_frame_a_view_sends_is_audited() {
    TestAppContext::new().open::<Nameless>();
}

#[derive(Default, Serialize, Deserialize)]
struct Nested;
impl View for Nested {
    const NAME: &'static str = "Nested";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for Nested {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        (0..=crate::wire::MAX_DEPTH).fold(crate::div().child("leaf"), |inner, _| {
            crate::div().child(inner)
        })
    }
}

#[test]
#[should_panic(expected = "the host would cut this frame")]
fn a_frame_the_host_would_cut_fails_its_test() {
    TestAppContext::new().open::<Nested>();
}

/// A card a click selects, with a button on it that deletes; what stands
/// between them is `stop`.
#[derive(Default, Serialize, Deserialize)]
struct Card {
    stop: Stop,
    heard: Vec<String>,
}
#[derive(Clone, Copy, Default, Serialize, Deserialize)]
enum Stop {
    #[default]
    Nothing,
    /// The button consumes its click.
    Consumes,
    /// The button hides what is behind it from the pointer.
    Occludes,
    /// The button is in a dialog opened over the card's content.
    Dialog,
    /// The button is in a toolbar that consumes a click it does not take.
    Toolbar,
}
impl View for Card {
    const NAME: &'static str = "Card";
}
impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl crate::IntoElement {
        let delete = crate::div()
            .id("delete")
            .role(Role::Button)
            .focusable()
            .on_click(cx.listener(|card: &mut Self, _: &ClickEvent, _, cx| {
                card.heard.push("delete".into());
                cx.notify();
            }))
            .child("Delete");
        crate::div()
            .id("card")
            .role(Role::Group)
            .aria_label("Card")
            .focusable()
            .on_click(cx.listener(|card: &mut Self, _: &ClickEvent, _, cx| {
                card.heard.push("card".into());
                cx.notify();
            }))
            .child("Card")
            .child(match self.stop {
                Stop::Nothing => delete.into_any_element(),
                Stop::Consumes => delete.consumes_click().into_any_element(),
                Stop::Occludes => delete.occlude().into_any_element(),
                Stop::Dialog => {
                    crate::modal_overlay("dialog", "Delete?", crate::div(), Some(delete))
                        .into_any_element()
                }
                Stop::Toolbar => crate::div()
                    .id("toolbar")
                    .consumes_click()
                    .child(delete)
                    .into_any_element(),
            })
    }
}

/// A click goes out from the node pressed through every node around it
/// that listens, as gpui passes it, until a node consumes it, a node hides
/// what is behind it, or it leaves a dialog's layer.
#[test]
fn a_click_reaches_the_nodes_around_it_until_one_consumes_it() {
    let mut cx = TestAppContext::new();
    let card = cx.open::<Card>();
    for (stop, heard) in [
        (Stop::Nothing, &["delete", "card"][..]),
        (Stop::Consumes, &["delete"]),
        (Stop::Occludes, &["delete"]),
        (Stop::Dialog, &["delete"]),
    ] {
        cx.update(&card, |view, _, cx| {
            view.stop = stop;
            view.heard.clear();
            cx.notify();
        });
        cx.run_until_parked();
        cx.simulate_click("delete");
        card.read(|view| assert_eq!(view.heard, heard));
    }
}

/// A node that consumes a click with no click of its own: the host stops a
/// click in the click's listener, so in the app the card would still hear
/// the press. The host refuses the frame, and so the view's test fails.
#[test]
#[should_panic(expected = "consumes a click it does not take")]
fn a_click_consumed_where_no_click_is_taken_fails_the_test() {
    let mut cx = TestAppContext::new();
    let card = cx.open::<Card>();
    cx.update(&card, |view, _, cx| {
        view.stop = Stop::Toolbar;
        cx.notify();
    });
    cx.run_until_parked();
}

/// Twins: two siblings with one typed id, which the audit (it keys on
/// names) passes and the host refuses.
#[derive(Default, Serialize, Deserialize)]
struct Twins;
impl View for Twins {
    const NAME: &'static str = "Twins";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for Twins {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
            .child(crate::div().id(1usize))
            .child(crate::div().id(1usize))
    }
}

#[test]
#[should_panic(
    expected = "the host refuses this frame: Container at [1]: duplicate typed element identity \
                among siblings: 1 twice under the root"
)]
fn every_frame_is_held_to_the_host_sanitizer() {
    TestAppContext::new().open::<Twins>();
}

/// A target whose tooltip is the nameless button.
#[derive(Default, Serialize, Deserialize)]
struct NamelessTip;
impl View for NamelessTip {
    const NAME: &'static str = "NamelessTip";
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self
    }
}
impl Render for NamelessTip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
            .id("target")
            .child("Target")
            .tooltip(|_, cx| cx.new(|_| Nameless).into())
    }
}

#[test]
#[should_panic(expected = "Unnamed at nameless")]
fn a_tooltip_the_host_draws_is_audited() {
    let mut cx = TestAppContext::new();
    cx.open::<NamelessTip>();
    cx.simulate_hover("target", true);
}

/// A hundred rows in a uniform list.
#[derive(Default, Serialize, Deserialize)]
struct Rows;
impl View for Rows {
    const NAME: &'static str = "Rows";
}
impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::uniform_list("rows", 100, |range, _, _| {
            range
                .map(|row| crate::div().id(row).child(format!("row {row}")))
                .collect::<Vec<_>>()
        })
    }
}

fn rows(rows: impl Iterator<Item = usize>) -> Vec<String> {
    rows.map(|row| format!("row {row}")).collect()
}

/// A test sees the rows the host shows: on the first frame the rows that
/// fill the viewport the host opened the view in (and a margin past
/// them), then the ones scrolled to with the measurement row.
#[test]
fn a_uniform_list_shows_the_rows_its_viewport_shows() {
    let mut cx = TestAppContext::new();
    cx.open::<Rows>();
    let row = f32::from(crate::design::size::ROW);
    let screen = (super::VIEWPORT.1 / row).ceil() as usize + crate::list::MARGIN_ROWS;
    assert_eq!(cx.texts(), rows(0..screen));
    cx.simulate_viewport(10);
    assert_eq!(cx.texts(), rows(0..screen), "rows the window holds already");
    cx.simulate_range("rows", 40..50);
    let margin = crate::list::MARGIN_ROWS;
    assert_eq!(
        cx.texts(),
        rows(std::iter::once(0).chain(40 - margin..50 + margin))
    );
}

/// A pane that hears every key on its way down and back up, around a list
/// that hears the arrows, a button beside it, and one that moves the
/// keyboard to the list.
#[derive(Default, Serialize, Deserialize)]
struct Keys {
    heard: Vec<String>,
}
impl Keys {
    fn hear(
        at: &'static str,
    ) -> impl Fn(&mut Self, &KeyDownEvent, &mut Window, &mut Context<Self>) {
        move |view, event, _, cx| {
            view.heard.push(format!("{at} {}", event.keystroke.key));
            cx.notify();
        }
    }
}
impl View for Keys {
    const NAME: &'static str = "Keys";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
}
impl Render for Keys {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div()
            .id("pane")
            .role(Role::Group)
            .aria_label("Pane")
            .capture_key_down(cx.listener(Self::hear("pane capture")))
            .on_key_down(cx.listener(Self::hear("pane")))
            .child(
                crate::div()
                    .id("list")
                    .role(Role::ListBox)
                    .aria_label("Rows")
                    .focusable()
                    .on_key_down(cx.listener(Self::hear("list")))
                    .child("row"),
            )
            .child(
                crate::div()
                    .id("other")
                    .role(Role::Button)
                    .focusable()
                    .on_click(|_: &ClickEvent, _, _| {})
                    .child("Other"),
            )
            .child(
                crate::div()
                    .id("jump")
                    .role(Role::Button)
                    .focusable()
                    .on_click(|_: &ClickEvent, window, _| window.focus("list"))
                    .child("Jump"),
            )
            .child(
                crate::div()
                    .id("note")
                    .child(format!("{} keys", self.heard.len())),
            )
    }
}

/// Nothing has the keyboard: the host delivers the key to no node, and
/// the test fails instead of firing the list's listener.
#[test]
#[should_panic(
    expected = "the host refuses this key: \"list\" is not on the focus path (nothing has focus)"
)]
fn a_key_at_a_node_nothing_focused_is_refused() {
    let mut cx = TestAppContext::new();
    cx.open::<Keys>();
    cx.simulate_key_down("list", "down");
}

#[test]
#[should_panic(expected = "\"list\" is not on the focus path (focus is at \"other\")")]
fn a_key_at_a_sibling_of_the_focused_node_is_refused() {
    let mut cx = TestAppContext::new();
    cx.open::<Keys>();
    cx.simulate_click("other");
    cx.simulate_key_down("list", "down");
}

/// A key goes down the focus path to the capture listeners, then back up
/// it, focused node first; a press moves the keyboard, and so does the
/// view's own `Window::focus`.
#[test]
fn keys_go_down_the_focus_path_and_back_up_it() {
    let mut cx = TestAppContext::new();
    let keys = cx.open::<Keys>();
    cx.simulate_focus("list");
    cx.simulate_key_down("list", "down");
    keys.read(|view| assert_eq!(view.heard, ["pane capture down", "list down", "pane down"]));
    cx.simulate_click("other");
    assert_eq!(cx.focused().and_then(Node::key), Some("other"));
    cx.simulate_key_down("pane", "escape");
    keys.read(|view| assert_eq!(view.heard[3..], ["pane capture escape", "pane escape"]));
    cx.simulate_click("jump");
    assert_eq!(cx.focused().and_then(Node::key), Some("list"));
}

#[test]
#[should_panic(expected = "\"note\" cannot hold focus")]
fn a_node_that_cannot_hold_focus_is_not_focused() {
    let mut cx = TestAppContext::new();
    cx.open::<Keys>();
    cx.simulate_focus("note");
}

/// A screen with a dialog that opens over it, holding one button.
#[derive(Default, Serialize, Deserialize)]
struct Dialog {
    open: bool,
}
impl View for Dialog {
    const NAME: &'static str = "Dialog";
}
impl Render for Dialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl crate::IntoElement {
        let open = cx.listener(|view: &mut Self, _: &ClickEvent, _, cx| {
            view.open = true;
            cx.notify();
        });
        let base = crate::div()
            .id("screen")
            .role(Role::Button)
            .focusable()
            .on_click(open)
            .child("Open");
        let modal = self.open.then(|| {
            crate::div()
                .id("card")
                .child(crate::div().child("Sure?"))
                .child(
                    crate::div()
                        .id("confirm")
                        .role(Role::Button)
                        .focusable()
                        .on_click(|_: &ClickEvent, _, _| {})
                        .child("Confirm"),
                )
        });
        crate::modal_overlay("dialog", "Confirm", base, modal)
    }
}

/// A dialog that opens takes the keyboard to its first Tab stop, as the
/// host's focus trap does.
#[test]
fn a_dialog_that_opens_takes_the_keyboard() {
    let mut cx = TestAppContext::new();
    cx.open::<Dialog>();
    cx.simulate_click("screen");
    assert_eq!(cx.focused().and_then(Node::key), Some("confirm"));
}

/// Tab walks the view's Tab stops in document order; in an open dialog it
/// goes round the dialog's, never out to the screen under it.
#[test]
fn tab_stays_in_an_open_dialog() {
    let mut cx = TestAppContext::new();
    cx.open::<Dialog>();
    cx.simulate_tab(true);
    assert_eq!(cx.focused().and_then(Node::key), Some("screen"));
    cx.simulate_click("screen");
    assert_eq!(cx.focused().and_then(Node::key), Some("confirm"));
    for forward in [true, false] {
        cx.simulate_tab(forward);
        assert_eq!(
            cx.focused().and_then(Node::key),
            Some("confirm"),
            "the dialog's one stop, not the screen"
        );
    }
}

/// Three buttons; the first moves the keyboard on as Tab does, and a list
/// that asks for the keys by id is refused: the host focuses a container
/// or a field.
#[derive(Default, Serialize, Deserialize)]
struct Moves;
impl View for Moves {
    const NAME: &'static str = "Moves";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
}
impl Render for Moves {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        let press = |id: &'static str, then: fn(&mut Window)| {
            crate::div()
                .id(id)
                .role(Role::Button)
                .focusable()
                .on_click(move |_: &ClickEvent, window, _| then(window))
                .child(id)
        };
        crate::div()
            .id("moves")
            .child(press("next", |window| window.focus_next()))
            .child(press("back", |window| window.focus_prev()))
            .child(press("rows", |window| window.focus("table")))
            .child(crate::uniform_list("table", 3, |range, _, _| {
                range
                    .map(|row| crate::div().id(row).child(format!("row {row}")))
                    .collect()
            }))
    }
}

#[test]
fn focus_next_and_prev_move_as_tab_does() {
    let mut cx = TestAppContext::new();
    cx.open::<Moves>();
    cx.simulate_click("next");
    assert_eq!(cx.focused().and_then(Node::key), Some("back"));
    cx.simulate_click("back");
    assert_eq!(cx.focused().and_then(Node::key), Some("next"));
}

#[test]
#[should_panic(expected = "it focuses a container or a field")]
fn a_focus_on_a_uniform_list_is_refused() {
    let mut cx = TestAppContext::new();
    cx.open::<Moves>();
    cx.simulate_click("rows");
}

/// Asks the host three hundred times on its first tick.
#[derive(Default, Serialize, Deserialize)]
struct Fanout {
    answers: usize,
    #[serde(skip)]
    asks: Vec<Task<()>>,
}
impl View for Fanout {
    const NAME: &'static str = "Fanout";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        for n in 0..300 {
            let ask = cx.host().ask::<crate::methods::HostId>(format!("n{n}"));
            self.asks.push(cx.spawn(async move |this, cx| {
                ask.await.unwrap();
                this.update(cx, |view, cx| {
                    view.answers += 1;
                    cx.notify();
                })
                .unwrap();
            }));
        }
    }
}
impl Render for Fanout {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl crate::IntoElement {
        crate::div().child(self.answers.to_string())
    }
}

#[test]
fn three_hundred_asks_go_out_in_one_frame_and_every_one_is_answered() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<crate::methods::HostId>(|prefix| Ok(format!("{prefix}-1")));
    let fanout = cx.open::<Fanout>();
    let sent: Vec<_> = cx.reports().iter().map(|tick| tick.requests).collect();
    assert_eq!(sent.first(), Some(&300));
    assert_eq!(sent.iter().sum::<usize>(), 300);
    fanout.read(|view| assert_eq!(view.answers, 300));
    let asked = cx.host().requests::<crate::methods::HostId>();
    assert_eq!(asked.first().map(String::as_str), Some("n0"));
    assert_eq!(asked.last().map(String::as_str), Some("n299"));
}

/// What a tick cost: the first one renders the tree whole, a tick that
/// changes nothing sends nothing.
#[test]
fn a_tick_reports_what_it_rendered_and_sent() {
    let mut cx = TestAppContext::new();
    cx.open::<Keys>();
    let first = cx.reports()[0];
    assert!(first.rendered && first.patches == 0 && first.bytes > 0);
    assert_eq!(first.nodes, cx.root().count());
    assert!(first.lowered > 0, "a first frame lowers the tree");
    let quiet = cx.tick(Vec::new());
    assert!(!quiet.rendered && quiet.patches == 0 && !quiet.busy);
    assert_eq!(
        quiet.lowered, 0,
        "a tick that renders nothing lowers nothing"
    );
    cx.simulate_focus("list");
    cx.simulate_key_down("list", "down");
    let key = cx.reports()[0];
    assert!(key.rendered && key.patches > 0 && key.bytes < first.bytes);
}

/// A note with a button that starts it over.
#[derive(Default, Serialize, Deserialize)]
struct Note {
    text: crate::TextField,
    /// Every change the host told: the span it edited, if it edited one,
    /// and the text after it.
    #[serde(skip)]
    heard: Vec<(Option<std::ops::Range<usize>>, String)>,
}
impl View for Note {
    const NAME: &'static str = "Note";
}
impl Render for Note {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl crate::IntoElement {
        let heard = cx.listener(|note: &mut Self, change: &wire::TextChange, _, _| {
            let edit = change.edit.map(|edit| edit.range.range());
            note.heard.push((edit, change.text.clone()));
        });
        let again = cx.listener(|note: &mut Self, _: &ClickEvent, _, cx| {
            note.text.reset("new");
            cx.notify();
        });
        crate::div()
            .child(crate::Input::new("note", &self.text, "Note").on_change(heard))
            .child(
                crate::div()
                    .id("again")
                    .role(Role::Button)
                    .focusable()
                    .on_click(again)
                    .child("Start over"),
            )
    }
}

/// The host's engine holds a field's text; a frame's text is the view's
/// copy of it. A frame whose field is a new generation is a new document:
/// the engine's text becomes the frame's, the view hears it with no edit,
/// and the next thing typed is typed into it.
#[test]
fn a_new_generation_replaces_the_hosts_text_and_the_view_hears_it() {
    let mut cx = TestAppContext::new();
    let note = cx.open::<Note>();
    cx.simulate_input("note", "typed");
    cx.simulate_click("again");
    cx.simulate_input("note", "new!");
    note.read(|note| {
        assert_eq!(
            note.heard,
            [
                (Some(0..0), "typed".into()),
                (None, "new".into()),
                (Some(3..3), "new!".into()),
            ]
        );
        assert_eq!(note.text.state().revision, 3);
    });
}
