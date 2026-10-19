//! Widgets: a file a channel's meta names, drawn from any program's query read by that program's
//! schema, again on its blocks and on the file's; each kind in its place, errors said in place.
use super::*;
use crate::widget::{self, Filter, Scope, Slot};
use abi::BlobId;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use view::methods::Method;
use view::methods::{BlobGet, Call, NamedChanges, NamedQuery};

const ROOMS: &str = "/shared/widgets/rooms.toml";
const COUNT: &str = "/shared/widgets/count.toml";
const OWNERS: &str = "/shared/widgets/owners.toml";

const LISTED: &str = r##"
title = "Rooms"
[source]
program = "chat"
query = { Channels = { page = {} } }
rows = "Channels.items"
cursor = "Channels.page.after"
[view]
kind = "list"
except = { "channel.id" = "dm-7-8" }
item = "#{channel.name} at {head_seq}"
on_press = { open = "{channel.id}" }
"##;

const COUNTED: &str = r#"
title = "Open rooms"
[source]
program = "chat"
query = { Channels = { page = {} } }
rows = "Channels.items"
[view]
kind = "stat"
where = { "channel.archived" = false }
value = "{count}"
label = "of {Channels.height}"
"#;

const OWNED: &str = r##"
title = "Owners"
[source]
program = "chat"
query = { Channels = { page = {} } }
rows = "Channels.items"
[view]
kind = "table"
columns = [{ title = "Room", value = "#{channel.name}" }, { title = "Owner", value = "{channel.owner.Account|name}" }]
on_press = { open = "{channel.id}" }
"##;

const LANES: &str = r#"
title = "Lanes"
[source]
program = "chat"
query = { Channels = { page = {} } }
rows = "Channels.items"
[view]
kind = "columns"
group_by = "channel.archived"
order = ["true", "false"]
item = "{channel.name}"
"#;

const TICKED: &str = r#"
title = "Archived"
[source]
program = "chat"
query = { Channels = { page = {} } }
rows = "Channels.items"
[view]
kind = "checklist"
except = { "channel.id" = ["dm-7-8"] }
item = "{channel.name}"
checked = { "channel.archived" = true }
on_press = { open = "{channel.id}" }
"#;

// The files the fake files program holds: each path's object (a number
// that moves with each write) and text.
type Held = Rc<RefCell<BTreeMap<String, (u8, String)>>>;
type Rooms = Rc<RefCell<Vec<ChannelInfo>>>;

fn held(files: &[(&str, &str)]) -> Held {
    let files = files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (1, (*text).to_owned())))
        .collect();
    Rc::new(RefCell::new(files))
}

// `general` wearing `meta`, `dm-7-8`, and `ops`, archived.
fn rooms(meta: &[(&str, &str)]) -> Rooms {
    let mut general = channel("general", "General", 3);
    for (key, value) in meta {
        general.meta.insert((*key).into(), (*value).into());
    }
    let mut ops = channel("ops", "Ops", 5);
    ops.channel.archived = true;
    Rc::new(RefCell::new(vec![general, channel("dm-7-8", "dm", 1), ops]))
}

const CODE: [u8; 32] = [4; 32];

// Chat's code as the node holds it: a module carrying its schema.
fn chat_code() -> Vec<u8> {
    let schema = ::program::schema::interface::<::chat::Chat>();
    let payload = borsh::to_vec(&schema).unwrap();
    widget::module_with(::program::schema::SECTION, &payload)
}

// The programs, files and blobs a widget reads, answered: chat runs from
// `CODE`, files holds `held`.
fn serve(cx: &TestAppContext, rooms: Rooms, held: Held) {
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Channels { .. } => Reply::Channels(page(rooms.borrow().clone())),
                query => chat_reply(query),
            })
        }));
    cx.host().handle::<Ask<programs::Programs>>(|query| {
        Ok(match query {
            programs::Query::Program(name) => programs::Reply::Program {
                height: 1,
                entry: (name == "chat").then(|| programs::Entry {
                    program: name.clone(),
                    code: BlobId::Sha256(CODE),
                    params: Vec::new(),
                }),
            },
            query => panic!("unexpected programs query: {query:?}"),
        })
    });
    let code = format!("sha256:{}", abi::hex(&CODE));
    cx.host()
        .handle::<BlobGet>(move |id| Ok((id == code).then(chat_code)));
    cx.host().handle::<Ask<files::Files>>(move |query| {
        let snapshot = BlobId::Sha256([9; 32]);
        Ok(match query {
            files::Query::Stat { path, .. } => {
                let held = held.borrow();
                let entry = held.get(&path).map(|(object, text)| files::EntryInfo {
                    path: path.clone(),
                    node: files::Node::File {
                        file: BlobId::Sha256([*object; 32]),
                        size: text.len() as u64,
                        exec: false,
                    },
                    meta: Default::default(),
                });
                files::Reply::Stat {
                    snapshot: Some(snapshot),
                    entry,
                }
            }
            files::Query::Read { path, offset, .. } => {
                let text = held.borrow().get(&path).map(|(_, text)| text.clone());
                let text = text.unwrap_or_default();
                files::Reply::Contents(files::Contents {
                    snapshot,
                    size: text.len() as u64,
                    offset,
                    bytes: text.as_bytes()[offset as usize..].to_vec(),
                    eof: true,
                })
            }
            query => panic!("unexpected files query: {query:?}"),
        })
    });
}

// A desk with `general` open, its meta naming `meta`'s widgets.
fn desk(rooms: Rooms, held: Held) -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx, rooms, held);
    let view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    (cx, view)
}

// How many questions went to chat, the view's own and the widgets'.
fn asked_chat(cx: &TestAppContext) -> usize {
    let asked = cx.host().requests::<NamedQuery>();
    asked.iter().filter(|call| call.target == "chat").count()
}

fn said_starting(cx: &TestAppContext, start: &str) -> bool {
    cx.texts().iter().any(|text| text.starts_with(start))
}

#[test]
fn a_pinned_list_draws_a_programs_rows_by_its_schema_and_a_press_opens_the_room() {
    let held = held(&[(ROOMS, LISTED)]);
    let (mut cx, view) = desk(rooms(&[("widget.pinned", ROOMS)]), held);
    assert!(cx.has_text("Rooms"), "{:?}", cx.texts());
    assert!(cx.has_text("#General at 3"), "{:?}", cx.texts());
    assert!(cx.has_text("#Ops at 5"));
    assert!(!cx.has_text("#dm at 1"), "except leaves the dm out");
    let asked = cx.host().requests::<NamedQuery>();
    assert!(
        asked.iter().any(|call| call.target == "chat"),
        "the widget asks chat by name"
    );
    cx.simulate_click("chat-widget-pinned-1");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "ops"));
}

#[test]
fn a_cursor_reads_every_page_putting_each_next_where_it_says() {
    let (rooms, held) = (rooms(&[("widget.pinned", ROOMS)]), held(&[(ROOMS, LISTED)]));
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx, rooms.clone(), held);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            let all = rooms.borrow();
            let (items, next) = match query {
                Query::Channels { page } if page.after.is_none() => {
                    (all[..1].to_vec(), Some(vec![0xab]))
                }
                Query::Channels { page } => {
                    assert_eq!(page.after, Some(vec![0xab]));
                    (all[1..].to_vec(), None)
                }
                query => return Ok(chat_reply(query)),
            };
            let height = 1;
            Ok(Reply::Channels(::chat::PageResponse {
                height,
                items,
                next,
            }))
        }));
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.has_text("#General at 3"), "{:?}", cx.texts());
    assert!(
        cx.has_text("#Ops at 5"),
        "the second page: {:?}",
        cx.texts()
    );
}

#[test]
fn a_cursor_whose_reply_has_no_next_beside_its_rows_says_so() {
    let nested = LISTED.replace(
        "rows = \"Channels.items\"",
        "rows = \"Channels.items.0.meta\"",
    );
    let held = held(&[(ROOMS, &nested)]);
    let (cx, _) = desk(rooms(&[("widget.pinned", ROOMS)]), held);
    let said = "The reply doesn’t read: there is no `Channels.items.0.next`";
    assert!(said_starting(&cx, said), "{:?}", cx.texts());
}

#[test]
fn a_failed_read_says_which_and_asks_again_on_the_next_block() {
    let (rooms, held) = (rooms(&[("widget.pinned", ROOMS)]), held(&[(ROOMS, LISTED)]));
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx, rooms, held);
    let code = format!("sha256:{}", abi::hex(&CODE));
    let cut = Rc::new(RefCell::new(true));
    cx.host().handle::<BlobGet>(move |id| {
        if id == code && cut.replace(false) {
            let why = "no answer came back: error decoding response body";
            return Err(view::host::malformed(why.into()));
        }
        Ok((id == code).then(chat_code))
    });
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    let said = "Reading chat’s code: no answer came back: error decoding response body";
    assert!(cx.has_text(said), "{:?}", cx.texts());
    let files = cx.host().stream::<Changes<files::Files>>();
    files.send(block(10, Vec::new()));
    cx.run_until_parked();
    assert!(cx.has_text("#General at 3"), "{:?}", cx.texts());
}

#[test]
fn a_refused_question_names_the_program_asked() {
    // the widget's own question, which the view never asks
    let limited = COUNTED.replace("page = {}", "page = { limit = 99 }");
    let (rooms, held) = (
        rooms(&[("widget.pinned", ROOMS)]),
        held(&[(ROOMS, &limited)]),
    );
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx, rooms.clone(), held);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| match query {
            Query::Channels { page } if page.limit == Some(99) => {
                Err(view::host::malformed("no answer came back".into()))
            }
            Query::Channels { .. } => Ok(Reply::Channels(page(rooms.borrow().clone()))),
            query => Ok(chat_reply(query)),
        }));
    let _view = cx.open::<Chat>();
    seat(&mut cx);
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(
        cx.has_text("Asking chat: no answer came back"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_schema_is_kept_on_the_device_with_its_code_and_read_from_there() {
    use view::methods::{self, StoreGet, StoreSet};
    let code = format!("sha256:{}", abi::hex(&CODE));
    let fetched = |cx: &TestAppContext| {
        let asked = cx.host().requests::<BlobGet>();
        asked.iter().filter(|id| **id == code).count()
    };
    let held = held(&[(ROOMS, LISTED)]);
    let (cx, _) = desk(rooms(&[("widget.pinned", ROOMS)]), held.clone());
    assert_eq!(fetched(&cx), 1);
    let kept = cx.host().requests::<StoreSet>();
    let kept = kept
        .into_iter()
        .find(|(key, _)| key == "widget-schema/chat");
    let kept = kept.and_then(|(_, kept)| kept).expect("the schema is kept");
    let (from, section): (BlobId, Vec<u8>) = methods::decode(&kept).unwrap();
    assert_eq!(from, BlobId::Sha256(CODE));
    // a later run reads it off the device; kept from other code, it fetches
    let elsewhere = methods::encode(&(BlobId::Sha256([5; 32]), section));
    for (kept, fetches) in [(kept, 0), (elsewhere, 1)] {
        let mut cx = TestAppContext::new();
        configure(&mut cx);
        serve(&cx, rooms(&[("widget.pinned", ROOMS)]), held.clone());
        cx.host()
            .handle::<StoreGet>(move |key| Ok((key == "widget-schema/chat").then(|| kept.clone())));
        let _view = cx.open::<Chat>();
        seat(&mut cx);
        cx.simulate_click("chat-sidebar-channel-general");
        cx.run_until_parked();
        assert!(cx.has_text("#General at 3"), "{:?}", cx.texts());
        assert_eq!(fetched(&cx), fetches);
    }
}

#[test]
fn a_widget_with_no_rows_says_its_own_empty_words() {
    let only = "where = { \"channel.id\" = \"none\" }\nempty = \"Nothing waiting on a decision.\"";
    let none = TICKED.replace("except = { \"channel.id\" = [\"dm-7-8\"] }", only);
    let held = held(&[(ROOMS, &none)]);
    let (cx, _) = desk(rooms(&[("widget.pinned", ROOMS)]), held);
    let said = "Nothing waiting on a decision.";
    assert!(cx.has_text(said), "{:?}", cx.texts());
    assert_eq!(
        widget::parse(LISTED).unwrap().view.nothing(),
        "Nothing to show."
    );
}

#[test]
fn the_pinned_card_folds_and_unfolds() {
    let held = held(&[(ROOMS, LISTED)]);
    let (mut cx, _) = desk(rooms(&[("widget.pinned", ROOMS)]), held);
    let fold = cx.interactivity("chat-widget-pinned-fold");
    assert_eq!(fold.aria.expanded, Some(true));
    cx.simulate_click("chat-widget-pinned-fold");
    cx.run_until_parked();
    assert!(!cx.has_text("#General at 3"), "folded, the rows go");
    assert_eq!(
        cx.interactivity("chat-widget-pinned-fold").aria.expanded,
        Some(false)
    );
    cx.simulate_click("chat-widget-pinned-fold");
    cx.run_until_parked();
    assert!(cx.has_text("#General at 3"));
}

#[test]
fn a_block_of_the_source_asks_again_and_a_moved_file_is_read_again() {
    let (rooms, held) = (rooms(&[("widget.pinned", ROOMS)]), held(&[(ROOMS, LISTED)]));
    let (mut cx, _) = desk(rooms.clone(), held.clone());
    assert!(cx.has_text("#Ops at 5"));
    rooms.borrow_mut()[2].head_seq = 6;
    let chat = cx.host().stream::<Changes<::chat::Chat>>();
    chat.send(block(9, Vec::new()));
    cx.run_until_parked();
    assert!(cx.has_text("#Ops at 6"), "{:?}", cx.texts());
    // a files block that leaves the file as it was asks nothing again
    let asked = asked_chat(&cx);
    let files = cx.host().stream::<Changes<files::Files>>();
    files.send(block(10, Vec::new()));
    cx.run_until_parked();
    assert_eq!(asked_chat(&cx), asked);
    held.borrow_mut().insert(
        ROOMS.into(),
        (2, LISTED.replace("#{channel.name} at", "{channel.name}:")),
    );
    files.send(block(11, Vec::new()));
    cx.run_until_parked();
    assert!(cx.has_text("Ops: 6"), "{:?}", cx.texts());
}

#[test]
fn a_header_stat_counts_the_rows_where_keeps() {
    let held = held(&[(COUNT, COUNTED)]);
    let (cx, _) = desk(rooms(&[("widget.header", COUNT)]), held);
    assert!(cx.has_text("Open rooms: 2 of 1"), "{:?}", cx.texts());
}

#[test]
fn the_side_widget_opens_from_the_header_as_a_pane_with_a_table() {
    let held = held(&[(OWNERS, OWNED)]);
    let (mut cx, view) = desk(rooms(&[("widget.side", OWNERS)]), held);
    assert!(!cx.has_text("Owner"), "the pane waits for its button");
    cx.simulate_click("chat-room-widget");
    cx.run_until_parked();
    view.read(|chat| assert!(chat.widget_side_shown()));
    let table = cx.interactivity("chat-widget-side-table");
    assert_eq!(table.role, Some(view::Role::Table));
    assert!(cx.has_text("Room") && cx.has_text("Owner"));
    assert!(cx.has_text("#General"));
    assert!(
        cx.has_text("eddy"),
        "an account by its name: {:?}",
        cx.texts()
    );
    cx.simulate_click("chat-widget-side-1-link");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "dm-7-8"));
}

#[test]
fn columns_follow_their_order_and_count_their_cards() {
    let held = held(&[(ROOMS, LANES)]);
    let (cx, _) = desk(rooms(&[("widget.pinned", ROOMS)]), held);
    let texts = cx.texts();
    let at = |text: &str| texts.iter().rposition(|drawn| drawn == text);
    let lanes = ["true · 1", "Ops", "false · 2", "General", "dm"].map(at);
    assert!(lanes.iter().all(Option::is_some), "{texts:?}");
    assert!(
        lanes.is_sorted(),
        "in order, each card under its lane: {texts:?}"
    );
    assert!(cx.find("chat-widget-pinned-column-1").is_some());
}

#[test]
fn what_does_not_read_is_said_in_the_widgets_place() {
    let broken = LISTED.replace("kind = \"list\"", "kind = \"pie\"");
    let elsewhere = LISTED.replace("program = \"chat\"", "program = \"nowhere\"");
    let held = held(&[(ROOMS, &broken), (COUNT, &elsewhere)]);
    let meta = [
        ("widget.pinned", ROOMS),
        ("widget.header", COUNT),
        ("widget.side", "/shared/widgets/none.toml"),
    ];
    let (mut cx, _) = desk(rooms(&meta), held);
    assert!(
        said_starting(&cx, "The widget’s file doesn’t read:"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Rooms: This network runs no program called nowhere."));
    // a program that does not run is not followed: programs is, whose
    // block may deploy it
    let followed = cx.host().requests::<NamedChanges>();
    assert!(followed.contains(&"programs".to_owned()), "{followed:?}");
    assert!(!followed.contains(&"nowhere".to_owned()));
    cx.simulate_click("chat-room-widget");
    cx.run_until_parked();
    assert!(cx.has_text("No widget file at /shared/widgets/none.toml."));
}

#[test]
fn a_phone_shows_the_pinned_card_in_the_room_and_the_side_widget_in_details() {
    let held = held(&[(ROOMS, LISTED), (OWNERS, TICKED)]);
    let rooms = rooms(&[("widget.pinned", ROOMS), ("widget.side", OWNERS)]);
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx, rooms, held);
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    let _view = cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    seat(&mut cx);
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    assert!(cx.has_text("#General at 3"), "{:?}", cx.texts());
    assert!(!cx.has_text("☑ Ops"), "the side widget waits in details");
    cx.simulate_click("chat-room-details");
    cx.run_until_parked();
    assert!(cx.has_text("☑ Ops"), "{:?}", cx.texts());
    assert!(cx.has_text("☐ General"));
    let row = cx.interactivity("chat-widget-side-1");
    assert_eq!(row.role, Some(view::Role::Link));
    assert_eq!(row.aria.label.as_deref(), Some("Ops, done"));
}

#[test]
fn a_named_method_puts_the_same_bytes_on_the_wire_as_the_typed_one() {
    let query = Query::Channels {
        page: Default::default(),
    };
    let call = Call {
        target: "chat".into(),
        body: view::methods::encode(&query),
    };
    assert_eq!(
        NamedQuery::encode_request(&call),
        Ask::<::chat::Chat>::encode_request(&query)
    );
    assert_eq!(NamedQuery::KIND, Ask::<::chat::Chat>::KIND);
    assert_eq!(
        NamedChanges::encode_request(&"chat".to_owned()),
        Changes::<::chat::Chat>::encode_request(&())
    );
    assert_eq!(NamedChanges::KIND, Changes::<::chat::Chat>::KIND);
}

#[test]
fn templates_fill_paths_names_and_the_reader() {
    let row = json!({ "id": 21, "title": "Widgets", "owner": 7, "tags": ["a", "b"], "none": null });
    let names = chat::view::Names::empty();
    let scope = Scope {
        me: Some(21),
        names: Some(&names),
        count: None,
    };
    let filled = widget::fill(
        "J-{id} {title} {tags.1}{none} {me} {missing}|{open",
        &row,
        &scope,
    );
    assert_eq!(filled, "J-21 Widgets b 21 |{open");
    assert_eq!(widget::fill("{owner|name}", &row, &scope), "account 7");
    let query = json!({ "Jobs": { "viewer": "{me}", "note": "by {me}" } });
    assert_eq!(
        widget::with_me(&query, Some(21)),
        json!({ "Jobs": { "viewer": 21, "note": "by 21" } })
    );
    let mut paged = json!({ "Jobs": { "page": {} } });
    widget::put(&mut paged, "Jobs.page.after", json!("00ff")).unwrap();
    assert_eq!(paged, json!({ "Jobs": { "page": { "after": "00ff" } } }));
}

#[test]
fn where_and_except_compare_values_as_templates_write_them() {
    let filter = |text: &str| -> Filter { toml::from_str(text).unwrap() };
    let row = json!({ "status": "waiting", "owner": 21, "done": false });
    assert!(widget::holds(&filter("status = \"waiting\""), &row));
    assert!(widget::holds(
        &filter("status = [\"doing\", \"waiting\"]"),
        &row
    ));
    assert!(widget::holds(&filter("owner = 21\ndone = false"), &row));
    assert!(!widget::holds(&filter("owner = \"7\""), &row));
}

#[test]
fn a_file_that_misses_what_its_kind_needs_says_so() {
    let without = |field: &str| {
        let text = LISTED.replace(&format!("{field} ="), "unused =");
        widget::parse(&text).unwrap_err()
    };
    assert!(
        without("item").contains("unknown field"),
        "{}",
        without("item")
    );
    let list = LISTED.lines().filter(|line| !line.starts_with("item"));
    let list: Vec<&str> = list.collect();
    assert_eq!(
        widget::parse(&list.join("\n")).unwrap_err(),
        "a list widget needs `item`"
    );
    let parsed = widget::parse(LISTED).unwrap();
    assert_eq!(parsed.source.query, json!({ "Channels": { "page": {} } }));
    assert_eq!(
        parsed.view.except.get("channel.id"),
        Some(&Value::from("dm-7-8"))
    );
    let slots: Vec<&str> = Slot::ALL.iter().map(|slot| slot.key()).collect();
    assert_eq!(slots, ["widget.header", "widget.pinned", "widget.side"]);
}
