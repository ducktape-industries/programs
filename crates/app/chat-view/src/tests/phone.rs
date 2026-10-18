//! Chat on a phone: the Expo app's screens (`ui/phone/`), each in the
//! states the Expo app has and through each of its actions, and the sweep
//! that holds every frame to the touch ladder at 360, 390 and 430
//! (SPEC §8.1d, §10).
use super::*;
use view::testing;

/// Boots on a phone and seats a reader: the rooms page, no room open.
fn phone() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    let view = cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    seat(&mut cx);
    (cx, view)
}

/// `general` opened from the rooms page.
fn in_general() -> (TestAppContext, Entity<Chat>) {
    let (mut cx, view) = phone();
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    (cx, view)
}

/// A finger held on `key`: the touch click its route hears, long.
fn long_press(cx: &mut TestAppContext, key: &str) {
    let handler = cx
        .interactivity(key)
        .on_click
        .unwrap_or_else(|| panic!("{key} has no click route"));
    cx.tick(vec![wire::Event::Click {
        handler,
        event: wire::click::Click::Touch {
            position: Default::default(),
            tap_count: 1,
            long_press: true,
        },
    }]);
}

/// A finger lifted from `key` at once: a tap.
fn tap(cx: &mut TestAppContext, key: &str) {
    let handler = cx.interactivity(key).on_click.expect("a click route");
    cx.tick(vec![wire::Event::Click {
        handler,
        event: wire::click::Click::Touch {
            position: Default::default(),
            tap_count: 1,
            long_press: false,
        },
    }]);
}

/// The chat program's answer to the channel list replaced by `reply`.
fn channels_answer(cx: &TestAppContext, reply: fn() -> Result<Vec<ChannelInfo>, String>) {
    cx.host()
        .handle::<Ask<::chat::Chat>>(move |query| match query {
            Query::Channels { .. } => reply()
                .map(|rooms| Reply::Channels(page(rooms)))
                .map_err(|why| view::host::Error::new("refused", &why)),
            query => Ok(chat_reply(query)),
        });
}

/// The ops the reader's writes submitted, in order.
fn submitted(cx: &TestAppContext) -> Vec<Op> {
    cx.host().requests::<Submit<::chat::Chat>>()
}

/// `general` changed in place, and the frame drawn again.
fn general_is(cx: &mut TestAppContext, view: &Entity<Chat>, change: impl FnOnce(&mut ChannelInfo)) {
    cx.update(view, |chat, _, cx| {
        let rooms = chat.channels.ready_mut().expect("the rooms are read");
        let general = rooms
            .iter_mut()
            .find(|info| info.channel.id == "general")
            .expect("general is listed");
        change(general);
        cx.notify();
    });
    cx.run_until_parked();
}

/// The open room's rows replaced by `rows`, and the frame drawn again.
fn rows_are(cx: &mut TestAppContext, view: &Entity<Chat>, rows: Vec<MsgRow>) {
    cx.update(view, |chat, _, cx| {
        let room = chat.room.as_mut().expect("a room is open");
        room.messages = Loadable::Ready(rows);
        cx.notify();
    });
    cx.run_until_parked();
}

// ---------- the rooms (`app/(tabs)/chat/index.tsx`) ----------

#[test]
fn the_rooms_page_lists_channels_and_direct_messages_with_their_counts() {
    let (cx, _) = phone();
    assert!(cx.find("chat-rooms-page").is_some());
    assert!(cx.has_text("Chat"));
    assert!(cx.has_text("testnet · eddy"), "{:?}", cx.texts());
    assert!(cx.has_text("Channels") && cx.has_text("General"));
    assert!(
        cx.has_text("Direct messages"),
        "the room with reviewer is open"
    );
    assert!(
        cx.find("chat-rooms-dm-8-agent").is_some(),
        "an agent wears Agent"
    );
    assert!(
        !cx.has_text("People") && cx.find("chat-rooms-person-10").is_none(),
        "no People section"
    );
    assert!(
        !cx.has_text("Review web#3"),
        "a program's rooms stay out of the list"
    );
    assert_eq!(
        cx.back_handler(),
        None,
        "the first page has nowhere to go back to"
    );
}

#[test]
fn an_unread_room_wears_its_dot_and_a_read_one_none() {
    let (mut cx, view) = phone();
    assert!(cx.find("chat-rooms-channel-general-unread").is_none());
    cx.update(&view, |chat, _, cx| {
        chat.reads.cursors.insert("general".into(), 1);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-rooms-channel-general-unread").is_some());
}

#[test]
fn a_members_only_channel_wears_its_chip() {
    let (mut cx, view) = phone();
    general_is(&mut cx, &view, |general| {
        general.channel.post_policy = PostPolicy::MembersOnly;
    });
    assert!(cx.find("chat-rooms-channel-general-members-only").is_some());
}

#[test]
fn an_archived_channel_leaves_the_rooms_page_and_comes_back_unarchived() {
    let (mut cx, view) = phone();
    general_is(&mut cx, &view, |general| general.channel.archived = true);
    assert!(cx.find("chat-rooms-channel-general").is_none());
    assert!(
        !cx.has_text("No rooms yet"),
        "an archived channel is still the network's"
    );
    general_is(&mut cx, &view, |general| general.channel.archived = false);
    assert!(cx.find("chat-rooms-channel-general").is_some());
}

#[test]
fn the_rooms_page_leads_with_the_readers_avatar() {
    let (cx, view) = phone();
    view.read(|chat| assert_eq!(chat.reader_name(), "eddy"));
    assert!(cx.find("chat-rooms-page-account").is_some());
}

#[test]
fn the_rooms_page_says_it_is_loading_until_the_channels_land() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().never::<Ask<::chat::Chat>>();
    cx.open::<Chat>();
    cx.simulate_touch(true);
    seat(&mut cx);
    assert!(cx.has_text("Loading rooms…"), "{:?}", cx.texts());
}

#[test]
fn a_network_with_no_channels_says_so_and_offers_new_channel() {
    let (mut cx, _) = {
        let mut cx = TestAppContext::new();
        configure(&mut cx);
        channels_answer(&cx, || Ok(Vec::new()));
        let view = cx.open::<Chat>();
        cx.simulate_touch(true);
        seat(&mut cx);
        (cx, view)
    };
    assert!(cx.has_text("No rooms yet"));
    assert!(cx.has_text(
        "Nobody has created a channel on testnet. Open one, or start a direct message below."
    ));
    cx.simulate_click("chat-rooms-empty-new-channel");
    assert!(cx.find("chat-new-page").is_some());
}

#[test]
fn a_refused_channel_list_says_why_and_retry_asks_again() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    channels_answer(&cx, || Err("the index is behind".into()));
    cx.open::<Chat>();
    cx.simulate_touch(true);
    seat(&mut cx);
    assert!(cx.has_text("the index is behind"));
    let asked = cx.host().requests::<Ask<::chat::Chat>>().len();
    cx.simulate_click("chat-rooms-retry");
    cx.run_until_parked();
    assert!(cx.host().requests::<Ask<::chat::Chat>>().len() > asked);
}

#[test]
fn without_a_node_the_rooms_page_says_so() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.run_until_parked();
    assert!(cx.has_text("Not connected to a node."));
    // the title row keeps its buttons, as the Expo header's does
    assert!(cx.find("chat-rooms-search").is_some());
    assert_eq!(cx.back_handler(), None);
}

#[test]
fn a_row_opens_its_room_and_back_returns_to_the_rooms() {
    let (mut cx, view) = in_general();
    assert!(cx.find("chat-room-page").is_some());
    cx.simulate_back();
    assert!(cx.find("chat-rooms-page").is_some());
    view.read(|chat| assert!(chat.room.is_none()));
}

// ---------- search (`app/(tabs)/chat/search.tsx`) ----------

#[test]
fn search_asks_on_the_search_key_and_a_hit_opens_its_room_over_the_page() {
    let (mut cx, _) = phone();
    // a hit lands the room on its message, read around it
    cx.host().handle::<Ask<::chat::Chat>>(|query| {
        Ok(match query {
            Query::MessagesAround {
                channel_id, seq, ..
            } => {
                assert_eq!((channel_id.as_str(), seq), ("general", 1));
                Reply::Messages(vec![row(1, 7, "hello"), row(2, 8, "**hi** there")])
            }
            query => chat_reply(query),
        })
    });
    cx.simulate_click("Search");
    assert!(cx.find("chat-search-page").is_some());
    assert!(cx.has_text("All rooms"));
    cx.simulate_input("search", "hello");
    cx.simulate_submit("search");
    cx.run_until_parked();
    assert!(cx.has_text("Messages") && cx.has_text("2 · newest first"));
    assert!(cx.find("chat-search-hit-general-1").is_some());
    cx.simulate_click("chat-search-hit-general-1");
    cx.run_until_parked();
    assert!(cx.find("chat-room-page").is_some());
    cx.simulate_back();
    assert!(
        cx.find("chat-search-page").is_some(),
        "Back returns to the hits"
    );
    assert!(cx.has_text("2 · newest first"));
}

#[test]
fn a_landed_room_offers_latest_until_it_reaches_the_newest_and_latest_reads_the_tail() {
    let (mut cx, view) = phone();
    cx.host().handle::<Ask<::chat::Chat>>(|query| {
        Ok(match query {
            Query::MessagesAround { .. } => {
                Reply::Messages(vec![row(1, 7, "hello"), row(2, 8, "**hi** there")])
            }
            query => chat_reply(query),
        })
    });
    let land = |cx: &mut TestAppContext| {
        cx.simulate_click("chat-search-hit-general-1");
        cx.run_until_parked();
        assert!(cx.find("chat-jump-latest-button").is_some(), "landed");
    };
    cx.simulate_click("Search");
    cx.simulate_input("search", "hello");
    cx.simulate_submit("search");
    cx.run_until_parked();
    land(&mut cx);
    // the reader at the newest message: nothing newer, Latest goes
    cx.simulate_range("chat-timeline", 0..3);
    assert!(cx.find("chat-jump-latest-button").is_none());
    view.read(|chat| assert!(!chat.room.as_ref().unwrap().landed));
    cx.simulate_back();
    land(&mut cx);
    cx.simulate_click("chat-jump-latest-button");
    cx.run_until_parked();
    assert!(cx.find("chat-jump-latest-button").is_none());
    view.read(|chat| {
        let room = chat.room.as_ref().expect("the room stays open");
        assert!(!room.landed && room.messages.ready().is_some());
    });
}

#[test]
fn a_room_scrolled_up_offers_latest() {
    let (mut cx, view) = in_general();
    assert!(
        cx.find("chat-jump-latest-button").is_none(),
        "at the newest"
    );
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().expect("a room is open").at_tail = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-jump-latest-button").is_some());
}

#[test]
fn a_refused_older_page_is_said_at_the_lists_end() {
    let (mut cx, view) = in_general();
    cx.host().handle::<Ask<::chat::Chat>>(|query| match query {
        Query::Roots { .. } => Err(view::host::Error::new("refused", "the node is busy")),
        query => Ok(chat_reply(query)),
    });
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().expect("a room is open").has_older = true;
        chat.load_older(cx);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("Couldn’t read this room: the node is busy"));
    assert!(cx.find("chat-room-notice").is_none(), "not as a notice");
}

#[test]
fn a_search_field_is_one_box_with_its_magnifier_in_it() {
    let (mut cx, _view) = phone();
    cx.simulate_click("chat-rooms-search");
    cx.run_until_parked();
    // the field's own hairline is its edge (the host draws the focused
    // one); the box around it draws none, so no edge is doubled
    let hairline = view::StyleRefinement::default().border_1();
    assert_eq!(
        cx.style("search").border_widths.left,
        hairline.border_widths.left
    );
    let around = parent(cx.root(), "search").expect("the field's box");
    let around = around.style().map(|style| &cx.styles()[style]);
    assert!(around.is_none_or(|style| style.border_widths.left.is_none()));
}

#[test]
fn clear_empties_the_field_and_the_hits() {
    let (mut cx, view) = phone();
    cx.simulate_click("Search");
    cx.simulate_input("search", "hello");
    cx.simulate_submit("search");
    cx.run_until_parked();
    cx.simulate_click("Clear");
    assert!(!cx.has_text("Messages"));
    view.read(|chat| assert!(chat.search.query.is_empty() && chat.search.draft.is_blank()));
    assert!(cx.find("chat-search-page").is_some(), "the page stays");
}

#[test]
fn a_refused_search_says_why_and_retry_asks_again() {
    let (mut cx, _) = phone();
    cx.host().handle::<Ask<::chat::Chat>>(|query| match query {
        Query::Search { .. } => Err(view::host::Error::new("refused", "too many words")),
        query => Ok(chat_reply(query)),
    });
    cx.simulate_click("Search");
    cx.simulate_input("search", "hello");
    cx.simulate_submit("search");
    cx.run_until_parked();
    assert!(cx.has_text("too many words"));
    cx.simulate_click("chat-search-retry");
    cx.run_until_parked();
    assert!(cx.has_text("too many words"));
}

// ---------- new channel (`app/(tabs)/chat/new.tsx`) ----------

#[test]
fn create_is_faint_until_the_channel_has_a_name() {
    let (mut cx, _) = phone();
    cx.simulate_click("New channel");
    assert!(cx.find("chat-new-page").is_some());
    assert_eq!(cx.interactivity("create").aria.disabled, Some(true));
    cx.simulate_input("chat-new-name", "random");
    assert_eq!(cx.interactivity("create").aria.disabled, Some(false));
}

#[test]
fn a_new_channel_who_may_post_and_create_opens_it_in_the_forms_place() {
    let (mut cx, view) = phone();
    cx.simulate_click("New channel");
    cx.simulate_input("chat-new-name", "random");
    cx.simulate_click("Members only");
    cx.simulate_click("create");
    cx.run_until_parked();
    let Some(Op::CreateChannel {
        name, post_policy, ..
    }) = submitted(&cx).pop()
    else {
        panic!("a create went out: {:?}", submitted(&cx));
    };
    assert_eq!(
        (name.as_str(), post_policy),
        ("random", PostPolicy::MembersOnly)
    );
    view.read(|chat| {
        assert!(chat.create.is_none());
        assert_eq!(
            chat.room.as_ref().map(|room| room.id.as_str()),
            Some("channel-1")
        );
    });
}

#[test]
fn a_refused_create_says_why_under_the_name() {
    let (mut cx, _) = phone();
    cx.host()
        .refuse::<Submit<::chat::Chat>>("refused", "the name is taken");
    cx.simulate_click("New channel");
    cx.simulate_input("chat-new-name", "general");
    cx.simulate_submit("chat-new-name");
    cx.run_until_parked();
    assert!(cx.has_text("That didn’t go through: the name is taken"));
    assert!(cx.find("chat-new-page").is_some());
    // the field says so to assistive technology as well
    let wire::Node::Field { options, .. } = cx.node("chat-new-name") else {
        panic!("the name is a field");
    };
    assert!(options.invalid.is_some());
    assert_eq!(
        options.description.as_deref(),
        Some("That didn’t go through: the name is taken")
    );
}

// ---------- a room (`app/(tabs)/chat/[channel]/index.tsx`) ----------

#[test]
fn a_channel_names_itself_its_members_and_who_may_post() {
    let (mut cx, view) = in_general();
    assert!(cx.has_text("#General"));
    assert!(cx.has_text("Open"), "{:?}", cx.texts());
    assert!(
        cx.has_text("Message #General"),
        "the composer's placeholder"
    );
    general_is(&mut cx, &view, |general| general.channel.archived = true);
    assert!(cx.has_text("Archived"));
}

#[test]
fn a_direct_room_wears_the_persons_face_and_its_info_is_their_profile() {
    let (mut cx, _) = phone();
    cx.simulate_click("chat-rooms-dm-8");
    cx.run_until_parked();
    assert!(cx.find("chat-room-avatar").is_some());
    assert!(cx.has_text("reviewer"));
    assert_eq!(
        cx.interactivity("chat-room-details").aria.label.as_deref(),
        Some("Profile")
    );
}

#[test]
fn a_room_says_it_is_loading_then_shows_its_history_from_its_intro() {
    let (mut cx, view) = phone();
    cx.host().never::<Ask<::chat::Chat>>();
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    assert!(cx.has_text("Loading messages…"), "{:?}", cx.texts());
    rows_are(&mut cx, &view, vec![row(1, 7, "hello")]);
    assert!(cx.has_text(
        "This is the very beginning of #General. Say hello, or pin what the room is for."
    ));
    assert!(cx.has_text("hello"));
}

#[test]
fn a_refused_room_says_why_and_retry_reads_it_again() {
    let (mut cx, view) = in_general();
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().unwrap().messages =
            Loadable::Failed(view::host::Error::new("refused", "the room is gone"));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("the room is gone"));
    cx.simulate_click("chat-messages-retry");
    cx.run_until_parked();
    assert!(cx.has_text("hello"));
}

#[test]
fn a_day_opens_under_its_separator_and_the_chains_today_says_today() {
    let (mut cx, view) = in_general();
    let day = 86_400_000;
    let at = |seq, time| MsgRow {
        time,
        ..row(seq, 7, "said")
    };
    rows_are(&mut cx, &view, vec![at(1, 20 * day), at(2, 21 * day)]);
    assert!(cx.find("chat-day-m1").is_some() && cx.find("chat-day-m2").is_some());
    // a row under its separator still says where it sits (AX-112)
    for (key, place) in [("chat-message-m1-row", 1), ("chat-message-m2-row", 2)] {
        let aria = &cx.interactivity(key).aria;
        assert_eq!(
            (aria.position_in_set, aria.size_of_set),
            (Some(place), Some(2))
        );
    }
    assert!(!cx.has_text("Today"));
    cx.update(&view, |chat, _, cx| {
        chat.today = Some(crate::message::day_of(21 * day));
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("Today"));
}

#[test]
fn the_first_unread_message_opens_under_new() {
    let (mut cx, view) = in_general();
    cx.update(&view, |chat, _, cx| {
        chat.reads.boundary = 1;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.find("chat-unread-marker").is_some());
    assert!(cx.has_text("New"));
    assert!(
        cx.find("chat-message-m2-marked").is_some(),
        "over m2, the first past 1"
    );
}

#[test]
fn each_write_gate_takes_the_composers_place_with_its_strip() {
    let (mut cx, view) = in_general();
    assert!(cx.find("draft-general/editor").is_some());
    general_is(&mut cx, &view, |general| general.channel.archived = true);
    assert!(
        cx.has_text("This channel is archived. It keeps its history and takes no new messages.")
    );
    assert!(cx.find("draft-general/editor").is_none());
    cx.simulate_click("chat-room-unarchive");
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::SetChannelArchived {
            archived: false,
            ..
        })
    ));
    general_is(&mut cx, &view, |general| {
        general.channel.archived = false;
        general.channel.owner = Principal::Account(9);
        general.channel.post_policy = PostPolicy::MembersOnly;
    });
    assert!(cx.has_text("This channel is members-only and your key is not on its roster. Ask a member to add your key from Channel details."));
    cx.host().stream::<HostSession>().send(Session {
        account: None,
        ..session()
    });
    cx.run_until_parked();
    assert!(cx.has_text("To send messages, create or join an account in Account. You can read this channel without an account."));
}

#[test]
fn only_the_owner_is_offered_unarchive() {
    let (mut cx, view) = in_general();
    general_is(&mut cx, &view, |general| {
        general.channel.archived = true;
        general.channel.owner = Principal::Account(9);
    });
    assert!(cx.find("chat-room-unarchive").is_none());
}

#[test]
fn the_composer_sends_and_its_send_fills_once_there_is_text() {
    let (mut cx, _) = in_general();
    assert_eq!(cx.interactivity("send").aria.disabled, Some(true));
    cx.simulate_input("draft-general/editor", "on its way");
    assert_ne!(cx.interactivity("send").aria.disabled, Some(true));
    cx.simulate_click("send");
    cx.run_until_parked();
    assert!(matches!(submitted(&cx).pop(), Some(Op::PostMessage { .. })));
}

#[test]
fn a_post_on_its_way_shows_faded_until_it_lands() {
    let (mut cx, view) = in_general();
    cx.host().never::<Submit<::chat::Chat>>();
    cx.simulate_input("draft-general/editor", "on its way");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    assert!(cx.has_text("on its way"));
    view.read(|chat| assert!(chat.messages(Pane::Timeline).iter().any(|m| m.pending)));
}

#[test]
fn at_offers_the_people_with_their_accounts() {
    let (mut cx, _) = in_general();
    cx.simulate_input("draft-general/editor", "@ad");
    assert!(
        cx.has_text("ada") && cx.has_text("account 10"),
        "{:?}",
        cx.texts()
    );
}

// ---------- a message (`src/chat/MessageRow.tsx`, `Rich.tsx`) ----------

#[test]
fn a_keyboard_press_on_a_message_opens_its_actions() {
    let (mut cx, view) = in_general();
    let handler = cx
        .interactivity("chat-message-m1-row")
        .on_click
        .expect("the row answers a press");
    cx.tick(vec![wire::Event::Click {
        handler,
        event: wire::click::Click::Keyboard {
            button: wire::click::KeyboardButton::Enter,
            bounds: Default::default(),
        },
    }]);
    view.read(|chat| {
        assert!(
            chat.menu
                .as_ref()
                .is_some_and(|menu| menu.mode == Mode::More)
        )
    });
    assert!(cx.find("chat-actions-sheet").is_some());
}

#[test]
fn a_tap_on_a_message_does_nothing_and_a_long_press_opens_its_actions() {
    let (mut cx, view) = in_general();
    tap(&mut cx, "chat-message-m1-row");
    view.read(|chat| assert!(chat.menu.is_none()));
    long_press(&mut cx, "chat-message-m1-row");
    assert!(matches!(
        cx.node("chat-actions-sheet"),
        wire::Node::Sheet { label: Some(label), .. } if label == "Message actions"
    ));
    assert!(
        cx.style("chat-message-m1-row").background.is_some(),
        "lit behind the scrim"
    );
    cx.simulate_back();
    assert!(cx.find("chat-actions-sheet").is_none());
    assert!(
        cx.find("chat-room-page").is_some(),
        "Back closed the sheet, not the page"
    );
}

#[test]
fn a_screen_reader_opens_the_same_actions_from_the_row() {
    let (mut cx, _) = in_general();
    cx.simulate_a11y_action(
        "chat-message-m1-row",
        wire::Action::CustomAction,
        Some(wire::ActionData::CustomAction(1)),
    );
    assert!(cx.find("chat-actions-sheet").is_some());
}

#[test]
fn a_reaction_chip_takes_the_readers_own_back_and_plus_opens_the_picker() {
    let (mut cx, view) = in_general();
    let mut liked = row(1, 7, "hello");
    liked.reactions = vec![::chat::Reaction {
        emoji: "👍".into(),
        count: 2,
        reacted_by_me: true,
    }];
    rows_are(&mut cx, &view, vec![liked]);
    cx.simulate_click("chat-message-m1-reaction-add");
    assert!(cx.find("chat-picker-sheet").is_some());
    cx.update(&view, |chat, _, cx| {
        chat.close_menu();
        cx.notify();
    });
    cx.simulate_click("chat-message-m1-reaction-👍");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::RemoveReaction { emoji, .. }) if emoji == "👍"
    ));
}

#[test]
fn a_root_with_replies_has_its_thread_button_and_it_opens_the_thread() {
    let (mut cx, view) = in_general();
    let mut root = row(1, 7, "hello");
    root.reply_count = 2;
    rows_are(&mut cx, &view, vec![root]);
    assert!(cx.has_text("2 replies"));
    cx.simulate_click("chat-message-m1-replies");
    cx.run_until_parked();
    assert!(cx.find("chat-thread-page").is_some());
    // reading a thread raises no keyboard (Expo autofocuses only a reply)
    assert_ne!(
        cx.focused().and_then(|node| node.key()),
        Some("draft-general-1/editor")
    );
}

#[test]
fn a_deleted_message_says_so_and_an_edited_one_says_edited() {
    let (mut cx, view) = in_general();
    let gone = MsgRow {
        deleted: true,
        ..row(1, 7, "hello")
    };
    let edited = MsgRow {
        rev: 1,
        ..row(2, 8, "fixed")
    };
    rows_are(&mut cx, &view, vec![gone, edited]);
    assert!(cx.has_text("Message deleted") && cx.has_text("edited"));
}

#[test]
fn rich_text_draws_its_quote_code_list_and_divider() {
    let (mut cx, view) = in_general();
    let mut rich = row(1, 7, "rich");
    rich.blocks =
        chat::parse_message("> a quote\n```rust\nfn main() {}\n```\n- one\n---\n**bold** `code`");
    rows_are(&mut cx, &view, vec![rich]);
    assert!(
        cx.has_text("rust"),
        "the language as written, the eyebrow's capitals the host's: {:?}",
        cx.texts()
    );
    assert!(
        cx.find("chat-message-m1-block-1-scroll").is_some(),
        "code scrolls sideways"
    );
    assert!(cx.find("chat-message-m1-block-2-item").is_some());
    assert!(cx.find("chat-message-m1-block-3").is_some(), "the divider");
}

// ---------- the actions sheet and the delete dialog (`src/chat/Actions.tsx`) ----------

#[test]
fn the_actions_offer_only_what_the_reader_may_do() {
    let (mut cx, view) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    for label in [
        "Reply in thread",
        "Copy text",
        "Copy link",
        "Edit message",
        "Delete message",
    ] {
        assert!(cx.has_text(label), "{label} on the reader's own");
    }
    cx.simulate_back();
    long_press(&mut cx, "chat-message-m2-row");
    assert!(
        !cx.has_text("Edit message"),
        "another's message is not the reader's to edit"
    );
    assert!(cx.has_text("Delete message"), "the owner deletes anyone's");
    cx.simulate_back();
    general_is(&mut cx, &view, |general| {
        general.channel.owner = Principal::Account(9)
    });
    long_press(&mut cx, "chat-message-m2-row");
    assert!(!cx.has_text("Delete message"));
}

#[test]
fn the_sheet_names_who_wrote_it_when_and_where() {
    let (mut cx, _) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    assert!(cx.has_text("eddy · block 1"), "{:?}", cx.texts());
}

#[test]
fn a_quick_reaction_reacts_and_closes_the_sheet() {
    let (mut cx, view) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("chat-actions-react-👍");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::AddReaction { emoji, .. }) if emoji == "👍"
    ));
    view.read(|chat| assert!(chat.menu.is_none()));
}

#[test]
fn copy_text_puts_the_words_on_the_clipboard() {
    let (mut cx, _) = in_general();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host()
        .handle::<view::methods::ClipboardWrite>(move |text| {
            *seen.borrow_mut() = text;
            Ok(())
        });
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("Copy text");
    cx.run_until_parked();
    assert_eq!(*copied.borrow(), "hello");
    assert!(cx.find("chat-actions-sheet").is_none());
}

#[test]
fn edit_puts_the_message_in_the_composer_under_its_bar() {
    let (mut cx, view) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("Edit message");
    cx.run_until_parked();
    assert!(cx.has_text("Editing message"));
    assert_eq!(
        cx.interactivity("send").aria.label.as_deref(),
        Some("Save edit")
    );
    assert!(cx.find("edit-general-1/editor").is_some());
    cx.simulate_click("Stop editing");
    view.read(|chat| assert!(chat.menu.is_none()));
    assert!(cx.find("draft-general/editor").is_some());
}

#[test]
fn delete_asks_first_cancel_keeps_the_message_and_delete_sends() {
    let (mut cx, _) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("Delete message");
    assert!(cx.has_text("Delete this message?"));
    assert!(
        cx.has_text("It leaves the room for everyone. What stays is its place in the timeline.")
    );
    cx.simulate_click("Cancel");
    assert!(cx.find("chat-delete-dialog").is_none());
    assert!(submitted(&cx).is_empty());
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("Delete message");
    cx.simulate_click("chat-menu-confirm-delete");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::DeleteMessage { seq: 1, .. })
    ));
}

#[test]
fn reply_in_thread_opens_the_thread() {
    let (mut cx, _) = in_general();
    long_press(&mut cx, "chat-message-m2-row");
    cx.simulate_click("Reply in thread");
    cx.run_until_parked();
    assert!(cx.find("chat-thread-page").is_some());
    assert!(cx.has_text("Reply in thread"), "the thread's composer");
    assert_eq!(
        cx.focused().and_then(|node| node.key()),
        Some("draft-general-2/editor"),
        "the reply field takes the keys"
    );
}

// ---------- the picker (`src/chat/Actions.tsx` `Picker`) ----------

#[test]
fn the_picker_browses_by_tab_and_a_pick_reacts_and_closes() {
    let (mut cx, view) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("All reactions");
    assert!(cx.has_text("Frequently used") && cx.has_text("Smileys"));
    cx.simulate_click("chat-reaction-tab-Nature");
    assert!(
        cx.has_text("Nature") && cx.has_text("🐱"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("chat-reaction-frequent-🎉");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::AddReaction { emoji, .. }) if emoji == "🎉"
    ));
    view.read(|chat| assert!(chat.menu.is_none()));
}

#[test]
fn the_picker_counts_what_a_search_found_or_says_none_did() {
    let (mut cx, _) = in_general();
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("All reactions");
    cx.simulate_input("chat-reaction-search", "heart");
    assert!(cx.texts().iter().any(|text| text.starts_with("Results · ")));
    cx.simulate_input("chat-reaction-search", "zzzz");
    assert!(cx.has_text("No emoji match"));
}

#[test]
fn the_picker_draws_one_cell_fewer_a_row_than_fit_as_the_expo_app_does() {
    let (mut cx, view) = in_general();
    cx.simulate_resize(402., 874.);
    view.read(|chat| assert_eq!(chat.layout.viewport.0, 402.));
    long_press(&mut cx, "chat-message-m1-row");
    cx.simulate_click("All reactions");
    // 370 between the gutters holds eight 44 cells; Expo's rows carry seven
    let row = cx.node("chat-reaction-frequent-row-0");
    assert_eq!(row.children().len(), 7);
    assert!(
        cx.find("chat-reaction-frequent-row-1").is_some(),
        "the eighth wraps"
    );
}

// ---------- a thread (`app/(tabs)/chat/[channel]/thread/[root].tsx`) ----------

#[test]
fn a_thread_shows_its_root_its_count_and_no_replies_yet() {
    let (mut cx, view) = in_general();
    thread(&mut cx, &view, 1);
    assert!(cx.find("chat-thread-page").is_some());
    assert!(cx.has_text("Thread") && cx.has_text("#General"));
    assert!(cx.has_text("0 replies"), "{:?}", cx.texts());
    assert!(cx.has_text("No replies yet"));
    assert!(cx.find("draft-general-1/editor").is_some());
    cx.simulate_back();
    assert!(cx.find("chat-room-page").is_some());
}

#[test]
fn a_thread_runs_without_the_rooms_day_separators() {
    let (mut cx, view) = in_general();
    let day = 86_400_000;
    let root = MsgRow {
        time: 20 * day,
        ..row(1, 7, "hello")
    };
    rows_are(&mut cx, &view, vec![root]);
    assert!(cx.find("chat-day-m1").is_some(), "the room's has one");
    thread(&mut cx, &view, 1);
    assert!(cx.find("chat-thread-page").is_some());
    assert!(cx.find("chat-day-m1").is_none());
}

#[test]
fn a_thread_says_it_is_loading_and_a_refused_one_retries() {
    let (mut cx, view) = in_general();
    cx.host().never::<Ask<::chat::Chat>>();
    thread(&mut cx, &view, 1);
    assert!(cx.has_text("Loading the thread…"));
    cx.host().handle::<Ask<::chat::Chat>>(|query| match query {
        Query::Thread { .. } => Err(view::host::Error::new("refused", "the thread is gone")),
        query => Ok(chat_reply(query)),
    });
    thread(&mut cx, &view, 1);
    assert!(cx.has_text("the thread is gone"));
    cx.host()
        .handle::<Ask<::chat::Chat>>(|query| Ok(chat_reply(query)));
    cx.simulate_click("chat-messages-retry");
    cx.run_until_parked();
    assert!(cx.has_text("No replies yet"));
}

// ---------- channel details (`app/(tabs)/chat/[channel]/details.tsx`) ----------

#[test]
fn details_name_the_channel_its_facts_members_and_the_owners_actions() {
    let (mut cx, _) = in_general();
    cx.simulate_click("Channel details");
    assert!(cx.find("chat-details-page").is_some());
    assert!(
        cx.has_text("Open · created 1 Jan 1970 · owner eddy"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("No members added. An open channel needs none."));
    for label in [
        "Add a member…",
        "Owner actions",
        "Rename channel",
        "Archive channel",
    ] {
        assert!(cx.has_text(label), "{label}");
    }
}

#[test]
fn a_reader_who_does_not_own_the_channel_has_no_owners_actions() {
    let (mut cx, view) = in_general();
    general_is(&mut cx, &view, |general| {
        general.channel.owner = Principal::Account(9)
    });
    cx.simulate_click("Channel details");
    assert!(!cx.has_text("Owner actions") && !cx.has_text("Add a member…"));
}

#[test]
fn the_owners_sheets_add_and_rename_and_back_closes_a_sheet_first() {
    let (mut cx, _) = in_general();
    cx.simulate_click("Channel details");
    cx.simulate_click("Add a member…");
    assert!(matches!(
        cx.node("chat-details-sheet"),
        wire::Node::Sheet { .. }
    ));
    assert!(cx.has_text("account 10"), "the people not seated");
    cx.simulate_back();
    assert!(cx.find("chat-details-sheet").is_none());
    assert!(cx.find("chat-details-page").is_some());
    cx.simulate_click("Add a member…");
    cx.simulate_click("chat-details-person-10");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::SetMembership { member: true, .. })
    ));
    cx.simulate_click("Rename channel");
    cx.simulate_input("chat-details-rename/name", "lobby");
    cx.simulate_click("chat-details-rename/submit");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::RenameChannel { name, .. }) if name == "lobby"
    ));
}

#[test]
fn the_add_and_rename_fields_send_on_the_keyboards_return() {
    let (mut cx, _) = in_general();
    cx.simulate_click("Channel details");
    cx.simulate_click("Add a member…");
    cx.simulate_input("chat-details-add/account", "10");
    cx.simulate_submit("chat-details-add/account");
    cx.run_until_parked();
    assert!(cx.find("chat-details-sheet").is_none());
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::SetMembership { member: true, .. })
    ));
    cx.simulate_click("Rename channel");
    cx.simulate_input("chat-details-rename/name", "lobby");
    cx.simulate_submit("chat-details-rename/name");
    cx.run_until_parked();
    assert!(cx.find("chat-details-sheet").is_none());
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::RenameChannel { name, .. }) if name == "lobby"
    ));
}

#[test]
fn a_number_that_is_no_account_leaves_the_add_sheet_open() {
    let (mut cx, _) = in_general();
    cx.simulate_click("Channel details");
    cx.simulate_click("Add a member…");
    cx.simulate_input("chat-details-add/account", "ada");
    cx.simulate_click("chat-details-add/submit");
    cx.run_until_parked();
    assert!(
        cx.find("chat-details-add/account").is_some(),
        "the sheet stays"
    );
    assert!(submitted(&cx).is_empty() && cx.find("chat-room-notice").is_none());
}

#[test]
fn a_rename_to_the_name_it_has_sends_nothing() {
    let (mut cx, _) = in_general();
    cx.simulate_click("Channel details");
    cx.simulate_click("Rename channel");
    cx.simulate_input("chat-details-rename/name", "General");
    cx.simulate_click("chat-details-rename/submit");
    cx.run_until_parked();
    assert!(submitted(&cx).is_empty());
}

#[test]
fn details_say_they_are_loading_until_the_channel_and_members_land() {
    let (mut cx, view) = in_general();
    cx.simulate_click("Channel details");
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().expect("a room is open").members = Loadable::Idle;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("Loading members…"));
    cx.update(&view, |chat, _, cx| {
        chat.channels = Loadable::Idle;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(cx.has_text("Loading the channel…"));
}

#[test]
fn the_owner_removes_a_member_from_their_sheet() {
    let (mut cx, view) = in_general();
    cx.update(&view, |chat, _, cx| {
        chat.room.as_mut().unwrap().members = Loadable::Ready(vec![::chat::MemberRow {
            principal: Principal::Account(10),
            height: 1,
            time: 0,
        }]);
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("Channel details");
    cx.simulate_click("chat-details-member-<@10>");
    assert!(cx.has_text("Remove from this channel"));
    cx.simulate_click("chat-details-remove");
    cx.run_until_parked();
    assert!(matches!(
        submitted(&cx).pop(),
        Some(Op::SetMembership { member: false, .. })
    ));
}

// ---------- the sweep ----------

#[test]
fn every_screen_and_sheet_fits_a_phone() {
    let (mut cx, view) = phone();
    testing::sweep(&mut cx, |cx| {
        assert!(cx.find("chat-rooms-page").is_some());
        cx.simulate_click("Search");
        cx.simulate_input("search", "hello");
        cx.simulate_submit("search");
        cx.run_until_parked();
        cx.simulate_back();
        cx.simulate_click("New channel");
        cx.simulate_back();
        cx.simulate_click("chat-rooms-new-dm");
        cx.simulate_input("chat-dm-pick-search", "ad");
        cx.simulate_back();
        cx.simulate_click("chat-rooms-channel-general");
        cx.run_until_parked();
        cx.simulate_input("draft-general/editor", "@ad");
        cx.simulate_input("draft-general/editor", "");
        long_press(cx, "chat-message-m1-row");
        cx.simulate_click("All reactions");
        cx.simulate_back();
        long_press(cx, "chat-message-m1-row");
        cx.simulate_click("Edit message");
        cx.simulate_click("Stop editing");
        long_press(cx, "chat-message-m1-row");
        cx.simulate_click("Delete message");
        cx.simulate_click("Cancel");
        long_press(cx, "chat-message-m2-row");
        cx.simulate_click("Reply in thread");
        cx.run_until_parked();
        cx.simulate_back();
        cx.simulate_click("Channel details");
        cx.simulate_click("Add a member…");
        cx.simulate_back();
        cx.simulate_click("Rename channel");
        cx.simulate_back();
        cx.simulate_back();
        cx.simulate_back();
        cx.simulate_click("chat-rooms-dm-8");
        cx.run_until_parked();
        cx.simulate_back();
        view.read(|chat| assert!(chat.room.is_none()));
    });
}

/// The one pick (STRUCTURE v2): a finger draws the phone's tree, a
/// pointer the desktop's, from the same state.
#[test]
fn a_finger_draws_the_phone_tree_and_a_pointer_the_desktop() {
    let (mut cx, _) = phone();
    assert!(cx.find("chat-rooms-page").is_some() && cx.find("chat-sidebar").is_none());
    cx.simulate_touch(false);
    assert!(cx.find("chat-sidebar").is_some() && cx.find("chat-rooms-page").is_none());
}

/// The node whose child `key` is.
fn parent<'a>(node: &'a wire::Node, key: &str) -> Option<&'a wire::Node> {
    node.children()
        .iter()
        .find_map(|child| match child.key() == Some(key) {
            true => Some(node),
            false => parent(child, key),
        })
}
