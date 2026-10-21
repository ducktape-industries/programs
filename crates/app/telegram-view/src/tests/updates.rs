use super::{
    chats::{message, peer},
    history::opened,
    server::{self, Boxed},
};
use crate::{Telegram, updates::Position};
use grammers_tl_types::{self as tl, Identifiable, Serializable};

fn state(pts: i32, seq: i32) -> tl::enums::updates::State {
    tl::enums::updates::State::State(tl::types::updates::State {
        pts,
        qts: 0,
        date: server::UNIX as i32,
        seq,
        unread_count: 0,
    })
}

fn update(id: i32, pts: i32, text: &str) -> Vec<u8> {
    tl::types::UpdateShort {
        update: tl::enums::Update::NewMessage(tl::types::UpdateNewMessage {
            message: message(id, peer(7), peer(8), server::UNIX as i32, text),
            pts,
            pts_count: 1,
        }),
        date: server::UNIX as i32,
    }
    .boxed()
}

#[test]
fn a_live_message_updates_the_open_chat_and_its_preview_once() {
    let (mut cx, mut server) = opened();
    deliver(&mut server, &mut cx, update(11, 11, "Arrived live"));
    assert!(cx.has_text("Arrived live"), "{:?}", cx.texts());
    deliver(&mut server, &mut cx, update(11, 11, "Arrived live"));
    assert_eq!(
        cx.texts()
            .iter()
            .filter(|text| text.as_str() == "Arrived live")
            .count(),
        1
    );
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 11);
    cx.simulate_click("back");
    assert!(cx.has_text("Arrived live"));
}

#[test]
fn a_pts_gap_fetches_slices_and_replays_messages_received_during_recovery() {
    let (mut cx, mut server) = opened();
    deliver(&mut server, &mut cx, update(13, 13, "After gap"));
    let (id, request) = server.take::<tl::functions::updates::GetDifference>();
    assert_eq!(
        (request.pts, request.qts, request.date),
        (10, 0, server::UNIX as i32)
    );
    assert!(!cx.has_text("After gap"));
    deliver(&mut server, &mut cx, update(14, 14, "During recovery"));
    server.reply(
        &mut cx,
        id,
        tl::types::updates::DifferenceSlice {
            new_messages: vec![message(
                11,
                peer(7),
                peer(8),
                server::UNIX as i32,
                "First missed",
            )],
            new_encrypted_messages: vec![],
            other_updates: vec![],
            chats: vec![],
            users: vec![],
            intermediate_state: state(11, 1),
        }
        .boxed(),
    );
    let (id, request) = server.take::<tl::functions::updates::GetDifference>();
    assert_eq!(request.pts, 11);
    server.reply(
        &mut cx,
        id,
        tl::types::updates::Difference {
            new_messages: vec![message(
                12,
                peer(7),
                peer(8),
                server::UNIX as i32,
                "Second missed",
            )],
            new_encrypted_messages: vec![],
            other_updates: vec![],
            chats: vec![],
            users: vec![],
            state: state(12, 1),
        }
        .boxed(),
    );
    for text in [
        "First missed",
        "Second missed",
        "After gap",
        "During recovery",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 14);
}

#[test]
fn a_restart_recovers_from_the_stored_update_checkpoint() {
    let (mut cx, mut server) = opened();
    deliver(&mut server, &mut cx, update(11, 11, "Before restart"));
    let snapshot = cx.snapshot().unwrap();
    cx.restore::<Telegram>(&snapshot).unwrap();
    server::seat(&mut cx, "a key");
    server.opened(&mut cx);
    let (id, request) = server.take::<tl::functions::updates::GetDifference>();
    assert_eq!(
        (request.pts, request.qts, request.date),
        (11, 0, server::UNIX as i32)
    );
    server.reply(
        &mut cx,
        id,
        tl::types::updates::Difference {
            new_messages: vec![message(
                12,
                peer(7),
                peer(8),
                server::UNIX as i32,
                "While disconnected",
            )],
            new_encrypted_messages: vec![],
            other_updates: vec![],
            chats: vec![],
            users: vec![],
            state: state(12, 1),
        }
        .boxed(),
    );
    assert!(cx.has_text("While disconnected"));
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 12);
}

#[test]
fn a_seq_gap_and_updates_too_long_request_difference_without_repeating_the_marker() {
    let (mut cx, mut server) = opened();
    deliver(
        &mut server,
        &mut cx,
        tl::types::Updates {
            updates: vec![],
            users: vec![],
            chats: vec![],
            date: server::UNIX as i32,
            seq: 3,
        }
        .boxed(),
    );
    let (id, _) = server.take::<tl::functions::updates::GetDifference>();
    server.reply(
        &mut cx,
        id,
        tl::types::updates::DifferenceEmpty {
            date: server::UNIX as i32,
            seq: 3,
        }
        .boxed(),
    );
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().seq, 3);
    deliver(&mut server, &mut cx, tl::enums::Updates::TooLong.to_bytes());
    let (id, _) = server.take::<tl::functions::updates::GetDifference>();
    server.reply(
        &mut cx,
        id,
        tl::types::updates::DifferenceEmpty {
            date: server::UNIX as i32,
            seq: 3,
        }
        .boxed(),
    );
    assert!(!server.calls.iter().any(|call| {
        call.body
            .starts_with(&tl::functions::updates::GetDifference::CONSTRUCTOR_ID.to_le_bytes())
    }));
}

#[test]
fn a_channel_gap_fetches_its_own_difference_and_advances_its_own_pts() {
    let (mut cx, mut server) = opened();
    cx.simulate_click("back");
    cx.simulate_click("refresh");
    server.pump(&mut cx);
    let (id, _) = server.take::<tl::functions::updates::GetDifference>();
    server.reply(
        &mut cx,
        id,
        tl::types::updates::DifferenceEmpty {
            date: server::UNIX as i32,
            seq: 1,
        }
        .boxed(),
    );
    let (id, _) = server.take::<tl::functions::messages::GetDialogs>();
    let channel = tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 99 });
    let mut dialog = super::chats::dialog(channel.clone(), 5);
    if let tl::enums::Dialog::Dialog(dialog) = &mut dialog {
        dialog.pts = Some(20);
    }
    server.reply(
        &mut cx,
        id,
        tl::types::messages::Dialogs {
            dialogs: vec![dialog],
            messages: vec![message(
                5,
                channel.clone(),
                peer(8),
                server::UNIX as i32,
                "Channel before",
            )],
            chats: vec![tl::enums::Chat::ChannelForbidden(
                tl::types::ChannelForbidden {
                    broadcast: true,
                    megagroup: false,
                    monoforum: false,
                    id: 99,
                    access_hash: 999,
                    title: "News".into(),
                    until_date: None,
                },
            )],
            users: vec![],
        }
        .boxed(),
    );
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdateShort {
            date: server::UNIX as i32,
            update: tl::enums::Update::NewChannelMessage(tl::types::UpdateNewChannelMessage {
                message: message(
                    8,
                    channel.clone(),
                    peer(8),
                    server::UNIX as i32,
                    "Channel live",
                ),
                pts: 23,
                pts_count: 1,
            }),
        }
        .boxed(),
    );
    let (id, request) = server.take::<tl::functions::updates::GetChannelDifference>();
    assert_eq!(request.pts, 20);
    assert_eq!(
        request.channel,
        tl::enums::InputChannel::Channel(tl::types::InputChannel {
            channel_id: 99,
            access_hash: 999
        })
    );
    server.reply(
        &mut cx,
        id,
        tl::types::updates::ChannelDifference {
            r#final: true,
            pts: 22,
            timeout: None,
            new_messages: vec![message(
                7,
                channel,
                peer(8),
                server::UNIX as i32,
                "Channel missed",
            )],
            other_updates: vec![],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text("Channel live"));
    let position = cx.host().kept::<Position>("updates").unwrap();
    assert_eq!(position.pts, 10);
    assert_eq!(position.channels[&99], 23);
}

#[test]
fn edits_and_deletions_update_the_open_history() {
    let (mut cx, mut server) = opened();
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdateShort {
            update: tl::enums::Update::EditMessage(tl::types::UpdateEditMessage {
                message: message(10, peer(7), peer(8), server::UNIX as i32, "Edited text"),
                pts: 11,
                pts_count: 1,
            }),
            date: server::UNIX as i32,
        }
        .boxed(),
    );
    assert!(cx.has_text("Edited text"));
    assert!(!cx.has_text("Hello from Bob"));
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdateShort {
            update: tl::enums::Update::DeleteMessages(tl::types::UpdateDeleteMessages {
                messages: vec![10],
                pts: 12,
                pts_count: 1,
            }),
            date: server::UNIX as i32,
        }
        .boxed(),
    );
    assert!(!cx.has_text("Edited text"));
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 12);
}

fn deliver(
    server: &mut super::server::Server,
    cx: &mut view::testing::TestAppContext,
    body: Vec<u8>,
) {
    server.send(cx, body);
    server.pump(cx);
}

#[test]
fn overlapping_pts_and_seq_are_ignored_by_the_three_way_rule() {
    let (mut cx, mut server) = opened();
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdateShort {
            date: server::UNIX as i32,
            update: tl::enums::Update::DeleteMessages(tl::types::UpdateDeleteMessages {
                messages: vec![10],
                pts: 11,
                pts_count: 2,
            }),
        }
        .boxed(),
    );
    assert!(cx.has_text("Hello from Bob"));
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 10);
    assert!(!server.calls.iter().any(|call| {
        call.body
            .starts_with(&tl::functions::updates::GetDifference::CONSTRUCTOR_ID.to_le_bytes())
    }));
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdatesCombined {
            updates: vec![],
            users: vec![],
            chats: vec![],
            date: server::UNIX as i32,
            seq_start: 1,
            seq: 3,
        }
        .boxed(),
    );
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().seq, 1);
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdatesCombined {
            updates: vec![],
            users: vec![],
            chats: vec![],
            date: server::UNIX as i32,
            seq_start: 2,
            seq: 3,
        }
        .boxed(),
    );
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().seq, 3);
}

#[test]
fn an_untracked_channel_starts_at_the_updates_previous_pts() {
    let (mut cx, mut server) = opened();
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdateShort {
            date: server::UNIX as i32,
            update: tl::enums::Update::NewChannelMessage(tl::types::UpdateNewChannelMessage {
                message: message(
                    100,
                    tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 99 }),
                    peer(8),
                    server::UNIX as i32,
                    "First channel update",
                ),
                pts: 300,
                pts_count: 2,
            }),
        }
        .boxed(),
    );
    assert_eq!(
        cx.host().kept::<Position>("updates").unwrap().channels[&99],
        300
    );
    assert!(!server.calls.iter().any(|call| {
        call.body.starts_with(
            &tl::functions::updates::GetChannelDifference::CONSTRUCTOR_ID.to_le_bytes(),
        )
    }));
}

#[test]
fn difference_too_long_discards_old_pages_and_establishes_a_new_checkpoint() {
    let (mut cx, mut server) = opened();
    deliver(&mut server, &mut cx, tl::enums::Updates::TooLong.to_bytes());
    let (id, _) = server.take::<tl::functions::updates::GetDifference>();
    server.reply(
        &mut cx,
        id,
        tl::types::updates::DifferenceTooLong { pts: 100 }.boxed(),
    );
    assert!(!cx.has_text("Hello from Bob"));
    let (id, _) = server.take::<tl::functions::updates::GetState>();
    let tl::enums::updates::State::State(state) = state(100, 10);
    server.reply(&mut cx, id, state.boxed());
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().pts, 100);
    let (_, _) = server.take::<tl::functions::messages::GetHistory>();
}

#[test]
fn channel_difference_too_long_replaces_only_that_channels_history() {
    use crate::chats::Peer;
    let mut cx = super::server::Server::context();
    let mut server = super::server::Server::default();
    let entity = server.open(&mut cx);
    super::login::sign_in(&mut cx, &mut server);
    cx.update(&entity, |view, _, cx| {
        view.cache_peers(
            vec![],
            vec![tl::enums::Chat::ChannelForbidden(
                tl::types::ChannelForbidden {
                    broadcast: true,
                    megagroup: false,
                    monoforum: false,
                    id: 99,
                    access_hash: 999,
                    title: "News".into(),
                    until_date: None,
                },
            )],
            cx,
        );
        view.channel_position(99, 20, cx);
        view.open_chat(Peer::Channel(99), cx);
    });
    server.pump(&mut cx);
    let channel = tl::enums::Peer::Channel(tl::types::PeerChannel { channel_id: 99 });
    let (id, _) = server.take::<tl::functions::messages::GetHistory>();
    server.reply(
        &mut cx,
        id,
        tl::types::messages::Messages {
            messages: vec![message(
                20,
                channel.clone(),
                peer(8),
                server::UNIX as i32,
                "Old channel page",
            )],
            topics: vec![],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(cx.has_text("Old channel page"));
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdateShort {
            date: server::UNIX as i32,
            update: tl::enums::Update::ChannelTooLong(tl::types::UpdateChannelTooLong {
                channel_id: 99,
                pts: Some(40),
            }),
        }
        .boxed(),
    );
    let (id, _) = server.take::<tl::functions::updates::GetChannelDifference>();
    let mut dialog = super::chats::dialog(channel.clone(), 40);
    if let tl::enums::Dialog::Dialog(dialog) = &mut dialog {
        dialog.pts = Some(40);
    }
    server.reply(
        &mut cx,
        id,
        tl::types::updates::ChannelDifferenceTooLong {
            r#final: true,
            timeout: None,
            dialog,
            messages: vec![message(
                40,
                channel,
                peer(8),
                server::UNIX as i32,
                "Current channel page",
            )],
            chats: vec![],
            users: vec![],
        }
        .boxed(),
    );
    assert!(!cx.has_text("Old channel page"));
    assert!(cx.has_text("Current channel page"));
    let position = cx.host().kept::<Position>("updates").unwrap();
    assert_eq!(position.channels[&99], 40);
    assert_eq!(position.pts, 10);
}

#[test]
fn old_seq_markers_are_ignored_and_pts_progress_independently() {
    let (mut cx, mut server) = opened();
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdatesCombined {
            updates: vec![tl::enums::Update::PtsChanged],
            users: vec![],
            chats: vec![],
            date: server::UNIX as i32,
            seq_start: 1,
            seq: 3,
        }
        .boxed(),
    );
    assert_eq!(cx.host().kept::<Position>("updates").unwrap().seq, 1);
    assert!(!server.calls.iter().any(|call| {
        call.body
            .starts_with(&tl::functions::updates::GetDifference::CONSTRUCTOR_ID.to_le_bytes())
    }));
    deliver(
        &mut server,
        &mut cx,
        tl::types::UpdatesCombined {
            updates: vec![tl::enums::Update::NewMessage(tl::types::UpdateNewMessage {
                message: message(
                    11,
                    peer(8),
                    peer(8),
                    server::UNIX as i32,
                    "Before seq recovery",
                ),
                pts: 11,
                pts_count: 1,
            })],
            users: vec![],
            chats: vec![],
            date: server::UNIX as i32,
            seq_start: 3,
            seq: 3,
        }
        .boxed(),
    );
    assert!(cx.has_text("Before seq recovery"));
    let (_, request) = server.take::<tl::functions::updates::GetDifference>();
    assert_eq!(request.pts, 11);
}
