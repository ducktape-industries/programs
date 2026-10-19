use super::super::Send;
use super::super::draft::host;
use super::*;
use serde::{Deserialize, Serialize};
use view::testing::{self, TestAppContext};
use view::{Keystroke, Modifiers};
use wire::keyboard::{Key, Named};

mod placement;

#[derive(Default, Serialize, Deserialize)]
struct ComposerView {
    draft: Draft,
    choices: Vec<MentionChoice>,
    pane: Option<Place>,
    #[serde(skip)]
    events: Vec<String>,
}

impl View for ComposerView {
    const NAME: &'static str = "ComposerView";
    // the composer moves focus back to its editor and asks it for edits
    // (`host.widget`)
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
    fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
        Self {
            draft: Draft::from_body("hello", &[]),
            ..Self::default()
        }
    }
}

impl Render for ComposerView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let draft = self.draft.clone();
        let choices = self.choices.clone();
        view(
            &draft,
            "c",
            "New message",
            "Message #general",
            "Send",
            None,
            true,
            &choices,
            self.pane,
            cx,
            |view, event, window, cx| {
                view.events.push(match &event {
                    Event::Action(tag) => format!("action:{tag}"),
                    Event::Changed(_) => "changed".into(),
                    Event::Key(..) => "key".into(),
                    Event::Placed(_) => "placed".into(),
                    Event::Dropped(_) => "dropped".into(),
                });
                let choices = view.choices.clone();
                if let Outcome::Action(tag) = view.draft.handle(event, "c", &choices, window) {
                    view.events.push(format!("outcome:{tag}"));
                }
                cx.notify();
            },
        )
    }
}

fn walk(node: &wire::Node, seen: &mut impl FnMut(&wire::Node)) {
    seen(node);
    for child in node.children() {
        walk(child, seen);
    }
}

/// The composer's tree, audited, and the table its nodes' styles are in.
fn drawn_styled(draft: &Draft) -> (wire::Node, wire::Styles) {
    let (tree, styles) = lowered_styled(draft, "c", &[]);
    testing::assert_accessible(&tree);
    (tree, styles)
}

/// The composer's tree, held to the audit as a view's tests hold it.
fn drawn_with(draft: &Draft, key: &str, choices: &[MentionChoice]) -> wire::Node {
    let tree = lowered(draft, key, choices);
    testing::assert_accessible(&tree);
    tree
}

/// The composer's tree unaudited: only for what the tree carries beside
/// the open @-mention menu, whose faults are
/// `the_mention_menu_is_not_yet_reachable`'s to name.
fn lowered(draft: &Draft, key: &str, choices: &[MentionChoice]) -> wire::Node {
    lowered_styled(draft, key, choices).0
}

fn lowered_styled(
    draft: &Draft,
    key: &str,
    choices: &[MentionChoice],
) -> (wire::Node, wire::Styles) {
    let (tree, styles, _) = testing::lower(|cx| {
        view(
            draft,
            key,
            "New message",
            "Message #general",
            "Send",
            None,
            true,
            choices,
            Some(PANE),
            cx,
            |_: &mut ComposerView, _, _, _| {},
        )
    });
    (tree, styles)
}

fn find_field(root: &wire::Node) -> Option<&wire::Node> {
    if matches!(root, wire::Node::Field { .. }) {
        return Some(root);
    }
    root.children().iter().find_map(find_field)
}

fn field_node(root: &wire::Node) -> &wire::Node {
    find_field(root).expect("composer includes one field")
}

fn node<'a>(root: &'a wire::Node, key: &str) -> Option<&'a wire::Node> {
    if root.key() == Some(key) {
        return Some(root);
    }
    root.children().iter().find_map(|child| node(child, key))
}

fn clickable(root: &wire::Node, key: &str) -> Option<u32> {
    let Some(wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = node(root, key)
    else {
        return None;
    };
    interactivity.on_click
}

fn roster() -> Vec<MentionChoice> {
    vec![MentionChoice {
        token: "<@1>".into(),
        label: "Ada".into(),
        hint: None,
        offered: true,
    }]
}

fn caret(body: &str, at: usize) -> Draft {
    let mut draft = Draft::from_body(body, &roster());
    host::selects(&draft.field, at..at);
    draft.placed = Some(COMPOSER);
    draft
}

const PANE: Place = Place {
    x: 0.,
    y: 0.,
    width: 800.,
    height: 600.,
};

const COMPOSER: Place = Place {
    x: 20.,
    y: 480.,
    width: 760.,
    height: 100.,
};

fn claimed(draft: &Draft) -> Vec<wire::KeyClaim> {
    // the key claims, with the menu open or shut
    let root = lowered(draft, "c", &roster());
    let wire::Node::Field { claims, .. } = field_node(&root) else {
        unreachable!()
    };
    claims.to_vec()
}

fn bare(key: Named) -> wire::KeyClaim {
    wire::KeyClaim {
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        command: false,
    }
}

/// One edit the view asked of the host: target, revision, range, text, token.
type Replace = (
    wire::ElementIdWire,
    u64,
    std::ops::Range<usize>,
    String,
    Option<String>,
);

/// The edits the view asked of the host, in order.
fn replaces(cx: &TestAppContext) -> Vec<Replace> {
    cx.host()
        .requests::<HostWidget>()
        .into_iter()
        .filter_map(|command| match command {
            wire::WidgetCommand::Replace {
                target,
                revision,
                range,
                text,
                token,
                ..
            } => Some((
                target.last().cloned().unwrap(),
                revision,
                range.range(),
                text,
                token,
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn every_mark_is_the_same_square_and_the_field_writes_at_body_size() {
    let (root, styles) = drawn_styled(&Draft::default());
    let mut marks = Vec::new();
    let mut body_size = None;
    let mut editor_bounds = None;
    walk(&root, &mut |node| match node {
        wire::Node::Container(wire::ContainerNode {
            style,
            interactivity: Some(interactivity),
            ..
        }) if interactivity.role == Some(Role::Button) => {
            let style = &styles[*style];
            let side = px(design::height::CONTROL as f32);
            if style.size.width == Some(side.into()) && style.size.height == Some(side.into()) {
                marks.push(interactivity.aria.label.clone());
            }
        }
        wire::Node::Field { style, .. } => {
            let style = &styles[*style];
            body_size = style.text.font_size;
            editor_bounds = Some((style.min_size.height, style.max_size.height));
        }
        _ => {}
    });
    assert_eq!(
        marks.len(),
        5,
        "attach, bold, italic, code and quote are control-high squares"
    );
    assert_eq!(body_size, Some(px(design::type_scale::BODY as f32).into()));
    // the field grows with what is written, between a floor and a ceiling
    assert!(
        matches!(editor_bounds, Some((Some(_), Some(_)))),
        "{editor_bounds:?}"
    );
}

/// A draft restored from a body carries its mention as one span of the
/// field, and is a document the host has not seen.
#[test]
fn a_restored_mention_draft_is_a_new_document_with_its_mention_as_a_span() {
    let choices = roster();
    let before = Draft::default().field.generation();
    let draft = Draft::from_body("Hi <@1>", &choices);
    let root = drawn_with(&draft, "c", &choices);
    let wire::Node::Field {
        value,
        tokens,
        generation,
        ..
    } = field_node(&root)
    else {
        unreachable!()
    };
    assert_eq!(value, "Hi @Ada");
    assert_eq!(
        &tokens[..],
        &[wire::TextToken {
            range: wire::TextRange::from(3..7),
            id: "<@1>".into(),
        }]
    );
    assert!(*generation > before);
}

#[test]
fn toolbar_mention_and_restore_actions_have_reachable_aria_routes() {
    let choices = roster();
    let mut draft = caret("@A", 2);
    draft.failed_send = Some(Send {
        body: "older".into(),
    });
    let root = lowered(&draft, "c", &choices);
    for (key, label) in [
        ("c/bold", "Bold"),
        ("c/italic", "Italic"),
        ("c/code", "Code"),
        ("c/quote", "Quote"),
        ("c/restore", "Restore"),
    ] {
        let Some(wire::Node::Container(wire::ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) = node(&root, key)
        else {
            panic!("missing composer action {key}");
        };
        assert!(interactivity.on_click.is_some(), "{key} has no route");
        assert!(interactivity.focusable, "{key} takes no focus");
        assert_eq!(interactivity.aria.label.as_deref(), Some(label));
    }
    let Some(wire::Node::Container(wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = node(&root, "c/mention/<@1>")
    else {
        panic!("missing mention action");
    };
    assert!(interactivity.on_click.is_some());
    assert_eq!(interactivity.role, Some(Role::MenuItem));
    assert_eq!(interactivity.aria.label.as_deref(), Some("@Ada"));
    // with the menu shut, the whole composer passes the audit
    host::selects(&draft.field, 0..0);
    drawn_with(&draft, "c", &choices);
}

/// The known faults, each and no other: the @-mention rows are menu items
/// with no menu, which no key reaches (the keys stay in the editor) and
/// whose `selected` a menu item does not carry. The honest shape is an
/// EditableComboBox editor whose active descendant is the picked option,
/// a wire change of its own; this test fails once that lands.
#[test]
fn the_mention_menu_is_not_yet_reachable() {
    use wire::FaultKind::{Orphan, Unreachable, UnreadState};
    let faults: Vec<_> = wire::audit(&lowered(&caret("@A", 2), "c", &roster()))
        .into_iter()
        .map(|fault| (fault.path.last().cloned(), fault.kind))
        .collect();
    let at = |kind| (Some("c/mention/<@1>".to_owned()), kind);
    assert_eq!(faults, [at(UnreadState), at(Unreachable), at(Orphan)]);
}

#[test]
fn the_arrows_are_claimed_only_while_the_menu_is_open() {
    let closed = claimed(&caret("hello", 5));
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(!closed.contains(&bare(key)), "{key:?} belongs to the host");
    }
    let open = claimed(&caret("@A", 2));
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(open.contains(&bare(key)), "{key:?} belongs to the menu");
    }
}

/// Editing and history are the engine's: no state of the draft claims
/// Backspace, Delete, Tab, undo or redo, and a read-only composer claims
/// nothing.
#[test]
fn no_claim_is_on_a_key_the_engine_owns() {
    for draft in [
        Draft::default(),
        caret("hello", 2),
        caret("hello", 5),
        caret("@A", 2),
    ] {
        let claims = claimed(&draft);
        assert!(claims.contains(&bare(Named::Enter)));
        assert!(
            claims.iter().all(|claim| !claim.engine_owned()),
            "{claims:?}"
        );
    }
    let (root, _, _) = testing::lower(|cx| {
        view(
            &caret("@A", 2),
            "c",
            "New message",
            "Message #general",
            "Send",
            None,
            false,
            &roster(),
            None,
            cx,
            |_: &mut ComposerView, _, _, _| {},
        )
    });
    let wire::Node::Field {
        claims,
        options,
        on_key,
        ..
    } = field_node(&root)
    else {
        unreachable!()
    };
    assert!(claims.is_empty() && on_key.is_none() && options.read_only);
}

fn key(keystroke: &str) -> wire::keyboard::KeyState {
    (&Keystroke::parse(keystroke).unwrap()).into()
}

/// Down picks the second name and Enter takes it as one span with a space
/// after; Escape shuts the menu and Enter then sends, once per press.
/// Every edit is asked of the host at the revision the draft knows, never
/// applied here.
#[test]
fn menu_navigation_then_enter_picks_a_stable_identity_and_escape_leaves_enter_to_send() {
    let choices = vec![
        MentionChoice {
            token: "<@1>".into(),
            label: "Ada".into(),
            hint: None,
            offered: true,
        },
        MentionChoice {
            token: "<@2>".into(),
            label: "Alan".into(),
            hint: None,
            offered: true,
        },
    ];
    let mut draft = caret("@A", 2);
    let press = |draft: &mut Draft, keystroke: &str, repeat: bool| {
        let tag = draft.key_tag(&key(keystroke), repeat, &choices)?;
        Some(draft.act(&tag, "c/editor", &choices))
    };
    press(&mut draft, "down", false).unwrap();
    assert_eq!(draft.menu_index, 1);
    let (edits, _) = press(&mut draft, "enter", false).unwrap();
    assert_eq!(
        edits
            .iter()
            .map(|edit| match edit {
                wire::WidgetCommand::Replace {
                    range, text, token, ..
                } => (range.range(), text.clone(), token.clone()),
                other => panic!("{other:?}"),
            })
            .collect::<Vec<_>>(),
        [
            (0..2, "@Alan".into(), Some("<@2>".into())),
            (2..2, " ".into(), None)
        ]
    );
    // the keys the menu owns go back to the engine once it is shut
    press(&mut draft, "escape", false).unwrap();
    assert!(draft.menu_dismissed && draft.query().is_none());
    assert!(press(&mut draft, "down", false).is_none());
    assert!(
        press(&mut draft, "enter", true).is_none(),
        "a held Enter sends nothing"
    );
    let (edits, outcome) = press(&mut draft, "enter", false).unwrap();
    assert!(matches!(outcome, Outcome::Action(tag) if tag == "send"));
    assert_eq!(edits.len(), 1, "one clear of the field");
    assert_eq!(draft.submitted.take().unwrap().body, "@A");
}

/// Enter in the composer sends: the view hears it, the host clears the
/// field at the revision the draft knew and says so. Send is live while
/// the draft holds text and dead once it is empty.
#[test]
fn enter_sends_and_the_host_clears_the_field() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<ComposerView>();
    assert!(clickable(cx.root(), "c/send").is_some(), "text to send");
    cx.simulate_field_key("c/editor", "enter");
    view.read(|view| {
        assert!(view.events.iter().any(|event| event == "outcome:send"));
        assert_eq!(view.draft.field.text(), "", "the host cleared the field");
        assert_eq!(host::revision(&view.draft.field), 1);
    });
    let field = wire::ElementIdWire::Name("c/editor".into());
    assert_eq!(replaces(&cx), [(field, 0, 0..5, String::new(), None)]);
    assert!(
        clickable(cx.root(), "c/send").is_none(),
        "nothing left to send"
    );
}

/// An `@` whose name no one in the roster has is text: Enter sends it as
/// it sends a draft with no `@`. `meet @ 5pm` holds an open query, and so
/// does `hi @Ada` before the roster arrives.
#[test]
fn enter_sends_when_no_name_matches_the_name_being_typed() {
    for (choices, typed) in [(roster(), "meet @ 5pm"), (Vec::new(), "hi @Ada")] {
        let mut cx = TestAppContext::new();
        let view = cx.open::<ComposerView>();
        cx.update(&view, |view, _, cx| {
            view.choices = choices;
            cx.notify();
        });
        cx.simulate_input("c/editor", typed);
        view.read(|view| assert!(view.draft.query().is_some(), "{typed:?} opens a query"));
        cx.simulate_field_key("c/editor", "enter");
        view.read(|view| {
            assert!(
                view.events.iter().any(|event| event == "outcome:send"),
                "{typed:?}: {:?}",
                view.events
            );
            assert_eq!(view.draft.submitted.as_ref().unwrap().body, typed);
            assert_eq!(view.draft.field.text(), "", "the host cleared {typed:?}");
        });
    }
}

/// A press on a mark focuses the field again and asks the host to wrap
/// the selection; the host's answer comes back as a change.
#[test]
fn a_mark_pressed_asks_the_host_for_the_edit_and_hands_the_keys_back() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<ComposerView>();
    cx.simulate_click("c/bold");
    view.read(|view| assert!(view.events.iter().any(|event| event == "action:bold")));
    // the press focused the mark; the keys go back to the editor, named
    // by its whole path
    let editor = cx.find("c/editor").and_then(wire::Node::identity).cloned();
    assert!(
        cx.host()
            .requests::<HostWidget>()
            .iter()
            .any(|command| matches!(
                command,
                wire::WidgetCommand::Focus { target }
                    if target.len() > 1 && target.last() == editor.as_ref()
            )),
        "{:?}",
        cx.host().requests::<HostWidget>()
    );
    assert_eq!(cx.focused().and_then(wire::Node::key), Some("c/editor"));
    let field = wire::ElementIdWire::Name("c/editor".into());
    assert_eq!(
        replaces(&cx),
        [(field, 0, 5..5, "****".into(), None)],
        "a caret at the end of `hello` gets an empty pair"
    );
    view.read(|view| {
        assert!(view.events.iter().any(|event| event == "changed"));
        assert_eq!(view.draft.field.text(), "hello****");
        assert_eq!(view.draft.field.selection(), 7..7);
        assert_eq!(host::revision(&view.draft.field), 1);
    });
}

/// A view that draws its composer from a draft that is not in its state:
/// it keeps a draft only once something is typed, and until then lowers a
/// temporary one made in `render`. The mistake chat made for a room nobody
/// wrote in yet; no view may make it.
#[derive(Default, Serialize, Deserialize)]
struct LazyDraft {
    draft: Option<Draft>,
}

impl View for LazyDraft {
    const NAME: &'static str = "LazyDraft";
    const CAPABILITIES: &'static [Capability] = &[Capability::Host];
}

impl Render for LazyDraft {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let empty = Draft::default();
        let draft = self.draft.as_ref().unwrap_or(&empty);
        view(
            draft,
            "c",
            "New message",
            "Message #general",
            "Send",
            None,
            true,
            &[],
            None,
            cx,
            |view: &mut LazyDraft, event, window, cx| {
                let draft = view.draft.get_or_insert_default();
                draft.handle(event, "c", &[], window);
                cx.notify();
            },
        )
    }
}

/// A change is the host's word on the document the frame lowered, and no
/// other document takes it: the SDK keeps that, since a document the view
/// reset must not take a word on the one it left. So what is typed into a
/// field that is not in the view's state is lost. The binding lands the
/// change in the temporary draft the editor was drawn from, which went with
/// its frame; the draft the view makes in its listener is another document
/// and refuses it. The next frame shows that draft's document, empty, and
/// the host takes its text from it: to the writer the first thing typed is
/// gone. Every change from that frame on is the draft's. (The `"hi"` below
/// is the whole field typed again, not an `i` after the `h`.)
#[test]
fn what_is_typed_into_a_draft_made_in_render_is_lost() {
    let mut cx = TestAppContext::new();
    let view = cx.open::<LazyDraft>();
    view.read(|view| assert!(view.draft.is_none()));
    cx.simulate_input("c/editor", "h");
    view.read(|view| {
        let draft = view.draft.as_ref().expect("the listener made the draft");
        assert_eq!(draft.field.text(), "", "a word on the temporary's document");
    });
    let wire::Node::Field { value, .. } = field_node(cx.root()) else {
        panic!("the editor")
    };
    assert_eq!(value, "", "and the host's text is the new draft's");
    cx.simulate_input("c/editor", "hi");
    view.read(|view| assert_eq!(view.draft.as_ref().unwrap().field.text(), "hi"));
    let wire::Node::Field { value, .. } = field_node(cx.root()) else {
        panic!("the editor")
    };
    assert_eq!(value, "hi");
}

/// An agent that would not answer the writer says so beside its name, a
/// warning badge the item's name carries too; a choice with no hint draws
/// none, and the badge adds no fault to the menu's known ones.
#[test]
fn a_mention_choice_carries_its_hint() {
    let choices = vec![
        MentionChoice {
            token: "<@1>".into(),
            label: "Ada".into(),
            hint: None,
            offered: true,
        },
        MentionChoice {
            token: "<@2>".into(),
            label: "Scout".into(),
            hint: Some("Answers its manager only".into()),
            offered: true,
        },
    ];
    let root = lowered(&caret("@", 1), "c", &choices);
    let label = |key| match node(&root, key) {
        Some(wire::Node::Container(wire::ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) => interactivity.aria.label.as_deref().map(str::to_owned),
        _ => None,
    };
    assert_eq!(
        label("c/mention/<@2>").as_deref(),
        Some("@Scout, Answers its manager only")
    );
    assert_eq!(label("c/mention/<@1>").as_deref(), Some("@Ada"));
    assert!(node(&root, "c/mention/<@2>/hint").is_some());
    assert!(clickable(&root, "c/mention/<@2>").is_some(), "still picked");
    assert!(node(&root, "c/mention/<@1>/hint").is_none());
    let faults = wire::audit(&root);
    assert!(
        faults
            .iter()
            .all(|fault| fault.path.last().is_some_and(|key| !key.ends_with("/hint"))),
        "{faults:?}"
    );
}
