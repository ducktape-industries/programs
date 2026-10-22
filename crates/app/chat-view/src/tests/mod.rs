//! The view against a fake host: every method it asks is answered here, and
//! each test drives the frame the way a person would.
use super::*;
use chat::{ChannelInfo, MsgRow, Op, PostPolicy, Principal, Query, Reply};
use view::Loadable;
use view::testing::TestAppContext;
use view::wire;
use view::{Entity, StyleRefinement, Styled};

use identity::Identity;
use view::methods::Change;
use view::methods::Query as Ask;
use view::methods::{Changes, HostId, HostSession, HostVisible, Session, Submit};

mod agents;
mod answering;
mod archived;
mod asks;
mod attachments;
mod context_menus;
mod cost;
mod dms;
mod faces;
mod filters;
mod folders;
mod held;
mod history;
mod measure;
mod menus;
mod message;
mod notices;
mod order;
mod phone;
mod placeholders;
mod policies;
mod reader;
mod rich;
mod room;
mod rules;
mod scroll;
mod search;
mod typing;
mod widgets;
mod writing;

// a room many screens tall: the size the frame-cost guards were taken on
const ROOM_ROWS: usize = 256;

// The rows a pane `height` tall shows, counted from the row height as `Chat::screenful` does.
fn screenful(height: f32) -> u64 {
    (height / view::design::height::ROW as f32).ceil() as u64 + 1
}

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

#[test]
fn a_desk_window_opens_chat_at_its_width() {
    let text = std::str::from_utf8(&MANIFEST_SECTION).unwrap();
    let manifest = wire::manifest::Manifest::parse(text).unwrap();
    assert_eq!(manifest.width, Some(1200));
}

// A change's review room, as forge opens it: forge's own.
fn review_room() -> ChannelInfo {
    let mut room = channel("forge:web:3", "Review web#3", 2);
    room.channel.owner = Principal::Account(9);
    room
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

fn parsed_row(seq: u64, author: u64, text: &str) -> MsgRow {
    MsgRow {
        blocks: chat::parse_message(text),
        ..row(seq, author, text)
    }
}

fn notice_row(seq: u64, author: u64) -> MsgRow {
    let notice = chat::Notice {
        kind: "pod".into(),
        group: "builds-1".into(),
        subject: "pod builds-1".into(),
        link: None,
        status: chat::message::Status::Failed,
        detail: "exit code 1".into(),
        concerns: Vec::new(),
    };
    MsgRow {
        blocks: vec![chat::Block::Notice(notice)],
        ..row(seq, author, "")
    }
}

fn with_menu_on(cx: &mut TestAppContext, view: &Entity<Chat>, row: MsgRow) {
    let seq = row.seq;
    cx.update(view, |chat, _, cx| {
        let room = chat.room.as_mut().expect("a room is open");
        room.messages
            .ready_mut()
            .expect("its rows are read")
            .push(row);
        chat.menu = Some(Menu {
            pane: Pane::Timeline,
            seq,
            rev: 0,
            mode: Mode::More,
            at: (611., 455.),
        });
        cx.notify();
    });
    cx.run_until_parked();
}

fn marked_row(seq: u64) -> MsgRow {
    let text = format!("row {seq}: **bold**, `code` and a [link](https://x.example/{seq})");
    let even = seq.is_multiple_of(2);
    let mut row = parsed_row(seq, if even { 7 } else { 8 }, &text);
    row.reply_count = seq % 3;
    row.reactions = vec![::chat::Reaction {
        emoji: "👍".into(),
        count: seq,
        reacted_by_me: even,
    }];
    row
}

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

fn quiet_methods(cx: &mut TestAppContext) {
    cx.host().handle::<Ask<Identity>>(|query| match query {
        identity::Query::List { .. } => Ok(no_cards()),
        identity::Query::Narrowed { .. } => Ok(none_narrowed()),
        query => panic!("unexpected identity query: {query:?}"),
    });
}

fn none_narrowed() -> identity::Reply {
    identity::Reply::Narrowed(identity::PageResponse {
        height: 1,
        items: Vec::new(),
        next: None,
    })
}

fn no_cards() -> identity::Reply {
    identity::Reply::Accounts(identity::PageResponse {
        height: 1,
        items: Vec::new(),
        next: None,
    })
}

fn configure(cx: &mut TestAppContext) {
    quiet_methods(cx);
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(|query| Ok(chat_reply(query))));
    cx.host().handle::<Ask<ownership::Ownership>>(|query| {
        Ok(match query {
            ownership::Query::Record { .. } => ownership::Reply::Record(None),
            ownership::Query::Effective { .. } => ownership::Reply::Policy(None),
            query => panic!("unexpected ownership query: {query:?}"),
        })
    });
    cx.host()
        .handle::<Submit<::chat::Chat>>(|_| Ok(view::testing::applied(Vec::new())));
}

fn a_policy(id: u64, owner: u64, name: &str, write: ownership::Writers) -> ownership::Policy {
    ownership::Policy {
        id,
        owner: ownership::Party::Account(owner),
        name: name.into(),
        write,
        retired: false,
        switched_to: None,
    }
}

fn owned(
    cx: &TestAppContext,
    key: Vec<u8>,
    owner: u64,
    policy: ownership::Policy,
    policies: Vec<ownership::Policy>,
) {
    let record = ownership::Record {
        owner: ownership::Party::Account(owner),
        policy: policy.id,
        kind: None,
    };
    cx.host().handle::<Ask<ownership::Ownership>>(move |query| {
        Ok(match query {
            ownership::Query::Record { key: asked, .. } => {
                ownership::Reply::Record((asked == key).then(|| record.clone()))
            }
            ownership::Query::Effective { key: asked, .. } => {
                ownership::Reply::Policy((asked == key).then(|| policy.clone()))
            }
            ownership::Query::Policies { .. } => ownership::Reply::Policies(page(policies.clone())),
            ownership::Query::Defaults { .. } => ownership::Reply::Defaults(page(Vec::new())),
            ownership::Query::OnlyMe { .. } => ownership::Reply::OnlyMe(None),
            ownership::Query::Open { .. } => {
                ownership::Reply::Open(policies.first().map(|policy| policy.id))
            }
            query => panic!("unexpected ownership query: {query:?}"),
        })
    });
    cx.host().handle::<Ask<Identity>>(|query| {
        Ok(match query {
            identity::Query::Profile { number } => {
                let name = match number {
                    7 => "eddy",
                    8 => "reviewer",
                    9 => "forge",
                    _ => "ada",
                };
                identity::Reply::Profile(Some(identity::Profile {
                    number,
                    name: name.into(),
                    kind: identity::Kind::User,
                }))
            }
            identity::Query::List { .. } => no_cards(),
            identity::Query::Narrowed { .. } => none_narrowed(),
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
    cx.host()
        .handle::<Submit<ownership::Ownership>>(|_| Ok(view::testing::applied(Vec::new())));
}

fn rooms_alone(
    mut answer: impl FnMut(Query) -> Result<Reply, view::host::Error> + 'static,
) -> impl FnMut(Query) -> Result<Reply, view::host::Error> + 'static {
    move |query| match query {
        Query::Channel { channel_id } => {
            let every = Query::Channels {
                page: chat::PageRequest::default(),
            };
            let Reply::Channels(rooms) = answer(every)? else {
                unreachable!("Channels answers Channels")
            };
            let room = rooms
                .items
                .into_iter()
                .find(|room| room.channel.id == channel_id);
            Ok(Reply::Channel(room))
        }
        // nobody is answering in a room unless a test says so
        Query::Drafts { .. } => Ok(Reply::Drafts(Vec::new())),
        Query::Busy { .. } => Ok(Reply::Busy(None)),
        Query::Foreigners { .. } => Ok(Reply::Foreigners(page(Vec::new()))),
        query => answer(query),
    }
}

fn chat_reply(query: Query) -> Reply {
    match query {
        Query::Accounts { .. } => Reply::Accounts(page(vec![
            person(7, "eddy"),
            chat::Profile {
                kind: chat::Kind::Program {
                    script: None,
                    manager: 7,
                    category: chat::Category::Agent,
                    standing: chat::Standing::Active,
                },
                ..person(8, "reviewer")
            },
            chat::Profile {
                kind: chat::Kind::System("forge".into()),
                ..person(9, "forge")
            },
            person(10, "ada"),
        ])),
        Query::Channels { .. } => Reply::Channels(page(vec![
            channel("general", "General", 3),
            channel("dm-7-8", "dm", 1),
            review_room(),
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
        Query::TextSearch { text, .. } => {
            assert_eq!(text, "hello");
            Reply::TextHits(page(vec![
                row(1, 7, "hello"),
                MsgRow {
                    channel_id: "dm-7-8".into(),
                    ..row(1, 8, "hello")
                },
            ]))
        }
        Query::Reactors { seqs, .. } => reactors(seqs),
        Query::Repliers { roots, .. } => {
            Reply::Repliers(roots.into_iter().map(|root| (root, Vec::new())).collect())
        }
        Query::Listening { .. } => Reply::Listening(page(Vec::new())),
        Query::Notices { .. } => Reply::Notices(page(Vec::new())),
        Query::PostedIn { .. } => Reply::PostedIn(page(Vec::new())),
        Query::Seated { .. } => Reply::Seated(Vec::new()),
        Query::ChosenBy { accounts, .. } => Reply::ChosenBy(
            accounts
                .into_iter()
                .map(|account| chat::AccountChoice {
                    account,
                    listening: false,
                    notices: None,
                })
                .collect(),
        ),
        query => panic!("unexpected chat query: {query:?}"),
    }
}

fn threaded(cx: &TestAppContext, head: u64, thread: impl Fn(u64) -> Option<u64> + 'static) {
    let message = move |seq: u64| MsgRow {
        thread: thread(seq),
        height: seq,
        ..row(seq, 7 + seq % 2, &format!("row {seq}"))
    };
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(Vec::new())),
                Query::Channels { .. } => {
                    Reply::Channels(page(vec![channel("general", "General", head)]))
                }
                Query::MessagesAround { seq, page, .. } => {
                    let half = page.limit() / 2;
                    let lo = seq.saturating_sub(half).max(1);
                    let hi = seq.saturating_add(half).min(head);
                    Reply::Messages((lo..=hi).map(&message).collect())
                }
                Query::Roots { .. } => Reply::Roots(::chat::PageResponse {
                    height: 1,
                    items: (1..=head)
                        .map(&message)
                        .filter(|row| row.thread.is_none())
                        .collect(),
                    next: None,
                }),
                Query::Thread { root_seq, page, .. } => {
                    let after = page.after.as_deref().map_or(root_seq, cursor_seq);
                    let mut replies = (after + 1..=head)
                        .map(&message)
                        .filter(|row| row.thread == Some(root_seq));
                    let items: Vec<MsgRow> = replies.by_ref().take(page.limit() as usize).collect();
                    let more = replies.next().is_some();
                    let next = items.last().filter(|_| more);
                    let next = next.map(|row| chat::replies_after("general", root_seq, row.seq));
                    Reply::Thread {
                        root: Some(message(root_seq)),
                        replies: ::chat::PageResponse {
                            height: 1,
                            items,
                            next,
                        },
                    }
                }
                Query::Members { .. } => Reply::Members(page(Vec::new())),
                query => chat_reply(query),
            })
        }));
}

fn cursor_seq(after: &[u8]) -> u64 {
    let tail: [u8; 8] = after[after.len() - 8..].try_into().expect("a chat cursor");
    u64::from_be_bytes(tail)
}

fn reach_first(cx: &mut TestAppContext, view: &Entity<Chat>) {
    cx.simulate_range("chat-message-list", 0..10);
    let sensor = view.read(|chat| crate::listing::first_row(chat, Pane::Timeline));
    cx.simulate_bounds(&sensor, (0., 0.), (400., 26.));
    cx.run_until_parked();
}

fn scrolled_to(view: &Entity<Chat>, pane: Pane) -> Option<usize> {
    view.read(|chat| {
        let drawn = chat.drawn(pane).borrow();
        let state = drawn
            .list
            .as_ref()
            .map(|(_, state)| state.logical_scroll_top());
        state.map(|top| top.item_ix)
    })
}

fn session() -> Session {
    Session {
        signer: "0102".into(),
        account: Some(7),
        connected: true,
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    }
}

fn seat(cx: &mut TestAppContext) {
    cx.host().stream::<HostSession>().send(session());
    cx.host().stream::<HostVisible>().send(true);
    cx.run_until_parked();
}

fn restored(cx: &TestAppContext) -> (TestAppContext, Entity<Chat>) {
    let bytes = cx.snapshot().unwrap();
    let mut restored = TestAppContext::new();
    configure(&mut restored);
    let view = restored.restore::<Chat>(&bytes).unwrap();
    restored.run_until_parked();
    (restored, view)
}

fn thread(cx: &mut TestAppContext, view: &Entity<Chat>, root: u64) {
    cx.update(view, |chat, _, cx| {
        cx.notify();
        chat.reply_in_thread(root, cx);
    });
    cx.run_until_parked();
}

fn edit(cx: &mut TestAppContext, view: &Entity<Chat>, seq: u64) {
    cx.update(view, |chat, window, cx| {
        cx.notify();
        chat.open_menu(Pane::Timeline, seq, 0, Mode::Editing, window, cx);
    });
    cx.run_until_parked();
}

fn opened() -> (TestAppContext, Entity<Chat>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let view = cx.open::<Chat>();
    cx.run_until_parked();
    assert!(
        !cx.has_text("Not connected"),
        "a session not handed yet is no verdict"
    );
    seat(&mut cx);
    assert!(cx.has_text("General"));
    assert!(cx.has_text("No channel open"));
    assert!(cx.has_text("Channels"));
    assert!(
        cx.has_text("Review web#3"),
        "a program's room is listed as any room is"
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

fn block(height: u64, keys: Vec<Vec<u8>>) -> Option<Change> {
    Some(Change { height, keys })
}

fn posted(channel: &str) -> Vec<Vec<u8>> {
    vec![
        chat::tables::ROOTS.key(&(channel.to_owned(), 0u64)),
        chat::tables::HEADS.key(&channel.to_owned()),
    ]
}

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

#[test]
fn a_block_re_reads_only_the_tables_it_wrote_to() {
    let (mut cx, _) = opened();
    let changes = cx.host().stream::<Changes<::chat::Chat>>();
    let counts = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Ask<::chat::Chat>>();
        let count = |pick: fn(&Query) -> bool| asked.iter().filter(|query| pick(query)).count();
        (
            count(|query| matches!(query, Query::Channels { .. })),
            count(|query| matches!(query, Query::Channel { .. })),
            count(|query| matches!(query, Query::Roots { .. })),
            count(|query| matches!(query, Query::Members { .. })),
        )
    };
    let (list, room, rows, roster) = counts(&cx);
    let seated = chat::tables::MEMBERS.key(&("general".to_owned(), Principal::Account(9)));
    changes.send(block(4, vec![seated]));
    cx.run_until_parked();
    assert_eq!(
        counts(&cx),
        (list, room, rows, roster + 1),
        "a member seated"
    );
    changes.send(block(5, posted("general")));
    cx.run_until_parked();
    assert_eq!(
        counts(&cx),
        (list, room + 1, rows + 1, roster + 1),
        "a message posted"
    );
    let asked = cx.host().requests::<Ask<::chat::Chat>>();
    let general = Query::Channel {
        channel_id: "general".into(),
    };
    assert!(asked.contains(&general), "{asked:?}");
    changes.send(None);
    cx.run_until_parked();
    assert_eq!(
        counts(&cx),
        (list + 1, room + 1, rows + 2, roster + 2),
        "a reopened link"
    );
}

#[test]
fn export_chat_screens() {
    let Some(out) = ::view::testing::screens("CHAT_SCREEN_EXPORT", "chat/fixtures") else {
        return;
    };
    let (cx, _view) = opened();
    std::fs::write(
        out.join("room-light.json"),
        serde_json::to_vec(&cx.whole_frame()).unwrap(),
    )
    .unwrap();
    let (width, height) = cx.viewport();
    let listed = serde_json::json!([{ "name": "room-light", "width": width as u32, "height": height as u32, "theme": "light" }]);
    std::fs::write(
        out.join("room-light-manifest.json"),
        serde_json::to_vec_pretty(&listed).unwrap(),
    )
    .unwrap();
}

impl crate::room_list::RoomList<'_> {
    fn ids(&self) -> Vec<String> {
        let channels = self.channels.iter().map(|info| info.channel.id.clone());
        channels
            .chain(self.dms.iter().map(|dm| dm.info.channel.id.clone()))
            .collect()
    }
}

fn person(number: u64, name: &str) -> chat::Profile {
    chat::Profile {
        number,
        name: name.into(),
        kind: chat::Kind::User,
    }
}

// j-131 message 15's table as it was typed, under a heading, then a fenced
// block: what the markdown screens show.
const TYPED_TABLE: &str = "## First macOS table
| footprint share (MiB) | idle | chat + 5 rooms | Δ |
|---|---|---|---|
| **total** (phys_footprint, matches Activity Monitor) | 104.8 | 151.3 | +46.5 |
| heap, dirty pages of the malloc regions | 30.9 | 60.6 | +29.7 |
| ↳ of which the allocator says is in use | 24.3 | 35.7 | +11.4 |
| GPU (OS regions: IOAccelerator + IOSurface), dirty | 54.0 | 63.5 | +9.5 |
| untagged anonymous (wasm memory), dirty | 1.4 | 8.4 | +7.0 |
| ↳ chat/1 view, linear memory resident | — | 7.2 | |
| notifiers | 0.5 | 0.5 | |
| pictures + rasters | 0.0 | 0.8 | |
The fix is one line, `#[cfg(all(test, target_os = \"linux\"))]`:
```rust
#[cfg(all(test, target_os = \"linux\"))]
fn with_center(x: u32) -> u32 { x * 2 } // dead on macOS
```";
