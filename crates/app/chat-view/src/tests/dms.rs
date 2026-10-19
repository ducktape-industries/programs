//! Direct messages on the desktop and the phone: the rooms the reader holds,
//! newest message first, and New message, the picker of whom to write to.
use super::room::claims;
use super::*;

fn dm(peer: u64, head_seq: u64, created_at: u64) -> ChannelInfo {
    let mut info = channel(&chat::dm_channel_id(7, peer), "dm", head_seq);
    info.channel.created_at = created_at;
    info
}

fn dm_id(peer: u64) -> String {
    chat::dm_channel_id(7, peer)
}

fn answer(cx: &TestAppContext, rooms: Vec<ChannelInfo>, newest: Vec<(u64, u64)>) {
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(rooms.clone())),
                Query::MessagesAround {
                    channel_id, seq, ..
                } => {
                    let time = newest
                        .iter()
                        .find(|(peer, _)| dm_id(*peer) == channel_id)
                        .map_or(0, |(_, time)| *time);
                    Reply::Messages(vec![MsgRow {
                        channel_id,
                        time,
                        ..row(seq, 8, "said")
                    }])
                }
                query => chat_reply(query),
            })
        }));
}

fn desk(rooms: Vec<ChannelInfo>, newest: Vec<(u64, u64)>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    answer(&cx, rooms, newest);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    (cx, view)
}

fn listed(view: &Entity<Chat>) -> Vec<String> {
    view.read(|chat| chat.room_list().ids())
}

fn three_dms() -> Vec<ChannelInfo> {
    vec![
        channel("general", "General", 3),
        dm(8, 2, 100),
        dm(10, 0, 500),
        dm(11, 4, 50),
    ]
}

fn asked_heads(cx: &TestAppContext) -> usize {
    cx.host()
        .requests::<Ask<::chat::Chat>>()
        .iter()
        .filter(|query| matches!(query, Query::MessagesAround { .. }))
        .count()
}

fn search(cx: &mut TestAppContext, view: &Entity<Chat>, text: &str) {
    cx.update(view, |chat, _, cx| {
        chat.dm_pick
            .as_mut()
            .expect("the picker is open")
            .query
            .reset(text);
        cx.notify();
    });
    cx.run_until_parked();
}

#[test]
fn direct_messages_hold_only_the_rooms_the_reader_has() {
    let (cx, view) = opened();
    assert!(cx.find("chat-sidebar-dm-8").is_some(), "the agent's room");
    for (peer, who) in [(9, "forge's account"), (10, "ada"), (7, "the reader")] {
        assert!(
            cx.find(&format!("chat-sidebar-dm-{peer}")).is_none(),
            "{who} has no row"
        );
    }
    assert_eq!(listed(&view), ["general".to_owned(), dm_id(8)]);
}

#[test]
fn direct_messages_go_newest_first() {
    let (mut cx, view) = desk(three_dms(), vec![(8, 300), (11, 900)]);
    assert_eq!(
        listed(&view),
        ["general".to_owned(), dm_id(11), dm_id(10), dm_id(8)]
    );
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "down");
    assert!(claims(&cx, "chat-sidebar-dm-11"));
    cx.simulate_key_down("chat-sidebar-rooms-list", "down");
    assert!(claims(&cx, "chat-sidebar-dm-10"));
}

#[test]
fn a_new_message_moves_its_room_to_the_top() {
    let (mut cx, view) = desk(three_dms(), vec![(8, 300), (11, 900)]);
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let read = asked_heads(&cx);
    assert_eq!(read, 2, "the two rooms with a message");
    changes.send(block(5, posted("general")));
    cx.run_until_parked();
    assert_eq!(asked_heads(&cx), read, "no direct room's head moved");
    let mut rooms = three_dms();
    rooms[1].head_seq = 3;
    answer(&cx, rooms, vec![(8, 1_000), (11, 900)]);
    changes.send(block(6, posted(&dm_id(8))));
    cx.run_until_parked();
    assert_eq!(asked_heads(&cx), read + 1, "the agent's room alone");
    assert_eq!(
        listed(&view),
        ["general".to_owned(), dm_id(8), dm_id(11), dm_id(10)]
    );
}

#[test]
fn direct_messages_offers_new_message_with_no_room_under_it() {
    let (cx, view) = desk(vec![channel("general", "General", 3)], Vec::new());
    assert!(cx.find("chat-sidebar-dm-header").is_some());
    assert!(cx.find("chat-sidebar-new-dm").is_some());
    assert_eq!(listed(&view), ["general"]);
}

#[test]
fn new_message_lists_people_and_agents_but_no_program_account_or_the_reader() {
    let (mut cx, view) = opened();
    cx.simulate_click("chat-sidebar-new-dm");
    assert!(cx.find("chat-dm-pick-card").is_some(), "the picker is open");
    assert!(cx.find("chat-dm-pick-8").is_some(), "the agent");
    assert!(cx.find("chat-dm-pick-8-agent").is_some(), "wears Agent");
    assert!(cx.find("chat-dm-pick-10").is_some(), "ada");
    assert!(cx.find("chat-dm-pick-9").is_none(), "forge's own account");
    assert!(cx.find("chat-dm-pick-7").is_none(), "the reader");
    search(&mut cx, &view, "AD");
    assert!(cx.find("chat-dm-pick-10").is_some(), "by name");
    assert!(cx.find("chat-dm-pick-8").is_none());
    search(&mut cx, &view, "8");
    assert!(cx.find("chat-dm-pick-8").is_some(), "by number");
    assert!(cx.find("chat-dm-pick-10").is_none());
    search(&mut cx, &view, "forge");
    assert!(cx.find("chat-dm-pick-none").is_some(), "nobody matches");
    cx.simulate_click("chat-dm-pick-cancel");
    view.read(|chat| assert!(chat.dm_pick.is_none(), "Cancel closes it"));
}

#[test]
fn choosing_someone_opens_their_room() {
    let (mut cx, view) = opened();
    cx.simulate_click("chat-sidebar-new-dm");
    cx.simulate_click("chat-dm-pick-10");
    cx.run_until_parked();
    view.read(|chat| {
        assert!(chat.dm_pick.is_none(), "the picker closed");
        assert_eq!(chat.room.as_ref().unwrap().id, dm_id(10));
        let post = crate::composer::Target::Post {
            channel: dm_id(10),
            thread: None,
        };
        assert!(matches!(
            chat.dm_to_open(&post),
            Some(Op::CreateDmChannel {
                counterpart: 10,
                ..
            })
        ));
    });
    assert!(cx.find("chat-dm-pick-card").is_none());
    assert!(cx.has_text("Message ada"), "{:?}", cx.texts());
    assert!(cx.find("chat-sidebar-dm-10").is_none(), "no message yet");
    cx.simulate_click("chat-sidebar-new-dm");
    cx.simulate_input("chat-dm-pick-search", "review");
    cx.simulate_submit("chat-dm-pick-search");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, dm_id(8)));
}

// ---------- on a phone ----------

// Boots on a phone with `rooms` and seats a reader: the rooms page.
fn hand(rooms: Vec<ChannelInfo>, newest: Vec<(u64, u64)>) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    answer(&cx, rooms, newest);
    let view = cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    seat(&mut cx);
    (cx, view)
}

fn held() -> Vec<ChannelInfo> {
    vec![channel("general", "General", 3), dm(8, 1, 0)]
}

// The direct rooms' rows the rooms page draws, top to bottom.
fn drawn_dms(cx: &TestAppContext) -> Vec<String> {
    cx.node("chat-rooms")
        .children()
        .iter()
        .filter_map(|node| node.key())
        .filter(|key| key.starts_with("chat-rooms-dm-"))
        .map(str::to_owned)
        .collect()
}

fn rows(peers: [u64; 3]) -> [String; 3] {
    peers.map(|peer| format!("chat-rooms-dm-{peer}"))
}

#[test]
fn a_phone_lists_the_direct_rooms_the_reader_holds_and_no_people() {
    let (cx, _) = hand(held(), Vec::new());
    assert_eq!(drawn_dms(&cx), ["chat-rooms-dm-8"]);
    assert!(cx.find("chat-rooms-dm-8-agent").is_some(), "wears Agent");
    assert!(!cx.has_text("People"), "{:?}", cx.texts());
    assert!(cx.find("chat-rooms-people-head").is_none());
    assert!(cx.find("chat-rooms-person-10").is_none(), "ada has no row");
}

#[test]
fn a_phone_lists_direct_messages_newest_first_and_a_new_message_moves_its_room_up() {
    let (mut cx, view) = hand(three_dms(), vec![(8, 300), (11, 900)]);
    assert_eq!(drawn_dms(&cx), rows([11, 10, 8]));
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let read = asked_heads(&cx);
    let mut rooms = three_dms();
    rooms[1].head_seq = 3;
    answer(&cx, rooms, vec![(8, 1_000), (11, 900)]);
    changes.send(block(6, posted(&dm_id(8))));
    cx.run_until_parked();
    assert_eq!(asked_heads(&cx), read + 1, "the agent's room alone");
    assert_eq!(drawn_dms(&cx), rows([8, 11, 10]));
    assert_eq!(
        listed(&view),
        ["general".to_owned(), dm_id(8), dm_id(11), dm_id(10)]
    );
}

#[test]
fn a_phone_offers_new_message_with_no_direct_room_under_it() {
    let (cx, view) = hand(vec![channel("general", "General", 3)], Vec::new());
    assert!(cx.find("chat-rooms-dms-head").is_some());
    assert!(cx.find("chat-rooms-new-dm").is_some());
    assert!(drawn_dms(&cx).is_empty());
    assert_eq!(listed(&view), ["general"]);
}

#[test]
fn a_phones_new_message_page_lists_people_and_agents_but_no_program_account_or_the_reader() {
    let (mut cx, view) = hand(held(), Vec::new());
    cx.simulate_click("chat-rooms-new-dm");
    assert!(cx.find("chat-dm-pick-page").is_some(), "a page of its own");
    assert!(cx.find("chat-dm-pick-card").is_none(), "not the dialog");
    assert_eq!(
        cx.focused().and_then(|node| node.key()),
        Some("chat-dm-pick-search"),
        "the search takes the keys"
    );
    assert!(cx.find("chat-dm-pick-8").is_some(), "the agent");
    assert!(cx.find("chat-dm-pick-8-agent").is_some(), "wears Agent");
    assert!(cx.find("chat-dm-pick-10").is_some(), "ada");
    assert!(cx.find("chat-dm-pick-9").is_none(), "forge's own account");
    assert!(cx.find("chat-dm-pick-7").is_none(), "the reader");
    search(&mut cx, &view, "AD");
    assert!(cx.find("chat-dm-pick-10").is_some(), "by name");
    assert!(cx.find("chat-dm-pick-8").is_none());
    search(&mut cx, &view, "forge");
    assert!(cx.find("chat-dm-pick-none").is_some(), "nobody matches");
}

#[test]
fn a_phones_pick_opens_the_room_in_the_pages_place_and_back_returns_to_the_rooms() {
    let (mut cx, view) = hand(held(), Vec::new());
    cx.simulate_click("chat-rooms-new-dm");
    cx.simulate_back();
    assert!(cx.find("chat-rooms-page").is_some(), "Back leaves the page");
    view.read(|chat| assert!(chat.dm_pick.is_none()));
    cx.simulate_click("chat-rooms-new-dm");
    cx.simulate_click("chat-dm-pick-10");
    cx.run_until_parked();
    assert!(cx.find("chat-room-page").is_some());
    view.read(|chat| {
        assert!(chat.dm_pick.is_none(), "the picker closed");
        assert_eq!(chat.room.as_ref().unwrap().id, dm_id(10));
        let post = crate::composer::Target::Post {
            channel: dm_id(10),
            thread: None,
        };
        assert!(matches!(
            chat.dm_to_open(&post),
            Some(Op::CreateDmChannel {
                counterpart: 10,
                ..
            })
        ));
    });
    cx.simulate_back();
    assert!(cx.find("chat-rooms-page").is_some(), "Back returns");
    assert!(cx.find("chat-rooms-dm-10").is_none(), "no message yet");
    cx.simulate_click("chat-rooms-new-dm");
    cx.simulate_input("chat-dm-pick-search", "review");
    cx.simulate_submit("chat-dm-pick-search");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, dm_id(8)));
}
