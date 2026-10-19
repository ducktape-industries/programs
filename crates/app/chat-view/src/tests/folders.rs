//! Channel folders: a `/` in a channel's name files it under a folder in both trees' room lists, a
//! folder opens and closes and stays so on the device, and a closed one counts its unread rooms.
use super::room::claims;
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use view::methods::{self, StoreGet, StoreSet};

fn rooms() -> Vec<ChannelInfo> {
    vec![
        channel("general", "general", 3),
        channel("j-097", "jobs/j-097 · channel folders", 4),
        channel("eng", "engineering", 2),
        channel("random", "random", 1),
        channel("j-098", "jobs / j-098 · board", 5),
        channel("ci", "engineering/ci", 6),
        channel("deep", "a/b/c", 1),
        channel("dm-7-8", "dm", 1),
    ]
}

// The view over `rooms`, eddy's cursors `kept` on the device (j-097 unread
// at 0 of 4) and the folders `closed`.
fn desk(closed: &[&str]) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let reads = BTreeMap::from([("j-097".to_owned(), 0u64), ("ci".to_owned(), 6)]);
    let closed: BTreeSet<String> = closed.iter().map(|path| path.to_string()).collect();
    cx.host().handle::<StoreGet>(move |key| {
        Ok(match key.as_str() {
            "reads/0102" => Some(methods::encode(&reads)),
            "folders" => Some(methods::encode(&closed)),
            _ => None,
        })
    });
    cx.host().handle::<Ask<::chat::Chat>>(rooms_alone(|query| {
        Ok(match query {
            Query::Channels { .. } => Reply::Channels(page(rooms())),
            query => chat_reply(query),
        })
    }));
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.run_until_parked();
    (cx, view)
}

// The folders last kept on the device.
fn kept(cx: &TestAppContext) -> Option<BTreeSet<String>> {
    let sets = cx.host().requests::<StoreSet>();
    let (_, value) = sets.into_iter().rfind(|(key, _)| key == "folders")?;
    Some(methods::decode(&value.expect("a set, not a removal")).unwrap())
}

fn open_room(view: &Entity<Chat>) -> Option<String> {
    view.read(|chat| chat.room.as_ref().map(|room| room.id.clone()))
}

#[test]
fn a_slash_files_a_channel_under_its_folder_by_its_last_part() {
    let (mut cx, _) = desk(&[]);
    for shown in [
        "general",
        "jobs",
        "j-097 · channel folders",
        "j-098 · board",
        "c",
    ] {
        assert!(cx.has_text(shown), "{shown}: {:?}", cx.texts());
    }
    assert!(!cx.has_text("jobs/j-097 · channel folders"));
    cx.simulate_focus("chat-sidebar-rooms-list");
    assert!(claims(&cx, "chat-sidebar-channel-general"));
    for row in [
        "chat-sidebar-folder-jobs",
        "chat-sidebar-channel-j-097",
        "chat-sidebar-channel-j-098",
        "chat-sidebar-folder-engineering",
        "chat-sidebar-channel-ci",
        "chat-sidebar-channel-random",
        "chat-sidebar-folder-a",
        "chat-sidebar-folder-a/b",
        "chat-sidebar-channel-deep",
        "chat-sidebar-dm-8",
    ] {
        cx.simulate_key_down("chat-sidebar-rooms-list", "down");
        assert!(claims(&cx, row), "{row}");
    }
    let folder = cx.interactivity("chat-sidebar-folder-jobs");
    assert_eq!(folder.role, Some(view::Role::ListBoxOption));
    assert_eq!(folder.aria.expanded, Some(true));
    assert_eq!(folder.aria.selected, Some(false));
}

#[test]
fn names_without_a_slash_keep_their_rows_and_order() {
    let (cx, view) = desk(&[]);
    assert!(cx.has_text("random"));
    assert!(cx.find("chat-sidebar-folder-general").is_none());
    view.read(|chat| {
        let list = chat.room_list();
        let lines = chat.lines(&list.channels);
        let spots: Vec<String> = lines
            .iter()
            .map(|line| match line.spot() {
                crate::room_list::Spot::Room(room) => room,
                crate::room_list::Spot::Folder { path, .. } => format!("{path}/"),
            })
            .collect();
        assert_eq!(
            spots.join(" "),
            "general jobs/ j-097 j-098 engineering/ ci random a/ a/b/ deep"
        );
    });
}

#[test]
fn a_closed_folder_hides_its_rooms_counts_the_unread_and_is_kept() {
    let (mut cx, _) = desk(&[]);
    assert!(cx.find("chat-sidebar-channel-j-097-unread").is_some());
    assert!(cx.find("chat-sidebar-folder-jobs-count").is_none(), "open");
    cx.simulate_click("chat-sidebar-folder-jobs");
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-j-097").is_none());
    assert!(cx.find("chat-sidebar-channel-j-098").is_none());
    assert!(cx.find("chat-sidebar-folder-jobs-count").is_some());
    let folder = cx.interactivity("chat-sidebar-folder-jobs");
    assert_eq!(folder.aria.expanded, Some(false));
    assert_eq!(folder.aria.description.as_deref(), Some("1 unread room"));
    assert_eq!(kept(&cx), Some(BTreeSet::from(["jobs".to_owned()])));

    cx.simulate_click("chat-sidebar-folder-engineering-fold");
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-ci").is_none());
    assert!(
        cx.find("chat-sidebar-folder-engineering-count").is_none(),
        "nothing unread inside: no count"
    );

    cx.simulate_click("chat-sidebar-folder-jobs");
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-j-097").is_some());
    assert!(cx.find("chat-sidebar-folder-jobs-count").is_none());
    assert_eq!(kept(&cx), Some(BTreeSet::from(["engineering".to_owned()])));
}

#[test]
fn folders_kept_closed_come_back_closed() {
    let (cx, _) = desk(&["jobs", "a/b"]);
    assert!(cx.find("chat-sidebar-channel-j-097").is_none());
    assert!(cx.find("chat-sidebar-folder-jobs-count").is_some());
    assert!(cx.find("chat-sidebar-folder-a/b").is_some());
    assert!(cx.find("chat-sidebar-channel-deep").is_none());
    assert!(cx.find("chat-sidebar-channel-ci").is_some());
}

#[test]
fn a_folder_named_as_a_channel_opens_it_and_its_chevron_folds() {
    let (mut cx, view) = desk(&[]);
    assert!(
        cx.find("chat-sidebar-channel-eng").is_none(),
        "the folder's own row"
    );
    cx.simulate_click("chat-sidebar-folder-engineering");
    cx.run_until_parked();
    assert_eq!(open_room(&view).as_deref(), Some("eng"));
    assert!(
        cx.find("chat-sidebar-channel-ci").is_some(),
        "a press opens, not folds"
    );
    let row = cx.interactivity("chat-sidebar-folder-engineering");
    assert_eq!(row.aria.selected, Some(true));
    let fold = cx.interactivity("chat-sidebar-folder-engineering-fold");
    assert_eq!(fold.role, Some(view::Role::Button));
    assert_eq!(fold.aria.expanded, Some(true));
    cx.simulate_click("chat-sidebar-folder-engineering-fold");
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-channel-ci").is_none());
    assert_eq!(open_room(&view).as_deref(), Some("eng"), "the room stays");
}

#[test]
fn the_arrows_open_and_close_a_folder_and_a_hidden_room_hands_it_its_row() {
    let (mut cx, view) = desk(&[]);
    cx.simulate_click("chat-sidebar-channel-j-098");
    cx.run_until_parked();
    cx.simulate_focus("chat-sidebar-rooms-list");
    cx.simulate_key_down("chat-sidebar-rooms-list", "up");
    cx.simulate_key_down("chat-sidebar-rooms-list", "up");
    assert!(claims(&cx, "chat-sidebar-folder-jobs"));
    cx.simulate_key_down("chat-sidebar-rooms-list", "left");
    assert!(cx.find("chat-sidebar-channel-j-097").is_none());
    assert!(claims(&cx, "chat-sidebar-folder-jobs"));
    cx.simulate_key_down("chat-sidebar-rooms-list", "right");
    assert!(cx.find("chat-sidebar-channel-j-097").is_some());
    cx.simulate_key_down("chat-sidebar-rooms-list", "enter");
    assert!(
        cx.find("chat-sidebar-channel-j-097").is_none(),
        "Enter folds"
    );
    assert_eq!(open_room(&view).as_deref(), Some("j-098"));

    cx.update(&view, |chat, _, cx| {
        chat.rooms_cursor = None;
        cx.notify();
    });
    cx.run_until_parked();
    assert!(
        claims(&cx, "chat-sidebar-folder-jobs"),
        "the open room, folded away, stands on its folder"
    );
}

#[test]
fn the_phone_files_channels_under_folders_too() {
    let (mut cx, view) = desk(&[]);
    cx.simulate_touch(true);
    assert!(cx.find("chat-rooms-folder-jobs").is_some());
    assert!(cx.find("chat-rooms-channel-j-097").is_some());
    assert!(cx.has_text("j-097 · channel folders"));
    cx.simulate_click("chat-rooms-folder-jobs");
    cx.run_until_parked();
    assert!(cx.find("chat-rooms-channel-j-097").is_none());
    assert!(cx.find("chat-rooms-folder-jobs-count").is_some());
    assert_eq!(
        cx.interactivity("chat-rooms-folder-jobs").aria.expanded,
        Some(false)
    );
    cx.simulate_click("chat-rooms-folder-engineering-fold");
    cx.run_until_parked();
    assert!(cx.find("chat-rooms-channel-ci").is_none());
    cx.simulate_click("chat-rooms-folder-engineering-open");
    cx.run_until_parked();
    assert_eq!(open_room(&view).as_deref(), Some("eng"));
}

// `CHAT_SCREEN_EXPORT=1` writes the room lists' frames for the app's
// node-less renderer: desktop and phone, each with every folder open and
// with jobs and engineering closed, listed in `folders-manifest.json`.
#[test]
fn export_folder_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for (touch, (width, height)) in [(false, (1000, 640)), (true, (390, 844))] {
        for (state, closed) in [("open", &[][..]), ("closed", &["jobs", "engineering"][..])] {
            let (mut cx, _) = desk(closed);
            cx.simulate_touch(touch);
            cx.run_until_parked();
            let device = if touch { "phone" } else { "desktop" };
            let name = format!("folders-{device}-{state}");
            let frame = serde_json::to_vec(&cx.whole_frame()).unwrap();
            std::fs::write(out.join(format!("{name}.json")), frame).unwrap();
            let size = serde_json::json!({ "name": name, "width": width, "height": height });
            manifest.push(size);
        }
    }
    let listed = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(out.join("folders-manifest.json"), listed).unwrap();
}
