//! Archived channels leave the room lists as they are archived and come
//! back unarchived; a link or the open room still reaches one.
use super::room::claims;
use super::*;

fn archived(mut info: ChannelInfo) -> ChannelInfo {
    info.channel.archived = true;
    info
}

fn answer(cx: &TestAppContext, rooms: Vec<ChannelInfo>) {
    cx.host().handle::<Ask<::chat::Chat>>(move |query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(rooms.clone())),
            query => chat_reply(query),
        })
    });
}

fn desk(rooms: Vec<ChannelInfo>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer(&cx, rooms);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    (cx, view)
}

// what archiving or unarchiving `id` writes: its channel row
fn archiving(id: &str) -> Vec<Vec<u8>> {
    vec![chat::tables::CHANNELS.key(&id.to_owned())]
}

fn listed(view: &Entity<Chat>) -> Vec<String> {
    view.read(|chat| chat.room_list().ids())
}

#[test]
fn an_archived_channel_is_listed_in_neither_tree() {
    let rooms = vec![
        channel("general", "General", 3),
        archived(channel("old", "Old", 0)),
        channel("dm-7-8", "dm", 1),
    ];
    let (mut cx, view) = desk(rooms);
    assert_eq!(listed(&view), ["general", "dm-7-8"]);
    assert!(cx.find("chat-sidebar-channel-general").is_some());
    assert!(cx.find("chat-sidebar-channel-old").is_none());
    cx.simulate_touch(true);
    assert!(cx.find("chat-rooms-channel-general").is_some());
    assert!(cx.find("chat-rooms-channel-old").is_none());
    view.read(|chat| {
        let ids = chat.room_list().ids();
        assert!(!ids.contains(&"old".to_owned()), "{ids:?}");
    });
}

#[test]
fn archiving_hides_a_room_as_its_block_lands_and_unarchiving_brings_it_back() {
    let general = channel("general", "General", 3);
    let other = channel("other", "Other", 0);
    let (mut cx, view) = desk(vec![general.clone(), other.clone()]);
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    assert!(cx.find("chat-sidebar-channel-other").is_some());
    answer(&cx, vec![general.clone(), archived(other.clone())]);
    changes.send(block(4, archiving("other")));
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-other").is_none());
    assert_eq!(listed(&view), ["general"]);
    answer(&cx, vec![general, other]);
    changes.send(block(5, archiving("other")));
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-other").is_some());
    assert_eq!(listed(&view), ["general", "other"]);
}

// The open room archived stays open, read-only, and leaves the list; the
// arrows stand on the channel that took its row and walk on from there.
#[test]
fn the_open_room_archived_stays_open_and_the_arrows_take_the_next_room() {
    let rooms = vec![
        channel("alpha", "Alpha", 0),
        channel("general", "General", 3),
        channel("zed", "Zed", 0),
        channel("dm-7-8", "dm", 1),
    ];
    let (mut cx, view) = desk(rooms.clone());
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.find("draft-general/editor").is_some());
    let mut gone = rooms;
    gone[1].channel.archived = true;
    answer(&cx, gone);
    changes.send(block(4, archiving("general")));
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    assert!(cx.find("chat-room-archived").is_some());
    assert!(cx.find("draft-general/editor").is_none(), "read-only");
    assert!(cx.find("chat-sidebar-channel-general").is_none());
    assert!(claims(&cx, "chat-sidebar-channel-zed"));
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "down");
    assert!(claims(&cx, "chat-sidebar-dm-8"));
    cx.simulate_key_down("chat-sidebar-rooms-list", "enter");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "dm-7-8"));
}

// The last channel hidden has no channel after it: the one before takes
// its row.
#[test]
fn the_last_channel_hidden_hands_its_row_to_the_one_before() {
    let rooms = vec![
        channel("alpha", "Alpha", 0),
        channel("general", "General", 3),
        archived(channel("zed", "Zed", 0)),
        channel("dm-7-8", "dm", 1),
    ];
    let (mut cx, view) = desk(rooms);
    cx.update(&view, |chat, _, cx| {
        chat.rooms_cursor = Some("zed".into());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(claims(&cx, "chat-sidebar-channel-general"));
}

#[test]
fn a_link_still_opens_an_archived_channel_read_only() {
    let rooms = vec![
        channel("general", "General", 3),
        archived(channel("old", "Old", 0)),
    ];
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer(&cx, rooms);
    let routes = cx.host().stream::<view::methods::HostRoute>();
    let view = cx.open::<Chat>();
    seat(&mut cx);
    let link = crate::links::channel_link("testnet#0a1b2c3d", "old", None).unwrap();
    routes.send(ducklink::Link::parse(&link).unwrap().tail.join("/"));
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "old"));
    assert!(cx.find("chat-room-archived").is_some());
    assert!(cx.find("draft-old/editor").is_none(), "read-only");
    assert!(cx.find("chat-sidebar-channel-old").is_none());
}
