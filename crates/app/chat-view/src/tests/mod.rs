//! The view against a fake host: every method it asks is answered here, and
//! each test drives the frame the way a person would.
use super::*;
use chat::{ChannelInfo, MessageHits, MsgRow, Op, PostPolicy, Principal, Query, Reply};
use view::Loadable;
use view::testing::TestAppContext;
use view::wire;
use view::{Entity, StyleRefinement, Styled};

use identity::Identity;
use view::methods::Change;
use view::methods::Query as Ask;
use view::methods::{Changes, HostId, HostSession, HostVisible, Session, Submit};

mod agents;
mod archived;
mod attachments;
mod categories;
mod context_menus;
mod dms;
mod menus;
mod message;
mod notices;
mod phone;
mod reader;
mod rich;
mod room;
mod scroll;
mod widgets;
mod writing;

fn page<T>(items: Vec<T>) -> ::chat::PageResponse<T> {
    ::chat::PageResponse {
        height: 1,
        items,
        next: None,
    }
}

fn channel(id: &str, name: &str, head_seq: u64) -> ChannelInfo {
    ChannelInfo {
        channel: ::chat::ChannelRow {
            id: id.into(),
            name: name.into(),
            created_at: 0,
            post_policy: PostPolicy::Open,
            owner: Principal::Account(7),
            archived: false,
        },
        head_seq,
        meta: Default::default(),
    }
}

#[test]
fn the_root_tracks_the_shared_theme_and_is_accessible() {
    let (mut cx, _) = opened();
    let dark = view::Theme::dark();
    cx.set_global(dark);
    let Some(wire::Node::Container(view::wire::ContainerNode { style, .. })) = cx.find("chat-root")
    else {
        panic!("chat root is a styled container");
    };
    let style = &cx.styles()[*style];
    assert_eq!(
        style
            .background
            .as_ref()
            .and_then(|fill| fill.color())
            .and_then(|background| background.as_solid()),
        Some(dark.background)
    );
    assert_eq!(style.text.color, Some(dark.foreground));
}

/// A desk opens chat wide enough for a thread beside the room: its
/// manifest, as the host reads it, asks 1200 px.
#[test]
fn a_desk_window_opens_chat_at_its_width() {
    let text = std::str::from_utf8(&MANIFEST_SECTION).unwrap();
    let manifest = wire::manifest::Manifest::parse(text).unwrap();
    assert_eq!(manifest.width, Some(1200));
}

fn row(seq: u64, author: u64, text: &str) -> MsgRow {
    MsgRow {
        channel_id: "general".into(),
        seq,
        message_id: format!("m{seq}"),
        height: 1,
        blocks: vec![chat::Block::paragraph(text)],
        text: text.into(),
        ..MsgRow::by(Principal::Account(author))
    }
}

fn marked_row(seq: u64) -> MsgRow {
    let text = format!("row {seq}: **bold**, `code` and a [link](https://x.example/{seq})");
    let even = seq.is_multiple_of(2);
    let mut row = row(seq, if even { 7 } else { 8 }, &text);
    row.blocks = chat::parse_message(&text);
    row.reply_count = seq % 3;
    row.reactions = vec![::chat::Reaction {
        emoji: "👍".into(),
        count: seq,
        reacted_by_me: even,
    }];
    row
}

// Who chose the 👍 on each of `seqs`, as a marked row wears it: eddy and
// ada.
fn reactors(seqs: Vec<u64>) -> Reply {
    let by = vec![Principal::Account(7), Principal::Account(10)];
    Reply::Reactors(
        seqs.into_iter()
            .map(|seq| chat::ReactedBy {
                seq,
                emoji: "👍".into(),
                by: by.clone(),
            })
            .collect(),
    )
}

/// The host methods chat only talks to, never hears back from here.
fn quiet_methods(cx: &mut TestAppContext) {
    cx.host().handle::<view::methods::StoreGet>(|_| Ok(None));
}

fn configure(cx: &mut TestAppContext) {
    quiet_methods(cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(|query| Ok(chat_reply(query)));
    // the one agent, eddy's own, answers only him
    cx.host().handle::<Ask<access::Access>>(|query| {
        Ok(match query {
            access::Query::List { program, key }
                if program == harness::PROGRAM && key == harness::agent_key(8) =>
            {
                access::Reply::List(None)
            }
            query => panic!("unexpected access query: {query:?}"),
        })
    });
    cx.host()
        .handle::<Submit<::chat::Chat>>(|_| Ok(view::testing::applied(Vec::new())));
}

/// What the fake chat program answers: three rooms, two rows in `general`
/// and a roster of eddy, his agent, forge and ada.
fn chat_reply(query: Query) -> Reply {
    match query {
        Query::Accounts { .. } => Reply::Accounts(page(vec![
            person(7, "eddy"),
            // an agent eddy manages
            chat::Profile {
                kind: chat::Kind::Program {
                    script: None,
                    manager: 7,
                    category: chat::Category::Agent,
                    standing: chat::Standing::Active,
                },
                ..person(8, "reviewer")
            },
            // forge's own account
            chat::Profile {
                kind: chat::Kind::System("forge".into()),
                ..person(9, "forge")
            },
            // a person eddy has no room with yet
            person(10, "ada"),
        ])),
        Query::Channels { .. } => Reply::Channels(page(vec![
            channel("general", "General", 3),
            channel("dm-7-8", "dm", 1),
            channel("forge:web:3", "Review web#3", 2),
        ])),
        Query::Roots { channel_id, .. } => Reply::Roots(page(if channel_id == "general" {
            vec![row(1, 7, "hello"), row(2, 8, "**hi** there")]
        } else {
            Vec::new()
        })),
        Query::MessagesAround {
            channel_id, seq, ..
        } => Reply::Messages(vec![MsgRow {
            channel_id,
            ..row(seq, 8, "**hi** there")
        }]),
        Query::Members { .. } => Reply::Members(page(Vec::new())),
        Query::Thread { .. } => Reply::Thread {
            root: None,
            replies: page(Vec::new()),
        },
        Query::Search { text, .. } => {
            assert_eq!(text, "hello");
            Reply::Hits(MessageHits {
                // two channels, one seq: the dm's first line says it too
                hits: vec![
                    row(1, 7, "hello"),
                    MsgRow {
                        channel_id: "dm-7-8".into(),
                        ..row(1, 8, "hello")
                    },
                ],
                capped: false,
            })
        }
        Query::Reactors { seqs, .. } => reactors(seqs),
        // eddy chose nothing for any room
        Query::Listening { .. } => Reply::Listening(page(Vec::new())),
        Query::Notices { .. } => Reply::Notices(page(Vec::new())),
        query => panic!("unexpected chat query: {query:?}"),
    }
}

/// The reader the host seats: key `0102`, account 7, on testnet.
fn session() -> Session {
    Session {
        signer: "0102".into(),
        account: Some(7),
        connected: true,
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    }
}

/// The host seats [`session`] in the open view and shows its tab.
fn seat(cx: &mut TestAppContext) {
    cx.host().stream::<HostSession>().send(session());
    cx.host().stream::<HostVisible>().send(true);
    cx.run_until_parked();
}

/// The view `cx` holds, saved and restored on a host configured afresh.
fn restored(cx: &TestAppContext) -> (TestAppContext, Entity<Chat>) {
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    configure(&mut restored);
    let view = restored.restore::<Chat>(&bytes).unwrap();
    restored.run_until_parked();
    (restored, view)
}

/// The thread under message `root` opened beside the room, as the desktop
/// opens one: its reply field takes the keys.
fn thread(cx: &mut TestAppContext, view: &Entity<Chat>, root: u64) {
    cx.update(view, |chat, _, cx| {
        cx.notify();
        chat.reply_in_thread(root, cx);
    });
    cx.run_until_parked();
}

/// Message `seq`'s edit field opened in the timeline.
fn edit(cx: &mut TestAppContext, view: &Entity<Chat>, seq: u64) {
    cx.update(view, |chat, window, cx| {
        cx.notify();
        chat.open_menu(Pane::Timeline, seq, 0, Mode::Editing, window, cx);
    });
    cx.run_until_parked();
}

/// Boots, seats a reader, lists rooms and opens `general` with two rows.
fn opened() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    assert!(cx.has_text("Not connected"));
    seat(&mut cx);
    assert!(cx.has_text("General"));
    assert!(cx.has_text("No channel open"));
    assert!(cx.has_text("Channels"));
    assert!(
        !cx.has_text("Review web#3"),
        "a program's rooms stay out of the channel list"
    );
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.room.as_ref().unwrap().id, "general"));
    let Some(wire::Node::Container(view::wire::ContainerNode {
        style,
        interactivity: Some(interactivity),
        ..
    })) = cx.find("chat-sidebar-channel-general")
    else {
        panic!("channel row is a native container");
    };
    let style = &cx.styles()[*style];
    assert!(style.size.width.is_some(), "channel rows fill the sidebar");
    assert_eq!(interactivity.role, Some(view::Role::ListBoxOption));
    assert_eq!(interactivity.aria.selected, Some(true));
    assert!(
        interactivity.on_click.is_some(),
        "channel rows keep their route"
    );
    (cx, view)
}

/// A block at `height` that wrote `keys`, as `program.changes` carries it.
fn block(height: u64, keys: Vec<Vec<u8>>) -> Option<Change> {
    Some(Change { height, keys })
}

/// What a message posted in `channel` writes: its root and the channel's
/// head.
fn posted(channel: &str) -> Vec<Vec<u8>> {
    vec![
        chat::tables::ROOTS.key(&(channel.to_owned(), 0u64)),
        chat::tables::HEADS.key(&channel.to_owned()),
    ]
}

/// A block that changed nothing chat shows: a chat block that wrote a room
/// re-reads the channel list, the room and its roster, identity's block
/// re-reads the names, the same answers land, and the view draws nothing
/// for the items or the landings.
#[test]
fn a_block_whose_rereads_land_the_same_rows_draws_nothing() {
    let (mut cx, _) = opened();
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let identity = cx.host().stream::<Changes<Identity>>();
    assert!(cx.has_text("hello"), "{:?}", cx.texts());
    let (asked, renders) = (
        cx.host().requests::<Ask<::chat::Chat>>().len(),
        cx.renders(),
    );
    let mut keys = posted("general");
    keys.push(chat::tables::MEMBERS.key(&("general".to_owned(), Principal::Account(9))));
    changes.send(block(4, keys));
    cx.run_until_parked();
    let reread = cx.host().requests::<Ask<::chat::Chat>>().len();
    assert!(
        reread >= asked + 3,
        "the list, the rows and the roster: {reread}"
    );
    assert_eq!(cx.renders(), renders, "the same rows drew nothing");
    identity.send(block(5, Vec::new()));
    cx.run_until_parked();
    assert!(
        cx.host().requests::<Ask<::chat::Chat>>().len() > reread,
        "the names re-read"
    );
    assert_eq!(cx.renders(), renders, "the same names drew nothing");
}

/// A block re-reads only what it wrote to, as the program declares it: a
/// member seated re-reads the room's roster and nothing else, a message
/// posted re-reads the rows and the channel list (its head moved) and not
/// the roster, and a reopened link (`None`) re-reads all three.
#[test]
fn a_block_re_reads_only_the_tables_it_wrote_to() {
    let (mut cx, _) = opened();
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let counts = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Ask<::chat::Chat>>();
        let count = |pick: fn(&Query) -> bool| asked.iter().filter(|query| pick(query)).count();
        (
            count(|query| matches!(query, Query::Channels { .. })),
            count(|query| matches!(query, Query::Roots { .. })),
            count(|query| matches!(query, Query::Members { .. })),
        )
    };
    let (list, rows, roster) = counts(&cx);
    let seated = chat::tables::MEMBERS.key(&("general".to_owned(), Principal::Account(9)));
    changes.send(block(4, vec![seated]));
    cx.run_until_parked();
    assert_eq!(counts(&cx), (list, rows, roster + 1), "a member seated");
    changes.send(block(5, posted("general")));
    cx.run_until_parked();
    assert_eq!(
        counts(&cx),
        (list + 1, rows + 1, roster + 1),
        "a message posted"
    );
    changes.send(None);
    cx.run_until_parked();
    assert_eq!(
        counts(&cx),
        (list + 2, rows + 2, roster + 2),
        "a reopened link"
    );
}

/// `CHAT_SCREEN_EXPORT=1` writes the opened room's frame for the app's
/// node-less renderer (`ducktape-app --render-tree`), light and dark.
#[test]
fn export_chat_screens() {
    if std::env::var_os("CHAT_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/chat/fixtures");
    std::fs::create_dir_all(&out).unwrap();
    let (cx, _view) = opened();
    std::fs::write(
        out.join("room-light.json"),
        serde_json::to_vec(&cx.whole_frame()).unwrap(),
    )
    .unwrap();
}

/// A person's profile in the roster.
fn person(number: u64, name: &str) -> chat::Profile {
    chat::Profile {
        number,
        name: name.into(),
        kind: chat::Kind::User,
    }
}
