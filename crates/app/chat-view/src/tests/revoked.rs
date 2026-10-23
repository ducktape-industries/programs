//! A revoked agent: no picker offers it and no member list seats it, and its direct room stays,
//! its name marked "(revoked)" (J-381).
use super::*;

const GONE: u64 = 11;

fn with_gone(query: Query) -> Reply {
    let Reply::Accounts(mut roster) = chat_reply(query) else {
        unreachable!("Accounts answers Accounts")
    };
    roster.items.push(chat::Profile {
        kind: chat::Kind::Program {
            script: None,
            manager: 7,
            category: chat::Category::Agent,
            standing: chat::Standing::Revoked,
        },
        ..person(GONE, "gone")
    });
    Reply::Accounts(roster)
}

fn opened_with_gone() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let direct = chat::dm_channel_id(7, GONE);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => with_gone(query),
                Query::Channels { .. } => Reply::Channels(page(vec![
                    channel("general", "General", 3),
                    channel(&direct, "dm", 1),
                ])),
                Query::Members { .. } => Reply::Members(page(
                    [7, 8, GONE]
                        .map(|number| chat::MemberRow {
                            principal: chat::Principal::Account(number),
                            height: 0,
                            time: 0,
                        })
                        .to_vec(),
                )),
                query => chat_reply(query),
            })
        }));
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    (cx, view)
}

#[test]
fn a_revoked_agents_direct_room_stays_and_says_so() {
    let (cx, _) = opened_with_gone();
    assert!(
        cx.find(&format!("chat-sidebar-dm-{GONE}")).is_some(),
        "its past messages stay reachable"
    );
    assert!(cx.has_text("gone (revoked)"), "{:?}", cx.texts());
}

#[test]
fn new_message_offers_no_revoked_agent() {
    let (mut cx, _) = opened_with_gone();
    cx.simulate_click("chat-sidebar-new-dm");
    assert!(cx.find("chat-dm-pick-8").is_some(), "the active agent");
    assert!(
        cx.find(&format!("chat-dm-pick-{GONE}")).is_none(),
        "the revoked one"
    );
}

#[test]
fn a_rooms_members_and_mentions_leave_a_revoked_agent_out() {
    let (mut cx, view) = opened_with_gone();
    cx.update(&view, |room, _, cx| room.choose("general".into(), cx));
    cx.run_until_parked();
    view.read(|room| {
        let seated: Vec<_> = room
            .roster()
            .into_iter()
            .map(|(principal, _)| principal)
            .collect();
        assert_eq!(seated, [7, 8].map(chat::Principal::Account));
        let offered: Vec<_> = room
            .mention_choices()
            .into_iter()
            .map(|choice| choice.label)
            .collect();
        assert!(!offered.contains(&"gone".to_owned()), "{offered:?}");
    });
}
