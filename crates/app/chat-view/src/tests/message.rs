use super::*;
use std::cell::RefCell;
use std::rc::Rc;
use view::{StyleRefinement, Styled};

#[test]
fn the_action_strip_hangs_over_the_rows_top_and_stays_under_the_pointer() {
    use view::px;
    let (mut cx, _view) = opened();
    assert!(
        cx.find("chat-message-m1-actions").is_none(),
        "no strip on a row the pointer is not over"
    );
    cx.simulate_hover("chat-message-m1-row", true);
    let strip = cx.interactivity("chat-message-m1-actions");
    assert!(
        strip.occlude,
        "the strip occludes the row above its overhang"
    );
    assert!(
        strip.on_hover.is_some(),
        "the strip says when the pointer is on it"
    );
    assert!(strip.group_hover.is_none());
    let row = cx.style("chat-message-m1-row");
    assert_eq!(row.padding.top, Some(px(8.).into()), "the run's gap");
    let card = cx.style("chat-message-m1-card");
    assert_eq!(card.padding.top, Some(px(4.).into()), "the card hugs");
    let style = cx.style("chat-message-m1-actions");
    assert_eq!(
        style.inset.top,
        Some(px(8. - 13.).into()),
        "half of its 26 over the card"
    );
    assert_eq!(
        style.visibility, None,
        "drawn only while the pointer is on the row"
    );
    let raised = view::Theme::default().surface_raised;
    let ground = |cx: &TestAppContext| {
        cx.style("chat-message-m1-card")
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|background| background.as_solid())
    };
    assert_eq!(ground(&cx), Some(raised));
    cx.simulate_hover("chat-message-m1-actions", true);
    cx.simulate_hover("chat-message-m1-row", false);
    assert!(
        cx.find("chat-message-m1-actions").is_some(),
        "the strip stays under the pointer"
    );
    assert_eq!(ground(&cx), Some(raised), "the card keeps its hover ground");
    cx.simulate_hover("chat-message-m2-row", true);
    assert!(cx.find("chat-message-m1-actions").is_none());
    assert!(cx.find("chat-message-m2-actions").is_some());
    assert_ne!(ground(&cx), Some(raised));
    cx.simulate_hover("chat-message-m2-row", false);
    assert!(cx.find("chat-message-m2-actions").is_none());
    cx.simulate_hover("chat-message-m1-row", true);
    assert!(
        cx.find("chat-message-m1").unwrap().children().is_empty(),
        "native action clicks must not bubble through selection"
    );
    assert!(cx.find("chat-message-m1-thumbs-up").is_some());
}

fn fire(cx: &mut TestAppContext, view: &Entity<Chat>, mine: bool) {
    cx.update(view, |chat, _, cx| {
        chat.room.as_mut().unwrap().messages.ready_mut().unwrap()[0]
            .reactions
            .push(chat::Reaction {
                emoji: "🔥".into(),
                count: 2,
                reacted_by_me: mine,
            });
        cx.notify();
    });
    cx.run_until_parked();
}

pub(super) fn hover(cx: &mut TestAppContext, view: &Entity<Chat>, seq: u64) {
    cx.update(view, |chat, _, cx| {
        chat.hovered = Some(crate::Hovered {
            key: (Pane::Timeline, seq),
            strip: false,
        });
        cx.notify();
    });
    cx.run_until_parked();
}

#[test]
fn copy_range_keeps_its_distinct_message_plate() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        chat.copy = Some(CopyRange {
            pane: Pane::Timeline,
            anchor: 1,
            head: 2,
        });
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(view::wire::ContainerNode { style, .. })) =
        cx.find("chat-message-m1-card")
    else {
        panic!("message card")
    };
    let style = &cx.styles()[*style];
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|color| color.as_solid()),
        Some(view::Theme::light().surface_raised)
    );
}

#[test]
fn reaction_rows_keep_add_action_and_selected_accessibility() {
    let (mut cx, view) = opened();
    fire(&mut cx, &view, true);
    let Some(wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find("chat-message-m1-reaction-🔥")
    else {
        panic!("reaction pill")
    };
    assert_eq!(interactivity.aria.description.as_deref(), Some("2"));
    assert!(interactivity.aria.toggled.is_some());
    assert!(cx.find("chat-message-m1-reaction-add").is_some());
    cx.simulate_click("chat-message-m1-reaction-add");
    view.read(|chat| {
        assert!(
            chat.menu
                .as_ref()
                .is_some_and(|menu| menu.mode == Mode::Reactions && menu.seq == 1)
        )
    });
}

#[test]
fn a_reaction_chip_is_named_by_its_emoji_whether_toggled_or_not() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let rows = chat.room.as_mut().unwrap().messages.ready_mut().unwrap();
        for (row, mine) in rows.iter_mut().zip([true, false]) {
            row.reactions.push(chat::Reaction {
                emoji: "👍".into(),
                count: 1,
                reacted_by_me: mine,
            });
        }
        cx.notify();
    });
    cx.run_until_parked();
    hover(&mut cx, &view, 1);
    let aria = |key: &str| match cx.find(key) {
        Some(wire::Node::Container(view::wire::ContainerNode {
            interactivity: Some(interactivity),
            ..
        })) => interactivity.aria.clone(),
        _ => panic!("{key} is a container"),
    };
    let (on, off) = (
        aria("chat-message-m1-reaction-👍"),
        aria("chat-message-m2-reaction-👍"),
    );
    assert_eq!(off.label, on.label, "one name, toggled or not");
    assert_eq!(on.label.as_deref(), Some("👍 reaction"));
    assert_eq!(on.toggled, Some(true.into()));
    assert_eq!(off.toggled, Some(false.into()));
    assert_eq!(
        aria("chat-message-m1-reaction-add").label.as_deref(),
        Some("Add reaction")
    );
    assert_eq!(
        aria("chat-message-m1-thumbs-up").label.as_deref(),
        Some("React with 👍")
    );
}

#[test]
fn a_reaction_chip_names_who_chose_it() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap()[0]
            .reactions
            .push(chat::Reaction {
                emoji: "👍".into(),
                count: 2,
                reacted_by_me: true,
            });
        room.read_marks(cx);
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(chip),
        ..
    })) = cx.find("chat-message-m1-reaction-👍")
    else {
        panic!("the chip is a container");
    };
    assert_eq!(chip.aria.description.as_deref(), Some("2: eddy, ada"));
    assert!(chip.tooltip.is_some(), "the names show on hover");
}

#[test]
fn landed_rows_read_who_reacted_and_a_block_reads_them_again() {
    let who = Rc::new(RefCell::new(vec![Principal::Account(7)]));
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let by = who.clone();
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { channel_id, .. } if channel_id == "general" => {
                    let mut row = row(1, 7, "hello");
                    row.reactions = vec![chat::Reaction {
                        emoji: "👍".into(),
                        count: 1,
                        reacted_by_me: false,
                    }];
                    Reply::Roots(page(vec![row]))
                }
                Query::MessagesAround { channel_id, .. } if channel_id == "general" => {
                    let mut row = row(1, 7, "hello");
                    row.reactions = vec![chat::Reaction {
                        emoji: "👍".into(),
                        count: 1,
                        reacted_by_me: false,
                    }];
                    Reply::Messages(vec![row])
                }
                Query::Reactors { seqs, .. } => Reply::Reactors(
                    seqs.into_iter()
                        .map(|seq| chat::ReactedBy {
                            seq,
                            emoji: "👍".into(),
                            by: by.borrow().clone(),
                        })
                        .collect(),
                ),
                query => chat_reply(query),
            })
        }));
    cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let names = |cx: &TestAppContext| match cx.find("chat-message-m1-reaction-👍") {
        Some(wire::Node::Container(view::wire::ContainerNode {
            interactivity: Some(chip),
            ..
        })) => chip.aria.description.clone(),
        _ => panic!("the chip is a container"),
    };
    assert_eq!(names(&cx).as_deref(), Some("1: eddy"));
    *who.borrow_mut() = vec![Principal::Account(10)];
    let general = "general".to_owned();
    cx.host().stream::<Changes<::chat::Chat>>().send(block(
        4,
        vec![
            chat::tables::MESSAGES.key(&(general.clone(), 1u64)),
            chat::tables::REACTIONS.key(&(general, 1u64, "👍".to_owned(), Principal::Account(10))),
        ],
    ));
    cx.run_until_parked();
    assert_eq!(names(&cx).as_deref(), Some("1: ada"));
}

#[test]
fn the_small_press_targets_are_at_least_24_px_each_way() {
    use view::px;
    let (mut cx, view) = opened();
    fire(&mut cx, &view, true);
    let style = |key: &str| cx.style(key).clone();
    for key in ["chat-message-m1-height", "chat-sidebar-new-channel"] {
        let style = style(key);
        assert_eq!(
            (style.min_size.width, style.min_size.height),
            (Some(px(24.).into()), Some(px(24.).into())),
            "{key}"
        );
    }
    for key in [
        "chat-message-m1-reaction-🔥",
        "chat-message-m1-reaction-add",
    ] {
        let style = style(key);
        assert_eq!(
            (style.min_size.width, style.size.height),
            (Some(px(24.).into()), Some(px(24.).into())),
            "{key}"
        );
    }
}

#[test]
fn the_strip_and_the_glyph_buttons_are_at_least_24_px_each_way() {
    use view::{design, px};
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        chat.search.draft.reset("hello");
        chat.confirmation = "Saved".into();
        chat.hovered = Some(crate::Hovered {
            key: (Pane::Timeline, 1),
            strip: false,
        });
        cx.notify();
    });
    cx.run_until_parked();
    let style = |key: &str| cx.style(key).clone();
    assert!(design::size::ROW >= px(24.));
    for key in ["thread", "thumbs-up", "react", "more"] {
        let style = style(&format!("chat-message-m1-{key}"));
        assert_eq!(
            (style.size.width, style.size.height),
            (Some(design::size::ROW.into()), Some(px(24.).into())),
            "{key}"
        );
    }
    for key in [
        "chat-sidebar-clear-search",
        "chat-room-confirmation-dismiss",
    ] {
        let style = style(key);
        assert_eq!(
            (style.min_size.width, style.min_size.height),
            (Some(px(24.).into()), Some(px(24.).into())),
            "{key}"
        );
    }
}

#[test]
fn thread_root_uses_reply_count_as_a_separator() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        room.messages.ready_mut().unwrap()[0].reply_count = 2;
        room.thread = Some(Thread {
            root: 1,
            replies: Loadable::Ready(Vec::new()),
            ..Thread::default()
        });
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-message-m1-reply-separator").is_some());
    assert!(cx.has_text("2 replies"));
}

#[test]
fn replies_read_as_a_button() {
    let (mut cx, view) = opened();
    cx.update(&view, |chat, _, cx| {
        if let Some(Loadable::Ready(rows) | Loadable::Reloading(rows, _)) =
            chat.room.as_mut().map(|room| &mut room.messages)
        {
            rows[0].reply_count = 3;
        }
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Container(view::wire::ContainerNode {
        style,
        interactivity: Some(interactivity),
        ..
    })) = cx.find("chat-message-m1-replies")
    else {
        panic!("replies button")
    };
    let style = &cx.styles()[*style];
    assert_eq!(interactivity.role, Some(view::Role::Button));
    assert!(!interactivity.focusable && interactivity.hover.is_some());
    assert_eq!(style.mouse_cursor, Some(view::CursorStyle::PointingHand));
    assert!(cx.has_text("Open thread →"));
    cx.simulate_click("chat-message-m1-replies");
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().unwrap().thread.as_ref().map(|t| t.root),
            Some(1)
        );
    });
}

#[test]
fn an_arrow_on_the_emoji_tabs_opens_the_next_category() {
    let (mut cx, view) = opened();
    hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    let tabs = cx.interactivity("chat-reaction-tabs");
    assert_eq!(tabs.role, Some(view::Role::TabList));
    assert!(tabs.focusable && tabs.tab_stop == Some(true));
    assert!(!cx.interactivity("chat-reaction-tab-Smileys").focusable);
    assert!(
        cx.interactivity("chat-reaction-tab-Smileys")
            .aria
            .active_descendant
    );
    cx.simulate_focus("chat-reaction-tabs");
    cx.simulate_key_down("chat-reaction-tabs", "right");
    view.read(|chat| assert_eq!(chat.picker.tab, 1));
    assert!(
        cx.find("chat-reaction-People-👋").is_some(),
        "the next category"
    );
    assert!(
        cx.find("chat-reaction-Smileys-😀").is_none(),
        "in place of the first"
    );
    let people = cx.interactivity("chat-reaction-tab-People");
    assert_eq!(people.aria.selected, Some(true));
    assert!(people.aria.active_descendant);
}

#[test]
fn the_picker_searches_and_enter_picks_the_first_match() {
    let (mut cx, view) = opened();
    hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    assert!(cx.find("chat-reaction-🔥").is_some(), "the frequent row");
    assert!(
        cx.find("chat-reaction-Smileys-😀").is_some(),
        "the first tab"
    );
    cx.simulate_click("chat-reaction-tab-Food");
    assert!(cx.find("chat-reaction-Food-🍕").is_some());
    let focus = ui::menu::focus_key(Pane::Timeline, Mode::Reactions);
    cx.simulate_input(&focus, "duck");
    assert!(cx.has_text("1 MATCH"));
    cx.simulate_submit(&focus);
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(op, Op::AddReaction { emoji, .. } if emoji == "🦆"))
    );
    view.read(|chat| {
        assert!(chat.menu.is_none());
        assert_eq!(chat.recent_emoji.first().map(String::as_str), Some("🦆"));
    });
}

#[test]
fn the_timeline_is_a_grid_whose_arrows_walk_messages_and_their_controls() {
    let (mut cx, view) = opened();
    fire(&mut cx, &view, false);
    let grid = cx.interactivity("chat-message-list");
    assert_eq!(grid.role, Some(view::Role::Grid));
    assert!(grid.focusable && grid.tab_stop == Some(true));
    let newest = cx.interactivity("chat-message-m2");
    assert_eq!(newest.role, Some(view::Role::GridCell));
    assert!(newest.aria.active_descendant, "the newest message on entry");
    assert!(!newest.focusable && newest.on_click.is_some());
    let row = cx.interactivity("chat-message-m2-row");
    assert_eq!(row.role, Some(view::Role::Row));
    assert!(!row.focusable && !row.aria.active_descendant && row.on_click.is_none());
    assert!(
        row.aria
            .label
            .as_deref()
            .is_some_and(|label| label.starts_with("Select message, shows its actions:"))
    );

    cx.simulate_focus("chat-message-list");
    cx.simulate_key_down("chat-message-list", "up");
    assert!(cx.interactivity("chat-message-m1").aria.active_descendant);
    assert!(!super::room::claims(&cx, "chat-message-m2"));
    let Some(wire::Node::List { commands, .. }) = cx
        .find("chat-message-list")
        .and_then(|grid| grid.children().first())
    else {
        panic!("the grid holds the list");
    };
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, wire::ListCommand::ScrollToRevealItem(1))),
        "the list reveals the message (after the intro row) before it claims: {commands:?}"
    );

    cx.simulate_key_down("chat-message-list", "right");
    let avatar = cx.interactivity("chat-message-m1-avatar");
    assert_eq!(avatar.role, Some(view::Role::Link));
    assert!(avatar.aria.active_descendant && !avatar.focusable);
    cx.simulate_key_down("chat-message-list", "right");
    let link = cx.interactivity("chat-message-m1-height");
    assert_eq!(link.role, Some(view::Role::Link));
    assert!(link.aria.active_descendant && !link.focusable);
    let cell = cx.interactivity("chat-message-m1-height-cell");
    assert_eq!(cell.role, Some(view::Role::GridCell));
    assert!(
        !cell.aria.active_descendant,
        "the control claims, not its cell"
    );
    cx.simulate_key_down("chat-message-list", "right");
    let chip = cx.interactivity("chat-message-m1-reaction-🔥");
    assert_eq!(chip.role, Some(view::Role::Button));
    assert!(chip.aria.active_descendant && !chip.focusable);
    assert!(!cx.interactivity("chat-message-m1").aria.active_descendant);
    cx.simulate_key_down("chat-message-list", "enter");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(op, Op::AddReaction { emoji, seq: 1, .. } if emoji == "🔥"))
    );
    cx.simulate_key_down("chat-message-list", "home");
    assert!(cx.interactivity("chat-message-m1").aria.active_descendant);
    assert!(cx.find("chat-message-m1-actions").is_none());
    cx.simulate_key_down("chat-message-list", "enter");
    view.read(|chat| {
        let menu = chat.menu.as_ref().expect("the message is chosen");
        assert_eq!((menu.seq, menu.mode), (1, Mode::Toolbar));
    });
    assert!(cx.find("chat-message-m1-actions").is_some());
    cx.simulate_key_down("chat-message-list", "end");
    assert!(
        cx.interactivity("chat-message-m1-more")
            .aria
            .active_descendant
    );
    let strip = cx.find("chat-message-m1-actions").expect("the strip");
    assert!(strip.children().iter().all(|cell| {
        cell.interactivity()
            .is_some_and(|cell| cell.role == Some(view::Role::GridCell))
            && cell.children().len() == 1
    }));
    assert!(
        cx.find("chat-message-m1-row")
            .expect("the row")
            .children()
            .iter()
            .any(|child| child.key() == Some("chat-message-m1-actions")),
        "the strip's cells are the row's, not the card's"
    );
    cx.simulate_key_down("chat-message-list", "enter");
    view.read(|chat| {
        let menu = chat.menu.as_ref().expect("the menu opened");
        assert_eq!(menu.mode, Mode::More);
        assert_eq!(
            menu.at,
            chat.key_spot(Pane::Timeline),
            "a key opens it at the list's spot"
        );
    });
}

#[test]
fn a_reaction_chip_is_its_own_cell_of_the_row() {
    let (mut cx, view) = opened();
    let fire = || chat::Reaction {
        emoji: "🔥".into(),
        count: 2,
        reacted_by_me: false,
    };
    cx.update(&view, |chat, _, cx| {
        let room = chat.room.as_mut().unwrap();
        let rows = room.messages.ready_mut().unwrap();
        rows[0].reactions.push(fire());
        rows[0].reply_count = 1;
        room.thread = Some(Thread {
            root: 1,
            replies: Loadable::Ready(vec![MsgRow {
                thread: Some(1),
                reactions: vec![fire()],
                ..row(3, 8, "a reply")
            }]),
            ..Thread::default()
        });
        cx.notify();
    });
    cx.run_until_parked();
    fn is_cell(node: &wire::Node) -> bool {
        node.interactivity()
            .is_some_and(|node| node.role == Some(view::Role::GridCell))
    }
    fn holds(node: &wire::Node, key: &str) -> bool {
        node.children()
            .iter()
            .any(|child| child.key() == Some(key) || holds(child, key))
    }
    fn cells(node: &wire::Node, inside: bool, found: &mut Vec<String>) -> bool {
        let cell = is_cell(node);
        if cell {
            found.push(node.key().unwrap_or_default().to_owned());
        }
        let nested = cell && inside;
        node.children().iter().fold(nested, |nested, child| {
            cells(child, inside || cell, found) || nested
        })
    }
    for (m, pane) in [("m1", "timeline"), ("m3", "thread")] {
        let chip = format!("chat-message-{m}-reaction-🔥");
        let cell = cx.find(&format!("{chip}-cell")).expect("the chip's cell");
        assert!(is_cell(cell), "{pane}");
        assert_eq!(
            cell.children()
                .iter()
                .map(wire::Node::key)
                .collect::<Vec<_>>(),
            [Some(chip.as_str())],
            "{pane}: the chip, alone in its cell"
        );
        let message = cx.find(&format!("chat-message-{m}")).unwrap();
        assert!(is_cell(message) && message.children().is_empty(), "{pane}");
        let wire::Node::Container(view::wire::ContainerNode { style, .. }) = message else {
            panic!("{pane}: the message's cell")
        };
        let style = &cx.styles()[*style];
        assert_eq!(
            style.position,
            StyleRefinement::default().absolute().position,
            "{pane}"
        );
        assert_eq!(
            style.inset,
            StyleRefinement::default().inset_0().inset,
            "{pane}"
        );
        let row = cx.find(&format!("chat-message-{m}-row")).unwrap();
        let mut found = Vec::new();
        assert!(!cells(row, false, &mut found), "{pane}: a cell in a cell");
        assert!(found.contains(&format!("{chip}-cell")), "{pane}: {found:?}");
        assert!(holds(row, &chip), "{pane}");
    }
    let mut found = Vec::new();
    cells(cx.find("chat-message-m1-row").unwrap(), false, &mut found);
    assert_eq!(
        found,
        [
            "chat-message-m1",
            "chat-message-m1-avatar-cell",
            "chat-message-m1-height-cell",
            "chat-message-m1-reaction-🔥-cell",
            "chat-message-m1-reaction-add-cell",
            "chat-message-m1-replies-cell",
        ]
    );
    cx.simulate_click("chat-message-m1");
    view.read(|chat| {
        let menu = chat.menu.as_ref().expect("the message is chosen");
        assert_eq!(
            (menu.pane, menu.seq, menu.mode),
            (Pane::Timeline, 1, Mode::Toolbar)
        );
    });
    fn clickable(node: &wire::Node, found: &mut Vec<(String, bool)>) {
        if let Some(interactivity) = node.interactivity()
            && interactivity.on_click.is_some()
        {
            let key = node.key().unwrap_or_default().to_owned();
            found.push((key, interactivity.consumes_click));
        }
        node.children()
            .iter()
            .for_each(|child| clickable(child, found));
    }
    let mut found = Vec::new();
    clickable(cx.find("chat-message-m1-row").unwrap(), &mut found);
    let (card, controls): (Vec<_>, Vec<_>) = found
        .into_iter()
        .partition(|(key, _)| key == "chat-message-m1");
    assert_eq!(card, [("chat-message-m1".to_owned(), false)]);
    assert!(controls.len() >= 6, "{controls:?}");
    assert!(
        controls.iter().all(|(_, consumes)| *consumes),
        "{controls:?}"
    );
    cx.update(&view, |chat, _, cx| {
        let channels = chat.channels.ready_mut().unwrap();
        let general = channels
            .iter_mut()
            .find(|info| info.channel.id == "general");
        general.unwrap().channel.archived = true;
        cx.notify();
    });
    cx.run_until_parked();
    view.read(|chat| assert!(!chat.may_write()));
    let disabled = |id: &str| {
        let node = cx.find(id).and_then(wire::Node::interactivity);
        node.unwrap_or_else(|| panic!("{id}")).aria.disabled
    };
    for id in [
        "chat-message-m1-reaction-🔥-cell",
        "chat-message-m1-reaction-add-cell",
        "chat-message-m1-thumbs-up-cell",
        "chat-message-m1-react-cell",
        "chat-message-m3-reaction-🔥-cell",
        "chat-message-m3-reaction-add-cell",
    ] {
        assert_eq!(disabled(id), Some(true), "{id}");
    }
    for id in [
        "chat-message-m1",
        "chat-message-m1-height-cell",
        "chat-message-m1-replies-cell",
        "chat-message-m1-more-cell",
    ] {
        assert_eq!(disabled(id), None, "{id}");
    }
}

#[test]
fn enter_presses_no_control_of_a_row_not_drawn_since_the_cursor_moved() {
    let (mut cx, view) = opened();
    fire(&mut cx, &view, false);
    cx.simulate_focus("chat-message-list");
    cx.simulate_key_down("chat-message-list", "up");
    for _ in 0..3 {
        cx.simulate_key_down("chat-message-list", "right");
    }
    assert!(super::room::claims(&cx, "chat-message-m1-reaction-🔥"));
    cx.update(&view, |chat, _, _| {
        chat.timeline_cursor.controls_of = Some("m2".into());
    });
    cx.simulate_key_down("chat-message-list", "enter");
    cx.run_until_parked();
    let reacted = |cx: &TestAppContext| {
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(op, Op::AddReaction { emoji, seq: 1, .. } if emoji == "🔥"))
    };
    assert!(!reacted(&cx), "a stale row's control is not pressed");
    cx.update(&view, |_, _, cx| cx.notify());
    cx.run_until_parked();
    cx.simulate_key_down("chat-message-list", "enter");
    cx.run_until_parked();
    assert!(reacted(&cx));
}

#[test]
fn two_arrows_and_enter_on_the_emoji_grid_add_the_third_emoji() {
    let (mut cx, view) = opened();
    hover(&mut cx, &view, 1);
    cx.simulate_click("chat-message-m1-react");
    let grid = cx.interactivity("chat-reaction-grid");
    assert_eq!(grid.role, Some(view::Role::Grid));
    assert!(grid.focusable && grid.tab_stop == Some(true));
    let rows = cx.find("chat-reaction-grid").unwrap().children();
    assert_eq!(rows.len(), emoji::PER_TAB.div_ceil(8));
    assert_eq!(rows[0].children().len(), 8, "rows of eight");
    let category = &emoji::CATEGORIES[0];
    let third = category.emoji[2].0;
    let key = |emoji: &str| format!("chat-reaction-{}-{emoji}", category.name);
    assert!(
        cx.interactivity(&key(category.emoji[0].0))
            .aria
            .active_descendant
    );
    assert!(!cx.interactivity(&key(third)).focusable);
    cx.simulate_focus("chat-reaction-grid");
    cx.simulate_key_down("chat-reaction-grid", "right");
    cx.simulate_key_down("chat-reaction-grid", "right");
    assert!(cx.interactivity(&key(third)).aria.active_descendant);
    cx.simulate_key_down("chat-reaction-grid", "down");
    assert!(
        cx.interactivity(&key(category.emoji[10].0))
            .aria
            .active_descendant
    );
    cx.simulate_key_down("chat-reaction-grid", "up");
    cx.simulate_key_down("chat-reaction-grid", "enter");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(op, Op::AddReaction { emoji, seq: 1, .. } if emoji == third))
    );
    view.read(|chat| assert!(chat.menu.is_none(), "a reaction closes the picker"));
}
