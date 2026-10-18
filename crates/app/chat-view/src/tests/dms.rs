//! Direct messages on the desktop: the rooms the reader holds, newest
//! message first, and New message, the picker of whom to write to.
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
    cx.host().handle::<Ask<::chat::Chat>>(move |query| {
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
    });
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
    view.read(|chat| chat.sidebar_rooms().ids())
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
