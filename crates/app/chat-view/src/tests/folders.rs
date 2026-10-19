//! Channel folders, the one grouping: a `/` in a channel's name files it under a folder in both
//! trees' room lists, a folder opens and closes and stays so, and a closed one counts its unread.
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
fn a_slash_files_a_channel_under_its_folder_by_its_first_slash() {
    let (mut cx, _) = desk(&[]);
    for shown in [
        "general",
        "jobs/",
        "j-097 · channel folders",
        "j-098 · board",
        "b/c",
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
            "general jobs/ j-097 j-098 engineering/ ci random a/ deep"
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
    let (cx, _) = desk(&["jobs", "a"]);
    assert!(cx.find("chat-sidebar-channel-j-097").is_none());
    assert!(cx.find("chat-sidebar-folder-jobs-count").is_some());
    assert!(cx.find("chat-sidebar-folder-a").is_some());
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

// The `/` in names is the one grouping: a `category` meta files nothing, the
// create form and the details have no category, and a block that writes a
// channel's meta still reads the list again.
#[test]
fn a_category_files_nothing_and_new_channels_carry_none() {
    let (mut cx, _) = desk(&[]);
    let mut moved = rooms();
    moved[0].channel.name = "teams/general".into();
    moved[0].meta.insert("category".into(), "Ops".into());
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(moved.clone())),
                query => chat_reply(query),
            })
        }));
    let meta = chat::tables::CHANNEL_META.key(&"general".to_owned());
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(5, vec![meta]));
    cx.run_until_parked();
    assert!(cx.find("chat-sidebar-folder-teams").is_some(), "read again");
    assert!(cx.find("chat-sidebar-category-Ops").is_none());

    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx.simulate_click("chat-room-details");
    assert!(cx.find("chat-details-category").is_none());

    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_click("chat-sidebar-new-channel");
    assert!(cx.find("chat-create-category").is_none());
    cx.simulate_input("chat-create-name", "jobs/j-102");
    cx.simulate_submit("chat-create-name");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<::chat::Chat>>().pop();
    assert!(
        matches!(&sent, Some(Op::CreateChannel { name, .. }) if name == "jobs/j-102"),
        "{sent:?}"
    );
}

// A folder's header reads its name with its `/`, and its chevron ends the row: the lead column
// first, as a room row's, then the name taking the slack, the chevron last.
#[test]
fn a_folders_header_keeps_its_slash_and_its_chevron_ends_the_row() {
    let (cx, _) = desk(&[]);
    assert!(cx.has_text("jobs/"), "{:?}", cx.texts());
    assert!(!cx.has_text("jobs"), "the header keeps its delimiter");
    for row in [
        "chat-sidebar-folder-jobs",
        "chat-sidebar-folder-engineering",
    ] {
        let Some(wire::Node::Container(wire::ContainerNode { children, .. })) = cx.find(row) else {
            panic!("{row} is a row");
        };
        let fold = format!("{row}-fold");
        assert_eq!(
            children.last(),
            cx.find(&fold),
            "{row}'s chevron is its last part"
        );
        let Some(wire::Node::Container(wire::ContainerNode { style, .. })) = children.get(1) else {
            panic!("{row}'s name is a container");
        };
        let grow = cx.styles()[*style].flex_grow;
        assert_eq!(grow, Some(1.), "{row}'s name takes the slack before it");
    }
}

fn field_of(cx: &TestAppContext, key: &str) -> (String, Vec<wire::TextToken>) {
    let Some(wire::Node::Field { value, tokens, .. }) = cx.find(key) else {
        panic!("{key} is a field");
    };
    (value.clone(), tokens.to_vec())
}

fn chip_of(folder: &str) -> wire::TextToken {
    let end = folder.len() as u32 + 1;
    wire::TextToken {
        range: wire::TextRange { start: 0, end },
        id: format!("{}{folder}", wire::CHIP),
    }
}

// The new channel's name: the text before the first `/` typed becomes one chip at the field's
// start, a later `/` is the name's, Backspace right after the chip makes it text again, and the
// channel is created under the whole name.
#[test]
fn a_slash_in_a_new_channels_name_chips_its_folder_and_backspace_takes_it_back() {
    let (mut cx, _) = desk(&[]);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.simulate_click("chat-sidebar-new-channel");
    cx.simulate_input("chat-create-name", "/x");
    cx.run_until_parked();
    assert_eq!(
        field_of(&cx, "chat-create-name").1,
        [],
        "no folder before it"
    );
    cx.simulate_input("chat-create-name", "jobs");
    cx.simulate_input("chat-create-name", "jobs/");
    cx.run_until_parked();
    let (text, tokens) = field_of(&cx, "chat-create-name");
    assert_eq!((text.as_str(), tokens), ("jobs/", vec![chip_of("jobs")]));

    cx.simulate_input("chat-create-name", "");
    cx.run_until_parked();
    let (text, tokens) = field_of(&cx, "chat-create-name");
    assert_eq!(
        (text.as_str(), tokens.len()),
        ("jobs", 0),
        "the chip is text again"
    );

    cx.simulate_input("chat-create-name", "jobs/");
    cx.run_until_parked();
    cx.simulate_input("chat-create-name", "jobs/j-102/x");
    cx.run_until_parked();
    let (text, _) = field_of(&cx, "chat-create-name");
    assert_eq!(text, "jobs/j-102/x", "a later slash is the name's");
    cx.simulate_submit("chat-create-name");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<::chat::Chat>>().pop();
    let named = matches!(&sent, Some(Op::CreateChannel { name, .. }) if name == "jobs/j-102/x");
    assert!(named, "{sent:?}");
}

#[test]
fn the_phones_new_channel_page_chips_a_pasted_folder_too() {
    let (mut cx, _) = desk(&[]);
    cx.simulate_touch(true);
    cx.simulate_click("chat-rooms-new-channel");
    cx.simulate_input("chat-new-name", "teams/ops/web");
    cx.run_until_parked();
    let (text, tokens) = field_of(&cx, "chat-new-name");
    assert_eq!(
        (text.as_str(), tokens),
        ("teams/ops/web", vec![chip_of("teams")])
    );
}

// A folder's header and the rooms under it lead with one column, the kit row's: the header's name
// starts where the rooms' names do.
#[test]
fn a_folders_name_starts_where_its_rooms_names_do() {
    let (cx, _) = desk(&[]);
    let lead = |key: &str| {
        let Some(wire::Node::Container(wire::ContainerNode { children, .. })) = cx.find(key) else {
            panic!("{key} is a row");
        };
        let Some(wire::Node::Container(wire::ContainerNode { style, .. })) = children.first()
        else {
            panic!("{key} leads with a column");
        };
        let style = &cx.styles()[*style];
        (style.size.width, style.flex_shrink)
    };
    let header = lead("chat-sidebar-folder-jobs");
    assert!(header.0.is_some(), "the lead column has the kit's width");
    assert_eq!(header, lead("chat-sidebar-channel-j-097"));
    assert_eq!(header, lead("chat-sidebar-channel-general"));
}
