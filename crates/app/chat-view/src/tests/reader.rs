//! Who reads: the seated key, the account it gains, the names it learns.
use super::*;

#[test]
fn an_account_gained_later_re_enables_create_channel() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Chat>();
    props.send(Session {
        account: None,
        ..session()
    });
    cx.host().stream::<HostVisible>().send(true);
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.write_gate(), Some(crate::session::Gate::NoAccount)));
    cx.simulate_click("chat-sidebar-new-channel");
    cx.run_until_parked();
    assert!(cx.has_text("Create an account to create a channel"));

    props.send(session());
    cx.run_until_parked();

    assert!(!cx.has_text("Create an account to create a channel"));
    view.read(|chat| assert_eq!(chat.my_account(), Some(7)));
}

fn answer_by_account(cx: &TestAppContext) {
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Listening { account: 7, .. } => Reply::Listening(page(vec!["general".into()])),
            Query::PostedIn { account: 7, .. } => Reply::PostedIn(page(vec!["general".into()])),
            query => chat_reply(query),
        })
    }));
}

fn switch_to_ada(cx: &mut TestAppContext) {
    cx.host().stream::<HostSession>().send(Session {
        signer: "0a0b".into(),
        account: Some(10),
        ..session()
    });
    cx.run_until_parked();
}

#[test]
fn a_key_switched_while_connected_reads_the_new_accounts_rooms_in_place_of_the_old() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer_by_account(&cx);
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    seat(&mut cx);
    view.read(|chat| {
        assert!(chat.listening.contains("general") && chat.posted.contains("general"));
    });

    cx.host().stream::<HostSession>().send(Session {
        signer: "0a0b".into(),
        account: Some(10),
        ..session()
    });
    cx.run_until_parked();
    view.read(|chat| {
        assert!(chat.listening.is_empty(), "{:?}", chat.listening);
        assert!(chat.posted.is_empty(), "{:?}", chat.posted);
    });
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    assert!(
        (asked.iter()).any(|query| matches!(query, Query::Listening { account: 10, .. })),
        "the new account's rooms are asked"
    );
}

#[test]
fn a_key_switched_while_connected_reads_the_open_room_for_the_new_viewer() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer_by_account(&cx);
    cx.open::<Chat>();
    cx.run_until_parked();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let ada = vec![Principal::Account(10)];
    let read_for_ada = |cx: &TestAppContext| {
        (cx.host().requests::<Ask<::chat::Chat>>().iter()).any(|query| match query {
            Query::Roots {
                channel_id, viewer, ..
            }
            | Query::MessagesAround {
                channel_id, viewer, ..
            } => channel_id == "general" && *viewer == ada,
            _ => false,
        })
    };
    assert!(!read_for_ada(&cx));
    switch_to_ada(&mut cx);
    assert!(
        read_for_ada(&cx),
        "the room is read again as the new viewer"
    );
}

#[test]
fn the_old_accounts_answers_landing_after_a_switch_are_dropped() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer_by_account(&cx);
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    seat(&mut cx);
    let asked_for_eddy = |cx: &TestAppContext| {
        (cx.host().requests::<Ask<::chat::Chat>>().iter())
            .filter(|query| matches!(query, Query::Listening { account: 7, .. }))
            .count()
    };
    let answered = asked_for_eddy(&cx);
    cx.host().never::<Ask<::chat::Chat>>();
    cx.host().stream::<Changes<::chat::Chat>>().send(None);
    cx.run_until_parked();
    assert!(
        asked_for_eddy(&cx) > answered,
        "eddy's rooms are asked again, and held"
    );

    answer_by_account(&cx);
    switch_to_ada(&mut cx);
    view.read(|chat| assert!(chat.listening.is_empty() && chat.posted.is_empty()));
    cx.host().answer_held::<Ask<::chat::Chat>>();
    cx.run_until_parked();
    view.read(|chat| {
        assert!(chat.listening.is_empty(), "{:?}", chat.listening);
        assert!(chat.posted.is_empty(), "{:?}", chat.posted);
    });
}

#[test]
fn a_peers_name_gained_later_replaces_its_numeric_fallback() {
    let known = std::rc::Rc::new(std::cell::Cell::new(false));
    let has_gary = known.clone();
    let mut cx = TestAppContext::new();
    quiet_methods(&mut cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => {
                    let mut accounts = vec![person(7, "eddy")];
                    if has_gary.get() {
                        accounts.push(person(9, "gary"));
                    }
                    Reply::Accounts(page(accounts))
                }
                Query::Channels { .. } => {
                    Reply::Channels(page(vec![channel("general", "General", 2)]))
                }
                Query::Roots { .. } => {
                    Reply::Roots(page(vec![row(1, 7, "hello"), row(2, 9, "hi from gary")]))
                }
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
                Query::RoomOrder { .. } => Reply::RoomOrder(Vec::new()),
                Query::Listening { .. } => Reply::Listening(page(Vec::new())),
                query => panic!("unexpected chat query: {query:?}"),
            })
        }));
    cx.host()
        .handle::<Submit<::chat::Chat>>(|_| Ok(view::testing::applied(Vec::new())));

    let live = cx.host().stream::<Changes<Identity>>();
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();

    let offered = |view: &Entity<Chat>| {
        view.read(|chat| {
            chat.mention_choices()
                .iter()
                .any(|choice| choice.label == "gary")
        })
    };
    assert!(
        cx.has_text("account 9"),
        "an unregistered-at-load author falls back to a numeric label"
    );
    assert!(!cx.has_text("gary"));
    assert!(!offered(&view), "not known to the roster yet");

    known.set(true);
    live.send(Some(view::methods::Change {
        height: 1,
        keys: Vec::new(),
    }));
    cx.run_until_parked();

    assert!(
        cx.has_text("gary"),
        "the roster re-reads on identity's live stream"
    );
    assert!(!cx.has_text("account 9"));
    assert!(offered(&view), "the re-read roster offers the peer too");
}

#[test]
fn the_roster_is_read_past_its_first_page() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Accounts { page } => {
                let start = page.after.map_or(1, |after| after[0] as u64 * 256 + 1);
                let end = (start + 255).min(600);
                let next = (end < 600).then(|| vec![(end / 256) as u8]);
                Reply::Accounts(::chat::PageResponse {
                    height: 1,
                    items: (start..=end)
                        .map(|number| person(number, &format!("user{number}")))
                        .collect(),
                    next,
                })
            }
            Query::Channels { .. } => Reply::Channels(page(Vec::new())),
            Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
            Query::RoomOrder { .. } => Reply::RoomOrder(Vec::new()),
            Query::Listening { .. } => Reply::Listening(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    let view = cx.open::<Chat>();
    seat(&mut cx);
    view.read(|chat| {
        let names = chat.names.ready().expect("the roster landed");
        assert_eq!(names.people().count(), 600);
        assert_eq!(names.name(&Principal::Account(600)), Some("user600"));
    });
}
