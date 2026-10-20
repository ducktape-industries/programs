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
