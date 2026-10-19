//! The rooms the reader is in: both lists hold them alone (opened on this device, listened to,
//! owned), and search finds the rest by name, above the messages.
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
            Query::Search { .. } => Reply::Hits(MessageHits {
                hits: vec![row(1, 7, "a review")],
                capped: false,
            }),
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
