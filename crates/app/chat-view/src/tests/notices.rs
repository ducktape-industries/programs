//! The pane's side of notices: it posts none and counts no badge (chat's
//! notifier does), takes a read room's notices down, names the room on
//! screen to the host, and keeps the read cursors between runs.
use super::*;

/// A direct message landing in a room the reader is not in is no notice
/// and no badge from the pane, however many panes are open: that is the
/// notifier's, which the host runs once. Opening the room reads the host's
/// rows under the tag the notifier's notices carry, and names the room as
/// the one on screen.
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

/// A relaunch starts with no cursors in memory: the ones kept on the device
/// come back, so rooms still unread are dotted, and reading a room keeps its
/// new cursor, which the notifier counts from.
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
            query => panic!("unexpected chat query: {query:?}"),
        })
    }));
    cx.open::<Chat>();
    seat(&mut cx);
    assert!(cx.find("chat-sidebar-channel-general-unread").is_some());
    assert!(cx.find("chat-sidebar-dm-8-unread").is_some());

    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let (key, kept) = cx
        .host()
        .requests::<StoreSet>()
        .pop()
        .expect("the read is kept");
    assert_eq!(key, "reads/0102");
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

/// The room's details offer its notices as one radio group, read as they
/// open: the reader's choice checked, the arrows moving among them, and
/// each press, Enter or Space one op that makes it the room's.
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

    // an arrow moves among them and writes nothing; Enter chooses
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

// the details offer each account the reader manages a group of its own in a room, checked as chat
// holds its choice, beside the reader's own; a press writes the choice for it, and a direct room
// offers none
#[test]
fn the_details_choose_a_managed_agents_notices() {
    use view::methods::Submit;
    let (mut cx, view) = opened();
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Notices { account: 8, .. } => {
                Reply::Notices(page(vec![("general".into(), chat::Notices::Off)]))
            }
            query => chat_reply(query),
        })
    }));
    cx.simulate_click("chat-room-details");
    cx.run_until_parked();
    let group = cx.interactivity("chat-details-notices-8");
    assert_eq!(group.role, Some(view::Role::RadioGroup));
    assert_eq!(group.aria.label.as_deref(), Some("What reviewer hears"));
    let checked = |cx: &TestAppContext, id: &str| cx.interactivity(id).aria.toggled;
    let on = Some(view::Toggled::True);
    assert_eq!(checked(&cx, "chat-details-notices-8-off"), on);
    assert_eq!(
        checked(&cx, "chat-details-notices-default"),
        on,
        "the reader's own"
    );

    cx.simulate_click("chat-details-notices-8-every");
    cx.simulate_click("chat-details-notices-8-mentions");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<::chat::Chat>>();
    assert_eq!(
        sent[sent.len() - 2..],
        [
            chat::Op::ListenFor {
                account: 8,
                channel_id: "general".into(),
                listening: true,
            },
            chat::Op::SetNoticesFor {
                account: 8,
                channel_id: "general".into(),
                notices: Some(chat::Notices::Mentions),
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
    assert!(cx.find("chat-details-notices-8").is_none(), "a direct room");
}
