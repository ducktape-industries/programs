//! Categories on the desktop: a channel's `category` meta files it under a section of its
//! own in the sidebar, which the arrows walk, and its details set and clear it.
use super::room::claims;
use super::*;

fn filed(id: &str, category: &str) -> ChannelInfo {
    let mut info = channel(id, id, 0);
    info.meta.insert("category".into(), category.into());
    info
}

fn rooms() -> Vec<ChannelInfo> {
    vec![
        filed("j-020", "Jobs"),
        channel("general", "General", 3),
        filed("ops", "Teams"),
        filed("j-012", "jobs"),
        filed("blank", "   "),
        filed("design", "Design"),
        channel("dm-7-8", "dm", 1),
    ]
}

fn general_in(category: &str) -> Vec<ChannelInfo> {
    let mut rooms = rooms();
    rooms[1].meta.insert("category".into(), category.into());
    rooms
}

fn answer(cx: &TestAppContext, rooms: Vec<ChannelInfo>) {
    cx.host().handle::<Ask<::chat::Chat>>(move |query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(rooms.clone())),
            Query::Channel { channel_id } => Reply::Channel(
                rooms
                    .iter()
                    .find(|room| room.channel.id == channel_id)
                    .cloned(),
            ),
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

fn order(view: &Entity<Chat>) -> String {
    view.read(|chat| chat.sidebar_rooms().ids().join(" "))
}

fn last_submitted(cx: &TestAppContext) -> Option<Op> {
    cx.host().requests::<Submit<::chat::Chat>>().pop()
}

fn submits(cx: &TestAppContext) -> usize {
    cx.host().requests::<Submit<::chat::Chat>>().len()
}

#[test]
fn categories_are_sections_in_name_order_between_channels_and_direct_messages() {
    let (cx, view) = desk(rooms());
    assert_eq!(order(&view), "general blank design j-020 j-012 ops dm-7-8");
    for header in ["Design", "Jobs", "jobs", "Teams"] {
        let id = format!("chat-sidebar-category-{header}");
        assert!(cx.find(&id).is_some(), "{id}");
    }
    let texts = cx.texts();
    let at = |text: &str| {
        texts
            .iter()
            .position(|drawn| drawn == text)
            .unwrap_or_else(|| panic!("{text} is drawn: {texts:?}"))
    };
    let headers = [
        "Channels",
        "Design",
        "Jobs",
        "jobs",
        "Teams",
        "Direct messages",
    ];
    let drawn = headers.map(at);
    assert!(drawn.is_sorted(), "{headers:?} at {drawn:?}");
}

#[test]
fn the_arrows_walk_channels_then_each_category_then_direct_messages() {
    let (mut cx, _) = desk(rooms());
    cx.simulate_focus("chat-sidebar-rooms-list");
    assert!(claims(&cx, "chat-sidebar-channel-general"));
    for row in [
        "chat-sidebar-channel-blank",
        "chat-sidebar-channel-design",
        "chat-sidebar-channel-j-020",
        "chat-sidebar-channel-j-012",
        "chat-sidebar-channel-ops",
        "chat-sidebar-dm-8",
    ] {
        cx.simulate_key_down("chat-sidebar-rooms-list", "down");
        assert!(claims(&cx, row), "{row}");
    }
}

#[test]
fn the_phone_lists_the_channels_as_chat_does() {
    let (_, view) = desk(rooms());
    view.read(|chat| {
        let list = chat.room_list();
        assert!(list.sections.is_empty());
        let ids: Vec<&str> = list
            .channels
            .iter()
            .map(|info| info.channel.id.as_str())
            .collect();
        assert_eq!(ids.join(" "), "j-020 general ops j-012 blank design");
    });
}

#[test]
fn a_block_that_writes_a_channels_meta_reads_the_list_again() {
    let (mut cx, view) = desk(rooms());
    answer(&cx, general_in("Teams"));
    let meta = chat::tables::CHANNEL_META.key(&"general".to_owned());
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(5, vec![meta]));
    cx.run_until_parked();
    assert_eq!(order(&view), "blank design j-020 j-012 general ops dm-7-8");
}

#[test]
fn details_set_a_category_and_clear_it_and_the_channel_moves_with_it() {
    let (mut cx, view) = desk(rooms());
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_click("chat-room-details");
    assert!(
        cx.find("chat-details-clear-category").is_none(),
        "general has none to clear"
    );
    let before = submits(&cx);
    cx.simulate_click("chat-details-set-category");
    cx.run_until_parked();
    assert_eq!(submits(&cx), before, "a blank category sends nothing");

    answer(&cx, general_in("Jobs"));
    cx.simulate_input("chat-details-category", "  Jobs ");
    cx.simulate_click("chat-details-set-category");
    cx.run_until_parked();
    assert!(matches!(
        last_submitted(&cx),
        Some(Op::SetChannelMeta { channel_id, key, value })
            if channel_id == "general" && key == "category" && value == "Jobs"
    ));
    assert_eq!(order(&view), "blank design j-020 general j-012 ops dm-7-8");
    assert!(cx.find("chat-details-clear-category").is_some());
    let before = submits(&cx);
    cx.simulate_click("chat-details-set-category");
    cx.run_until_parked();
    assert_eq!(submits(&cx), before, "the category it has sends nothing");

    answer(&cx, rooms());
    cx.simulate_click("chat-details-clear-category");
    cx.run_until_parked();
    assert!(matches!(
        last_submitted(&cx),
        Some(Op::RemoveChannelMeta { channel_id, key })
            if channel_id == "general" && key == "category"
    ));
    assert!(order(&view).starts_with("general "), "back under Channels");
    assert!(cx.find("chat-details-clear-category").is_none());
    let Some(wire::Node::Field { value, .. }) = cx.find("chat-details-category") else {
        panic!("the category is a field");
    };
    assert!(value.is_empty(), "the field is emptied: {value:?}");
}

#[test]
fn a_channel_created_with_a_category_carries_it_and_one_without_is_a_plain_create() {
    let (mut cx, _) = desk(rooms());
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_click("chat-sidebar-new-channel");
    cx.simulate_input("chat-create-name", "j-021");
    cx.simulate_input("chat-create-category", " Jobs ");
    cx.simulate_submit("chat-create-category");
    cx.run_until_parked();
    assert!(matches!(
        last_submitted(&cx),
        Some(Op::CreateChannelWithMeta { channel_id, name, meta, .. })
            if channel_id == "channel-1"
                && name == "j-021"
                && meta.get("category").map(String::as_str) == Some("Jobs")
                && meta.len() == 1
    ));
    cx.simulate_click("chat-sidebar-new-channel");
    cx.simulate_input("chat-create-name", "random");
    cx.simulate_submit("chat-create-name");
    cx.run_until_parked();
    assert!(matches!(
        last_submitted(&cx),
        Some(Op::CreateChannel { name, .. }) if name == "random"
    ));
}
