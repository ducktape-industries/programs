//! Agents in the room, read from identity alone: whom they would not
//! answer, said before the reader writes; where their work is shown; and
//! the card an agent's face opens.
use super::*;
use identity::Ownership;

/// An agent ada (10) manages, its script calling `calls`.
fn agent(number: u64, name: &str, calls: Option<&str>) -> chat::Profile {
    chat::Profile {
        kind: chat::Kind::Program {
            script: calls.map(|calls| abi::role::identity::Script {
                calls: calls.into(),
                config: String::new(),
            }),
            manager: 10,
            category: chat::Category::Agent,
            standing: chat::Standing::Active,
        },
        ..person(number, name)
    }
}

/// eddy (7) reads `general`, where he, scout, atlas and sage wrote. ada
/// manages the three agents: scout answers her only, atlas her and bo,
/// sage anyone; scout's and atlas's scripts call harness, sage has none.
fn agents() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(vec![
                person(7, "eddy"),
                person(10, "ada"),
                agent(11, "scout", Some("harness")),
                agent(12, "atlas", Some("harness")),
                agent(13, "sage", None),
                person(14, "bo"),
            ])),
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 4),
                channel("dm-7-11", "dm", 0),
                channel("dm-7-12", "dm", 0),
                channel("dm-7-13", "dm", 0),
            ])),
            Query::Roots { channel_id, .. } if channel_id == "general" => Reply::Roots(page(vec![
                row(1, 7, "hello"),
                row(2, 11, "on it"),
                row(3, 12, "done"),
                row(4, 13, "idle"),
            ])),
            query => chat_reply(query),
        })
    }));
    cx.host().handle::<Ask<access::Access>>(|query| {
        Ok(match query {
            access::Query::List { program, key } if program == harness::PROGRAM => {
                access::Reply::List(match key {
                    key if key == harness::agent_key(11) => Some(Ownership::Owner),
                    key if key == harness::agent_key(12) => {
                        Some(Ownership::Listed(vec![identity::Party::Account(14)]))
                    }
                    key if key == harness::agent_key(13) => Some(Ownership::Anyone),
                    key => panic!("{key} is no agent's"),
                })
            }
            query => panic!("unexpected access query: {query:?}"),
        })
    });
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    (cx, view)
}

fn opened_link(cx: &TestAppContext) -> Option<String> {
    cx.host().requests::<view::methods::LinkOpen>().pop()
}

/// The node the view last gave the keys to.
fn focus_given(cx: &TestAppContext) -> Option<wire::ElementIdWire> {
    let asked = cx.host().requests::<view::methods::HostWidget>();
    asked.into_iter().rev().find_map(|command| match command {
        wire::WidgetCommand::Focus { target } => target.last().cloned(),
        _ => None,
    })
}

fn named(key: &str) -> Option<wire::ElementIdWire> {
    Some(wire::ElementIdWire::Name(key.into()))
}

/// The mention menu's choices say why an agent would not answer the
/// writer: scout answers its manager only, atlas the people it lists, and
/// sage, open to anyone, says nothing; a person says nothing. (The menu
/// draws the hint as a badge: `composer/binding/tests.rs`; with the menu
/// open, the frame fails the audit on the menu's known faults.)
#[test]
fn the_mention_menu_says_which_agents_would_not_answer_the_writer() {
    let (_cx, view) = agents();
    let hints: Vec<(String, Option<String>)> = view.read(|chat| {
        let choices = chat.mention_choices();
        choices.into_iter().map(|c| (c.label, c.hint)).collect()
    });
    let hint = |label: &str| {
        let (_, hint) = hints.iter().find(|(l, _)| l == label).expect(label);
        hint.clone()
    };
    assert_eq!(hint("scout").as_deref(), Some("Answers its manager only"));
    assert_eq!(hint("atlas").as_deref(), Some("Answers listed people only"));
    assert_eq!(hint("sage").as_deref(), Some("Runs outside harness"));
    assert_eq!(hint("ada"), None);
}

/// A dm with an agent that would not answer the reader says so in its
/// header, and the composer stays open; one open to anyone says nothing,
/// and neither does a channel.
#[test]
fn a_dm_header_says_when_the_agent_will_not_reply() {
    let (mut cx, _) = agents();
    assert!(cx.find("chat-room-unanswered").is_none(), "a channel");
    for (peer, line) in [
        (
            11,
            Some("scout answers its manager only. Messages here get no reply."),
        ),
        (
            12,
            Some("atlas answers listed people only. Messages here get no reply."),
        ),
        (13, Some("sage runs outside harness.")),
    ] {
        cx.simulate_click(&format!("chat-sidebar-dm-{peer}"));
        cx.run_until_parked();
        match line {
            Some(line) => assert!(cx.has_text(line), "{:?}", cx.texts()),
            None => assert!(cx.find("chat-room-unanswered").is_none()),
        }
        assert!(cx.find(&format!("draft-dm-7-{peer}/editor")).is_some());
    }
}

/// An agent's message offers "View work record", which opens the room's
/// place in the program the agent's script calls; a person's message and an
/// agent's with no script do not offer it.
#[test]
fn an_agents_message_opens_its_work_record() {
    let (mut cx, view) = agents();
    let more = |cx: &mut TestAppContext, seq| {
        cx.update(&view, |chat, window, cx| {
            cx.notify();
            chat.open_menu(Pane::Timeline, seq, 0, Mode::More, window, cx);
        });
        cx.run_until_parked();
    };
    more(&mut cx, 2);
    assert!(cx.has_text("View work record"));
    cx.simulate_click("chat-menu-work-record");
    cx.run_until_parked();
    assert_eq!(
        opened_link(&cx).as_deref(),
        Some("duck://testnet-0a1b2c3d/harness/11/chat/general")
    );
    view.read(|chat| assert!(chat.menu.is_none()));
    for seq in [1, 4] {
        more(&mut cx, seq);
        assert!(cx.find("chat-menu-work-record").is_none(), "message {seq}");
    }
}

/// An agent's face opens its card: its name, what it is and who manages
/// it, whom it answers, its work record here and its profile, the link
/// the face opened before. An agent with no script has no work record.
#[test]
fn an_agents_face_opens_its_card() {
    let (mut cx, view) = agents();
    assert!(
        cx.interactivity("chat-message-m3-avatar")
            .aria
            .label
            .as_deref()
            .is_some_and(|label| label.ends_with("’s card")),
        "an agent's face says it opens a card"
    );
    cx.simulate_click("chat-message-m3-avatar");
    cx.run_until_parked();
    view.read(|chat| {
        let menu = chat.menu.as_ref().expect("the card is open");
        assert_eq!((menu.seq, menu.mode), (3, Mode::Profile));
    });
    assert!(opened_link(&cx).is_none(), "no link yet");
    assert!(cx.find("chat-card-badge").is_some());
    assert!(cx.has_text("Agent · managed by ada"));
    assert!(cx.has_text("Answers its manager and bo"));
    assert_eq!(
        focus_given(&cx),
        named("chat-card-work-record"),
        "its first button takes the keys"
    );
    cx.simulate_click("chat-card-work-record");
    cx.run_until_parked();
    assert_eq!(
        opened_link(&cx).as_deref(),
        Some("duck://testnet-0a1b2c3d/harness/12/chat/general")
    );
    view.read(|chat| assert!(chat.menu.is_none()));

    cx.simulate_click("chat-message-m2-avatar");
    cx.run_until_parked();
    assert!(cx.has_text("Answers its manager only"));
    cx.simulate_click("chat-card-open-profile");
    cx.run_until_parked();
    assert_eq!(
        opened_link(&cx).as_deref(),
        Some("duck://testnet-0a1b2c3d/identity/11")
    );

    cx.simulate_click("chat-message-m4-avatar");
    cx.run_until_parked();
    assert!(cx.has_text("Answers anyone"));
    assert!(cx.find("chat-card-work-record").is_none());
    assert_eq!(focus_given(&cx), named("chat-card-open-profile"));
}

/// A person's face still opens their profile, with no card.
#[test]
fn a_persons_face_opens_their_profile() {
    let (mut cx, view) = agents();
    cx.simulate_click("chat-message-m1-avatar");
    cx.run_until_parked();
    assert_eq!(
        opened_link(&cx).as_deref(),
        Some("duck://testnet-0a1b2c3d/identity/7")
    );
    view.read(|chat| assert!(chat.menu.is_none()));
    assert!(cx.find("chat-card").is_none());
}

// an agent whose script calls no harness says it runs outside harness on its card, and harness's
// list on its key is never read for it
#[test]
fn an_agent_harness_does_not_run_says_so_on_its_card() {
    let (mut cx, _view) = agents();
    cx.simulate_click("chat-message-m4-avatar");
    cx.run_until_parked();
    assert!(cx.find("chat-card-answers").is_some());
    assert!(cx.has_text("Runs outside harness"), "{:?}", cx.texts());
    assert!(!cx.has_text("Answers anyone"), "{:?}", cx.texts());
}
