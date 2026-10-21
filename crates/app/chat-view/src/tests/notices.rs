//! The pane's side of notices: it posts none and counts no badge (chat's
//! notifier does), takes a read room's notices down, names the room on
//! screen to the host, and keeps the read cursors between runs.
use super::*;

#[test]
fn a_pane_posts_nothing_and_takes_a_read_rooms_notices_down() {
    use view::methods::{HostBadge, HostShowing, NotifyPost, NotifySeen};
    let (mut cx, view) = opened();
    assert_eq!(
        cx.host().requests::<HostShowing>().last().unwrap(),
        "general"
    );
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query @ Query::MessagesAround { .. } => chat_reply(query),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.channels_landed(
            vec![channel("general", "General", 3), channel("dm-7-8", "dm", 2)],
            cx,
        );
    });
    cx.run_until_parked();
    assert!(cx.host().requests::<NotifyPost>().is_empty());
    assert!(cx.host().requests::<HostBadge>().is_empty());
    assert!(cx.find("chat-sidebar-dm-8-unread").is_some());
    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.choose("dm-7-8".into(), cx)
    });
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<NotifySeen>(), ["@reviewer"]);
    assert_eq!(
        cx.host().requests::<HostShowing>().last().unwrap(),
        "dm-7-8"
    );
    assert!(cx.host().requests::<NotifyPost>().is_empty());
}

#[test]
fn kept_cursors_bring_the_badge_back_after_a_relaunch() {
    use std::collections::BTreeMap;
    use view::methods::{self, StoreGet, StoreSet};
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<StoreGet>(|key| {
        Ok((key == "reads/0102").then(|| {
            methods::encode(&BTreeMap::from([
                ("general".to_owned(), 1u64),
                ("dm-7-8".to_owned(), 0),
                ("gone".to_owned(), 4),
            ]))
        }))
    });
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 3),
                channel("dm-7-8", "dm", 1),
            ])),
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            query @ Query::MessagesAround { .. } => chat_reply(query),
            Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
            Query::RoomOrder { .. } => Reply::RoomOrder(Vec::new()),
            Query::Listening { .. } => Reply::Listening(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.open::<Chat>();
    seat(&mut cx);
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
    assert!(cx.find("chat-sidebar-dm-8-unread").is_some());

    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let (_, kept) = cx
        .host()
        .requests::<StoreSet>()
        .into_iter()
        .rfind(|(key, _)| key.as_str() == "reads/0102")
        .expect("the read is kept");
    let kept: BTreeMap<String, u64> = methods::decode(&kept.unwrap()).unwrap();
    assert_eq!(
        kept,
        BTreeMap::from([("general".into(), 3), ("dm-7-8".into(), 0)]),
        "a room no longer listed is not kept"
    );
}

#[test]
fn a_refused_store_read_still_keeps_cursors() {
    use std::collections::BTreeMap;
    use view::host::malformed;
    use view::methods::{self, StoreGet, StoreSet};
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<StoreGet>(|key| match key.as_str() {
        "reads/0102" => Err(malformed("the store is unavailable".into())),
        _ => Ok(None),
    });
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
            Query::Channels { .. } => Reply::Channels(page(vec![channel("general", "General", 3)])),
            Query::Roots { .. } => Reply::Roots(page(Vec::new())),
            Query::Members { .. } => Reply::Members(page(Vec::new())),
            Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
            Query::RoomOrder { .. } => Reply::RoomOrder(Vec::new()),
            Query::Listening { .. } => Reply::Listening(page(Vec::new())),
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.open::<Chat>();
    seat(&mut cx);
    let asked = cx.host().requests::<StoreGet>();
    assert_eq!(
        asked.iter().filter(|key| *key == "reads/0102").count(),
        1,
        "{asked:?}"
    );
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let (_, kept) = cx
        .host()
        .requests::<StoreSet>()
        .into_iter()
        .rfind(|(key, _)| key == "reads/0102")
        .expect("the read is kept");
    let kept: BTreeMap<String, u64> = methods::decode(&kept.unwrap()).unwrap();
    assert_eq!(kept, BTreeMap::from([("general".into(), 3)]));
}

#[test]
fn the_details_choose_a_rooms_notices() {
    use view::methods::Submit;
    let (mut cx, _view) = opened();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Notices { .. } => {
                Reply::Notices(page(vec![("general".into(), chat::Notices::Mentions)]))
            }
            query => chat_reply(query),
        })
    }));
    cx.simulate_click("chat-room-details");
    cx.run_until_parked();
    let group = cx.interactivity("chat-details-notices").role;
    assert_eq!(group, Some(view::Role::RadioGroup));
    let checked = |cx: &TestAppContext, slug| {
        let id = format!("chat-details-notices-{slug}");
        cx.interactivity(&id).aria.toggled
    };
    assert_eq!(checked(&cx, "mentions"), Some(view::Toggled::True));
    assert_eq!(checked(&cx, "default"), Some(view::Toggled::False));
    assert!(cx.has_text("All messages"));

    let sent = |cx: &TestAppContext| cx.host().requests::<Submit<::chat::Chat>>();
    let before = sent(&cx).len();
    cx.simulate_focus("chat-details-notices");
    cx.simulate_key_down("chat-details-notices", "down");
    cx.run_until_parked();
    assert_eq!(sent(&cx).len(), before, "a step is no write");
    let off = cx.interactivity("chat-details-notices-off");
    assert!(off.aria.active_descendant);
    assert_eq!(off.aria.toggled, Some(view::Toggled::False));
    cx.simulate_key_down("chat-details-notices", "enter");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx).last(),
        Some(&chat::Op::SetNotices {
            channel_id: "general".into(),
            notices: Some(chat::Notices::Off),
        })
    );

    cx.simulate_click("chat-details-notices-every");
    cx.simulate_click("chat-details-notices-off");
    cx.run_until_parked();
    let sent = sent(&cx);
    assert_eq!(
        sent[sent.len() - 2..],
        [
            chat::Op::Listen {
                channel_id: "general".into(),
                listening: true,
            },
            chat::Op::SetNotices {
                channel_id: "general".into(),
                notices: Some(chat::Notices::Off),
            },
        ]
    );
}

fn eddys_agents(cx: &TestAppContext) {
    let agent = |number, name: &str, standing| chat::Profile {
        kind: chat::Kind::Program {
            script: None,
            manager: 7,
            category: chat::Category::Agent,
            standing,
        },
        ..person(number, name)
    };
    let roster = vec![
        person(7, "eddy"),
        agent(8, "reviewer", chat::Standing::Active),
        agent(11, "scout", chat::Standing::Active),
        agent(12, "retired", chat::Standing::Revoked),
    ];
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(roster.clone())),
                Query::ChosenBy {
                    channel_id,
                    accounts,
                } => {
                    assert_eq!(channel_id, "general");
                    assert_eq!(accounts, [8, 11], "the agents that act, in one read");
                    Reply::ChosenBy(
                        accounts
                            .into_iter()
                            .map(|account| chat::AccountChoice {
                                account,
                                listening: false,
                                notices: (account == 8).then_some(chat::Notices::Off),
                            })
                            .collect(),
                    )
                }
                query => chat_reply(query),
            })
        }));
    cx.host().handle::<Ask<access::Access>>(|query| {
        Ok(match query {
            access::Query::List { .. } => access::Reply::List(None),
            query => panic!("unexpected access query: {query:?}"),
        })
    });
}

#[test]
fn the_details_choose_what_the_readers_agents_hear() {
    use view::methods::Submit;
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    eddys_agents(&cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_click("chat-room-details");
    cx.run_until_parked();
    assert!(
        cx.find("chat-details-agent-12").is_none(),
        "a revoked agent"
    );
    let pick = |whose: &str| format!("chat-details-{whose}-pick");
    let said = |cx: &TestAppContext, whose: &str| cx.interactivity(&pick(whose)).aria.label.clone();
    assert_eq!(
        said(&cx, "agent-8").as_deref(),
        Some("What reviewer hears: Off")
    );
    assert_eq!(
        said(&cx, "agent-11").as_deref(),
        Some("What scout hears: Default")
    );
    assert_eq!(
        said(&cx, "agents-all").as_deref(),
        Some("What your agents hear: Mixed")
    );

    let sent = |cx: &TestAppContext| cx.host().requests::<Submit<::chat::Chat>>();
    let before = sent(&cx).len();
    cx.simulate_click(&pick("agents-all"));
    cx.run_until_parked();
    assert!(
        cx.find("chat-notices-menu").is_none(),
        "it waits for its button's place"
    );
    cx.simulate_bounds("chat-notices-menu-anchor", (700., 400.), (140., 32.));
    cx.run_until_parked();
    assert!(cx.find("chat-notices-menu").is_some());
    cx.simulate_click("chat-notices-menu/mentions");
    cx.run_until_parked();
    assert!(cx.find("chat-notices-menu").is_none(), "a pick closes it");
    cx.simulate_click(&pick("agent-11"));
    cx.run_until_parked();
    cx.simulate_bounds("chat-notices-menu-anchor", (700., 480.), (140., 32.));
    cx.run_until_parked();
    cx.simulate_click("chat-notices-menu/every");
    cx.run_until_parked();
    let general = || "general".to_owned();
    assert_eq!(
        sent(&cx)[before..],
        [
            chat::Op::ChooseForMany {
                accounts: vec![8, 11],
                channel_id: general(),
                listening: false,
                notices: Some(chat::Notices::Mentions),
            },
            chat::Op::ChooseForMany {
                accounts: vec![11],
                channel_id: general(),
                listening: true,
                notices: None,
            },
        ]
    );

    cx.update(&view, |chat, _, cx| {
        cx.notify();
        chat.choose("dm-7-8".into(), cx)
    });
    cx.run_until_parked();
    if cx.find("chat-details-notices").is_none() {
        cx.simulate_click("chat-room-details");
        cx.run_until_parked();
    }
    assert!(cx.find("chat-details-notices").is_some());
    assert!(cx.find(&pick("agent-8")).is_none(), "a direct room");
}

#[test]
fn export_details_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for (touch, (width, height)) in [(false, (1000, 900)), (true, (390, 1400))] {
        let mut cx = TestAppContext::new();
        configure(&mut cx);
        eddys_agents(&cx);
        let only = a_policy(4, 7, ownership::ONLY_ME, ownership::Writers::Owner);
        let general = chat::tables::CHANNELS.key(&"general".to_owned());
        owned(&cx, general, 7, only.clone(), vec![only]);
        cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
        cx.open::<Chat>();
        cx.simulate_touch(touch);
        cx.simulate_resize(width as f32, height as f32);
        cx.run_until_parked();
        seat(&mut cx);
        let (device, room, details) = match touch {
            true => ("phone", "chat-rooms-channel-general", "Channel details"),
            false => (
                "desktop",
                "chat-sidebar-channel-general",
                "chat-room-details",
            ),
        };
        cx.simulate_click(room);
        cx.run_until_parked();
        cx.simulate_click(details);
        cx.run_until_parked();
        for state in ["list", "choosing"] {
            if state == "choosing" {
                cx.simulate_click("chat-details-agents-all-pick");
                cx.run_until_parked();
                if !touch {
                    cx.simulate_bounds("chat-notices-menu-anchor", (790., 422.), (148., 28.));
                    cx.run_until_parked();
                }
            }
            let name = format!("details-{device}-{state}");
            let frame = serde_json::to_vec(&cx.whole_frame()).unwrap();
            std::fs::write(out.join(format!("{name}.json")), frame).unwrap();
            let size = serde_json::json!({ "name": name, "width": width, "height": height });
            manifest.push(size);
        }
    }
    let listed = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(out.join("details-manifest.json"), listed).unwrap();
}
