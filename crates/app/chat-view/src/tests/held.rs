//! The rooms both lists hold, as one tree: every channel its room's rules list for the reader,
//! each folder once with every such room its name files there, and no row unfolding the rest;
//! search finds rooms by name, above the messages.
use super::phone::phone;
use super::*;

// A busy network: General eddy owns; Design (its `category` meta read by nobody now), two jobs'
// rooms (one filed under jobs/), a change's review and Random, all others'; his direct room. He
// listens to `listening`.
fn busy_network(cx: &TestAppContext, listening: &'static [&'static str]) {
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            let theirs = |id: &str, name: &str| {
                let mut info = channel(id, name, 1);
                info.channel.owner = Principal::Account(9);
                info
            };
            Ok(match query {
                Query::Channels { .. } => {
                    let mut design = theirs("design", "Design");
                    design.meta.insert("category".into(), "Teams".into());
                    Reply::Channels(page(vec![
                        channel("general", "General", 3),
                        design,
                        theirs("j-075", "j-075"),
                        theirs("j-101", "jobs/j-101 · channel folders"),
                        theirs("forge:web:3", "Review web#3"),
                        theirs("random", "Random"),
                        channel("dm-7-8", "dm", 1),
                    ]))
                }
                Query::Listening { .. } => {
                    let rooms = listening.iter().map(|room| room.to_string()).collect();
                    Reply::Listening(page(rooms))
                }
                query => chat_reply(query),
            })
        }));
}

#[test]
fn both_lists_hold_every_room_the_rules_list_for_the_reader() {
    let (mut cx, view) = phone();
    busy_network(&cx, &["design"]);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    let every = [
        "general",
        "design",
        "j-075",
        "j-101",
        "forge:web:3",
        "random",
    ];
    let listed = |cx: &TestAppContext, tree: &str| -> Vec<&'static str> {
        every
            .into_iter()
            .filter(|room| cx.find(&format!("chat-{tree}-channel-{room}")).is_some())
            .collect()
    };
    assert_eq!(
        listed(&cx, "rooms"),
        every,
        "opened or not, listened to or not"
    );
    assert!(cx.find("chat-rooms-folder-jobs").is_some());
    assert!(
        cx.find("chat-rooms-category-Teams").is_none(),
        "a category meta files nothing"
    );
    cx.update(&view, |chat, _, cx| chat.choose("random".into(), cx));
    cx.run_until_parked();
    cx.simulate_back();
    cx.run_until_parked();
    let kept = cx.host().requests::<view::methods::StoreSet>();
    let opened = kept
        .iter()
        .any(|(key, value)| key.ends_with("/opened") && value.is_some());
    assert!(opened, "kept on the device");
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert_eq!(listed(&cx, "sidebar"), every, "the desk's the same");
}

// A network whose rooms' names hold "review": a change's room, a job's, an archived one and a
// direct room; a search answers one message.
fn review_rooms(cx: &TestAppContext) {
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => {
                let mut old = channel("old", "Old review", 1);
                old.channel.archived = true;
                let job = |id: &str, name: &str| {
                    let mut info = channel(id, name, 1);
                    info.channel.owner = Principal::Account(9);
                    info
                };
                Reply::Channels(page(vec![
                    channel("general", "General", 3),
                    review_room(),
                    job("j-101", "jobs/j-101 · review folders"),
                    old,
                    channel("dm-7-8", "review", 1),
                ]))
            }
            Query::TextSearch { .. } => Reply::TextHits(page(vec![row(1, 7, "a review")])),
            query => chat_reply(query),
        })
    }));
}

// The rooms search lists, by id.
fn found(cx: &TestAppContext) -> Vec<&'static str> {
    ["general", "forge:web:3", "j-101", "old", "dm-7-8"]
        .into_iter()
        .filter(|room| cx.find(&format!("chat-search-room-{room}")).is_some())
        .collect()
}

#[test]
fn the_phones_search_finds_rooms_by_name_above_the_messages() {
    let (mut cx, view) = phone();
    review_rooms(&cx);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    cx.simulate_click("Search");
    cx.simulate_input("search", "Review");
    cx.simulate_submit("search");
    cx.run_until_parked();
    assert_eq!(
        found(&cx),
        ["forge:web:3", "j-101"],
        "archived and direct rooms never"
    );
    let results = cx.node("chat-search-results").children();
    let first = results
        .first()
        .and_then(|node| node.key().map(|key| key.to_string()));
    assert_eq!(first.as_deref(), Some("chat-search-rooms-head"));
    assert!(cx.find("chat-search-head").is_some(), "the messages follow");
    cx.simulate_click("chat-search-room-j-101");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "j-101"));
    assert!(cx.find("chat-room-page").is_some());
}

#[test]
fn the_desks_search_finds_rooms_by_name_with_no_room_open() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    review_rooms(&cx);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    cx.simulate_input("chat-sidebar-search", "review");
    cx.simulate_submit("chat-sidebar-search");
    cx.run_until_parked();
    assert_eq!(found(&cx), ["forge:web:3", "j-101"]);
    let said = cx
        .texts()
        .iter()
        .any(|text| text.contains("rooms for “review”"));
    assert!(said, "{:?}", cx.texts());
    cx.simulate_click("chat-search-room-forge:web:3");
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(chat.room.as_ref().unwrap().id, "forge:web:3");
        assert!(chat.search.query.is_empty(), "the search let go");
    });
    assert!(cx.find("chat-sidebar-channel-forge:web:3").is_some());
}

#[derive(serde::Serialize)]
struct Before<'a> {
    #[serde(flatten)]
    chat: &'a Chat,
    others_open: bool,
    more_open: std::collections::BTreeSet<String>,
}

#[test]
fn a_view_moved_into_new_code_keeps_what_the_reader_set() {
    let (_old, view) = phone();
    let jobs = std::collections::BTreeSet::from(["jobs".to_owned()]);
    let before = view.read(|chat| {
        view::wire::try_encode(&Before {
            chat,
            others_open: true,
            more_open: jobs.clone(),
        })
    });
    let bytes = before.unwrap();
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    busy_network(&cx, &["design"]);
    let opened = std::collections::BTreeSet::from(["random".to_owned()]);
    cx.host().handle::<view::methods::StoreGet>(move |key| {
        Ok((key == "reads/0102/opened").then(|| view::methods::encode(&opened)))
    });
    let view = cx
        .restore::<Chat>(&bytes)
        .expect("a state that still holds the more row's fields");
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    seat(&mut cx);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    assert!(
        view.read(|chat| chat.is_in(chat.info("random").expect("random"))),
        "opened on the device before the swap"
    );
    cx.update(&view, |chat, _, cx| chat.choose("j-075".into(), cx));
    cx.run_until_parked();
    cx.simulate_back();
    cx.run_until_parked();
    let sets = cx.host().requests::<view::methods::StoreSet>();
    let kept = sets
        .into_iter()
        .rfind(|(key, _)| key == "reads/0102/opened")
        .and_then(|(_, value)| value)
        .expect("the rooms opened here, kept");
    let kept: std::collections::BTreeSet<String> = view::methods::decode(&kept).unwrap();
    assert_eq!(
        kept,
        std::collections::BTreeSet::from(["j-075".to_owned(), "random".to_owned()])
    );
}

#[test]
fn a_members_only_room_is_listed_only_where_the_reader_may_post() {
    let (mut cx, view) = phone();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        let members_only = |id: &str, owner: u64| {
            let mut info = channel(id, id, 1);
            info.channel.owner = Principal::Account(owner);
            info.channel.post_policy = PostPolicy::MembersOnly;
            info
        };
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 1),
                members_only("team", 9),
                members_only("secret", 9),
                members_only("mine", 7),
            ])),
            Query::Seated {
                principal,
                channel_ids,
            } => {
                assert_eq!(principal, Principal::Account(7));
                let seats = channel_ids.into_iter().filter(|id| id == "team");
                Reply::Seated(seats.collect())
            }
            query => chat_reply(query),
        })
    }));
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    let listed = |cx: &TestAppContext, tree: &str| -> Vec<&'static str> {
        ["general", "team", "secret", "mine"]
            .into_iter()
            .filter(|room| cx.find(&format!("chat-{tree}-channel-{room}")).is_some())
            .collect()
    };
    assert_eq!(
        listed(&cx, "rooms"),
        ["general", "team", "mine"],
        "seated in team, owner of mine though not seated"
    );
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert_eq!(listed(&cx, "sidebar"), ["general", "team", "mine"]);
}

#[test]
fn a_refused_seated_read_hides_no_members_only_room() {
    let (mut cx, view) = phone();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        let members_only = |id: &str| {
            let mut info = channel(id, id, 1);
            info.channel.owner = Principal::Account(9);
            info.channel.post_policy = PostPolicy::MembersOnly;
            info
        };
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 1),
                members_only("team"),
                members_only("secret"),
            ])),
            Query::Seated { .. } => {
                return Err(view::host::Error::new("refused", "chat has no Seated yet"));
            }
            query => chat_reply(query),
        })
    }));
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    view.read(|chat| {
        let list = chat.room_list();
        let mut ids: Vec<&str> = (list.channels.iter())
            .map(|info| info.channel.id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            ["general", "secret", "team"],
            "seats unknown, nothing hidden"
        );
    });
    assert!(cx.find("chat-rooms-channel-secret").is_some());
}

#[test]
fn a_room_listed_for_the_involved_shows_only_once_the_reader_sits_in_it_or_owns_it() {
    let (mut cx, view) = phone();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        let involved = |id: &str, owner: u64| {
            let mut info = channel(id, id, 1);
            info.channel.owner = Principal::Account(owner);
            let listed = (::chat::LISTED.to_owned(), ::chat::INVOLVED.to_owned());
            info.meta.extend([listed]);
            info
        };
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 1),
                involved("j-001", 9),
                involved("j-002", 9),
                involved("j-003", 7),
            ])),
            Query::Seated {
                principal,
                channel_ids,
            } => {
                assert_eq!(principal, Principal::Account(7));
                let seats = channel_ids.into_iter().filter(|id| id == "j-001");
                Reply::Seated(seats.collect())
            }
            query => chat_reply(query),
        })
    }));
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    let listed = |cx: &TestAppContext, tree: &str| -> Vec<&'static str> {
        ["general", "j-001", "j-002", "j-003"]
            .into_iter()
            .filter(|room| cx.find(&format!("chat-{tree}-channel-{room}")).is_some())
            .collect()
    };
    assert_eq!(
        listed(&cx, "rooms"),
        ["general", "j-001", "j-003"],
        "seated in j-001, owner of j-003"
    );
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert_eq!(listed(&cx, "sidebar"), ["general", "j-001", "j-003"]);
}

fn foldered_network(cx: &TestAppContext) {
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        let theirs = |id: &str, name: &str| {
            let mut info = channel(id, name, 1);
            info.channel.owner = Principal::Account(9);
            info
        };
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("j-140", "jobs/j-140 · phone no loading", 1),
                theirs("j-097", "jobs/j-097 · new nodes join fast"),
                theirs("j-111", "jobs/j-111 · chat round"),
                theirs("forge:programs:340", "pr/programs#340 forge notices"),
                theirs("forge:programs:341", "pr/programs#341 folders"),
                theirs("random", "Random"),
            ])),
            Query::Listening { .. } => Reply::Listening(page(Vec::new())),
            query => chat_reply(query),
        })
    }));
}

#[test]
fn a_room_the_reader_never_opened_is_listed_once_in_its_folder() {
    let (mut cx, view) = phone();
    foldered_network(&cx);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    let drawn = view.read(|chat| {
        let list = chat.room_list();
        let lines = chat.lines(&list.channels);
        let lines = lines.iter().map(|line| match line {
            crate::room_list::Line::Room { info, .. } => info.channel.id.clone(),
            crate::room_list::Line::Folder(folder) => format!("{}/", folder.path),
        });
        lines.collect::<Vec<_>>().join(" ")
    });
    assert_eq!(
        drawn,
        "jobs/ j-140 j-097 j-111 pr/ forge:programs:340 forge:programs:341 random"
    );
    let rooms = [
        "j-140",
        "j-097",
        "j-111",
        "forge:programs:340",
        "forge:programs:341",
        "random",
    ];
    let said = |cx: &TestAppContext, word: &str| {
        let texts = cx.texts();
        texts.iter().filter(|text| text.trim() == word).count()
    };
    for (tree, touch, (width, height)) in [
        ("rooms", true, (390., 844.)),
        ("sidebar", false, (1200., 800.)),
    ] {
        cx.simulate_touch(touch);
        cx.simulate_resize(width, height);
        for room in rooms {
            let row = format!("chat-{tree}-channel-{room}");
            assert!(cx.find(&row).is_some(), "{room} on the {tree}");
        }
        assert!(cx.find(&format!("chat-{tree}-folder-jobs")).is_some());
        assert!(cx.find(&format!("chat-{tree}-folder-pr")).is_some());
        assert_eq!(said(&cx, "jobs"), 1, "jobs/ once on the {tree}");
        assert_eq!(said(&cx, "pr"), 1, "pr/ once on the {tree}");
        let more = cx.texts().iter().any(|text| text.contains("more channel"));
        assert!(!more, "no more row on the {tree}: {:?}", cx.texts());
    }
}
