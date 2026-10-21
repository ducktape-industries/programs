//! Widgets: a file a channel's meta names, drawn from any program's query read by that program's
//! schema, again on its blocks and on the file's; each kind in its place, errors said in place.
use super::*;
use crate::widget::{self, Filter, Scope, Shown, Slot};
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

type Held = Rc<RefCell<BTreeMap<String, (u8, String)>>>;
type Rooms = Rc<RefCell<Vec<ChannelInfo>>>;

fn held(files: &[(&str, &str)]) -> Held {
    let files = files
        .iter()
        .map(|(path, text)| ((*path).to_owned(), (1, (*text).to_owned())))
        .collect();
    Rc::new(RefCell::new(files))
}

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

fn chat_code() -> Vec<u8> {
    let schema = ::program::schema::interface::<::chat::Chat>();
    let payload = borsh::to_vec(&schema).unwrap();
    widget::module_with(::program::schema::SECTION, &payload)
}

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
        now: None,
        room: None,
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

const BOARD: &str = "/shared/widgets/board.toml";

// The board as orthory asked for it: a section per status with its icon and
// tone, rows in parts, a mark for a conversation at work; done left out, todo
// folded.
const BOARDED: &str = r#"
title = "Board"
[source]
program = "chat"
query = { Channels = { page = {} } }
rows = "Channels.items"
[view]
kind = "list"
except = { status = ["done"] }
group_by = "status"
order = ["waiting", "doing", "review", "deploying", "todo"]
fold = ["todo"]
item = "{title}"
lead = "J-{id}"
chip = "{owner|name}"
trail = "{updated.time|ago}"
badge = "{mark.doing}"
badges = [
    { when = "Turn", mark = "●", tone = "success", label = "working now" },
    { when = "Build", mark = "◐", tone = "warning", label = "waiting on a build" },
]
on_press = { open = "{channel}" }
[view.icons]
waiting = "🔴"
doing = "🛠️"
review = "👀"
deploying = "🚀"
todo = "📋"
[view.tones]
waiting = "danger"
doing = "accent"
review = "warning"
deploying = "success"
todo = "faint"
"#;

// The chain's newest block time, which `|ago` counts from.
const NOW: u64 = 1_792_400_000_000;

// Job `id` as board's `Jobs` reads in JSON, written `minutes` ago, with its
// owner's conversation at `doing` (a `Working` mark) or idle.
fn job(id: u32, title: &str, status: &str, owner: u64, minutes: u64, doing: Option<&str>) -> Value {
    let time = NOW - minutes * 60_000;
    json!({
        "id": id,
        "title": title,
        "status": status,
        "owner": owner,
        "channel": format!("j-{id:03}"),
        "updated": { "height": 1, "time": time, "by": owner },
        "mark": doing.map(|doing| json!({ "job": id, "doing": doing })),
    })
}

fn jobs() -> Vec<Value> {
    vec![
        job(
            91,
            "Phone simulator tests on the Mac",
            "waiting",
            17,
            140,
            None,
        ),
        job(84, "Who may write a widget file", "waiting", 15, 600, None),
        job(
            88,
            "The board widget as a list grouped by status",
            "doing",
            21,
            0,
            Some("Turn"),
        ),
        job(
            87,
            "A widget's failed read heals on the next block",
            "doing",
            21,
            3,
            Some("Build"),
        ),
        job(
            75,
            "Pictures in chat on phones",
            "doing",
            21,
            1,
            Some("Turn"),
        ),
        job(83, "Agents see images", "doing", 21, 12, None),
        job(
            81,
            "Memory scratch and CPU requests for build pods",
            "doing",
            21,
            6,
            Some("Build"),
        ),
        job(
            82,
            "Board closes only a room's own conversation",
            "review",
            21,
            25,
            None,
        ),
        job(86, "Fuel per frame for every view", "review", 21, 48, None),
        job(
            80,
            "A preview build for every change",
            "review",
            17,
            130,
            None,
        ),
        job(
            79,
            "Harness resumes a turn after a code swap",
            "deploying",
            17,
            9,
            None,
        ),
        job(
            90,
            "Agents' memory as rules, not logs",
            "todo",
            17,
            300,
            None,
        ),
        job(
            92,
            "The pond on a phone's home screen",
            "todo",
            15,
            900,
            None,
        ),
        job(
            65,
            "Close every finished job's room",
            "done",
            21,
            3000,
            None,
        ),
    ]
}

// A desk (a phone when `touch`) with `general` open, its meta naming the board
// in `slots`, the board showing `jobs()` by tech's, chief's and orthory's
// names.
fn board(touch: bool, slots: &[&str]) -> (TestAppContext, Entity<Chat>) {
    let meta: Vec<(&str, &str)> = slots.iter().map(|slot| (*slot, BOARD)).collect();
    let rooms = rooms(&meta);
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    serve(&cx, rooms.clone(), held(&[(BOARD, BOARDED)]));
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(vec![
                    person(15, "orthory"),
                    person(17, "chief"),
                    person(21, "tech"),
                ])),
                Query::Channels { .. } => Reply::Channels(page(rooms.borrow().clone())),
                query => chat_reply(query),
            })
        }));
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    let view = cx.open::<Chat>();
    match touch {
        true => {
            cx.simulate_touch(true);
            cx.simulate_resize(390., 844.);
        }
        false => cx.simulate_resize(1280., 800.),
    }
    seat(&mut cx);
    let room = match touch {
        true => "chat-rooms-channel-general",
        false => "chat-sidebar-channel-general",
    };
    cx.simulate_click(room);
    cx.run_until_parked();
    cx.update(&view, |chat, _, cx| {
        let live = chat
            .widgets
            .files
            .get_mut(BOARD)
            .expect("the room names the board");
        let Shown::Ready { rows, .. } = &mut live.shown else {
            panic!("the board reads: {:?}", live.shown);
        };
        *rows = jobs();
        chat.widgets.now = Some(NOW);
        cx.notify();
    });
    cx.run_until_parked();
    (cx, view)
}

#[test]
fn a_grouped_list_heads_each_status_with_its_icon_its_count_and_its_marks() {
    let (cx, _) = board(false, &["widget.pinned", "widget.header"]);
    let texts = cx.texts();
    let at = |text: &str| texts.iter().position(|drawn| drawn == text);
    let order = ["waiting", "doing", "review", "deploying", "todo"].map(at);
    assert!(order.iter().all(Option::is_some), "{texts:?}");
    assert!(order.is_sorted(), "in the file's order: {texts:?}");
    assert!(
        at("Close every finished job's room").is_none(),
        "except leaves done out"
    );
    assert!(
        at("Agents' memory as rules, not logs").is_none(),
        "todo starts folded"
    );
    let doing = cx.interactivity("chat-widget-pinned-section-1");
    assert_eq!(doing.role, Some(view::Role::Button));
    assert_eq!(doing.aria.expanded, Some(true));
    let counted = "doing, 5 rows, 2 working now, 2 waiting on a build";
    assert_eq!(doing.aria.label.as_deref(), Some(counted));
    let row = cx.interactivity("chat-widget-pinned-1-0");
    assert_eq!(row.role, Some(view::Role::Link));
    let said = "J-88, The board widget as a list grouped by status, tech, 0s, working now";
    assert_eq!(row.aria.label.as_deref(), Some(said));
    let line = "Board: 🔴 waiting 2 · 🛠️ doing 5 · 👀 review 3 · 🚀 deploying 1 · 📋 todo 2";
    assert!(cx.has_text(line), "{texts:?}");
}

#[test]
fn a_section_head_folds_and_opens_its_rows() {
    let (mut cx, _) = board(false, &["widget.pinned"]);
    cx.simulate_click("chat-widget-pinned-section-4");
    cx.run_until_parked();
    assert!(
        cx.has_text("Agents' memory as rules, not logs"),
        "todo opens"
    );
    let todo = cx.interactivity("chat-widget-pinned-section-4");
    assert_eq!(todo.aria.expanded, Some(true));
    assert!(cx.has_text("Agents see images"));
    cx.simulate_click("chat-widget-pinned-section-1");
    cx.run_until_parked();
    assert!(!cx.has_text("Agents see images"), "doing folds");
    let doing = cx.interactivity("chat-widget-pinned-section-1");
    assert_eq!(doing.aria.expanded, Some(false));
}

#[test]
fn a_phone_draws_a_row_as_its_title_over_a_line_of_the_rest() {
    let (mut cx, _) = board(true, &["widget.side"]);
    cx.simulate_click("chat-room-details");
    cx.run_until_parked();
    let title = "The board widget as a list grouped by status";
    assert!(cx.has_text(title), "{:?}", cx.texts());
    assert!(cx.has_text("J-88 · tech · 0s"), "{:?}", cx.texts());
}

#[test]
fn a_file_names_only_the_themes_tones() {
    let wrong = BOARDED.replace("doing = \"accent\"", "doing = \"blue\"");
    let said = widget::parse(&wrong).unwrap_err();
    assert!(said.starts_with("`blue` is no tone"), "{said}");
    let theme = view::Theme::light();
    for name in widget::TONES {
        assert!(widget::tone(&theme, Some(name)).is_some(), "{name}");
    }
}

#[test]
fn ago_counts_from_the_chains_newest_block() {
    let row = json!({ "at": NOW - 5 * 60_000 });
    let scope = Scope {
        me: None,
        names: None,
        count: None,
        now: Some(NOW),
        room: None,
    };
    assert_eq!(widget::fill("{at|ago} ago", &row, &scope), "5m ago");
    let before = Scope { now: None, ..scope };
    assert_eq!(widget::fill("{at|ago}", &row, &before), "");
}

// `CHAT_SCREEN_EXPORT=1` writes the board widget's frames for the app's
// node-less renderer: the desktop's side pane and the phone's details page,
// light and dark, listed in `board-manifest.json`.
#[test]
fn export_board_widget_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for dark in [false, true] {
        let theme = if dark { "dark" } else { "light" };
        for (touch, (width, height)) in [(false, (1280, 800)), (true, (390, 844))] {
            let (mut cx, _) = board(touch, &["widget.side"]);
            cx.simulate_theme(dark);
            let open = match touch {
                true => "chat-room-details",
                false => "chat-room-widget",
            };
            cx.simulate_click(open);
            cx.run_until_parked();
            let device = if touch { "phone" } else { "desktop" };
            let name = format!("board-{device}-{theme}");
            let frame = serde_json::to_vec(&cx.whole_frame()).unwrap();
            std::fs::write(out.join(format!("{name}.json")), frame).unwrap();
            let size = json!({ "name": name, "width": width, "height": height, "theme": theme });
            manifest.push(size);
        }
    }
    let listed = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(out.join("board-manifest.json"), listed).unwrap();
}

const JOB: &str = "/shared/widgets/job.toml";

// The job pane's shape over chat's own reply: the open room's channel as one
// row, and a file named for it beside it, drawn as markdown.
const JOBBED: &str = r#"
title = "This job"
[source]
program = "chat"
query = { Channel = { channel_id = "{room}" } }
rows = "Channel"
[source.text]
path = "/shared//notes/{channel.id}.md"
[view]
kind = "text"
"#;

const NOTES: &str = "/shared/notes/general.md";

const SUMMARY: &str = "**What:** the chat board widget as a list grouped by status, and a pane in every job room.
**Why:** orthory found the old board widget useless and too dry; eddy wants a room that explains itself.
**Decided:** status sections with icons, counts and tone bars; ● a turn, ◐ a build; this pane reads `summary.md`.
**State now:** the grouped list is green (#155); board `Working` is green; the PNG tool landed.
**Next:** board `JobIn` and the room meta upkeep, then screenshots for eddy.
**Waiting on:** nothing.";

#[test]
fn a_text_widget_draws_the_file_its_row_names_as_markdown_for_the_open_room() {
    let held = held(&[(JOB, JOBBED), (NOTES, SUMMARY)]);
    let (mut cx, _) = desk(rooms(&[("widget.side", JOB)]), held.clone());
    cx.simulate_click("chat-room-widget");
    cx.run_until_parked();
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    let room = Query::Channel {
        channel_id: "general".into(),
    };
    assert!(
        asked.contains(&room),
        "{{room}} is the open room: {asked:?}"
    );
    let first =
        "What: the chat board widget as a list grouped by status, and a pane in every job room.";
    assert!(cx.has_text(first), "{:?}", cx.texts());
    assert!(cx.has_text("Waiting on: nothing."));
    let blocks = cx.find("chat-widget-side-text-block-5").is_some();
    assert!(blocks, "a line is a block: {:?}", cx.texts());

    // the file moved: the next files block reads it again
    let moved = SUMMARY.replace("Waiting on:** nothing.", "Waiting on:** chief's merge.");
    held.borrow_mut().insert(NOTES.into(), (2, moved));
    let files = cx.host().stream::<Changes<files::Files>>();
    files.send(block(12, Vec::new()));
    cx.run_until_parked();
    assert!(
        cx.has_text("Waiting on: chief's merge."),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_text_widget_with_no_file_yet_says_so_and_its_header_says_the_first_line() {
    let bare = held(&[(JOB, JOBBED)]);
    let (mut cx, _) = desk(rooms(&[("widget.side", JOB)]), bare);
    cx.simulate_click("chat-room-widget");
    cx.run_until_parked();
    assert!(
        cx.has_text("Nothing at /shared/notes/general.md yet."),
        "{:?}",
        cx.texts()
    );
    let held = held(&[(JOB, JOBBED), (NOTES, SUMMARY)]);
    let (cx, _) = desk(rooms(&[("widget.header", JOB)]), held);
    let line = "This job: What: the chat board widget as a list grouped by status, and a pane in every job room.";
    assert!(cx.has_text(line), "{:?}", cx.texts());
    let bare = JOBBED.replace(
        "[source.text]\npath = \"/shared//notes/{channel.id}.md\"\n",
        "",
    );
    assert_ne!(bare, JOBBED, "the bare file drops [source.text]");
    assert_eq!(
        widget::parse(&bare).unwrap_err(),
        "a text widget needs `[source.text]`"
    );
}

#[test]
fn room_fills_queries_and_templates() {
    let query = json!({ "JobIn": { "channel": "{room}" } });
    assert_eq!(
        widget::with_room(&query, Some("j-088")),
        json!({ "JobIn": { "channel": "j-088" } })
    );
    let scope = Scope {
        me: None,
        names: None,
        count: None,
        now: None,
        room: Some("j-088"),
    };
    assert_eq!(widget::fill("#{room}", &json!({}), &scope), "#j-088");
}

// `CHAT_SCREEN_EXPORT=1` writes the job pane's frames: the desktop's side
// pane and the phone's details page, listed in `pane-manifest.json`.
#[test]
fn export_job_pane_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for (touch, (width, height)) in [(false, (1280, 800)), (true, (390, 1400))] {
        let held = held(&[(JOB, JOBBED), (NOTES, SUMMARY)]);
        let rooms = rooms(&[("widget.side", JOB)]);
        let mut cx = TestAppContext::new();
        configure(&mut cx);
        serve(&cx, rooms, held);
        cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
        let _view = cx.open::<Chat>();
        if touch {
            cx.simulate_touch(true);
        }
        cx.simulate_resize(width as f32, height as f32);
        seat(&mut cx);
        let (room, open) = match touch {
            true => ("chat-rooms-channel-general", "chat-room-details"),
            false => ("chat-sidebar-channel-general", "chat-room-widget"),
        };
        cx.simulate_click(room);
        cx.run_until_parked();
        cx.simulate_click(open);
        cx.run_until_parked();
        let name = format!("pane-{}-light", if touch { "phone" } else { "desktop" });
        let frame = serde_json::to_vec(&cx.whole_frame()).unwrap();
        std::fs::write(out.join(format!("{name}.json")), frame).unwrap();
        manifest.push(json!({ "name": name, "width": width, "height": height, "theme": "light" }));
    }
    let listed = serde_json::to_vec_pretty(&manifest).unwrap();
    std::fs::write(out.join("pane-manifest.json"), listed).unwrap();
}

#[test]
fn a_reply_path_names_a_variant_by_its_rust_name_or_in_snake_case() {
    let reply = json!({ "jobs": { "items": [1, 2] } });
    assert_eq!(widget::at(&reply, "Jobs.items"), Some(&json!([1, 2])));
    assert_eq!(widget::at(&reply, "jobs.items"), Some(&json!([1, 2])));
}
