//! Each person's picture is their face wherever chat draws one, read once
//! per blob from identity's cards: a message's author, a direct room's
//! person, and on the phone the reader's own face on the rooms page.
use super::*;
use view::methods::BlobGet;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR\0\0\0\x60\0\0\0\x60";

fn blob(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

fn card(number: u64) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: format!("account {number}"),
            avatar: Some(abi::BlobId::Sha256([number as u8; 32])),
            bio: None,
            updated_at: 0,
        },
        control: identity::Control::User { keys: Vec::new() },
    }
}

fn pictured(touch: bool) -> TestAppContext {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host().handle::<Ask<Identity>>(|query| match query {
        identity::Query::List { .. } => Ok(identity::Reply::Accounts(identity::PageResponse {
            height: 1,
            items: vec![card(7), card(8)],
            next: None,
        })),
        identity::Query::Narrowed { .. } => Ok(none_narrowed()),
        query => panic!("unexpected identity query: {query:?}"),
    });
    cx.host().handle::<BlobGet>(|_| Ok(Some(PNG.to_vec())));
    cx.open::<Chat>();
    if touch {
        cx.simulate_touch(true);
        cx.simulate_resize(390., 844.);
    }
    cx.run_until_parked();
    seat(&mut cx);
    cx
}

fn holds_picture(node: &wire::Node) -> bool {
    matches!(node, wire::Node::Image { .. }) || node.children().iter().any(holds_picture)
}

fn pictured_at(cx: &TestAppContext, key: &str) -> bool {
    cx.find(key).is_some_and(holds_picture)
}

fn read_once(cx: &TestAppContext) {
    let mut reads = cx.host().requests::<BlobGet>();
    reads.sort();
    assert_eq!(reads, [blob(7), blob(8)], "each picture read once");
}

#[test]
fn a_persons_picture_is_their_face_in_the_room_and_the_sidebar() {
    let mut cx = pictured(false);
    assert!(
        pictured_at(&cx, "chat-sidebar-dm-8-avatar"),
        "the dm's person"
    );
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "chat-message-m1-avatar"), "eddy's message");
    assert!(
        pictured_at(&cx, "chat-message-m2-avatar"),
        "reviewer's message"
    );
    cx.simulate_click("chat-sidebar-dm-8");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "chat-room-avatar"), "over the direct room");
    read_once(&cx);
}

#[test]
fn an_agents_picture_draws_with_no_frame() {
    let mut cx = pictured(false);
    let framed = |cx: &TestAppContext, key: &str| cx.style(key).border_widths.left.is_some();
    assert!(!framed(&cx, "chat-sidebar-dm-8-avatar"), "the dm's agent");
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(
        pictured_at(&cx, "chat-message-m2-avatar"),
        "reviewer's picture"
    );
    assert!(!framed(&cx, "chat-message-m2-avatar"), "reviewer's message");
}

#[test]
fn the_cards_picture_opens_whole_over_the_room() {
    let mut cx = pictured(false);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_click("chat-message-m2-avatar");
    cx.run_until_parked();
    assert!(cx.find("chat-card").is_some(), "reviewer's card");
    cx.simulate_click("chat-card-picture");
    cx.run_until_parked();
    assert!(cx.find("chat-card").is_none(), "the card gives way");
    assert!(
        pictured_at(&cx, "chat-picture/image"),
        "the picture, whole: {:?}",
        cx.texts()
    );
    assert!(cx.find("chat-picture/save").is_none(), "no file to save");
    assert!(cx.find("chat-picture/open").is_none(), "no file to open");
}

#[test]
fn on_a_phone_the_rooms_page_leads_with_the_readers_picture() {
    let mut cx = pictured(true);
    assert!(
        pictured_at(&cx, "chat-rooms-page-account"),
        "{:?}",
        cx.texts()
    );
    assert!(pictured_at(&cx, "chat-rooms-dm-8"), "the dm's person");
    cx.simulate_click("chat-rooms-dm-8");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "chat-room-avatar"), "over the direct room");
    read_once(&cx);
}
