//! One tree that breaks each rule, and one that keeps it and every other.
use super::*;
use crate::{ContainerNode, ElementIdWire, InputOptions, Invalid, Live, StyleId, TextRange};
use FaultKind::*;

fn text(content: &str) -> Node {
    Node::Text(TextNode {
        id: None,
        style: StyleId(0),
        content: content.into(),
        transform: None,
    })
}

fn el(key: &str, interactivity: Interactivity, children: Vec<Node>) -> Node {
    Node::Container(ContainerNode {
        id: Some(ElementIdWire::Name(key.into())),
        style: StyleId(0),
        interactivity: Some(Box::new(interactivity)),
        children,
    })
}

fn roled(role: Role) -> Interactivity {
    Interactivity {
        role: Some(role),
        ..Default::default()
    }
}

fn control(role: Role) -> Interactivity {
    Interactivity {
        on_click: Some(1),
        ..stop(roled(role))
    }
}

fn stop(interactivity: Interactivity) -> Interactivity {
    Interactivity {
        focusable: true,
        tab_stop: Some(true),
        ..interactivity
    }
}

fn labelled(interactivity: Interactivity, label: &str) -> Interactivity {
    Interactivity {
        aria: Aria {
            label: Some(label.into()),
            ..interactivity.aria
        },
        ..interactivity
    }
}

fn button(key: &str, label: &str) -> Node {
    el(key, control(Role::Button), vec![text(label)])
}

fn input(label: &str) -> Node {
    Node::Field {
        id: ElementIdWire::Name("name".into()),
        multiline: false,
        value: String::new(),
        cursor: TextRange::default(),
        generation: 0,
        revision: 0,
        tokens: Default::default(),
        claims: Default::default(),
        options: Box::new(InputOptions {
            label: label.into(),
            ..Default::default()
        }),
        placeholder: String::new(),
        secure: false,
        on_change: Some(1),
        on_key: None,
        on_submit: None,
        style: StyleId(0),
    }
}

fn rich(text: &str, clickable: std::ops::Range<usize>) -> Node {
    Node::RichText {
        id: None,
        style: StyleId(0),
        text: text.into(),
        runs: Default::default(),
        font_family_overrides: Vec::new(),
        clickable_ranges: vec![clickable],
        on_click: Some(1),
        on_hover: None,
        tooltip: None,
    }
}

fn handle(interactivity: Interactivity) -> Node {
    Node::ResizeHandle {
        id: ElementIdWire::Name("divider".into()),
        style: StyleId(0),
        interactivity: Some(Box::new(interactivity)),
        on_press: None,
        on_release: None,
        on_drag: Some(1),
        cursor: None,
        content: Box::new(Node::empty()),
    }
}

fn image(interactivity: Interactivity, label: Option<&str>) -> Node {
    Node::Image {
        id: None,
        hash: 1,
        data: None,
        label: label.map(Into::into),
        image_style: crate::ImageStyle {
            grayscale: false,
            object_fit: crate::ImageObjectFit::Contain,
        },
        loading: false,
        fallback: false,
        state_children: Vec::new(),
        style: StyleId(0),
        interactivity: Some(Box::new(interactivity)),
    }
}

fn kinds(tree: &Node) -> Vec<FaultKind> {
    audit(tree).iter().map(|fault| fault.kind).collect()
}

fn fails(kind: FaultKind, tree: Node) {
    let kinds = kinds(&tree);
    assert!(kinds.contains(&kind), "{kind:?} not among {kinds:?}");
}

fn passes(tree: Node) {
    assert_eq!(audit(&tree), Vec::new());
}

#[test]
fn named_trees_pass_and_unlabeled_clickables_are_reported() {
    let faulty = el(
        "App",
        Interactivity::default(),
        vec![
            el(
                "App/open",
                Interactivity {
                    on_click: Some(1),
                    ..Default::default()
                },
                vec![text("Open")],
            ),
            text("a"),
            Node::Overlay {
                id: ElementIdWire::Name("ask".into()),
                label: Some(String::new()),
                style: StyleId(0),
                on_dismiss: None,
                children: vec![],
            },
        ],
    );
    assert_eq!(
        audit(&faulty),
        vec![
            Fault {
                path: vec!["App".into(), "App/open".into()],
                kind: NoRole,
            },
            Fault {
                path: vec!["App".into(), "App/open".into()],
                kind: Unreachable,
            },
            Fault {
                path: vec!["App".into(), "ask".into()],
                kind: Unnamed,
            },
        ]
    );
    passes(el(
        "App",
        Interactivity::default(),
        vec![button("App/open", "Open")],
    ));
}

#[test]
fn an_interactive_node_of_any_kind_without_a_role_fails() {
    fails(
        NoRole,
        el("open", stop(Interactivity::default()), vec![text("Open")]),
    );
    fails(
        NoRole,
        image(
            Interactivity {
                on_click: Some(1),
                ..Default::default()
            },
            Some("Avatar"),
        ),
    );
    fails(
        NoRole,
        el("open", control(Role::GenericContainer), vec![text("Open")]),
    );
}

#[test]
fn a_node_only_the_pointer_hears_needs_no_role() {
    passes(el(
        "row",
        Interactivity {
            on_hover: Some(1),
            on_mouse_move: Some(2),
            on_mouse_down_out: Some(3),
            on_scroll_wheel: Some(4),
            on_modifiers_changed: Some(5),
            ..Default::default()
        },
        vec![button("open", "Open")],
    ));
}

#[test]
fn a_roled_control_overlay_or_picture_nothing_names_fails() {
    fails(
        Unnamed,
        el("close", control(Role::Button), vec![text("  ")]),
    );
    fails(Unnamed, image(roled(Role::Image), Some(" ")));
    fails(
        Unnamed,
        Node::Overlay {
            id: ElementIdWire::Name("ask".into()),
            label: Some("  ".into()),
            style: StyleId(0),
            on_dismiss: None,
            children: vec![],
        },
    );
}

#[test]
fn a_named_control_overlay_and_picture_pass() {
    passes(el(
        "close",
        labelled(control(Role::Button), "Close"),
        Vec::new(),
    ));
    passes(image(roled(Role::Image), Some("Avatar")));
    passes(Node::Overlay {
        id: ElementIdWire::Name("ask".into()),
        label: Some("Settings".into()),
        style: StyleId(0),
        on_dismiss: Some(1),
        children: vec![text("base"), text("dialog")],
    });
}

#[test]
fn a_field_without_a_label_fails() {
    fails(UnlabeledInput, input("  "));
}

#[test]
fn a_labelled_field_passes() {
    passes(input("Display name"));
}

#[test]
fn a_name_of_glyphs_or_of_the_role_fails() {
    fails(GlyphName, button("close", "×"));
    fails(GlyphName, button("send", "button"));
}

#[test]
fn a_name_with_words_passes() {
    passes(button("close", "× Close"));
}

#[test]
fn aria_without_a_role_fails() {
    fails(
        OrphanAria,
        el(
            "chat",
            labelled(Interactivity::default(), "Chat"),
            Vec::new(),
        ),
    );
}

#[test]
fn aria_with_a_role_passes() {
    passes(el("chat", labelled(roled(Role::Group), "Chat"), Vec::new()));
}

#[test]
fn a_role_without_its_state_fails() {
    fails(
        MissingState,
        el("dark", labelled(control(Role::Switch), "Dark"), Vec::new()),
    );
    fails(
        MissingState,
        el(
            "room",
            labelled(control(Role::ComboBox), "Room"),
            Vec::new(),
        ),
    );
    fails(
        MissingState,
        el("title", roled(Role::Heading), vec![text("Inbox")]),
    );
    let tab = el("chat", control(Role::Tab), vec![text("Chat")]);
    fails(MissingState, el("tabs", roled(Role::TabList), vec![tab]));
}

#[test]
fn a_role_with_its_state_passes() {
    let with = |role, aria: Aria| {
        el(
            "dark",
            labelled(
                Interactivity {
                    aria,
                    ..control(role)
                },
                "Dark",
            ),
            Vec::new(),
        )
    };
    passes(with(
        Role::Switch,
        Aria {
            toggled: Some(true.into()),
            ..Default::default()
        },
    ));
    passes(with(
        Role::ComboBox,
        Aria {
            expanded: Some(false),
            ..Default::default()
        },
    ));
    let heading = Interactivity {
        aria: Aria {
            level: Some(1),
            ..Default::default()
        },
        ..roled(Role::Heading)
    };
    passes(el("title", heading, vec![text("Inbox")]));
    let tab = Interactivity {
        aria: Aria {
            selected: Some(true),
            ..Default::default()
        },
        ..control(Role::Tab)
    };
    passes(el(
        "tabs",
        roled(Role::TabList),
        vec![el("chat", tab, vec![text("Chat")])],
    ));
}

#[test]
fn a_disabled_control_that_answers_fails() {
    let disabled = Interactivity {
        aria: Aria {
            disabled: Some(true),
            ..Default::default()
        },
        ..control(Role::Button)
    };
    fails(DisabledButLive, el("send", disabled, vec![text("Send")]));
}

#[test]
fn a_disabled_control_without_a_route_passes() {
    let disabled = Interactivity {
        aria: Aria {
            disabled: Some(true),
            ..Default::default()
        },
        ..stop(roled(Role::Button))
    };
    passes(el("send", disabled, vec![text("Send")]));
}

#[test]
fn a_control_inside_a_control_fails() {
    let card = el(
        "card",
        control(Role::Button),
        vec![text("Message"), button("remove", "Remove")],
    );
    fails(NestedInteractive, card);
}

#[test]
fn controls_side_by_side_pass() {
    let row = el(
        "row",
        roled(Role::Group),
        vec![button("open", "Open"), button("remove", "Remove")],
    );
    passes(row);
}

#[test]
fn a_click_no_key_reaches_fails() {
    let unfocusable = Interactivity {
        focusable: false,
        tab_stop: None,
        ..control(Role::Button)
    };
    fails(Unreachable, el("open", unfocusable, vec![text("Open")]));
}

fn rooms(keys: Option<u32>) -> Node {
    let option = Interactivity {
        focusable: false,
        tab_stop: None,
        aria: Aria {
            selected: Some(false),
            ..Default::default()
        },
        ..control(Role::ListBoxOption)
    };
    let list = labelled(
        Interactivity {
            on_key_down: keys,
            ..stop(roled(Role::ListBox))
        },
        "Rooms",
    );
    el(
        "rooms",
        list,
        vec![el("general", option, vec![text("general")])],
    )
}

#[test]
fn a_row_of_a_focused_composite_the_keys_walk_passes() {
    passes(rooms(Some(2)));
}

#[test]
fn a_row_of_a_focused_composite_no_key_walks_fails() {
    fails(Unreachable, rooms(None));
}

fn claimed(claims: [Interactivity; 2]) -> Node {
    let list = labelled(
        Interactivity {
            on_key_down: Some(2),
            ..stop(roled(Role::ListBox))
        },
        "Rooms",
    );
    let rows = claims
        .into_iter()
        .zip(["general", "random"])
        .map(|(row, name)| {
            let option = Interactivity {
                aria: Aria {
                    selected: Some(false),
                    ..row.aria
                },
                ..row
            };
            el(name, option, vec![text(name)])
        });
    el("rooms", list, rows.collect())
}

fn claim(focusable: bool, focus_handle: Option<u64>) -> Interactivity {
    Interactivity {
        focusable,
        tab_stop: focusable.then_some(true),
        focus_handle,
        aria: Aria {
            active_descendant: true,
            ..Default::default()
        },
        ..roled(Role::ListBoxOption)
    }
}

#[test]
fn a_state_its_role_does_not_read_fails() {
    let selected = |role| Interactivity {
        aria: Aria {
            selected: Some(true),
            ..Default::default()
        },
        ..control(role)
    };
    fails(
        UnreadState,
        el("general", selected(Role::Button), vec![text("general")]),
    );
    fails(
        UnreadState,
        el("row", selected(Role::ListItem), vec![text("src/lib.rs")]),
    );
    let toggled = Interactivity {
        aria: Aria {
            toggled: Some(gpui::Toggled::True),
            ..Default::default()
        },
        ..control(Role::Link)
    };
    fails(UnreadState, el("open", toggled, vec![text("Open")]));
    let heading = Interactivity {
        aria: Aria {
            level: Some(1),
            toggled: Some(gpui::Toggled::True),
            ..Default::default()
        },
        ..roled(Role::Heading)
    };
    fails(UnreadState, el("title", heading, vec![text("Inbox")]));
}

#[test]
fn a_state_its_role_reads_passes() {
    let tab = Interactivity {
        aria: Aria {
            selected: Some(true),
            ..Default::default()
        },
        ..control(Role::Tab)
    };
    let tabs = labelled(roled(Role::TabList), "Pages");
    passes(el("tabs", tabs, vec![el("home", tab, vec![text("Home")])]));
    let pressed = Interactivity {
        aria: Aria {
            toggled: Some(gpui::Toggled::False),
            ..Default::default()
        },
        ..control(Role::Button)
    };
    passes(el("bold", pressed, vec![text("Bold")]));
    let checked = |role| Interactivity {
        aria: Aria {
            selected: Some(false),
            toggled: Some(gpui::Toggled::True),
            ..Default::default()
        },
        ..roled(role)
    };
    for (within, role) in [
        (Role::ListBox, Role::ListBoxOption),
        (Role::Tree, Role::TreeItem),
    ] {
        let item = el("src", checked(role), vec![text("src")]);
        passes(el("files", roled(within), vec![item]));
    }
}

#[test]
fn an_active_descendant_the_host_drops_fails() {
    let quiet = roled(Role::ListBoxOption);
    fails(
        ActiveDescendant,
        claimed([claim(true, None), quiet.clone()]),
    );
    fails(ActiveDescendant, claimed([claim(false, Some(7)), quiet]));
    fails(
        ActiveDescendant,
        claimed([claim(false, None), claim(false, None)]),
    );
}

#[test]
fn one_active_descendant_that_takes_no_focus_passes() {
    passes(claimed([claim(false, None), roled(Role::ListBoxOption)]));
    let quiet = claimed([claim(true, None), claim(false, None)]);
    assert_eq!(kinds(&quiet), [ActiveDescendant]);
}

#[test]
fn two_composites_on_one_screen_each_keep_their_claim() {
    let tab = Interactivity {
        aria: Aria {
            selected: Some(true),
            active_descendant: true,
            ..Default::default()
        },
        on_click: Some(1),
        ..roled(Role::Tab)
    };
    let tabs = el(
        "tabs",
        Interactivity {
            on_key_down: Some(2),
            ..stop(labelled(roled(Role::TabList), "Pages"))
        },
        vec![el("code", tab, vec![text("Code")])],
    );
    let screen = el(
        "screen",
        Interactivity::default(),
        vec![
            tabs,
            claimed([claim(false, None), roled(Role::ListBoxOption)]),
        ],
    );
    passes(screen);
}

#[test]
fn a_second_claim_under_the_same_focusable_ancestor_fails() {
    let option = |key: &str| {
        el(
            key,
            Interactivity {
                aria: Aria {
                    selected: Some(false),
                    ..claim(false, None).aria
                },
                ..claim(false, None)
            },
            vec![text(key)],
        )
    };
    let group = |key: &str, row: Node| el(key, Interactivity::default(), vec![row]);
    let list = el(
        "rooms",
        Interactivity {
            on_key_down: Some(2),
            ..stop(labelled(roled(Role::ListBox), "Rooms"))
        },
        vec![
            group("channels", option("general")),
            group("people", option("minseo")),
        ],
    );
    assert_eq!(kinds(&list), [ActiveDescendant]);
}

#[test]
fn a_roleless_focusable_box_between_two_claims_does_not_restart_the_budget() {
    let option = |key: &str| {
        el(
            key,
            Interactivity {
                aria: Aria {
                    selected: Some(false),
                    ..claim(false, None).aria
                },
                ..claim(false, None)
            },
            vec![text(key)],
        )
    };
    let box_ = |key: &str, row: Node| {
        el(
            key,
            Interactivity {
                focusable: true,
                tab_stop: Some(true),
                ..Default::default()
            },
            vec![row],
        )
    };
    let list = el(
        "rooms",
        Interactivity {
            on_key_down: Some(2),
            ..stop(labelled(roled(Role::ListBox), "Rooms"))
        },
        vec![
            box_("channels", option("general")),
            box_("people", option("minseo")),
        ],
    );
    fails(ActiveDescendant, list);
}

#[test]
fn a_keyed_composite_out_of_the_tab_order_fails() {
    let mut list = rooms(Some(2));
    let Node::Container(ContainerNode {
        interactivity: Some(interactivity),
        ..
    }) = &mut list
    else {
        unreachable!()
    };
    interactivity.tab_stop = Some(false);
    fails(Unreachable, list);
}

#[test]
fn a_focusable_click_out_of_the_tab_order_fails() {
    let skipped = Interactivity {
        tab_stop: Some(false),
        ..control(Role::Button)
    };
    fails(Unreachable, el("busy", skipped, vec![text("Saving")]));
    let indexed = Interactivity {
        tab_stop: None,
        tab_index: Some(0),
        ..control(Role::Button)
    };
    passes(el("open", indexed, vec![text("Open")]));
}

#[test]
fn an_item_outside_its_container_fails() {
    fails(
        Orphan,
        el("copy", control(Role::MenuItem), vec![text("Copy")]),
    );
    fails(Orphan, el("row", roled(Role::Row), vec![text("alice")]));
}

#[test]
fn an_item_inside_its_container_passes() {
    let menu = labelled(roled(Role::Menu), "Actions");
    passes(el(
        "menu",
        menu,
        vec![el("copy", control(Role::MenuItem), vec![text("Copy")])],
    ));
}

#[test]
fn a_clickable_range_without_words_fails() {
    fails(RangeUnnamed, rich("see → here", 4..7));
    fails(RangeUnnamed, rich("see", 2..9));
}

#[test]
fn a_clickable_range_with_words_passes() {
    passes(rich("see → here", 8..12));
}

#[test]
fn a_bare_resize_handle_fails() {
    fails(BareHandle, handle(Interactivity::default()));
    fails(
        BareHandle,
        handle(labelled(stop(roled(Role::Splitter)), "Resize")),
    );
}

#[test]
fn a_named_focusable_splitter_that_moves_by_key_passes() {
    let splitter = Interactivity {
        on_key_down: Some(2),
        ..stop(labelled(roled(Role::Splitter), "Resize sidebar"))
    };
    passes(handle(splitter));
}

#[test]
fn an_action_the_node_cannot_answer_fails() {
    let pin = Interactivity {
        aria: Aria {
            custom_actions: vec![(1, "Pin".into())],
            ..Default::default()
        },
        ..control(Role::Button)
    };
    fails(ActionUnhandled, el("message", pin, vec![text("Message")]));
    let step = Interactivity {
        aria: Aria {
            actions: vec![(Action::Increment, 2)],
            ..Default::default()
        },
        ..labelled(control(Role::SpinButton), "Size")
    };
    fails(ActionUnhandled, el("size", step, Vec::new()));
    let expand = Interactivity {
        aria: Aria {
            actions: vec![(Action::Expand, 2)],
            ..Default::default()
        },
        ..control(Role::Button)
    };
    fails(ActionUnhandled, el("more", expand, vec![text("More")]));
    for actions in [
        vec![(Action::Click, 2)],
        vec![(Action::Expand, 2), (Action::Expand, 3)],
    ] {
        let dropped = Interactivity {
            aria: Aria {
                actions,
                expanded: Some(false),
                ..Default::default()
            },
            ..control(Role::Button)
        };
        fails(ActionUnhandled, el("more", dropped, vec![text("More")]));
    }
}

#[test]
fn an_action_with_its_route_and_state_passes() {
    let pin = Interactivity {
        aria: Aria {
            custom_actions: vec![(1, "Pin".into())],
            actions: vec![(Action::CustomAction, 3), (Action::Expand, 4)],
            expanded: Some(false),
            ..Default::default()
        },
        ..control(Role::Button)
    };
    passes(el("message", pin, vec![text("Message")]));
}

#[test]
fn a_key_an_earlier_sibling_holds_fails() {
    let row = el(
        "row",
        roled(Role::Group),
        vec![button("open", "Open"), button("open", "Again")],
    );
    assert_eq!(
        audit(&row),
        vec![Fault {
            path: vec!["row".into(), "open".into()],
            kind: DuplicateKey,
        }]
    );
}

#[test]
fn distinct_keys_pass() {
    passes(el(
        "row",
        roled(Role::Group),
        vec![button("open", "Open"), button("again", "Again")],
    ));
}

#[test]
fn a_status_that_is_not_live_or_says_nothing_fails() {
    fails(
        StatusNotLive,
        el("status", roled(Role::Status), vec![text("Saved")]),
    );
    let silent = Interactivity {
        aria: Aria {
            live: Some(Live::Assertive),
            ..Default::default()
        },
        ..roled(Role::Alert)
    };
    fails(StatusNotLive, el("alert", silent, vec![text(" ")]));
}

#[test]
fn a_live_status_with_words_passes() {
    let status = Interactivity {
        aria: Aria {
            live: Some(Live::Polite),
            ..Default::default()
        },
        ..roled(Role::Status)
    };
    passes(el("status", status, vec![text("Saved")]));
}

#[test]
fn an_invalid_field_that_does_not_say_why_fails() {
    let field = Interactivity {
        aria: Aria {
            invalid: Some(Invalid::True),
            ..Default::default()
        },
        ..labelled(stop(roled(Role::TextInput)), "Email")
    };
    fails(ErrorNoText, el("email", field, Vec::new()));
    let mut input = input("Email");
    if let Node::Field { options, .. } = &mut input {
        options.invalid = Some(Invalid::True);
    }
    fails(ErrorNoText, input);
}

#[test]
fn an_invalid_field_that_says_why_passes() {
    let field = |aria: Aria| {
        let interactivity = Interactivity {
            aria: Aria {
                invalid: Some(Invalid::True),
                label: Some("Email".into()),
                ..aria
            },
            ..stop(roled(Role::TextInput))
        };
        el("email", interactivity, Vec::new())
    };
    passes(field(Aria {
        description: Some("An address has an @".into()),
        ..Default::default()
    }));
    let mut input = input("Email");
    if let Node::Field { options, .. } = &mut input {
        options.invalid = Some(Invalid::True);
        options.description = Some("An address has an @".into());
    }
    passes(input);
}
