//! Sending, and creating channels.
use super::*;

#[test]
fn a_send_shows_greyed_then_lands_and_a_refusal_is_a_banner() {
    let (mut cx, view) = opened();
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert!(cx.has_text("2 results for “hello”"), "{:?}", cx.texts());
    // the clear control is a glyph named in words
    cx.simulate_click("chat-sidebar-clear-search");
    view.read(|chat| assert!(chat.search.query.is_empty()));
    // the host mints the ids: `message-1`, `channel-1`
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_input("draft-general/editor", "on its way");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    assert!(
        cx.host().requests::<Submit<::chat::Chat>>().iter().any(
            |op| matches!(op, Op::PostMessage { message_id, .. } if message_id == "message-1")
        )
    );
    // the program took it; the re-read after the send does not serve it yet
    assert!(cx.has_text("on its way") && greyed(&cx, "message-1"));
    // its block lands, and the rows read again serve it
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 3)])),
            Query::Roots { .. } => Reply::Roots(page(vec![
                row(1, 7, "hello"),
                row(2, 8, "**hi** there"),
                MsgRow {
                    message_id: "message-1".into(),
                    ..row(3, 7, "on its way")
                },
            ])),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    assert!(cx.has_text("on its way") && !greyed(&cx, "message-1"));
    assert_eq!(shown(&view, "message-1"), 1);
    cx.host().refuse::<Submit<::chat::Chat>>("no", "no");
    cx.simulate_click("chat-sidebar-new-channel");
    assert!(cx.has_text("Create a channel"));
    cx.simulate_input("chat-create-name", "random");
    cx.simulate_click("chat-create-members");
    cx.simulate_submit("chat-create-name");
    cx.run_until_parked();
    assert!(cx.host().requests::<Submit<::chat::Chat>>().iter().any(|op| matches!(op, Op::CreateChannel { channel_id, name, post_policy: PostPolicy::MembersOnly } if channel_id == "channel-1" && name == "random")));
    assert!(cx.has_text("That didn’t go through: no"));
    let (restored, view) = restored(&cx);
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    assert!(restored.host().requests::<Ask<::chat::Chat>>().len() >= 2);
}

#[test]
fn your_own_post_leaves_the_room_read_when_you_go() {
    let read = |view: &Entity<Chat>| {
        view.read(|chat| {
            let cursor = chat.reads.cursors["general"];
            (chat.unread(chat.info("general").unwrap()), cursor)
        })
    };
    let (cx, view) = posted_then_left(false);
    assert_eq!(read(&view), (false, 4));
    assert!(cx.find("chat-sidebar-channel-general-unread").is_none());
    let (cx, view) = posted_then_left(true);
    assert_eq!(read(&view), (true, 4), "one unread: seq 5, another's");
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
}

fn posted_then_left(others: bool) -> (TestAppContext, Entity<Chat>) {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "mine");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-dm-8");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "dm-7-8"));
    let head = if others { 5 } else { 4 };
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(vec![
                    channel("general", "General", head),
                    channel("dm-7-8", "dm", 1),
                    channel("forge:web:3", "Review web#3", 2),
                ])),
                Query::MessagesAround { channel_id, .. } if channel_id == "general" => {
                    let mine = MsgRow {
                        message_id: "message-1".into(),
                        ..row(4, 7, "mine")
                    };
                    let theirs = row(5, 8, "theirs");
                    let rows = [row(3, 7, "hello"), mine, theirs];
                    Reply::Messages(rows.into_iter().filter(|row| row.seq <= head).collect())
                }
                query => chat_reply(query),
            })
        }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(5, posted("general")));
    cx.run_until_parked();
    (cx, view)
}

/// How many timeline rows carry `message_id`.
fn shown(view: &Entity<Chat>, message_id: &str) -> usize {
    view.read(|chat| {
        chat.shown(Pane::Timeline)
            .filter(|row| row.message_id == message_id)
            .count()
    })
}

fn greyed(cx: &TestAppContext, message_id: &str) -> bool {
    cx.style(&format!("chat-message-{message_id}-card")).opacity == Some(0.5)
}

fn holding(view: &Entity<Chat>) -> Vec<String> {
    view.read(|chat| {
        let room = chat.room.as_ref().unwrap();
        room.pending
            .iter()
            .map(|row| row.message_id.clone())
            .collect()
    })
}

fn submit_in_flight(cx: &TestAppContext) -> u64 {
    use view::methods::Method;
    let requests = &cx.last_frame().requests;
    let submit = requests
        .iter()
        .find(|request| request.kind == Submit::<::chat::Chat>::KIND);
    submit.expect("the post is submitted").id
}

fn post_in_flight(text: &str) -> (TestAppContext, Entity<Chat>, u64) {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host().never::<Submit<::chat::Chat>>();
    cx.simulate_input("draft-general/editor", text);
    cx.simulate_field_key("draft-general/editor", "enter");
    let submit = submit_in_flight(&cx);
    (cx, view, submit)
}

#[test]
fn a_post_shows_greyed_while_its_submit_is_in_flight() {
    let (cx, view, _) = post_in_flight("on its way");
    assert!(cx.has_text("on its way"), "{:?}", cx.texts());
    assert!(greyed(&cx, "message-1"));
    assert_eq!(shown(&view, "message-1"), 1);
    assert_eq!(holding(&view), ["message-1"]);
    assert!(!cx.has_text("sending…"));
    assert!(!greyed(&cx, "m2"), "a served row keeps its colour");
}

#[test]
fn a_greyed_post_keeps_its_place_and_height_when_it_lands() {
    let (mut cx, view, _) = post_in_flight("on its way");
    let pending = cx.style("chat-message-message-1-card").clone();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 3)])),
            Query::Roots { .. } => Reply::Roots(page(vec![
                row(1, 7, "hello"),
                row(2, 8, "**hi** there"),
                MsgRow {
                    message_id: "message-1".into(),
                    ..row(3, 7, "on its way")
                },
            ])),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    let landed = cx.style("chat-message-message-1-card").clone();
    assert_eq!(
        StyleRefinement {
            opacity: None,
            ..pending
        },
        landed,
        "only the colour changes"
    );
    assert_eq!(shown(&view, "message-1"), 1);
    assert!(holding(&view).is_empty());
}

#[test]
fn a_post_the_room_shows_before_its_receipt_is_shown_once() {
    use view::methods::Method;
    let (mut cx, view, receipt) = post_in_flight("on its way");
    // its block lands first, and the re-read serves it
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 3)])),
            Query::Roots { .. } => Reply::Roots(page(vec![
                row(1, 7, "hello"),
                row(2, 8, "**hi** there"),
                MsgRow {
                    message_id: "message-1".into(),
                    ..row(3, 7, "on its way")
                },
            ])),
            // the receipt reads everything again, the roster too
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
            Query::Listening { .. } => Reply::Listening(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    assert!(cx.has_text("on its way") && !greyed(&cx, "message-1"));
    // then the receipt
    cx.simulate_event(wire::Event::Response {
        id: receipt,
        result: Ok(Submit::<::chat::Chat>::encode_reply(
            &view::testing::applied(Vec::new()),
        )),
        done: true,
    });
    assert!(cx.has_text("on its way") && !greyed(&cx, "message-1"));
    assert_eq!(shown(&view, "message-1"), 1);
    assert!(holding(&view).is_empty());
}

/// A post still sending, and the room opened again around a search hit
/// whose window serves it: the window settles it, as a re-read does.
#[test]
fn a_window_opened_around_a_hit_settles_a_post_it_serves() {
    let (mut cx, view, _) = post_in_flight("on its way");
    assert!(greyed(&cx, "message-1"));
    let landed = || MsgRow {
        message_id: "message-1".into(),
        ..row(3, 7, "on its way")
    };
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::TextSearch { .. } => Reply::TextHits(page(vec![row(1, 7, "hello")])),
                Query::MessagesAround { .. } => Reply::Messages(vec![
                    row(1, 7, "hello"),
                    row(2, 8, "**hi** there"),
                    landed(),
                ]),
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                query => panic!("unexpected chat query: {query:?}"),
            })
        }));
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    cx.simulate_click("chat-search-hit-general-1");
    cx.run_until_parked();
    assert_eq!(shown(&view, "message-1"), 1);
    assert!(cx.has_text("on its way") && !greyed(&cx, "message-1"));
}

#[test]
fn a_refused_post_leaves_the_timeline_and_keeps_the_text() {
    let (mut cx, view, submit) = post_in_flight("turned away");
    assert!(cx.has_text("turned away") && greyed(&cx, "message-1"));
    cx.simulate_event(wire::Event::Response {
        id: submit,
        result: Err(wire::Error::new("unauthorized", "not a member")),
        done: true,
    });
    assert!(
        cx.has_text("That didn’t go through: not a member"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("An earlier message wasn’t sent"));
    assert!(!cx.has_text("turned away"));
    assert_eq!(shown(&view, "message-1"), 0);
    assert!(holding(&view).is_empty());
    view.read(|chat| {
        let draft = &chat.drafts["draft-general"];
        assert_eq!(draft.failed_send.as_ref().unwrap().body, "turned away");
    });
}

/// A refused post shows no row: the composer says why and keeps the text
/// to restore.
#[test]
fn a_refused_post_shows_no_row_and_keeps_the_text() {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host()
        .refuse::<Submit<::chat::Chat>>("unauthorized", "not a member");
    cx.simulate_input("draft-general/editor", "turned away");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    assert!(
        cx.has_text("That didn’t go through: not a member"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("An earlier message wasn’t sent"));
    assert_eq!(shown(&view, "message-1"), 0);
    assert!(holding(&view).is_empty());
}

#[test]
fn a_disconnect_takes_a_greyed_post_out_of_the_timeline() {
    let (mut cx, view, _) = post_in_flight("on its way");
    assert!(greyed(&cx, "message-1"));
    cx.host().stream::<HostSession>().send(Session {
        connected: false,
        ..session()
    });
    cx.run_until_parked();
    assert_eq!(shown(&view, "message-1"), 0);
    assert!(holding(&view).is_empty());
    view.read(|chat| {
        let draft = &chat.drafts["draft-general"];
        assert_eq!(draft.failed_send.as_ref().unwrap().body, "on its way");
    });
}

#[test]
fn channel_create_preserves_busy_and_account_gates() {
    fn disabled(cx: &TestAppContext, id: &str) -> bool {
        let interactivity = cx.interactivity(id);
        interactivity.aria.disabled == Some(true) && interactivity.on_click.is_none()
    }

    let (mut cx, view) = opened();
    cx.simulate_click("chat-sidebar-new-channel");
    cx.update(&view, |chat, _, cx| {
        chat.create.as_mut().unwrap().busy = true;
        cx.notify();
    });
    cx.run_until_parked();
    let Some(wire::Node::Field {
        options, on_submit, ..
    }) = cx.find("chat-create-name")
    else {
        panic!("channel name input")
    };
    assert!(options.disabled);
    assert!(on_submit.is_none());
    for id in [
        "chat-create-members",
        "chat-create-cancel",
        "chat-create-submit",
    ] {
        assert!(disabled(&cx, id), "{id} must stay inert while busy");
    }

    cx.update(&view, |chat, _, cx| {
        let create = chat.create.as_mut().unwrap();
        create.busy = false;
        chat.session.account = None;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-submit"));
    assert!(cx.has_text("Create an account to create a channel"));
    assert!(!disabled(&cx, "chat-create-cancel"));
    let submitted = cx.host().requests::<Submit<::chat::Chat>>().len();
    cx.update(&view, |chat, _, cx| chat.create_channel(cx));
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<::chat::Chat>>().len(),
        submitted
    );

    cx.update(&view, |chat, _, cx| {
        chat.session.account = Some(7);
        chat.session.connected = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(disabled(&cx, "chat-create-submit"));
}

/// The composer follows the program's rule for a members-only room: its
/// owner writes without a seat, a seated member writes, a stranger reads.
#[test]
fn a_members_only_room_takes_its_owner_and_its_members() {
    let (mut cx, view) = opened();
    let gate = |cx: &mut TestAppContext, owner: u64, seated: bool| {
        cx.update(&view, |chat, _, cx| {
            let general = chat
                .channels
                .ready_mut()
                .unwrap()
                .iter_mut()
                .find(|info| info.channel.id == "general")
                .unwrap();
            general.channel.post_policy = PostPolicy::MembersOnly;
            general.channel.owner = Principal::Account(owner);
            let seats = if seated {
                vec![chat::MemberRow {
                    principal: Principal::Account(7),
                    height: 1,
                    time: 1,
                }]
            } else {
                Vec::new()
            };
            chat.room.as_mut().unwrap().members = Loadable::Ready(seats);
            cx.notify();
        });
        view.read(|chat| chat.write_gate())
    };
    assert_eq!(gate(&mut cx, 7, false), None, "the owner needs no seat");
    assert_eq!(gate(&mut cx, 8, true), None, "a seated member writes");
    assert_eq!(
        gate(&mut cx, 8, false),
        Some(crate::session::Gate::NotMember),
        "a stranger reads"
    );
}

/// Hits in two channels at the same seq are two rows the host can tell
/// apart; one identity for both stopped the view.
#[test]
fn search_hits_in_two_channels_at_one_seq_are_two_rows() {
    let (mut cx, _) = opened();
    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert!(cx.find("chat-search-hit-general-1").is_some());
    assert!(cx.find("chat-search-hit-dm-7-8-1").is_some());
    // the host's sanitizer refuses duplicate typed identities among siblings
    cx.frame_bytes();
}

#[test]
fn a_first_post_to_a_dm_opens_it_and_a_listed_one_does_not() {
    let (mut cx, view) = opened();
    cx.run_until_parked();
    let post = |channel: &str| crate::composer::Target::Post {
        channel: channel.into(),
        thread: None,
    };
    view.read(|chat| {
        assert!(matches!(
            chat.dm_to_open(&post("dm-7-9")),
            Some(Op::CreateDmChannel { counterpart: 9, .. })
        ));
        assert!(chat.dm_to_open(&post("dm-7-8")).is_none(), "already open");
        assert!(chat.dm_to_open(&post("general")).is_none(), "not a dm");
    });
}

/// What the composer of the draft `key` holds: the frame's text, which the
/// host adopted or the view followed.
fn held(cx: &TestAppContext, key: &str) -> String {
    let Some(wire::Node::Field { value, .. }) = cx.find(&format!("{key}/editor")) else {
        panic!("no composer for {key}");
    };
    value.clone()
}

/// "a" then "b" as two keys, each at the end of what the field holds: what
/// the field shows after them, and what Send would send.
fn a_then_b(cx: &mut TestAppContext, view: &Entity<Chat>, key: &str) -> (String, String) {
    for typed in ["a", "b"] {
        let text = format!("{}{typed}", held(cx, key));
        cx.simulate_input(&format!("{key}/editor"), &text);
    }
    let body = view.read(|chat| chat.drafts.get(key).map(|draft| draft.body()));
    (held(cx, key), body.unwrap_or_default())
}

#[test]
fn a_key_in_the_edit_field_is_kept() {
    let (mut cx, view) = opened();
    edit(&mut cx, &view, 1);
    let (shown, sent) = a_then_b(&mut cx, &view, "edit-general-1");
    assert_eq!((shown.as_str(), sent.as_str()), ("helloab", "helloab"));
}

/// A room typed in before keeps every key typed in it again, and after a
/// restore with its draft.
#[test]
fn a_room_typed_in_again_and_after_a_restore_keeps_every_key() {
    let (mut cx, view) = opened();
    let (first, _) = a_then_b(&mut cx, &view, "draft-general");
    let (shown, sent) = a_then_b(&mut cx, &view, "draft-general");
    assert_eq!(
        (&shown, &sent),
        (&format!("{first}ab"), &format!("{first}ab"))
    );
    let (mut restored, view) = restored(&cx);
    let (shown, sent) = a_then_b(&mut restored, &view, "draft-general");
    assert_eq!(
        (&shown, &sent),
        (&format!("{first}abab"), &format!("{first}abab")),
        "restored with its draft"
    );
}

/// `text` typed into the composer of the draft `key` in one change: the
/// field shows it and Send would send it.
fn types(cx: &mut TestAppContext, view: &Entity<Chat>, key: &str, text: &str) {
    cx.simulate_input(&format!("{key}/editor"), text);
    let body = view.read(|chat| chat.drafts.get(key).map(|draft| draft.body()));
    assert_eq!(
        (held(cx, key).as_str(), body.as_deref()),
        (text, Some(text)),
        "typed into {key}"
    );
}

/// Every way a room comes on screen seats its draft before the frame that
/// draws its composer: a link the view was opened with, a press in the
/// sidebar, a direct room nobody posted in yet, and a search hit.
#[test]
fn the_first_key_is_kept_however_the_room_came_on_screen() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let routes = cx.host().stream::<view::methods::HostRoute>();
    let view = cx.open::<Chat>();
    seat(&mut cx);
    let link = crate::links::channel_link("testnet#0a1b2c3d", "forge:web:3", None).unwrap();
    routes.send(ducklink::Link::parse(&link).unwrap().tail.join("/"));
    cx.run_until_parked();
    // five letters in one change, then a key
    types(&mut cx, &view, "draft-forge:web:3", "hello");
    types(&mut cx, &view, "draft-forge:web:3", "hello!");

    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    types(&mut cx, &view, "draft-general", "a");

    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.choose("dm-7-9".into(), cx)
    });
    cx.run_until_parked();
    view.read(|chat| assert!(chat.info("dm-7-9").is_none(), "not opened yet"));
    types(&mut cx, &view, "draft-dm-7-9", "a");

    cx.simulate_input("chat-sidebar-search", "hello");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    // the window around the hit is asked and not answered
    cx.host().never::<Ask<::chat::Chat>>();
    cx.simulate_click("chat-search-hit-dm-7-8-1");
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "dm-7-8"));
    types(&mut cx, &view, "draft-dm-7-8", "a");
}

/// A view restored with a thread open and nothing typed yet.
#[test]
fn the_first_key_is_kept_in_a_thread_restored_open() {
    let (mut cx, view) = opened();
    thread(&mut cx, &view, 1);
    let (mut restored, view) = restored(&cx);
    types(&mut restored, &view, "draft-general/1", "a");
    types(&mut restored, &view, "draft-general", "a");
}

/// A composer is one document for as long as it is on screen: the host
/// takes a field's text from the frame whenever its generation moves, so a
/// render that moved it would take back what was typed since.
#[test]
fn a_render_leaves_the_composer_the_document_it_was() {
    let (mut cx, view) = opened();
    let generation = |cx: &TestAppContext| {
        let Some(wire::Node::Field { generation, .. }) = cx.find("draft-general/editor") else {
            panic!("no composer");
        };
        *generation
    };
    let drawn = generation(&cx);
    cx.update(&view, |_, _, cx| cx.notify());
    cx.run_until_parked();
    assert_eq!(generation(&cx), drawn);
}

#[test]
fn a_thread_draft_stays_out_of_a_room_whose_name_ends_in_its_root() {
    let (mut cx, view) = opened();
    thread(&mut cx, &view, 1);
    let reply = crate::composer::Target::Post {
        channel: "general".into(),
        thread: Some(1),
    }
    .key();
    types(&mut cx, &view, &reply, "half a reply");
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.choose("general-1".into(), cx)
    });
    cx.run_until_parked();
    let room = crate::composer::Target::Post {
        channel: "general-1".into(),
        thread: None,
    }
    .key();
    assert_eq!(held(&cx, &room), "", "general-1 opens blank");
    view.read(|chat| assert_eq!(chat.drafts[&reply].body(), "half a reply"));
}

/// The drafts are saved with the view, so a room or a thread left with
/// nothing written leaves none behind, and one left with text keeps it.
#[test]
fn a_draft_left_blank_goes_and_one_with_text_stays() {
    let (mut cx, view) = opened();
    let kept =
        |view: &Entity<Chat>| view.read(|chat| chat.drafts.keys().cloned().collect::<Vec<_>>());
    let choose = |cx: &mut TestAppContext, room: &str| {
        cx.update(&view, |chat, _, cx| {
            cx.notify();
            chat.choose(room.into(), cx)
        });
        cx.run_until_parked();
    };
    assert_eq!(kept(&view), ["draft-general"]);
    choose(&mut cx, "dm-7-8");
    assert_eq!(kept(&view), ["draft-dm-7-8"], "general was left blank");
    types(&mut cx, &view, "draft-dm-7-8", "half a thought");
    choose(&mut cx, "general");
    assert_eq!(kept(&view), ["draft-dm-7-8", "draft-general"]);
    thread(&mut cx, &view, 1);
    assert!(kept(&view).contains(&"draft-general/1".to_owned()));
    cx.simulate_click("chat-thread-close");
    assert_eq!(kept(&view), ["draft-dm-7-8", "draft-general"]);
    choose(&mut cx, "dm-7-8");
    assert_eq!(kept(&view), ["draft-dm-7-8"]);
    assert_eq!(held(&cx, "draft-dm-7-8"), "half a thought");
}

/// An edit emptied to write it again is still on screen: its draft stays
/// when the room around it is entered again.
#[test]
fn an_edit_emptied_keeps_its_draft_while_it_is_open() {
    let (mut cx, view) = opened();
    edit(&mut cx, &view, 1);
    types(&mut cx, &view, "edit-general-1", "");
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.open("general".into(), cx);
    });
    cx.run_until_parked();
    types(&mut cx, &view, "edit-general-1", "again");
}

#[test]
fn a_message_of_files_alone_opens_its_edit_and_the_save_keeps_its_files() {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    let file = chat::message::Attachment {
        path: "/chat/general/m1/notes.txt".into(),
        snapshot: [5; 32],
        name: "notes.txt".into(),
        size: 3,
        content_type: "text/plain".into(),
    };
    let files_alone = MsgRow {
        blocks: vec![chat::Block::Attachment(file.clone())],
        ..row(1, 7, "")
    };
    cx.host().handle::<Ask<files::Files>>(|_| {
        Ok(files::Reply::Stat {
            snapshot: None,
            entry: None,
        })
    });
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![files_alone.clone()])),
                query => chat_reply(query),
            })
        }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    cx.update(&view, |chat, _, cx| {
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 0,
            mode: Mode::More,
            at: (611., 455.),
        });
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-menu-edit");
    cx.run_until_parked();
    assert!(
        cx.find("chat-message-editing").is_some(),
        "Edit opens its field"
    );
    cx.simulate_input("edit-general-1/editor", "the notes");
    cx.simulate_field_key("edit-general-1/editor", "enter");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<::chat::Chat>>();
    let Some(Op::EditMessage { seq: 1, blocks, .. }) = sent.last() else {
        panic!("an edit: {sent:?}")
    };
    let mut kept = chat::parse_message("the notes");
    kept.push(chat::Block::Attachment(file));
    assert_eq!(blocks, &kept);
}

#[test]
fn an_edit_saved_unchanged_keeps_italic_beside_a_letter_and_code_holding_backticks() {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    let source = "a*b*c, `` `tick `` and ``x ` y``, **_both_** and *it*";
    let marked = parsed_row(1, 7, source);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![marked.clone()])),
                query => chat_reply(query),
            })
        }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    edit(&mut cx, &view, 1);
    cx.simulate_focus("edit-general-1/editor");
    cx.simulate_field_key("edit-general-1/editor", "enter");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<::chat::Chat>>();
    let Some(Op::EditMessage { blocks, .. }) = sent.last() else {
        panic!("an edit: {sent:?}")
    };
    assert_eq!(blocks, &chat::parse_message(source), "saved as it was");
}

#[test]
fn a_notice_of_the_readers_own_offers_no_edit_and_opens_none() {
    let (mut cx, view) = opened();
    with_menu_on(&mut cx, &view, notice_row(3, 7));
    assert!(cx.find("chat-menu-delete").is_some(), "its menu is drawn");
    assert!(
        cx.find("chat-menu-edit").is_none(),
        "chat never edits a notice"
    );
    edit(&mut cx, &view, 3);
    assert!(cx.find("chat-message-editing").is_none());
    view.read(|chat| assert!(chat.editing().is_none()));
}

/// An edit left without saving leaves no draft behind: opening it again
/// starts from the message, so there is nothing of it to keep.
#[test]
fn an_edit_cancelled_leaves_no_draft_and_opens_from_the_message_again() {
    let (mut cx, view) = opened();
    edit(&mut cx, &view, 1);
    types(&mut cx, &view, "edit-general-1", "hello there");
    cx.simulate_click("Cancel");
    view.read(|chat| assert!(!chat.drafts.contains_key("edit-general-1")));
    edit(&mut cx, &view, 1);
    assert_eq!(held(&cx, "edit-general-1"), "hello", "seeded again");
    // one that holds a send stays: the send is not in the message
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        let failed_send = Some(crate::composer::Send { body: "x".into() });
        let parked = crate::composer::Draft {
            failed_send,
            ..Default::default()
        };
        chat.drafts.insert("edit-general-2".into(), parked);
    });
    cx.run_until_parked();
    view.read(|chat| assert!(chat.drafts.contains_key("edit-general-2")));
}

// The desktop's `@` menu, open in the whole view, whose every frame the
// audit holds: a list box of the people and agents it offers, each an
// option a key reaches; forge's own account is none of them.
#[test]
fn the_desktop_mention_menu_is_a_list_box_every_frame_passes() {
    let (mut cx, _) = opened();
    cx.simulate_bounds("chat-pane", (260., 0.), (840., 760.));
    cx.simulate_bounds("draft-general", (276., 640.), (808., 104.));
    cx.simulate_input("draft-general/editor", "@");
    let Some(wire::Node::UniformList { count, .. }) = cx.find("draft-general/mentions") else {
        panic!("the choices are a list: {:?}", cx.texts())
    };
    assert_eq!(*count, 3, "eddy, reviewer and ada");
    let menu = cx.interactivity("draft-general/mention-menu");
    assert_eq!(menu.role, Some(view::Role::ListBox));
    let ada = cx.interactivity("draft-general/mention/<@10>");
    assert_eq!(ada.role, Some(view::Role::ListBoxOption));
    assert!(ada.focusable);
    assert!(cx.find("draft-general/mention/<@9>").is_none());
}

// `#` offers the rooms of the list, never a direct one, as options of the
// same list box, every frame audited; picking General asks the field for a
// link to it as one span.
#[test]
fn hash_offers_the_rooms_and_a_pick_is_a_link_to_one() {
    let (mut cx, _) = opened();
    cx.simulate_bounds("chat-pane", (260., 0.), (840., 760.));
    cx.simulate_bounds("draft-general", (276., 640.), (808., 104.));
    cx.simulate_input("draft-general/editor", "#");
    let Some(wire::Node::UniformList { count, .. }) = cx.find("draft-general/mentions") else {
        panic!("the rooms are a list: {:?}", cx.texts())
    };
    assert_eq!(*count, 1, "General: a program's room waits for a letter");
    let token = "[#General](duck://testnet-0a1b2c3d/chat/general)";
    let general = format!("draft-general/mention/{token}");
    let row = cx.interactivity(&general);
    assert_eq!(row.role, Some(view::Role::ListBoxOption));
    cx.simulate_click(&general);
    cx.run_until_parked();
    let asked = cx.host().requests::<view::methods::HostWidget>();
    assert!(
        asked.iter().any(|command| matches!(
            command,
            wire::WidgetCommand::Replace { text, token: Some(id), .. }
                if text == "#General" && id == token
        )),
        "{asked:?}"
    );
}

// A bare `#` offers the rooms the reader is in (owned, opened, listened
// to), those a block posted in latest first, then A to Z. A work room and a
// room the reader is not in wait for a letter typed after the `#`; an
// archived room and a direct one are never offered.
#[test]
fn hash_offers_the_rooms_you_are_in_and_the_rest_when_typed() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let room = |id: &str, name: &str, owner: u64, archived: bool| {
        let mut info = channel(id, name, 1);
        info.channel.owner = Principal::Account(owner);
        info.channel.archived = archived;
        info
    };
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => {
                    let mut rooms = vec![
                        room("general", "General", 7, false),
                        room("design", "Design", 9, false),
                        room("random", "Random", 9, false),
                        room("lobby", "Lobby", 9, false),
                        room("attic", "Attic", 7, true),
                        room("dm-7-8", "dm", 7, false),
                    ];
                    let job =
                        |n: u64| room(&format!("j-{n}"), &format!("jobs/j-{n} · job"), 7, false);
                    rooms.extend((100..150).map(job));
                    Reply::Channels(page(rooms))
                }
                Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
                Query::Listening { .. } => Reply::Listening(page(vec!["design".into()])),
                query => chat_reply(query),
            })
        }));
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.update(&view, |chat, _, cx| {
        chat.opened.insert("random".into());
        cx.notify();
    });
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(5, posted("design")));
    cx.run_until_parked();
    cx.simulate_bounds("chat-pane", (260., 0.), (840., 760.));
    cx.simulate_bounds("draft-general", (276., 640.), (808., 104.));
    cx.simulate_input("draft-general/editor", "#");
    cx.run_until_parked();
    let Some(wire::Node::UniformList { count, .. }) = cx.find("draft-general/mentions") else {
        panic!("the rooms are a list: {:?}", cx.texts())
    };
    assert_eq!(*count, 3, "the three rooms eddy is in");
    let mut offered = |typed: &str| {
        cx.simulate_input("draft-general/editor", typed);
        view.read(|chat| {
            let choices = chat.mention_choices();
            let found = crate::composer::matching_choices(&choices, '#', &typed[1..]);
            let labels = found.into_iter().map(|choice| choice.label.clone());
            labels.collect::<Vec<_>>()
        })
    };
    assert_eq!(offered("#"), ["Design", "General", "Random"]);
    let jobs = offered("#j-1");
    assert_eq!(jobs.len(), 50, "{jobs:?}");
    assert_eq!(offered("#j-12").len(), 10);
    assert_eq!(offered("#lob"), ["Lobby"]);
    assert!(offered("#att").is_empty(), "an archived room");
    assert!(offered("#dm").is_empty(), "a direct room");
}

#[test]
fn the_pane_says_where_it_is_for_the_mention_menu() {
    let (mut cx, chat) = opened();
    cx.simulate_bounds("chat-pane", (0., 28.), (766., 501.));
    let pane = crate::composer::Place {
        x: 0.,
        y: 28.,
        width: 766.,
        height: 501.,
    };
    chat.read(|chat| assert_eq!(chat.layout.pane, Some(pane)));
}

fn refused_in_general() -> (TestAppContext, Entity<Chat>) {
    let (mut cx, view) = opened();
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host()
        .refuse::<Submit<::chat::Chat>>("unauthorized", "not a member");
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "turned away");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let notice = cx.interactivity("draft-general/note");
    assert_eq!(
        notice.aria.label.as_deref(),
        Some("That didn’t go through: not a member"),
        "the whole refusal, said to assistive technology"
    );
    (cx, view)
}

#[test]
fn a_refusal_notice_goes_on_a_click() {
    let (mut cx, view) = refused_in_general();
    cx.simulate_click("draft-general/note");
    cx.run_until_parked();
    assert!(cx.find("draft-general/note").is_none(), "{:?}", cx.texts());
    assert!(
        cx.has_text("An earlier message wasn’t sent"),
        "the text to restore stays"
    );
    view.read(|chat| assert!(chat.drafts["draft-general"].note.is_empty()));
}

#[test]
fn a_refusal_notice_goes_with_the_next_good_send() {
    let (mut cx, _) = refused_in_general();
    cx.host()
        .handle::<Submit<::chat::Chat>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "let in");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    assert!(cx.find("draft-general/note").is_none(), "{:?}", cx.texts());
}

#[test]
fn a_refusal_notice_goes_when_the_room_changes() {
    let (mut cx, view) = refused_in_general();
    cx.update(&view, |chat, _, cx| {
        chat.open("dm-7-8".into(), cx);
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.find("draft-general/note").is_none(), "{:?}", cx.texts());
    assert!(
        cx.has_text("An earlier message wasn’t sent"),
        "the text to restore stays"
    );
}
