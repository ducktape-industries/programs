//! The aria a view writes: `sanitize` pulls it into what the host maps.
use super::*;

#[test]
fn sanitize_refuses_a_consumed_key_gpui_cannot_read() {
    let frame = |keys: Vec<String>| Frame {
        root: Some(Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                consumes_keys: keys.into_iter().map(Into::into).collect(),
                ..Default::default()
            })),
            children: Vec::new(),
        })),
        ..Frame::default()
    };
    let sanitized =
        |keys: &[&str]| sanitize(&mut frame(keys.iter().map(|k| k.to_string()).collect()));
    assert!(
        sanitized(&["escape", "shift-tab", "ctrl-enter"])
            .unwrap()
            .is_empty()
    );
    for unread in ["esc-ape", "ctrl-a-b"] {
        assert_eq!(
            sanitized(&["escape", unread]),
            Err(Refused::Invalid("a consumed key gpui cannot read")),
            "{unread:?}"
        );
    }
}

#[test]
fn sanitize_refuses_a_consumed_click_with_no_click() {
    let frame = |on_click| Frame {
        root: Some(Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                on_click,
                consumes_click: true,
                ..Default::default()
            })),
            children: Vec::new(),
        })),
        ..Frame::default()
    };
    assert!(sanitize(&mut frame(Some(1))).unwrap().is_empty());
    assert_eq!(
        sanitize(&mut frame(None)),
        Err(Refused::Invalid("consumes a click it does not take"))
    );
}

fn sanitized(interactivity: Interactivity) -> Result<Interactivity, Refused> {
    let mut frame = Frame {
        root: Some(Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Some(Box::new(interactivity)),
            children: Vec::new(),
        })),
        ..Frame::default()
    };
    sanitize(&mut frame)?;
    let Some(Node::Container(ContainerNode { interactivity, .. })) = frame.root else {
        unreachable!("a container stays a container")
    };
    Ok(interactivity.map(|kept| *kept).unwrap_or_default())
}

fn aria(aria: Aria) -> Aria {
    sanitized(Interactivity {
        aria,
        ..Default::default()
    })
    .unwrap()
    .aria
}

#[test]
fn a_role_that_says_nothing_or_is_the_windows_is_dropped() {
    use gpui::Role::*;
    for role in [
        GenericContainer,
        Unknown,
        Window,
        Application,
        RootWebArea,
        Pane,
        Iframe,
        IframePresentational,
        WebView,
        TitleBar,
    ] {
        let kept = sanitized(Interactivity {
            role: Some(role),
            ..Default::default()
        });
        assert_eq!(kept.unwrap().role, None, "{role:?}");
    }
    let button = sanitized(Interactivity {
        role: Some(Button),
        ..Default::default()
    });
    assert_eq!(button.unwrap().role, Some(Button));
}

#[test]
fn a_heading_level_outside_1_to_6_is_no_level() {
    for (level, kept) in [
        (0, None),
        (1, Some(1)),
        (6, Some(6)),
        (7, None),
        (usize::MAX, None),
    ] {
        let heading = sanitized(Interactivity {
            role: Some(gpui::Role::Heading),
            aria: Aria {
                level: Some(level),
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(heading.unwrap().aria.level, kept, "level {level}");
    }
    let item = sanitized(Interactivity {
        role: Some(gpui::Role::TreeItem),
        aria: Aria {
            level: Some(usize::MAX),
            ..Default::default()
        },
        ..Default::default()
    });
    assert_eq!(item.unwrap().aria.level, Some(usize::MAX));
}

#[test]
fn a_focusable_node_is_not_its_own_active_descendant() {
    for (focusable, focus_handle) in [(true, None), (false, Some(7)), (false, None)] {
        let node = sanitized(Interactivity {
            role: Some(gpui::Role::ListBoxOption),
            focusable,
            focus_handle,
            aria: Aria {
                active_descendant: true,
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(
            node.unwrap().aria.active_descendant,
            !focusable && focus_handle.is_none()
        );
    }
}

fn claiming_option(key: &str) -> Node {
    Node::Container(ContainerNode {
        id: Some(ElementIdWire::Name(key.into())),
        style: PLAIN,
        interactivity: Some(Box::new(Interactivity {
            role: Some(gpui::Role::ListBoxOption),
            aria: Aria {
                active_descendant: true,
                ..Default::default()
            },
            ..Default::default()
        })),
        children: Vec::new(),
    })
}

#[test]
fn only_the_first_active_descendant_in_a_frame_is_kept() {
    let mut frame = Frame {
        root: Some(Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Default::default(),
            children: vec![claiming_option("first"), claiming_option("second")],
        })),
        ..Frame::default()
    };
    sanitize(&mut frame).unwrap();
    let claims: Vec<bool> = frame.root.unwrap().children()[..]
        .iter()
        .map(|child| match child {
            Node::Container(ContainerNode {
                interactivity: Some(interactivity),
                ..
            }) => interactivity.aria.active_descendant,
            _ => unreachable!("a container stays a container"),
        })
        .collect();
    assert_eq!(claims, [true, false]);
}

#[test]
fn a_claim_under_each_focusable_ancestor_is_kept() {
    let list = |key: &str, rows: Vec<Node>| {
        Node::Container(ContainerNode {
            id: Some(ElementIdWire::Name(key.into())),
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                role: Some(gpui::Role::ListBox),
                focusable: true,
                ..Default::default()
            })),
            children: rows,
        })
    };
    let mut frame = Frame {
        root: Some(Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Default::default(),
            children: vec![
                list(
                    "rooms",
                    vec![claiming_option("general"), claiming_option("random")],
                ),
                list("members", vec![claiming_option("minseo")]),
            ],
        })),
        ..Frame::default()
    };
    sanitize(&mut frame).unwrap();
    let mut claims = Vec::new();
    frame.root.unwrap().for_each_mut(&mut |node| {
        if let Node::Container(ContainerNode {
            id: Some(id),
            interactivity: Some(interactivity),
            ..
        }) = node
            && interactivity.role == Some(gpui::Role::ListBoxOption)
        {
            claims.push((id.clone(), interactivity.aria.active_descendant));
        }
    });
    let name = |key: &str| ElementIdWire::Name(key.into());
    assert_eq!(
        claims,
        [
            (name("general"), true),
            (name("random"), false),
            (name("minseo"), true)
        ]
    );
}

#[test]
fn a_roleless_focusable_box_does_not_restart_the_claim_budget() {
    let box_ = |key: &str, row: Node| {
        Node::Container(ContainerNode {
            id: Some(ElementIdWire::Name(key.into())),
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                focusable: true,
                ..Default::default()
            })),
            children: vec![row],
        })
    };
    let mut frame = Frame {
        root: Some(Node::Container(ContainerNode {
            id: Some(ElementIdWire::Name("pane".into())),
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                role: Some(gpui::Role::Group),
                focusable: true,
                ..Default::default()
            })),
            children: vec![
                box_("channels", claiming_option("general")),
                box_("people", claiming_option("minseo")),
            ],
        })),
        ..Frame::default()
    };
    sanitize(&mut frame).unwrap();
    let mut claims = Vec::new();
    frame.root.unwrap().for_each_mut(&mut |node| {
        if let Node::Container(ContainerNode {
            interactivity: Some(interactivity),
            ..
        }) = node
            && interactivity.role == Some(gpui::Role::ListBoxOption)
        {
            claims.push(interactivity.aria.active_descendant);
        }
    });
    assert_eq!(claims, [true, false]);
}

#[test]
fn live_off_is_no_live_region() {
    let off = aria(Aria {
        live: Some(Live::Off),
        ..Default::default()
    });
    assert_eq!(off.live, None);
    let polite = aria(Aria {
        live: Some(Live::Polite),
        ..Default::default()
    });
    assert_eq!(polite.live, Some(Live::Polite));
}

#[test]
fn actions_the_host_owns_or_repeats_are_dropped() {
    let kept = aria(Aria {
        actions: [
            Action::Click,
            Action::Increment,
            Action::Focus,
            Action::Blur,
            Action::SetValue,
            Action::ReplaceSelectedText,
            Action::SetTextSelection,
            Action::Increment,
            Action::Decrement,
        ]
        .into_iter()
        .zip(1..)
        .collect(),
        ..Default::default()
    });
    assert_eq!(
        kept.actions,
        vec![(Action::Increment, 2), (Action::Decrement, 9)]
    );
}

#[test]
fn custom_actions_are_unique() {
    let kept = aria(Aria {
        custom_actions: [1, 1, 2, 1, 3].map(|id| (id, "Pin".into())).to_vec(),
        ..Default::default()
    });
    assert_eq!(
        kept.custom_actions
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
}

#[test]
fn list_and_resize_handle_interactivity_is_sanitized() {
    let hostile = || Interactivity {
        role: Some(gpui::Role::GenericContainer),
        aria: Aria {
            live: Some(Live::Off),
            ..Default::default()
        },
        focus: Some(StyleId(1)),
        ..Default::default()
    };
    let mut frame = Frame {
        styles: common::table(&[gen_native_style(&mut Rng::new(5))]),
        root: Some(Node::ResizeHandle {
            id: ElementIdWire::Name("divider".into()),
            style: PLAIN,
            interactivity: Some(Box::new(hostile())),
            on_press: None,
            on_release: None,
            on_drag: None,
            cursor: None,
            content: Box::new(Node::List {
                id: ElementIdWire::Name("rows".into()),
                path: vec![
                    ElementIdWire::Name("divider".into()),
                    ElementIdWire::Name("rows".into()),
                ],
                item_count: 0,
                alignment: ListAlignment::Top,
                overdraw: 0.,
                sizing: ListSizingBehavior::Infer,
                following_tail: false,
                revision: 0,
                commands: Vec::new(),
                request_handler: 1,
                scroll_handler: None,
                range_start: 0,
                style: PLAIN,
                interactivity: Some(Box::new(hostile())),
                children: Vec::new(),
            }),
        }),
        ..Frame::default()
    };
    let styles = table::sanitized(&mut frame).unwrap();
    let Some(Node::ResizeHandle {
        interactivity: handle,
        content,
        ..
    }) = frame.root
    else {
        unreachable!()
    };
    let Node::List {
        interactivity: list,
        ..
    } = *content
    else {
        unreachable!()
    };
    let (Some(handle), Some(list)) = (handle, list) else {
        panic!("both keep their interactivity")
    };
    for interactivity in [handle, list] {
        assert_eq!(interactivity.role, None);
        assert_eq!(interactivity.aria.live, None);
        check_native_style(&styles[interactivity.focus.unwrap()]);
    }
}

#[test]
fn sanitize_refuses_on_back_below_the_root() {
    let container = |on_back, children| {
        Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                on_back,
                ..Default::default()
            })),
            children,
        })
    };
    let frame = |root| Frame {
        root: Some(root),
        ..Frame::default()
    };
    assert!(
        sanitize(&mut frame(container(
            Some(1),
            vec![container(None, vec![])]
        )))
        .is_ok(),
        "on_back at the root"
    );
    assert_eq!(
        sanitize(&mut frame(container(
            None,
            vec![container(Some(1), vec![])]
        ))),
        Err(Refused::Invalid("on_back below the root"))
    );
}

#[test]
fn sanitize_refuses_a_sheet_the_host_cannot_present() {
    let named = |name: &str, detents: &[f32], child: Node| Node::Sheet {
        id: ElementIdWire::Name(name.into()),
        label: Some("Pick one".into()),
        style: PLAIN,
        detents: detents.to_vec(),
        on_dismiss: None,
        child: Box::new(child),
    };
    let sheet = |detents: &[f32], child: Node| named("picker", detents, child);
    let leaf = || {
        Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: None,
            children: Vec::new(),
        })
    };
    let frame = |root| Frame {
        root: Some(root),
        ..Frame::default()
    };
    assert!(sanitize(&mut frame(sheet(&[0.6, 1.0], leaf()))).is_ok());
    assert!(sanitize(&mut frame(sheet(&[], leaf()))).is_ok());
    for (detents, reason) in [
        (
            &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9][..],
            "a sheet has at most eight detents",
        ),
        (&[1.5][..], "a sheet detent is a fraction of the body"),
        (&[f32::NAN][..], "a sheet detent is a fraction of the body"),
        (&[0.8, 0.5][..], "sheet detents ascend"),
        (&[0.5, 0.5][..], "sheet detents ascend"),
    ] {
        assert_eq!(
            sanitize(&mut frame(sheet(detents, leaf()))),
            Err(Refused::Invalid(reason)),
            "{detents:?}"
        );
    }
    assert_eq!(
        sanitize(&mut frame(sheet(&[1.0], sheet(&[1.0], leaf())))),
        Err(Refused::Invalid("a sheet inside a sheet"))
    );
    let beside = |children: Vec<Node>| {
        Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: None,
            children,
        })
    };
    assert_eq!(
        sanitize(&mut frame(beside(vec![
            sheet(&[1.0], leaf()),
            named("other", &[1.0], leaf())
        ]))),
        Err(Refused::Invalid("a second sheet in one frame")),
        "one layer means one scrim"
    );
    assert_eq!(
        sanitize(&mut frame(beside(vec![
            beside(vec![sheet(&[1.0], leaf())]),
            named("other", &[1.0], leaf())
        ]))),
        Err(Refused::Invalid("a second sheet in one frame")),
        "wherever the second one sits"
    );
    assert_eq!(
        sanitize(&mut frame(sheet(&[0.0, 1.0], leaf()))),
        Err(Refused::Invalid("a sheet detent is a fraction of the body")),
        "a detent at zero is no height to rest at"
    );
}

#[test]
fn sanitize_refuses_a_root_shape_inside_a_tooltip() {
    let leaf = |on_back| {
        Node::Container(ContainerNode {
            id: None,
            style: PLAIN,
            interactivity: Some(Box::new(Interactivity {
                on_back,
                ..Default::default()
            })),
            children: Vec::new(),
        })
    };
    let frame = |content: Node| Frame {
        root: Some(leaf(None)),
        tooltip_responses: vec![TooltipResponse {
            request: 1,
            character_index: None,
            content: Some(Box::new(content)),
        }],
        ..Frame::default()
    };
    assert!(sanitize(&mut frame(leaf(None))).is_ok());
    assert_eq!(
        sanitize(&mut frame(leaf(Some(1)))),
        Err(Refused::Invalid("on_back below the root"))
    );
    assert_eq!(
        sanitize(&mut frame(Node::Sheet {
            id: ElementIdWire::Name("picker".into()),
            label: Some("Pick one".into()),
            style: PLAIN,
            detents: vec![1.0],
            on_dismiss: None,
            child: Box::new(leaf(None)),
        })),
        Err(Refused::Invalid("a sheet inside a tooltip"))
    );
}
