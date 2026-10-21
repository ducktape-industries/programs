//! The rooms the reader is in: both lists hold them (opened on this device, listened to, owned),
//! the rest unfold under their last row, and search finds rooms by name, above the messages.
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
fn the_rooms_page_lists_only_the_rooms_the_reader_is_in() {
    let (mut cx, view) = phone();
    busy_network(&cx, &["design"]);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    assert!(cx.find("chat-rooms-channel-general").is_some(), "owned");
    assert!(
        cx.find("chat-rooms-category-Teams").is_none(),
        "a category meta files nothing"
    );
    assert!(
        cx.find("chat-rooms-channel-design").is_some(),
        "listened to"
    );
    for out in ["j-075", "j-101", "forge:web:3", "random"] {
        let row = format!("chat-rooms-channel-{out}");
        assert!(cx.find(&row).is_none(), "{out} is not the reader's");
    }
    assert!(cx.find("chat-rooms-folder-jobs").is_none());
    assert!(!cx.has_text("Work") && !cx.has_text("All channels"));
    assert!(
        cx.find("chat-rooms-none-held").is_none(),
        "the reader is in two"
    );
    cx.update(&view, |chat, _, cx| chat.choose("random".into(), cx));
    cx.run_until_parked();
    cx.simulate_back();
    cx.run_until_parked();
    assert!(
        cx.find("chat-rooms-channel-random").is_some(),
        "opened here, Random is the reader's"
    );
    let kept = cx.host().requests::<view::methods::StoreSet>();
    let opened = kept
        .iter()
        .any(|(key, value)| key.ends_with("/opened") && value.is_some());
    assert!(opened, "kept on the device");
}

// A reader in no channel of a network that holds some is told where the rest are, on both trees.
#[test]
fn a_reader_in_no_channel_is_told_search_finds_them() {
    let (mut cx, view) = phone();
    busy_network(&cx, &[]);
    cx.update(&view, |chat, _, cx| {
        chat.session.account = Some(10);
        chat.load_channels(cx)
    });
    cx.run_until_parked();
    assert!(cx.find("chat-rooms-none-held").is_some());
    assert!(
        cx.find("chat-rooms-empty").is_none(),
        "the network has rooms"
    );
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert!(cx.find("chat-sidebar-none-held").is_some());
}

// Whether the change's review room is listed: on the desk's sidebar, and on the phone's page.
fn review_listed(cx: &mut TestAppContext) -> (bool, bool) {
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    let desk = cx.find("chat-sidebar-channel-forge:web:3").is_some();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    let phone = cx.find("chat-rooms-channel-forge:web:3").is_some();
    (desk, phone)
}

// A change's review room is forge's: a reader who neither listens to it nor opened it finds it in
// neither list; listening puts it in both, and so does opening it.
#[test]
fn a_change_room_of_anothers_is_listed_once_listened_to_or_opened() {
    let (mut cx, view) = phone();
    busy_network(&cx, &["design"]);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    assert_eq!(review_listed(&mut cx), (false, false), "neither");
    busy_network(&cx, &["design", "forge:web:3"]);
    cx.update(&view, |chat, _, cx| chat.load_listening(cx));
    cx.run_until_parked();
    assert_eq!(review_listed(&mut cx), (true, true), "listened to");
    busy_network(&cx, &["design"]);
    cx.update(&view, |chat, _, cx| chat.load_listening(cx));
    cx.run_until_parked();
    assert_eq!(review_listed(&mut cx), (false, false), "no longer");
    cx.update(&view, |chat, _, cx| chat.choose("forge:web:3".into(), cx));
    cx.run_until_parked();
    cx.simulate_back();
    cx.run_until_parked();
    assert_eq!(review_listed(&mut cx), (true, true), "opened here");
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
    assert!(
        cx.find("chat-rooms-channel-j-101").is_none(),
        "not the reader's"
    );
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
    assert!(
        cx.find("chat-sidebar-channel-j-101").is_none(),
        "not the reader's"
    );
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
    assert!(
        cx.find("chat-sidebar-channel-forge:web:3").is_some(),
        "opened, it is the reader's"
    );
}

const OTHERS: [&str; 4] = ["j-075", "j-101", "forge:web:3", "random"];

// The channels of `OTHERS` a tree lists (`rooms` the phone's, `sidebar` the desk's).
fn others_listed(cx: &TestAppContext, tree: &str) -> Vec<&'static str> {
    OTHERS
        .into_iter()
        .filter(|room| cx.find(&format!("chat-{tree}-channel-{room}")).is_some())
        .collect()
}

// The channels the reader is not in wait under the list's last row, on both trees: it counts them,
// a press lists them under it by name, and one opened from there is the reader's.
#[test]
fn the_channels_the_reader_is_not_in_unfold_under_the_lists_last_row() {
    let (mut cx, view) = phone();
    busy_network(&cx, &["design"]);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    assert!(cx.has_text("4 more channels"), "{:?}", cx.texts());
    assert!(others_listed(&cx, "rooms").is_empty(), "folded");
    cx.simulate_click("chat-rooms-more");
    cx.run_until_parked();
    assert_eq!(
        others_listed(&cx, "rooms"),
        ["j-075", "forge:web:3", "random"],
        "unfolded, j-101 in its folder, closed"
    );
    cx.simulate_click("chat-rooms-more-folder-jobs");
    cx.run_until_parked();
    assert_eq!(others_listed(&cx, "rooms"), OTHERS, "its folder opened");
    view.read(|chat| {
        let list = chat.room_list();
        let ids: Vec<&str> = (list.others.iter())
            .map(|info| info.channel.id.as_str())
            .collect();
        assert_eq!(ids, ["j-075", "j-101", "random", "forge:web:3"], "by name");
    });
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert_eq!(others_listed(&cx, "sidebar"), OTHERS, "the desk's too");
    cx.simulate_click("chat-sidebar-more");
    cx.run_until_parked();
    assert!(others_listed(&cx, "sidebar").is_empty(), "folded again");
    cx.update(&view, |chat, _, cx| chat.choose("random".into(), cx));
    cx.run_until_parked();
    assert!(
        cx.has_text("3 more channels"),
        "opened, Random is the reader's"
    );
    assert!(cx.find("chat-sidebar-channel-random").is_some());
}

#[test]
fn the_rooms_the_reader_is_not_in_unfold_into_their_folders() {
    let (mut cx, view) = phone();
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
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    let shown = |cx: &TestAppContext, id: &str| cx.find(id).is_some();
    let rooms = |cx: &TestAppContext, tree: &str| -> Vec<&'static str> {
        [
            "j-097",
            "j-111",
            "forge:programs:340",
            "forge:programs:341",
            "random",
        ]
        .into_iter()
        .filter(|room| shown(cx, &format!("chat-{tree}-channel-{room}")))
        .collect()
    };
    assert!(shown(&cx, "chat-rooms-channel-j-140"), "the reader's own");
    assert!(rooms(&cx, "rooms").is_empty(), "none of theirs");
    assert!(cx.has_text("5 more channels"), "{:?}", cx.texts());
    cx.simulate_click("chat-rooms-more");
    cx.run_until_parked();
    assert!(
        shown(&cx, "chat-rooms-more-folder-jobs"),
        "{:?}",
        cx.texts()
    );
    assert!(shown(&cx, "chat-rooms-more-folder-pr"), "{:?}", cx.texts());
    assert_eq!(
        rooms(&cx, "rooms"),
        ["random"],
        "the folders closed, the unfiled loose"
    );
    cx.simulate_click("chat-rooms-more-folder-jobs");
    cx.run_until_parked();
    assert_eq!(
        rooms(&cx, "rooms"),
        ["j-097", "j-111", "random"],
        "jobs/ opened"
    );
    view.read(|chat| {
        assert!(
            chat.closed_folders.is_empty(),
            "the reader's own jobs/ untouched"
        )
    });
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert_eq!(
        rooms(&cx, "sidebar"),
        ["j-097", "j-111", "random"],
        "the desk's the same"
    );
    cx.simulate_click("chat-sidebar-more-folder-pr");
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-more-folder-jobs");
    cx.run_until_parked();
    assert_eq!(
        rooms(&cx, "sidebar"),
        ["forge:programs:340", "forge:programs:341", "random"],
        "pr/ opened, jobs/ closed again"
    );
    assert!(
        shown(&cx, "chat-sidebar-channel-j-140"),
        "the reader's jobs/ stays open"
    );
}

#[test]
fn a_more_folder_named_as_a_channel_opens_it_from_the_keys() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        let theirs = |id: &str, name: &str| {
            let mut info = channel(id, name, 1);
            info.channel.owner = Principal::Account(9);
            info
        };
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(vec![
                channel("general", "General", 1),
                theirs("jobs", "jobs"),
                theirs("jobs-x", "jobs/x"),
            ])),
            Query::Listening { .. } => Reply::Listening(page(Vec::new())),
            query => chat_reply(query),
        })
    }));
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-more");
    cx.run_until_parked();
    let row = "chat-sidebar-more-folder-jobs";
    assert!(cx.find(row).is_some(), "{:?}", cx.texts());
    let spot = crate::room_list::Spot::MoreFolder {
        path: "jobs".into(),
        own: Some("jobs".into()),
    };
    cx.update(&view, |chat, _, cx| {
        chat.rooms_cursor = Some(spot);
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        cx.interactivity(row).aria.active_descendant,
        "the arrows stand on it"
    );
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "shift-f10");
    let menu = view.read(|chat| chat.room_menu.as_ref().map(|menu| menu.room.clone()));
    assert_eq!(menu.as_deref(), Some("jobs"), "the menu key opens its menu");
    cx.update(&view, |chat, _, cx| {
        chat.room_menu = None;
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "enter");
    cx.run_until_parked();
    view.read(|chat| {
        assert_eq!(
            chat.room.as_ref().map(|room| room.id.as_str()),
            Some("jobs")
        )
    });
    cx.update(&view, |chat, _, cx| {
        chat.rooms_cursor = None;
        cx.notify();
    });
    cx.run_until_parked();
    let mine = "chat-sidebar-channel-jobs";
    assert!(
        cx.interactivity(mine).aria.active_descendant,
        "opened, it is the reader's, and its row stands active"
    );
}

#[test]
fn a_view_moved_into_new_code_keeps_what_the_reader_set() {
    let (mut old, view) = phone();
    old.update(&view, |chat, _, cx| {
        chat.fold_others(true);
        cx.notify();
    });
    let bytes = old.snapshot().unwrap();
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    busy_network(&cx, &["design"]);
    let opened = std::collections::BTreeSet::from(["random".to_owned()]);
    cx.host().handle::<view::methods::StoreGet>(move |key| {
        Ok((key == "reads/0102/opened").then(|| view::methods::encode(&opened)))
    });
    let view = cx.restore::<Chat>(&bytes).unwrap();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    seat(&mut cx);
    cx.update(&view, |chat, _, cx| chat.load_channels(cx));
    cx.run_until_parked();
    assert!(view.read(|chat| chat.others_open), "still unfolded");
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
    assert!(
        cx.find("chat-rooms-more").is_none(),
        "secret is no room the reader is not in yet: {:?}",
        cx.texts()
    );
    cx.simulate_touch(false);
    cx.simulate_resize(1200., 800.);
    assert_eq!(listed(&cx, "sidebar"), ["general", "team", "mine"]);
    assert!(cx.find("chat-sidebar-more").is_none());
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
        let held = list.channels.iter().chain(&list.others);
        let mut ids: Vec<&str> = held.map(|info| info.channel.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(
            ids,
            ["general", "secret", "team"],
            "seats unknown, nothing hidden"
        );
    });
    assert!(cx.has_text("2 more channels"), "{:?}", cx.texts());
}
