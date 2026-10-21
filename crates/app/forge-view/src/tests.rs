//! Every screen of this view, replayed from the forge program's own bytes.
//!
//! `crates/app/forge/fixtures/replies.bin` holds the program's real `Respond`
//! bytes; nothing here builds a reply by hand. The fake host decodes one and
//! hands it back, so an unhandled ask is a panic and a screen that reads a
//! field the program does not send cannot compile.
use super::*;
use crate::api::{Ask, SubmitForge};
use crate::state::{ChangeTab, Filter, RepoTab};
use forge::{ChangeFilter, ChangeState, Op, PageRequest, PageResponse, Query, Reply};
use view::methods::{HostId, HostSession, HostVisible, Session};
use view::methods::{Query as ProgramQuery, Submit};
use view::testing::{StreamSender, TestAppContext};
use view::{Entity, Theme, wire};

use identity::{Identity, Ownership, Party};
use view::methods::{Change, Changes};

#[path = "../../forge/fixtures/loader.rs"]
mod loader;

pub(crate) fn bytes(name: &str) -> Vec<u8> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../forge/fixtures");
    loader::bytes(&dir, name)
}

fn reply(name: &str) -> Reply {
    borsh::from_slice(&bytes(name)).unwrap_or_else(|error| panic!("decode {name}: {error}"))
}

fn refusal(name: &str) -> view::host::Error {
    let refusal: abi::Refusal =
        borsh::from_slice(&bytes(name)).unwrap_or_else(|error| panic!("decode {name}: {error}"));
    view::host::Error::new(&refusal.reason, &refusal.sentence)
}

fn change_fixture(mode: &str) -> &'static str {
    match mode {
        "reviewed" | "job" | "job-notices" => "change-reviewed",
        "merged" => "change-merged",
        "closed" => "change-closed",
        "outdated" => "change-outdated",
        _ => "change",
    }
}

fn answer(query: &Query, mode: &str) -> Reply {
    match query {
        Query::Repos { .. } if mode == "empty" => reply("repos-empty"),
        Query::Repos { .. } => reply("repos"),
        Query::Repo { .. } => reply("repo"),
        Query::Archived { .. } if mode == "archived" => reply("archived"),
        Query::Archived { .. } => reply("unarchived"),
        Query::Refs {
            page: PageRequest { after: None, .. },
            ..
        } if mode == "unborn" => reply("refs-empty"),
        Query::Refs {
            page: PageRequest { after: None, .. },
            ..
        } if mode == "many-refs" => many_refs(),
        Query::Refs {
            page: PageRequest { after: None, .. },
            ..
        } => reply("refs"),
        Query::Refs { .. } => reply("refs-next"),
        Query::Activity { .. } => reply("activity"),
        Query::Tree {
            page: PageRequest { after: None, .. },
            path,
            ..
        } if path.is_empty() => reply("tree"),
        Query::Tree { path, .. } if path.is_empty() => reply("tree-next"),
        Query::Tree { .. } => reply("tree-directory"),
        Query::Blob { oid, .. } => match oid.as_str() {
            "95d586e774a04676a07a142f0e2f97a4f32562cb" => reply("blob-binary"),
            "b90a09e55e43808905fe881245853c1b35b3fb82" => reply("blob-oversize"),
            _ => reply("blob"),
        },
        Query::Log {
            page: PageRequest { after: None, .. },
            ..
        } => reply("log"),
        Query::Log { .. } => reply("log-next"),
        Query::Diff { .. } if mode == "release" => release_diff(100, 1000),
        Query::Diff { .. } if mode == "wide" => release_diff(264, 3),
        Query::Diff { .. } => reply("diff-text"),
        Query::Compare { .. } if mode == "diverged" => reply("compare-diverged"),
        Query::Compare { .. } => reply("compare"),
        Query::Compares { .. } => reply("compares"),
        Query::Changes {
            filter:
                ChangeFilter {
                    state: Some(ChangeState::Closed),
                    ..
                },
            ..
        } => reply("changes-filtered"),
        Query::Changes { .. } if mode == "many-changes" => many_changes(),
        Query::Changes { .. } if mode == "empty" => reply("changes-empty"),
        Query::Changes { .. } => reply("changes"),
        Query::Change {
            page: PageRequest { after: None, .. },
            ..
        } => in_job_room(mode, moved_target(mode, reply(change_fixture(mode)))),
        Query::Change { .. } => in_job_room(mode, reply("change-reviews-next")),
        Query::Judgment { .. } if mode == "judgment-empty" => reply("judgment-empty"),
        Query::Judgment { .. } => reply("judgment"),
        Query::Preview { .. } if mode == "unpreviewed" => reply("preview-none"),
        Query::Preview { .. } => reply("preview"),
        Query::Statuses { commit, .. } if *commit == previewed_merge() => reply("statuses-merge"),
        Query::Statuses { .. } => reply("statuses"),
        other => panic!("no fixture for {other:?}"),
    }
}

// In mode `job`, change 1 as forge holds it once opened from a `j-7/…` branch while chat held
// room `j-7` open: it talks there.
fn in_job_room(mode: &str, mut read: Reply) -> Reply {
    let room = match mode {
        "job" => "j-7",
        "job-notices" => "j-8",
        _ => return read,
    };
    if let Reply::Change { change, .. } = &mut read {
        change.channel = room.into();
    }
    read
}

fn moved_target(mode: &str, mut read: Reply) -> Reply {
    if mode != "target-moved" {
        return read;
    }
    if let Reply::Change { target_head, .. } = &mut read {
        *target_head = Some("f".repeat(40));
    }
    read
}

fn previewed_merge() -> String {
    match reply("preview") {
        Reply::Preview {
            preview: Some(made),
            ..
        } => made.merge,
        other => panic!("the preview fixture holds one: {other:?}"),
    }
}

/// The changes of a repository with a long past: the first one listed,
/// 300 times over, numbered 1 to 300.
fn many_changes() -> Reply {
    let mut changes = reply("changes");
    if let Reply::Changes { page, .. } = &mut changes {
        let first = page.items[0].clone();
        page.items = (1..=300)
            .map(|n| forge::ChangeSummary {
                n,
                title: format!("{} ({n})", first.title),
                ..first.clone()
            })
            .collect();
        page.next = None;
    }
    changes
}

fn many_refs() -> Reply {
    let mut refs = reply("refs");
    if let Reply::Refs { page, .. } = &mut refs {
        page.items.extend((0..120).map(|n| forge::RefInfo {
            name: format!("refs/heads/feature/many-{n:03}").into_bytes(),
            target: format!("{n:040x}"),
        }));
    }
    refs
}

fn release_diff(files: u64, lines: u64) -> Reply {
    let files: Vec<forge::FileDiff> = (0..files)
        .map(|n| {
            let rows = (0..lines)
                .map(|at| forge::DiffLine {
                    kind: forge::LineKind::Added,
                    old_line: None,
                    new_line: Some(at + 1),
                    bytes: format!("pub fn f{n}_{at}(x: u64) -> u64 {{ x ^ {at} }}\n").into_bytes(),
                })
                .collect();
            forge::FileDiff {
                old_path: None,
                new_path: Some(format!("src/gen/file_{n:03}.rs").into_bytes()),
                status: forge::FileStatus::Added,
                old_oid: None,
                new_oid: Some(format!("{n:040x}")),
                old_kind: None,
                new_kind: Some(forge::EntryKind::File),
                old_size: 0,
                new_size: 40_000,
                content: forge::Content::Text,
                additions: lines,
                deletions: 0,
                hunks: vec![forge::DiffHunk {
                    old: forge::LineRange { start: 0, count: 0 },
                    new: forge::LineRange {
                        start: 1,
                        count: lines,
                    },
                    lines: rows,
                }],
            }
        })
        .collect();
    Reply::Diff {
        height: 1,
        base: Some("base".into()),
        head: "head".into(),
        total_files: files.len() as u64,
        page: forge::PageResponse {
            height: 1,
            items: files,
            next: None,
        },
    }
}

const FORGE: u64 = 900;

fn accounts() -> chat::PageResponse<chat::Profile> {
    let row = |number, name: &str| chat::Profile {
        number,
        name: name.into(),
        kind: chat::Kind::User,
    };
    let forge = chat::Profile {
        kind: chat::Kind::System(forge::PROGRAM.into()),
        ..row(FORGE, forge::PROGRAM)
    };
    chat::PageResponse {
        height: 1,
        next: None,
        items: vec![
            row(1, "Ada"),
            row(2, "Rae"),
            row(4, "Tal"),
            row(9, "Wren"),
            forge,
        ],
    }
}

fn message(seq: u64, author: forge::Principal, text: &str) -> chat::MsgRow {
    chat::MsgRow {
        channel_id: "forge:project:1".into(),
        seq,
        message_id: format!("m{seq}"),
        blocks: vec![chat::Block::paragraph(text)],
        text: text.into(),
        ..chat::MsgRow::by(author)
    }
}

fn forge_lines() -> Vec<chat::MsgRow> {
    let reviews = ["change-reviewed", "change-reviews-next"]
        .into_iter()
        .flat_map(|name| match reply(name) {
            Reply::Change { reviews, .. } => reviews.items,
            _ => panic!("{name} is a change"),
        });
    let forge_line = |seq: u64, message_id: String| chat::MsgRow {
        message_id,
        ..message(seq, forge::Principal::Account(FORGE), "raw forge text")
    };
    let mut rows = vec![forge_line(1, "forge:0000000000000001".into())];
    for review in reviews {
        rows.push(forge_line(rows.len() as u64 + 1, review.message_id));
    }
    rows.push(message(
        rows.len() as u64 + 1,
        forge::Principal::Account(2),
        "Reading it now",
    ));
    rows.push(forge_line(
        rows.len() as u64 + 1,
        "forge:00000000000000ff".into(),
    ));
    rows
}

// Room `j-7`, where changes 1 and 2 of `project` both talk: someone's word to the room, then
// forge's lines, each a `forge` block ending `<repo> <n>`: change 1's opening (answered once,
// in its thread), change 2's, and change 1's reviews.
fn job_room() -> Vec<chat::MsgRow> {
    let reviews = ["change-reviewed", "change-reviews-next"]
        .into_iter()
        .flat_map(|name| match reply(name) {
            Reply::Change { reviews, .. } => reviews.items,
            _ => panic!("{name} is a change"),
        });
    let line = |seq: u64, message_id: String, code: String| chat::MsgRow {
        channel_id: "j-7".into(),
        message_id,
        blocks: vec![chat::Block::Code {
            lang: Some(forge::PROGRAM.into()),
            text: code,
        }],
        ..message(seq, forge::Principal::Account(FORGE), "")
    };
    let mut rows = vec![
        chat::MsgRow {
            channel_id: "j-7".into(),
            ..message(1, forge::Principal::Account(9), "Morning, all")
        },
        chat::MsgRow {
            reply_count: 1,
            ..line(
                2,
                "forge:0000000000000001".into(),
                "opened project 1".into(),
            )
        },
        line(
            3,
            "forge:0000000000000002".into(),
            "opened project 2".into(),
        ),
    ];
    for review in reviews {
        let code = format!("review {} project 1", review.id);
        rows.push(line(rows.len() as u64 + 1, review.message_id, code));
    }
    rows
}

fn job_room_notices() -> Vec<chat::MsgRow> {
    job_room()
        .into_iter()
        .map(|row| {
            let blocks = match row.blocks.as_slice() {
                [chat::Block::Code { text, .. }] => vec![chat::Block::Notice(chat::Notice {
                    kind: "change".into(),
                    group: text.clone(),
                    subject: "project #1: Teach a".into(),
                    link: None,
                    status: chat::message::Status::Note,
                    detail: String::new(),
                    concerns: Vec::new(),
                })],
                _ => row.blocks.clone(),
            };
            chat::MsgRow {
                channel_id: "j-8".into(),
                blocks,
                ..row
            }
        })
        .collect()
}

/// Chat's answers: the roster, and each change channel's rows.
fn chat_answer(query: chat::Query) -> chat::Reply {
    match query {
        chat::Query::Accounts { .. } => chat::Reply::Accounts(accounts()),
        chat::Query::Roots { channel_id, .. } => chat::Reply::Roots(PageResponse {
            height: 1,
            items: match channel_id.as_str() {
                "forge:project:1" => forge_lines(),
                "forge:project:2" => (1..=2)
                    .map(|seq| chat::MsgRow {
                        channel_id: channel_id.clone(),
                        message_id: format!("forge:{seq:016x}"),
                        ..message(seq, forge::Principal::Account(FORGE), "raw forge text")
                    })
                    .collect(),
                "j-7" => job_room(),
                "j-8" => job_room_notices(),
                _ => Vec::new(),
            },
            next: None,
        }),
        chat::Query::Thread {
            channel_id,
            root_seq: 2,
            ..
        } if channel_id == "j-7" || channel_id == "j-8" => chat::Reply::Thread {
            root: None,
            replies: PageResponse {
                height: 1,
                items: vec![chat::MsgRow {
                    channel_id,
                    thread: Some(2),
                    ..message(7, forge::Principal::Account(2), "Reading it now")
                }],
                next: None,
            },
        },
        other => panic!("unexpected chat query: {other:?}"),
    }
}

fn access_answer(query: access::Query) -> access::Reply {
    match query {
        access::Query::List { program, key }
            if program == forge::PROGRAM && key == forge::repo_key("project") =>
        {
            access::Reply::List(Some(Ownership::Listed(vec![Party::Account(9)])))
        }
        access::Query::List { .. } => access::Reply::List(None),
        access::Query::Writers { key, .. } if key == forge::repo_key("project") => {
            access::Reply::Writers(program::Writers::Listed {
                at: key,
                owner: Some(Party::Account(1)),
                list: Ownership::Listed(vec![Party::Account(9)]),
            })
        }
        access::Query::Writers { .. } => access::Reply::Writers(program::Writers::Anyone),
        other => panic!("unexpected access query: {other:?}"),
    }
}

pub(crate) fn set_lists(cx: &TestAppContext) -> Vec<Option<Ownership>> {
    cx.host()
        .requests::<Submit<access::Access>>()
        .into_iter()
        .filter_map(|op| match op {
            access::Op::Set { program, key, list }
                if program == forge::PROGRAM && key == forge::repo_key("project") =>
            {
                Some(list)
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn listed(accounts: &[u64]) -> Option<Ownership> {
    Some(Ownership::Listed(
        accounts.iter().copied().map(Party::Account).collect(),
    ))
}

pub(crate) fn configure(cx: &mut TestAppContext, mode: &'static str) {
    cx.host()
        .handle::<Ask>(move |query| Ok(answer(&query, mode)));
    cx.host()
        .handle::<ProgramQuery<::chat::Chat>>(|query| Ok(chat_answer(query)));
    cx.host()
        .handle::<Submit<::chat::Chat>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.host()
        .handle::<SubmitForge>(|_| Ok(view::testing::applied(Vec::new())));
    cx.host()
        .handle::<ProgramQuery<access::Access>>(|query| Ok(access_answer(query)));
    cx.host()
        .handle::<Submit<access::Access>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.host()
        .handle::<ProgramQuery<ownership::Ownership>>(|query| {
            Ok(match query {
                ownership::Query::Record { .. } => ownership::Reply::Record(None),
                ownership::Query::Effective { key, .. } => {
                    ownership::Reply::Policy(project_writers(&key))
                }
                query => panic!("unexpected ownership query: {query:?}"),
            })
        });
    cx.host()
        .handle::<ProgramQuery<programs::Programs>>(|query| {
            Ok(match query {
                programs::Query::Roles => {
                    programs::Reply::Roles(abi::Roles::from_iter([(abi::role::ACCESS, "access")]))
                }
                query => panic!("unexpected programs query: {query:?}"),
            })
        });
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    // each card's picture: no account holds one
    cx.host().handle::<ProgramQuery<Identity>>(|query| {
        Ok(match query {
            identity::Query::Profile { .. } => identity::Reply::Profile(None),
            _ => no_accounts(),
        })
    });
}

fn project_writers(key: &[u8]) -> Option<ownership::Policy> {
    let project = forge::tables::CUSTODY.key(&"project".to_owned());
    (key == project.as_slice()).then(|| ownership::Policy {
        id: 2,
        owner: ownership::Party::Account(1),
        name: "project writers".into(),
        write: ownership::Writers::Listed(vec![ownership::Party::Account(9)]),
        retired: false,
        switched_to: None,
    })
}

// identity's accounts with none listed: no picture to draw.
pub(crate) fn no_accounts() -> identity::Reply {
    identity::Reply::Accounts(identity::PageResponse {
        height: 1,
        items: Vec::new(),
        next: None,
    })
}

fn seat(key: &[u8], account: Option<u64>) -> Session {
    Session {
        signer: abi::hex(key),
        account,
        connected: true,
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    }
}

pub(crate) fn booted(mode: &'static str) -> (TestAppContext, Entity<Forge>) {
    booted_as(mode, 2)
}

pub(crate) fn booted_as(mode: &'static str, account: u64) -> (TestAppContext, Entity<Forge>) {
    let mut cx = TestAppContext::new();
    cx.simulate_resize(1180., 760.);
    configure(&mut cx, mode);
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.send(seat(b"reviewer", Some(account)));
    cx.run_until_parked();
    (cx, view)
}

fn block(height: u64, keys: Vec<Vec<u8>>) -> Option<Change> {
    Some(Change { height, keys })
}

fn ref_key() -> Vec<u8> {
    forge::tables::REFS.key(&("project".to_owned(), b"refs/heads/main".to_vec()))
}

fn review_key() -> Vec<u8> {
    forge::tables::REVIEWS.key(&("project".to_owned(), 1u64, 1u64))
}

fn root_key(channel: &str) -> Vec<u8> {
    chat::tables::ROOTS.key(&(channel.to_owned(), 0u64))
}

fn member_key(channel: &str) -> Vec<u8> {
    chat::tables::MEMBERS.key(&(channel.to_owned(), forge::Principal::Account(2)))
}

fn answered_key(channel: &str) -> Vec<u8> {
    chat::tables::ANSWERED.key(&(channel.to_owned(), forge::Principal::Account(2), 0u64))
}

fn real_review_keys() -> Vec<Vec<u8>> {
    let (repo, n, id) = ("project".to_owned(), 1u64, 1u64);
    vec![
        review_key(),
        forge::tables::AUTHORED.key(&(repo.clone(), n, forge::Principal::Account(2), id)),
        forge::tables::CHANGES.key(&(repo.clone(), n)),
        forge::tables::MESSAGES.key(),
        forge::tables::REPOS.key(&repo),
        forge::tables::ACTIVITY.key(&(u64::MAX - 100, repo)),
        forge::tables::WRITES.key(),
    ]
}

fn posted_line_keys(channel: &str) -> Vec<Vec<u8>> {
    vec![
        chat::tables::MESSAGES.key(&(channel.to_owned(), 1u64)),
        root_key(channel),
        chat::tables::MESSAGE_IDS.key(&format!("{channel}:review:1")),
        chat::tables::HEADS.key(&channel.to_owned()),
    ]
}

struct Heads {
    forge: StreamSender<Changes<forge::Forge>>,
    chat: StreamSender<Changes<::chat::Chat>>,
    identity: StreamSender<Changes<Identity>>,
    visible: StreamSender<HostVisible>,
}

fn followed(mode: &'static str) -> (TestAppContext, Heads) {
    let mut cx = TestAppContext::new();
    configure(&mut cx, mode);
    let heads = Heads {
        forge: cx.host().stream(),
        chat: cx.host().stream(),
        identity: cx.host().stream(),
        visible: cx.host().stream(),
    };
    let props = cx.host().stream::<HostSession>();
    cx.open::<Forge>();
    props.send(seat(b"reviewer", Some(2)));
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    (cx, heads)
}

#[test]
fn a_block_whose_rereads_land_the_same_replies_draws_nothing() {
    let (mut cx, heads) = followed("default");
    let renders = cx.renders();
    let asked = cx.host().requests::<Ask>().len();
    heads.forge.send(block(100, vec![ref_key()]));
    cx.run_until_parked();
    assert!(
        cx.host().requests::<Ask>().len() > asked,
        "the refs re-read"
    );
    let asked = cx.host().requests::<Ask>().len();
    heads.chat.send(block(101, vec![root_key("general")]));
    heads.identity.send(block(102, Vec::new()));
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Ask>().len(),
        asked,
        "no forge read moved"
    );
    assert_eq!(cx.renders(), renders, "nothing drew");
}

// A refusal on screen stays, saying why and drawing nothing, through a block that wrote nothing
// it reads; the block that moves a ref it reads asks it again, and an answer replaces it.
#[test]
fn a_refused_read_is_asked_again_when_a_block_writes_what_it_reads() {
    let (mut cx, heads) = followed("default");
    cx.host().handle::<Ask>(|query| match query {
        Query::Log { .. } => Err(refusal("refused-not-found")),
        query => Ok(answer(&query, "default")),
    });
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    let sentence = refusal("refused-not-found").message;
    assert!(cx.has_text(&sentence), "{:?}", cx.texts());
    let logs = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Ask>();
        asked
            .iter()
            .filter(|query| matches!(query, Query::Log { .. }))
            .count()
    };
    let (renders, asked) = (cx.renders(), logs(&cx));
    cx.host()
        .handle::<Ask>(|query| Ok(answer(&query, "default")));
    heads.forge.send(block(100, vec![review_key()]));
    cx.run_until_parked();
    assert_eq!(logs(&cx), asked, "a review moves nothing the log reads");
    assert!(cx.has_text(&sentence));
    assert_eq!(cx.renders(), renders, "a refusal that holds drew nothing");
    heads.forge.send(block(101, vec![ref_key()]));
    cx.run_until_parked();
    assert!(!cx.has_text(&sentence), "{:?}", cx.texts());
    assert!(cx.has_text("Feature"), "{:?}", cx.texts());
}

// A change's conversation refused (the node was away) stays, drawing nothing, through a chat
// block that wrote nothing it reads; a block that posts in it asks it again, and the rows
// replace the refusal.
#[test]
fn a_refused_conversation_is_asked_again_when_a_block_posts_in_it() {
    let (mut cx, heads) = followed("default");
    cx.host()
        .handle::<ProgramQuery<::chat::Chat>>(|query| match query {
            chat::Query::Roots { .. } => Err(view::host::Error::new(
                "not_connected",
                "the node is not connected",
            )),
            query => Ok(chat_answer(query)),
        });
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert!(
        cx.find("forge-conversation-refused").is_some(),
        "{:?}",
        cx.texts()
    );
    let renders = cx.renders();
    cx.host()
        .handle::<ProgramQuery<::chat::Chat>>(|query| Ok(chat_answer(query)));
    heads
        .chat
        .send(block(100, vec![member_key("forge:project:1")]));
    cx.run_until_parked();
    assert!(cx.find("forge-conversation-refused").is_some());
    assert_eq!(cx.renders(), renders, "a refusal that holds drew nothing");
    heads
        .chat
        .send(block(101, vec![root_key("forge:project:1")]));
    cx.run_until_parked();
    assert!(
        cx.find("forge-conversation-refused").is_none(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Reading it now"), "{:?}", cx.texts());
}

#[test]
fn a_reader_change_shows_the_conversation_being_read_until_the_names_land() {
    use view::methods::Method;
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Forge>();
    props.send(seat(b"reviewer", Some(2)));
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert!(cx.has_text("Reading it now"), "{:?}", cx.texts());

    cx.host().never::<ProgramQuery<::chat::Chat>>();
    props.send(seat(b"another", Some(2)));
    let mut conversation = None;
    for _ in 0..50 {
        cx.tick(Vec::new());
        conversation = cx.last_frame().requests.iter().find_map(|request| {
            let asked = ProgramQuery::<::chat::Chat>::decode_request(&request.payload);
            matches!(asked, Ok(chat::Query::Roots { .. })).then_some(request.id)
        });
        if conversation.is_some() {
            break;
        }
    }
    let id = conversation.expect("the conversation is read again for the new reader");
    let rows = chat_answer(chat::Query::Roots {
        channel_id: "forge:project:1".into(),
        viewer: Vec::new(),
        page: chat::PageRequest::default(),
    });
    cx.tick(vec![wire::Event::Response {
        id,
        result: Ok(ProgramQuery::<::chat::Chat>::encode_reply(&rows)),
        done: true,
    }]);
    cx.run_until_parked();
    view.read(|forge| assert!(forge.names.is_loading(), "{:?}", forge.names));
    assert!(cx.has_text("Reading the conversation…"), "{:?}", cx.texts());
    assert!(!cx.has_text("Reading it now"), "{:?}", cx.texts());
}

#[test]
fn a_block_leaves_a_read_still_out_to_land() {
    let (mut cx, heads) = followed("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    let blocks = |cx: &mut TestAppContext| {
        for height in 100..103 {
            heads
                .chat
                .send(block(height, vec![root_key("forge:project:1")]));
            cx.run_until_parked();
        }
    };
    cx.host().never::<ProgramQuery<::chat::Chat>>();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    let conversations = |cx: &TestAppContext| {
        let asked = cx.host().requests::<ProgramQuery<::chat::Chat>>();
        asked
            .iter()
            .filter(|query| matches!(query, chat::Query::Roots { .. }))
            .count()
    };
    assert_eq!(conversations(&cx), 1);
    blocks(&mut cx);
    assert_eq!(
        conversations(&cx),
        1,
        "the conversation was not asked again"
    );
    cx.host().never::<Ask>();
    cx.simulate_click("forge-change-tab-files");
    cx.run_until_parked();
    let diffs = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Ask>();
        asked
            .iter()
            .filter(|query| matches!(query, Query::Diff { .. }))
            .count()
    };
    assert_eq!(diffs(&cx), 1);
    blocks(&mut cx);
    assert_eq!(diffs(&cx), 1, "the diff was not asked again");
}

pub(crate) fn opened(mode: &'static str) -> (TestAppContext, Entity<Forge>) {
    opened_as(mode, 2)
}

pub(crate) fn opened_as(mode: &'static str, account: u64) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = booted_as(mode, account);
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    (cx, view)
}

fn disabled(cx: &TestAppContext, id: &str) -> bool {
    let Some(wire::Node::Container(view::wire::ContainerNode {
        interactivity: Some(interactivity),
        ..
    })) = cx.find(id)
    else {
        panic!("{id} button")
    };
    interactivity.aria.disabled == Some(true) && interactivity.on_click.is_none()
}

fn seated(key: &[u8], account: Option<u64>) -> (TestAppContext, Entity<Forge>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "judgment");
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.send(seat(key, account));
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    (cx, view)
}

fn judged(cx: &TestAppContext) -> Vec<forge::Principal> {
    cx.host()
        .requests::<Ask>()
        .into_iter()
        .filter_map(|query| match query {
            Query::Judgment { principal, .. } => Some(principal),
            _ => None,
        })
        .collect()
}

#[test]
fn the_badge_is_a_dot_while_a_change_waits_on_my_judgment() {
    use view::methods::HostBadge;
    let (cx, _) = booted("default");
    assert_eq!(cx.host().requests::<HostBadge>().last(), Some(&1));
    let (mut cx, _) = booted("judgment-empty");
    assert_eq!(cx.host().requests::<HostBadge>().last(), Some(&0));
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<HostBadge>(), [0]);
    let (cx, _) = seated(b"stranger", None);
    assert_eq!(cx.host().requests::<HostBadge>().last(), Some(&0));
}

#[test]
fn a_key_without_an_account_writes_nothing() {
    let (cx, view) = seated(b"stranger", None);
    view.read(|forge| {
        assert_eq!(forge.my_account(), None);
        assert_eq!(forge.me_principal(), None);
        assert!(!forge.may_write());
    });
    assert!(disabled(&cx, "forge-filter-judgment"));
    assert!(disabled(&cx, "forge-filter-authored"));
    assert!(cx.find("forge-no-account").is_some());
    assert!(judged(&cx).is_empty());
}

#[test]
fn a_second_device_key_reads_as_the_same_person() {
    let (mut cx, view) = seated(b"tester-laptop", Some(1));
    cx.simulate_click("forge-filter-authored");
    cx.run_until_parked();
    let authored = cx.host().requests::<Ask>().into_iter().any(|query| {
        matches!(query, Query::Changes { filter, .. }
            if filter.author == Some(forge::Principal::Account(1)))
    });
    assert!(authored, "my changes are my account's");
    view.read(|forge| assert_eq!(forge.me_principal(), Some(forge::Principal::Account(1))));
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    let judged = judged(&cx);
    assert!(!judged.is_empty());
    assert!(
        judged
            .iter()
            .all(|who| *who == forge::Principal::Account(1)),
        "{judged:?}"
    );
}

#[test]
fn an_account_gained_later_is_who_forge_judges() {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "judgment");
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.send(seat(b"reviewer", None));
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    assert!(disabled(&cx, "forge-filter-judgment"));
    props.send(seat(b"reviewer", Some(2)));
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.my_account(), Some(2)));
    assert!(cx.find("forge-no-account").is_none());
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    assert_eq!(judged(&cx), [forge::Principal::Account(2)]);
}

#[test]
fn the_root_wears_the_shared_theme_and_is_accessible() {
    let (mut cx, _) = booted("default");
    let dark = Theme::dark();
    cx.set_global(dark);
    let style = cx.style("forge");
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
fn the_repositories_list_shows_every_column_of_the_plan() {
    let (mut cx, _) = booted("default");
    assert!(cx.has_text("Repositories"));
    assert!(cx.has_text("project"), "{:?}", cx.texts());
    assert!(cx.has_text("main"), "the default head is a badge");
    assert!(cx.has_text("Ada"), "the owner key resolves to a name");
    assert!(cx.has_text("6"), "the refs column");
    for column in ["Name", "Owner", "Default", "Refs", "Last active"] {
        assert!(cx.has_text(column), "{column}: {:?}", cx.texts());
    }
    assert!(cx.has_text("block 2"));
    assert!(
        cx.has_text("duck://testnet-0a1b2c3d/forge/project"),
        "the row shows where it clones from"
    );
    cx.simulate_click("forge-repo-project-activity");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        ["duck://explorer/block/2"]
    );
}

#[test]
fn an_empty_program_explains_how_a_repository_begins() {
    let (cx, _) = booted("empty");
    assert!(cx.has_text("No repositories yet"));
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("Create one with New repository"))
    );
}

#[test]
fn an_unborn_repo_says_so_instead_of_resolving_forever() {
    let (cx, _) = opened("unborn");
    assert!(cx.has_text("No commits yet"), "{:?}", cx.texts());
    assert!(!cx.has_text("Resolving the ref…"));
}

#[test]
fn an_unborn_repository_has_no_history_and_says_so_with_its_address() {
    let (mut cx, _) = opened("unborn");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    assert!(cx.has_text("No commits yet"), "{:?}", cx.texts());
    assert!(cx.find("forge-log-loading").is_none() && !cx.has_text("Retry"));
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("git push duck://testnet-0a1b2c3d/forge/project main")),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_refused_read_keeps_its_reason_and_offers_one_retry() {
    let mut cx = TestAppContext::new();
    cx.host()
        .handle::<Ask>(|_| Err(refusal("refused-object-not-held")));
    cx.host()
        .handle::<ProgramQuery<::chat::Chat>>(|_| Ok(chat::Reply::Accounts(accounts())));
    cx.host()
        .handle::<ProgramQuery<Identity>>(|_| Ok(no_accounts()));
    cx.open::<Forge>();
    cx.run_until_parked();
    let sentence = "object ffffffffffffffffffffffffffffffffffffffff is not held by this node";
    assert!(cx.has_text(sentence), "{:?}", cx.texts());
    let lists = |cx: &TestAppContext| {
        let asked = cx.host().requests::<Ask>();
        asked
            .iter()
            .filter(|query| matches!(query, Query::Repos { .. }))
            .count()
    };
    let asked = lists(&cx);
    cx.simulate_click("forge-repos-list-retry");
    cx.run_until_parked();
    assert_eq!(lists(&cx), asked + 1, "Retry asks the list again");
    assert!(cx.has_text(sentence), "and the reason stays while it holds");
}

#[test]
fn creating_a_repository_validates_its_name_then_shows_the_submission() {
    let (mut cx, view) = booted("default");
    cx.simulate_click("forge-new-repo");
    assert!(cx.has_text("New repository"));
    cx.simulate_input("forge-new-repo-name", "not a name");
    cx.simulate_click("forge-new-repo-submit");
    cx.run_until_parked();
    assert!(
        cx.has_text("A repository name is letters, digits, dot, dash or underscore"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().requests::<SubmitForge>().is_empty());
    cx.simulate_input("forge-new-repo-name", "ledger");
    cx.simulate_click("forge-new-repo-submit");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.new_repo.is_none()));
    cx.simulate_click("forge-new-repo");
    assert!(
        cx.has_text(
            "Pick SHA-1 to push an existing Git project. The format is fixed once created."
        )
    );
    cx.simulate_input("forge-new-repo-name", "imported");
    cx.simulate_click("forge-new-repo-sha1");
    cx.simulate_click("forge-new-repo-submit");
    cx.run_until_parked();
    let created: Vec<(String, abi::HashKind)> = cx
        .host()
        .requests::<SubmitForge>()
        .iter()
        .filter_map(|op| match op {
            Op::Create { repo, hash } => Some((repo.clone(), *hash)),
            _ => None,
        })
        .collect();
    assert_eq!(
        created,
        [
            ("ledger".to_owned(), abi::HashKind::Sha256),
            ("imported".to_owned(), abi::HashKind::Sha1),
        ]
    );
    view.read(|forge| assert!(forge.new_repo.is_none()));
}

fn focused(cx: &TestAppContext) -> Option<String> {
    cx.focused().and_then(wire::Node::key).map(str::to_owned)
}

#[test]
fn a_new_repository_form_opens_focused_and_enter_creates() {
    let (mut cx, view) = booted("default");
    cx.simulate_click("forge-new-repo");
    cx.run_until_parked();
    assert_eq!(focused(&cx).as_deref(), Some("forge-new-repo-name"));
    cx.simulate_input("forge-new-repo-name", "ledger");
    cx.simulate_submit("forge-new-repo-name");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.new_repo.is_none(), "the form closed"));
    assert!(
        cx.host()
            .requests::<SubmitForge>()
            .iter()
            .any(|op| matches!(op, forge::Op::Create { repo, .. } if repo == "ledger")),
        "Enter created it"
    );
}

#[test]
fn a_repository_opens_with_its_header_ref_picker_and_tabs() {
    let (cx, view) = opened("default");
    view.read(|forge| assert_eq!(forge.nav().repo.as_deref(), Some("project")));
    assert!(
        cx.has_text("owner Ada · 6 refs · active"),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("duck://testnet-0a1b2c3d/forge/project")),
        "{:?}",
        cx.texts()
    );
    for tab in RepoTab::ALL {
        assert!(
            cx.find(&format!("forge-tab-{}", tab.slug())).is_some(),
            "{} tab",
            tab.label()
        );
    }
    assert!(cx.find("forge-ref-refs/heads/clean").is_none());
    let mut cx = cx;
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    assert!(cx.find("forge-ref-refs/heads/clean").is_some());
    assert!(cx.find("forge-ref-refs/heads/conflict").is_some());
    assert!(cx.has_text("Branches 5"), "{:?}", cx.texts());
    assert!(cx.has_text("Tags 1"), "{:?}", cx.texts());
    cx.update(&view, |forge, _, cx| forge.open_menu(None, cx));
    cx.run_until_parked();
    assert!(cx.find("forge-ref-picker-menu").is_none());
}

#[test]
fn the_changes_tab_counts_on_every_tab() {
    let (cx, _) = opened("default");
    let tab = cx.node("forge-tab-changes");
    let texts: Vec<String> = tab
        .children()
        .iter()
        .flat_map(|child| child.children().iter().chain(std::iter::once(child)))
        .filter_map(|node| node.text().map(str::to_owned))
        .collect();
    assert!(
        texts.iter().any(|text| text.parse::<u64>().is_ok()),
        "the README tab shows the count: {texts:?}"
    );
}

#[test]
fn the_repository_tabs_move_on_an_arrow_and_open_on_enter() {
    let (mut cx, view) = opened("default");
    let list = cx.interactivity("forge-tab-list");
    assert_eq!(list.role, Some(view::Role::TabList));
    assert!(list.focusable && list.tab_stop == Some(true));
    assert!(!cx.interactivity("forge-tab-code").focusable);
    view.read(|forge| assert_eq!(forge.nav().tab, RepoTab::Readme));
    assert!(cx.interactivity("forge-tab-readme").aria.active_descendant);
    cx.simulate_focus("forge-tab-list");
    cx.simulate_key_down("forge-tab-list", "right");
    view.read(|forge| assert_eq!(forge.nav().tab, RepoTab::Readme));
    let code = cx.interactivity("forge-tab-code");
    assert!(code.aria.active_descendant);
    assert_eq!(code.aria.selected, Some(false));
    assert!(!cx.interactivity("forge-tab-readme").aria.active_descendant);
    cx.simulate_key_down("forge-tab-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().tab, RepoTab::Code));
    assert_eq!(cx.interactivity("forge-tab-code").aria.selected, Some(true));
    cx.simulate_key_down("forge-tab-list", "right");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.nav().tab, RepoTab::Refs);
        assert_eq!(forge.tab_cursor, None);
    });
    assert!(cx.interactivity("forge-tab-refs").aria.active_descendant);
}

#[test]
fn the_about_panel_docks_what_the_repo_record_carries() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-dock-about");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().dock, Some(crate::state::Dock::About)));
    assert!(cx.has_text("sha1"), "{:?}", cx.texts());
    assert!(cx.has_text("64 bytes"), "the founded inline blob bound");
    assert!(cx.has_text("Wren"), "the granted writer");
    cx.simulate_click("forge-dock-close");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().dock.is_none()));
}

#[test]
fn a_repo_opens_on_its_readme_and_code_holds_the_tree() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.head_name(), b"refs/heads/clean".to_vec());
        assert_eq!(forge.nav().tab, RepoTab::Readme, "README first");
    });
    assert!(cx.find("forge-repo-header").is_some());
    assert!(cx.find("forge-readme-body").is_some(), "{:?}", cx.texts());
    assert!(cx.find("forge-tree").is_none(), "no tree on the front page");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    assert!(cx.has_text("README.md"), "{:?}", cx.texts());
    assert!(cx.has_text("empty.txt"));
    assert!(cx.find("forge-code-empty").is_some(), "nothing open yet");
    cx.simulate_input("forge-tree-search", "empty");
    cx.run_until_parked();
    assert!(
        cx.find("forge-tree-README.md").is_none(),
        "the filter drops the row"
    );
    cx.simulate_input("forge-tree-search", "");
    cx.simulate_click("forge-tree-empty.txt");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().blob.is_some()));
    assert!(cx.find("forge-blob-lines").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("one"), "the first source line is drawn");
    cx.simulate_click("forge-tree-README.md");
    cx.run_until_parked();
    assert!(cx.find("forge-blob-markdown").is_some());
    assert!(cx.find("forge-blob-lines").is_none());
    cx.simulate_click("forge-blob-close");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().blob.is_none()));
}

#[test]
fn a_filter_field_is_named_apart_from_its_hint() {
    let (mut cx, _view) = opened("default");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    for (key, name, hint) in [
        (
            "forge-rail-search",
            "Filter repositories",
            "Search repositories",
        ),
        ("forge-tree-search", "Filter the file tree", "Filter files"),
    ] {
        let Some(wire::Node::Field {
            options,
            placeholder,
            ..
        }) = cx.find(key)
        else {
            panic!("no field {key}");
        };
        assert_eq!((options.label.as_str(), placeholder.as_str()), (name, hint));
    }
}

#[test]
fn a_relative_link_opens_its_file_in_the_code_tab() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.run_until_parked();
    let follow = |cx: &mut TestAppContext, dir: &[u8], dest: &str| {
        cx.update(&view, |forge, _, cx| forge.follow_link(dir, dest, cx));
        cx.run_until_parked();
    };
    follow(&mut cx, b"docs", "../src");
    view.read(|forge| {
        assert_eq!(forge.nav().tab, RepoTab::Code);
        assert!(forge.nav().expanded.contains(b"src".as_slice()));
    });
    let child = view.read(|forge| {
        forge
            .tree_rows()
            .into_iter()
            .find(|row| row.depth == 1 && !row.is_dir())
            .expect("a file inside src")
            .path
    });
    cx.update(&view, |forge, _, cx| {
        forge.nav_close_blob(cx);
        forge.open_tab(RepoTab::Readme, cx);
        forge.nav.expanded.clear();
    });
    cx.run_until_parked();
    follow(&mut cx, b"", &format!("./{}#top", path_of(&child)));
    view.read(|forge| {
        assert_eq!(forge.nav().tab, RepoTab::Code);
        assert_eq!(forge.nav().blob.as_ref().map(|(p, _)| p), Some(&child));
    });
    assert!(cx.find("forge-blob").is_some());
    follow(&mut cx, b"src", "../README.md");
    view.read(|forge| assert_eq!(forge.nav().tab, RepoTab::Readme));
    follow(&mut cx, b"", "missing.md");
    view.read(|forge| assert!(forge.notice.contains("missing.md"), "{}", forge.notice));
    follow(&mut cx, b"", "https://x.example");
    assert_eq!(
        cx.host().requests::<view::methods::LinkOpen>(),
        vec!["https://x.example"]
    );
}

#[test]
fn a_folder_opens_its_children_inline_and_keeps_its_state() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    let before = view.read(|forge| forge.tree_rows());
    let src = before
        .iter()
        .position(|row| row.path == b"src" && row.is_dir())
        .expect("src is a directory of the root");
    assert!(before.iter().all(|row| row.depth == 0));
    cx.simulate_click("forge-tree-src");
    cx.run_until_parked();
    let asked = cx.host().requests::<Ask>();
    assert!(
        asked
            .iter()
            .any(|query| matches!(query, Query::Tree { path, .. } if path == b"src")),
        "expanding reads the folder lazily"
    );
    let after = view.read(|forge| forge.tree_rows());
    assert_eq!(after[..=src], before[..=src]);
    let children: Vec<_> = after[src + 1..]
        .iter()
        .take_while(|row| row.depth == 1)
        .collect();
    assert!(!children.is_empty(), "{after:?}");
    assert!(children.iter().all(|row| row.path.starts_with(b"src/")));
    assert_eq!(after[src + 1 + children.len()..], before[src + 1..]);
    let child = path_of(&children[0].path);
    assert!(cx.find(&format!("forge-tree-{child}")).is_some());
    cx.simulate_click("forge-tab-readme");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().expanded.contains(b"src".as_slice())));
    assert!(cx.find(&format!("forge-tree-{child}")).is_some());
    cx.simulate_click("forge-tree-src");
    cx.run_until_parked();
    assert_eq!(view.read(|forge| forge.tree_rows()), before);
}

#[test]
fn a_folder_row_is_named_without_its_glyph() {
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    let aria = |cx: &TestAppContext| match cx.find("forge-tree-src") {
        Some(wire::Node::Container(row)) => row.interactivity.as_ref().unwrap().aria.clone(),
        _ => panic!("the src row"),
    };
    let shut = aria(&cx);
    assert_eq!(shut.label.as_deref(), Some("src"));
    assert_eq!(shut.expanded, Some(false));
    cx.simulate_click("forge-tree-src");
    cx.run_until_parked();
    let open = aria(&cx);
    assert_eq!(open.label.as_deref(), Some("src"));
    assert_eq!(open.expanded, Some(true));
}

fn path_of(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

#[test]
fn the_tree_walks_by_keyboard() {
    use crate::tree::Key;
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    let press = |cx: &mut TestAppContext, key| {
        cx.update(&view, |forge, _, cx| forge.tree_key(key, cx));
        cx.run_until_parked();
    };
    let cursor = |cx: &mut TestAppContext| {
        let _ = cx;
        view.read(|forge| forge.nav().cursor.clone().map(|path| path_of(&path)))
    };
    press(&mut cx, Key::Up);
    assert_eq!(cursor(&mut cx).as_deref(), Some("src"));
    let Some(wire::Node::Container(row)) = cx.find("forge-tree-src") else {
        panic!("the src row");
    };
    assert!(
        row.interactivity.as_ref().unwrap().aria.active_descendant
            && !row.interactivity.as_ref().unwrap().focusable
    );
    press(&mut cx, Key::Right);
    view.read(|forge| assert!(forge.nav().expanded.contains(b"src".as_slice())));
    press(&mut cx, Key::Right);
    let child = cursor(&mut cx).expect("stepped into src");
    assert!(child.starts_with("src/"), "{child}");
    press(&mut cx, Key::Left);
    assert_eq!(cursor(&mut cx).as_deref(), Some("src"), "left steps out");
    press(&mut cx, Key::Left);
    view.read(|forge| assert!(forge.nav().expanded.is_empty(), "left folds"));
    press(&mut cx, Key::Down);
    press(&mut cx, Key::Enter);
    view.read(|forge| {
        let (path, _) = forge.nav().blob.clone().expect("enter opens the file");
        assert_eq!(Some(path), forge.nav().cursor.clone());
    });
    press(&mut cx, Key::Up);
    assert_eq!(cursor(&mut cx).as_deref(), Some("src"));
}

#[test]
fn an_oversize_blob_is_a_header_not_a_body() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    cx.update(&view, |forge, _, cx| {
        forge.open_file(
            b"large.txt".to_vec(),
            "b90a09e55e43808905fe881245853c1b35b3fb82".into(),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.has_text("Too large to show"), "{:?}", cx.texts());
    assert!(cx.find("forge-blob-lines").is_none());
    cx.update(&view, |forge, _, cx| {
        forge.open_file(
            b"image.bin".to_vec(),
            "95d586e774a04676a07a142f0e2f97a4f32562cb".into(),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.has_text("Binary file"));
}

fn marked(node: &wire::Node) -> Vec<(String, bool)> {
    fn walk(node: &wire::Node, inside: bool, out: &mut Vec<(String, bool)>) {
        let inside = inside
            || node
                .interactivity()
                .and_then(|interactivity| interactivity.role)
                == Some(view::Role::ListMarker);
        match node {
            wire::Node::Text(text) => out.push((text.content.to_string(), inside)),
            wire::Node::RichText { text, .. } => out.push((text.to_string(), inside)),
            _ => {}
        }
        for child in node.children() {
            walk(child, inside, out);
        }
    }
    let mut out = Vec::new();
    walk(node, false, &mut out);
    out
}

#[test]
fn line_numbers_and_diff_signs_are_markers_and_the_code_is_not() {
    let (mut cx, _view) = opened("default");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    cx.simulate_click("forge-tree-empty.txt");
    cx.run_until_parked();
    let file = marked(cx.find("forge-blob-lines").expect("the file is open"));
    assert!(file.contains(&("1".into(), true)), "{file:?}");
    assert!(file.contains(&("one".into(), false)), "{file:?}");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    cx.simulate_click("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
    cx.run_until_parked();
    let diff = marked(cx.find("forge-commit-diff").expect("the commit's diff"));
    assert!(
        diff.iter().any(|(text, marker)| text == "+" && *marker),
        "{diff:?}"
    );
    assert!(
        diff.iter().any(|(text, marker)| text == "1" && *marker),
        "{diff:?}"
    );
    let code: Vec<_> = diff.iter().filter(|(_, marker)| !marker).collect();
    assert!(
        code.iter()
            .all(|(text, _)| text.parse::<u64>().is_err() && text != "+"),
        "no number or sign outside a marker: {code:?}"
    );
}

#[test]
fn commits_follows_the_cursor_and_opens_one_commit_with_its_diff() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    let asked = cx.host().requests::<Ask>();
    assert!(
        asked.iter().any(|query| matches!(
            query,
            Query::Log {
                page: PageRequest { after: Some(_), .. },
                ..
            }
        )),
        "the log follows its cursor"
    );
    view.read(|forge| {
        let (log, _) = forge.logs.get(&forge.log_query()).expect("the log is read");
        log.read(|log| {
            assert_eq!(log.rows().len(), 2, "both pages are one list");
            assert_eq!(log.count(), 2, "and the list ends with them");
        });
    });
    assert!(cx.has_text("Feature"), "{:?}", cx.texts());
    cx.simulate_click("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
    cx.run_until_parked();
    assert!(
        cx.has_text("Feature\n\nReview these bytes.\n"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("ebfb8b62…4e16"), "the parent is named");
    assert!(
        cx.find("forge-commit-diff-file-src/lib.rs").is_some(),
        "a commit's diff names its files above the rows"
    );
    assert!(cx.find("forge-commit-diff").is_some());
    cx.simulate_click("forge-commit-close");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().commit.is_none()));
}

#[test]
fn a_commit_with_many_files_keeps_its_diff_under_a_scrolling_strip() {
    let (mut cx, view) = opened("wide");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    cx.simulate_click("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
    cx.run_until_parked();
    let strip = cx.style("forge-commit-diff-files");
    assert_eq!(pixels(strip.size.height), Some(8. * 21. + 8.), "capped");
    assert!(
        matches!(
            cx.find("forge-commit-diff-strip"),
            Some(wire::Node::UniformList { count: 264, .. })
        ),
        "and scrolls its rows past the cap"
    );
    let drawn = |cx: &TestAppContext| {
        cx.find("forge-commit-diff-strip")
            .map_or(0, |strip| strip.children().len())
    };
    assert!(drawn(&cx) < 64, "{} of 264 files drawn", drawn(&cx));
    assert!(
        cx.style("forge-commit-diff-file-src/gen/file_000.rs")
            .size
            .width
            .is_some(),
        "a row is as wide as the strip"
    );
    let list = cx.interactivity("forge-commit-diff-files");
    assert_eq!(list.role, Some(view::Role::ListBox));
    assert!(
        list.focusable && list.tab_stop == Some(true),
        "one Tab stop"
    );
    cx.simulate_focus("forge-commit-diff-files");
    for _ in 0..150 {
        cx.simulate_key_down("forge-commit-diff-files", "down");
    }
    cx.run_until_parked();
    cx.simulate_viewport(8);
    assert!(
        cx.interactivity("forge-commit-diff-file-src/gen/file_150.rs")
            .aria
            .active_descendant,
        "the file walked to is drawn and claimed"
    );
    cx.simulate_key_down("forge-commit-diff-files", "enter");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/gen/file_150.rs".as_slice())
        )
    });
    assert!(cx.has_text("src/gen/file_150.rs · added · +3 −0"));
    let Some(wire::Node::UniformList { count, .. }) = cx.find("forge-commit-diff-strip") else {
        panic!("the strip");
    };
    assert_eq!(*count, 264, "the strip lists every file still");
    assert!(
        cx.interactivity("forge-commit-diff-file-src/gen/file_150.rs")
            .aria
            .selected
            == Some(true)
    );
    cx.simulate_click("forge-commit-files-all");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().diff_path.is_none()));
    assert!(cx.find("forge-commit-files-all").is_none());
    assert_eq!(
        lit_rows(&cx, "forge-commit-diff-files", Role::ListBoxOption),
        Vec::<String>::new()
    );
    cx.simulate_click("forge-commit-diff-file-src/gen/file_000.rs");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().diff_path.is_some()));
    cx.simulate_click("forge-commit-close");
    cx.run_until_parked();
    cx.simulate_click("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().diff_path.is_none()));
}

#[test]
fn refs_carry_their_distance_from_the_default_head_and_open_a_draft() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    assert!(cx.has_text("clean"), "{:?}", cx.texts());
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("1 ahead") && text.contains("fast-forward")),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.contains("forbids force pushes and ref deletions"))
    );
    let row = wire::Node::Container(control(&cx, "forge-ref-row-clean"));
    assert!(holds(&row, "forge-ref-row-clean-open") && holds(&row, "forge-compare-clean"));
    let open = wire::Node::Container(control(&cx, "forge-ref-row-clean-open"));
    assert!(!holds(&open, "forge-compare-clean"));
    cx.simulate_click("forge-compare-clean");
    cx.run_until_parked();
    view.read(|forge| {
        let form = forge.form.as_ref().expect("a change draft");
        assert_eq!(form.from, b"refs/heads/clean".to_vec());
        assert_eq!(form.into, b"refs/heads/main".to_vec());
    });
    assert!(cx.has_text("New change"));
}

#[test]
fn settings_shows_only_what_the_contract_exposes_and_grants_by_account() {
    let (mut cx, view) = opened_as("default", 1);
    cx.simulate_click("forge-tab-settings");
    cx.run_until_parked();
    assert!(cx.has_text("Allow force pushes"));
    assert!(cx.has_text("Allow ref deletion"));
    assert!(cx.has_text("Wren"), "the granted writer resolves to a name");
    cx.simulate_click("forge-settings-force");
    cx.simulate_click("forge-settings-head");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.simulate_click("forge-settings-save");
    cx.run_until_parked();
    assert!(cx.host().requests::<SubmitForge>().iter().any(|op| matches!(
        op,
        Op::Configure { repo, settings }
            if repo == "project" && settings.allow_force && settings.head == b"refs/heads/clean"
    )));
    cx.simulate_input("forge-settings-grant-input", "acct:4");
    cx.simulate_click("forge-settings-grant");
    cx.run_until_parked();
    assert_eq!(set_lists(&cx), [listed(&[9, 4])]);
    cx.simulate_input("forge-settings-grant-input", "ra");
    cx.run_until_parked();
    assert!(cx.find("forge-grant-pick-2").is_some(), "Rae matches");
    assert!(cx.find("forge-grant-pick-1").is_none(), "Ada does not");
    assert!(
        cx.find(&format!("forge-grant-pick-{FORGE}")).is_none(),
        "no program"
    );
    cx.simulate_click("forge-grant-pick-2");
    cx.run_until_parked();
    let filled = view.read(|forge| forge.repo_settings.as_ref().map(|form| form.grant.text()));
    assert_eq!(filled.as_deref(), Some("2"));
    assert!(
        cx.find("forge-grant-pick-2").is_none(),
        "a number searches nothing"
    );
    cx.simulate_click("forge-settings-grant");
    cx.run_until_parked();
    assert_eq!(set_lists(&cx)[1], listed(&[9, 2]));
    cx.simulate_input("forge-settings-grant-input", "acct:9");
    cx.simulate_click("forge-settings-grant");
    cx.run_until_parked();
    assert_eq!(set_lists(&cx).len(), 2, "a writer is granted nothing more");
    assert!(cx.has_text(words::WRITES_ALREADY));
    cx.simulate_click("forge-settings-revoke-acct-9");
    cx.run_until_parked();
    assert_eq!(
        set_lists(&cx)[2],
        Some(Ownership::Owner),
        "the last writer revoked leaves the owner alone"
    );
}

#[test]
fn enter_in_the_grant_field_grants_for_the_owner_only() {
    let (mut cx, _) = opened_as("default", 1);
    cx.simulate_click("forge-tab-settings");
    cx.run_until_parked();
    cx.simulate_input("forge-settings-grant-input", "acct:7");
    cx.simulate_submit("forge-settings-grant-input");
    cx.run_until_parked();
    assert_eq!(set_lists(&cx), [listed(&[9, 7])]);
    let (mut cx, _) = opened_as("default", 2);
    cx.simulate_click("forge-tab-settings");
    cx.run_until_parked();
    assert!(
        matches!(
            cx.find("forge-settings-grant-input"),
            Some(wire::Node::Field {
                on_submit: None,
                ..
            })
        ),
        "a reader's Enter grants nothing"
    );
}

#[test]
fn the_narrow_window_folds_the_rail_and_the_dock_into_toggles() {
    let (mut cx, view) = opened("default");
    cx.simulate_resize(720., 600.);
    cx.run_until_parked();
    view.read(|forge| assert!(forge.layout.narrow()));
    assert!(cx.find("forge-toggle-rail").is_some());
    assert!(cx.find("forge-rail").is_none(), "the rail folds away");
    cx.simulate_click("forge-toggle-rail");
    cx.run_until_parked();
    assert!(cx.find("forge-rail").is_some());
}

#[test]
fn narrow_side_panes_do_not_follow_the_repositories_toggle() {
    let (mut cx, _) = opened("default");
    cx.simulate_resize(760., 800.);
    cx.run_until_parked();
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    assert!(
        cx.find("forge-tree").is_some(),
        "no file open: the tree is the screen"
    );
    cx.simulate_click("forge-tree-README.md");
    cx.run_until_parked();
    assert!(
        cx.find("forge-tree").is_none(),
        "a file open takes the tree's place"
    );
    cx.simulate_click("forge-toggle-rail");
    cx.run_until_parked();
    assert!(cx.find("forge-rail").is_some(), "the rail unfolds");
    assert!(
        cx.find("forge-tree").is_none(),
        "the tree does not follow the rail"
    );
    cx.simulate_click("forge-blob-close");
    cx.run_until_parked();
    assert!(
        cx.find("forge-tree").is_some(),
        "the file closed: the tree is back"
    );

    let (mut cx, _) = change_screen("default", ChangeTab::Files);
    cx.simulate_resize(760., 800.);
    cx.run_until_parked();
    assert!(cx.find("forge-rail").is_none());
    assert!(
        cx.find("forge-file-tree").is_some(),
        "the file list stays beside the diff"
    );
}

#[test]
fn about_left_open_does_not_lay_a_change_s_details_over_it() {
    let (mut cx, _) = opened("default");
    cx.simulate_resize(760., 800.);
    cx.run_until_parked();
    cx.simulate_click("forge-dock-about");
    cx.run_until_parked();
    assert!(cx.find("forge-about").is_some());
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert!(cx.find("forge-details-over").is_none(), "{:?}", cx.texts());
}

#[test]
fn the_repositories_keep_their_table_at_the_width_a_window_opens() {
    let (mut cx, _) = booted("default");
    let column = |cx: &TestAppContext| cx.style("forge-repo-project-activity-cell").size.width;
    for width in [640., 768.] {
        cx.simulate_resize(width, 600.);
        cx.run_until_parked();
        assert!(
            cx.find("forge-repos-columns").is_some(),
            "header at {width}"
        );
        assert!(column(&cx).is_some(), "activity column at {width}");
        assert!(cx.find("forge-narrow-bar").is_none(), "bar at {width}");
    }
    cx.simulate_resize(600., 600.);
    cx.run_until_parked();
    assert!(cx.find("forge-repos-columns").is_none());
    assert!(
        column(&cx).is_none(),
        "a wrapped fact is as wide as it reads"
    );
    let Reply::Repos { page, .. } = reply("repos") else {
        panic!("the repos fixture")
    };
    for info in &page.items {
        let refs = view::design::plural(info.repo.refs_count, "ref", "refs");
        assert!(cx.has_text(&refs), "{refs}: {:?}", cx.texts());
    }
}

#[test]
fn the_rail_is_a_list_box_whose_enter_opens_the_active_repository() {
    let (mut cx, view) = opened("default");
    let rail = cx.interactivity("forge-rail-list");
    assert_eq!(rail.role, Some(view::Role::ListBox));
    assert!(rail.focusable && rail.tab_stop == Some(true));
    let project = cx.interactivity("forge-rail-repo-project");
    assert_eq!(project.role, Some(view::Role::ListBoxOption));
    assert!(!project.focusable && project.aria.active_descendant);
    assert_eq!(project.aria.selected, Some(true));
    let rows: Vec<String> = cx
        .find("forge-rail-list")
        .expect("the rail")
        .children()
        .iter()
        .filter_map(|row| row.key().map(str::to_owned))
        .collect();
    cx.simulate_focus("forge-rail-list");
    cx.simulate_key_down("forge-rail-list", "end");
    let last = rows.last().expect("a repository");
    assert!(cx.interactivity(last).aria.active_descendant);
    cx.update(&view, |forge, _, cx| forge.open_repos(cx));
    cx.run_until_parked();
    assert!(
        cx.find("forge-rail-list").is_none(),
        "the list screen has no rail"
    );
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    cx.simulate_focus("forge-rail-list");
    cx.simulate_key_down("forge-rail-list", "home");
    cx.simulate_key_down("forge-rail-list", "enter");
    cx.run_until_parked();
    let name = rows[0].trim_start_matches("forge-rail-repo-");
    view.read(|forge| assert_eq!(forge.nav().repo.as_deref(), Some(name)));
}

#[test]
fn a_walk_of_the_repositories_ends_with_a_move() {
    let (mut cx, view) = booted("default");
    let lit = |cx: &TestAppContext| lit_rows(cx, "forge-repos-list", Role::Row);
    cx.simulate_focus("forge-repos-list");
    cx.simulate_key_down("forge-repos-list", "right");
    assert_eq!(lit(&cx).len(), 1, "the arrows light the row they walk");
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    cx.update(&view, |forge, _, cx| forge.open_repos(cx));
    cx.run_until_parked();
    assert_eq!(lit(&cx), Vec::<String>::new(), "back at rest");
}

#[test]
fn a_snapshot_restores_the_same_screen_without_replaying_events() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    cx.update(&view, |forge, _, cx| {
        forge.open_change_tab(ChangeTab::Files, cx);
        forge.toggle_viewed(b"src/lib.rs", cx);
    });
    cx.run_until_parked();
    let snapshot = cx.snapshot().unwrap();

    let mut restored = TestAppContext::new();
    configure(&mut restored, "default");
    let view = restored.restore::<Forge>(&snapshot).unwrap();
    restored.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.nav().repo.as_deref(), Some("project"));
        assert_eq!(forge.nav().change, Some(1));
        assert_eq!(forge.nav().change_tab, ChangeTab::Files);
        assert!(forge.viewed.contains("project#1:src/lib.rs"));
    });
    assert!(
        !restored.host().requests::<Ask>().is_empty(),
        "a restored view reads again"
    );
}

#[test]
fn judgment_is_its_own_query_keyed_by_the_readers_account() {
    let (mut cx, view) = opened("judgment");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.filter, Filter::Judgment));
    assert!(
        cx.host()
            .requests::<Ask>()
            .iter()
            .any(|query| matches!(query, Query::Judgment { principal, .. } if *principal == forge::Principal::Account(2))),
        "the reader is their account, from the session"
    );
    assert!(cx.has_text("review requested"), "{:?}", cx.texts());
}

#[test]
fn an_empty_judgment_says_nothing_waits_on_you() {
    let (mut cx, _) = opened("judgment-empty");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    assert!(cx.has_text("Nothing waits on you"), "{:?}", cx.texts());
}

pub(crate) fn change_screen(mode: &'static str, tab: ChangeTab) -> (TestAppContext, Entity<Forge>) {
    change_screen_as(mode, tab, 2)
}

pub(crate) fn change_screen_as(
    mode: &'static str,
    tab: ChangeTab,
    account: u64,
) -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = opened_as(mode, account);
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    cx.simulate_click(&format!("forge-change-tab-{}", tab.slug()));
    cx.run_until_parked();
    (cx, view)
}

#[path = "change_tests.rs"]
mod change_tests;

#[path = "screen_tests.rs"]
mod screen_tests;

#[path = "log_tests.rs"]
mod log_tests;

#[path = "phone_tests.rs"]
mod phone_tests;

#[path = "policies_tests.rs"]
mod policies_tests;

#[path = "faces_tests.rs"]
mod faces_tests;

#[path = "link_tests.rs"]
mod link_tests;

#[test]
fn copy_puts_the_address_on_the_clipboard_without_opening_the_repository() {
    let (mut cx, view) = booted("default");
    let copied = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host()
        .handle::<view::methods::ClipboardWrite>(move |text| {
            *seen.borrow_mut() = text;
            Ok(())
        });
    cx.simulate_click("forge-repo-project-copy");
    cx.run_until_parked();
    assert_eq!(*copied.borrow(), "duck://testnet-0a1b2c3d/forge/project");
    assert!(cx.has_text("Copied"));
    view.read(|forge| assert!(forge.nav.repo.is_none(), "copy is not open"));
}

#[test]
fn the_small_press_targets_are_at_least_24_px_each_way() {
    let at_least_24 = |cx: &TestAppContext, key: &str| {
        let floor = &cx.style(key).min_size;
        let (width, height) = (pixels(floor.width), pixels(floor.height));
        assert!(
            width >= Some(24.) && height >= Some(24.),
            "{key}: {width:?} × {height:?}"
        );
    };
    let (cx, _) = booted("default");
    for key in ["forge-repo-project-copy", "forge-repo-project-activity"] {
        at_least_24(&cx, key);
    }
    let (cx, _) = opened("default");
    at_least_24(&cx, "forge-repo-activity");
}

pub(crate) fn pixels(length: Option<impl std::fmt::Display>) -> Option<f32> {
    length?.to_string().strip_suffix("px")?.parse().ok()
}

pub(crate) fn control(cx: &TestAppContext, key: &str) -> wire::ContainerNode {
    match cx.find(key) {
        Some(wire::Node::Container(node)) => node.clone(),
        other => panic!("{key} is no container: {other:?}"),
    }
}

pub(crate) fn holds(node: &wire::Node, key: &str) -> bool {
    node.children()
        .iter()
        .any(|child| child.key() == Some(key) || holds(child, key))
}

#[test]
fn a_repository_row_is_a_grid_row_whose_press_is_a_button_beside_its_controls() {
    let (cx, _view) = booted("default");
    let row = control(&cx, "forge-repo-project");
    assert_eq!(
        row.interactivity.as_ref().unwrap().role,
        Some(view::Role::Row)
    );
    assert!(
        !row.interactivity.as_ref().unwrap().focusable
            && row.interactivity.as_ref().unwrap().on_click.is_none()
    );
    let open = control(&cx, "forge-repo-project-open");
    assert_eq!(
        open.interactivity.as_ref().unwrap().role,
        Some(view::Role::Button)
    );
    assert!(
        !open.interactivity.as_ref().unwrap().focusable
            && open.interactivity.as_ref().unwrap().on_click.is_some()
    );
    assert!(open.interactivity.as_ref().unwrap().aria.active_descendant);
    let open = wire::Node::Container(open);
    for sibling in ["forge-repo-project-copy", "forge-repo-project-activity"] {
        assert!(!holds(&open, sibling), "{sibling} is inside the press");
    }
}

#[test]
fn the_repositories_grid_walks_cells_and_enter_presses_the_active_one() {
    let (mut cx, view) = booted("default");
    cx.simulate_resize(1000., 600.);
    cx.run_until_parked();
    let copied = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let seen = copied.clone();
    cx.host()
        .handle::<view::methods::ClipboardWrite>(move |text| {
            *seen.borrow_mut() = text;
            Ok(())
        });
    let grid = cx.interactivity("forge-repos-list");
    assert_eq!(grid.role, Some(view::Role::Grid));
    assert!(grid.focusable && grid.tab_stop == Some(true));
    let rows: Vec<String> = cx
        .find("forge-repos-list")
        .expect("the grid")
        .children()
        .iter()
        .filter(|node| {
            node.interactivity()
                .is_some_and(|row| row.role == Some(view::Role::Row))
        })
        .filter_map(|node| node.key().map(str::to_owned))
        .filter(|key| key != "forge-repos-columns")
        .collect();
    assert!(!rows.is_empty(), "{:?}", cx.texts());
    let header = cx.interactivity("forge-repos-columns");
    assert_eq!(header.role, Some(view::Role::Row));
    assert_eq!(
        cx.interactivity("forge-repos-column-Name").role,
        Some(view::Role::ColumnHeader)
    );
    let at = usize::from(rows.len() > 1);
    cx.simulate_focus("forge-repos-list");
    if at == 1 {
        cx.simulate_key_down("forge-repos-list", "down");
    }
    let name = rows[at].trim_start_matches("forge-repo-").to_owned();
    assert!(
        cx.interactivity(&format!("forge-repo-{name}-open"))
            .aria
            .active_descendant
    );
    cx.simulate_key_down("forge-repos-list", "right");
    let copy = cx.interactivity(&format!("forge-repo-{name}-copy"));
    assert!(copy.aria.active_descendant && !copy.focusable);
    assert!(
        !cx.interactivity(&format!("forge-repo-{name}-open"))
            .aria
            .active_descendant
    );
    cx.simulate_key_down("forge-repos-list", "enter");
    cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        format!("duck://testnet-0a1b2c3d/forge/{name}")
    );
    assert!(cx.has_text("Copied"));
    view.read(|forge| assert!(forge.nav.repo.is_none(), "copy is not open"));
    cx.simulate_key_down("forge-repos-list", "right");
    assert!(
        cx.interactivity(&format!("forge-repo-{name}-activity"))
            .aria
            .active_descendant
    );
    cx.simulate_key_down("forge-repos-list", "enter");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<view::methods::LinkOpen>().len(), 1);
    cx.simulate_key_down("forge-repos-list", "home");
    cx.simulate_key_down("forge-repos-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().repo.as_deref(), Some(name.as_str())));
}

#[test]
fn the_ref_menu_takes_the_keys_on_open_and_gives_them_back() {
    use wire::keyboard::{Key, Named};
    let (mut cx, view) = opened("default");
    let focused = |cx: &TestAppContext| -> Vec<String> {
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .filter_map(|command| match command {
                wire::WidgetCommand::Focus { target } => target.last().and_then(|id| match id {
                    wire::ElementIdWire::Name(name) => Some(name.to_string()),
                    _ => None,
                }),
                _ => None,
            })
            .collect()
    };
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    assert_eq!(
        focused(&cx).last().map(String::as_str),
        Some("forge-ref-picker-filter")
    );
    let Some(wire::Node::Field { claims, .. }) = cx.find("forge-ref-picker-filter") else {
        panic!("the filter box");
    };
    let claimed: Vec<&Key> = claims.iter().map(|claim| &claim.key).collect();
    for key in [Named::ArrowUp, Named::ArrowDown, Named::Escape] {
        assert!(claimed.contains(&&Key::Named(key)), "{key:?}");
    }
    assert!(
        !claimed.contains(&&Key::Named(Named::Enter)),
        "Enter submits"
    );
    let menu = cx.interactivity("forge-ref-picker-menu");
    assert_eq!(menu.role, Some(view::Role::Menu));
    assert!(menu.focusable && menu.on_key_down.is_some());
    assert!(
        cx.find("forge-menu-overlay").is_some(),
        "the menu floats in the overlay"
    );
    let items: Vec<String> = {
        let mut items = Vec::new();
        fn gather(node: &wire::Node, items: &mut Vec<String>) {
            if node
                .interactivity()
                .is_some_and(|item| item.role == Some(view::Role::MenuItemRadio))
            {
                items.push(node.key().unwrap_or_default().to_owned());
            }
            node.children()
                .iter()
                .for_each(|child| gather(child, items));
        }
        gather(
            cx.find("forge-ref-picker-menu").expect("the menu"),
            &mut items,
        );
        items
    };
    assert!(items.len() >= 2, "{items:?}");
    assert_eq!(items[0], "forge-ref-refs/heads/main", "the default first");
    assert!(cx.interactivity(&items[0]).aria.active_descendant);
    assert!(!cx.interactivity(&items[0]).focusable);
    cx.simulate_field_key("forge-ref-picker-filter", "escape");
    cx.run_until_parked();
    assert!(cx.find("forge-ref-picker-menu").is_none());
    assert_eq!(
        focused(&cx).last().map(String::as_str),
        Some("forge-ref-picker")
    );
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    cx.simulate_field_key("forge-ref-picker-filter", "down");
    cx.run_until_parked();
    assert!(cx.interactivity(&items[1]).aria.active_descendant);
    cx.simulate_submit("forge-ref-picker-filter");
    cx.run_until_parked();
    let picked = items[1].trim_start_matches("forge-ref-").to_owned();
    view.read(|forge| {
        assert_eq!(forge.nav().rev.as_deref(), Some(picked.as_bytes()));
        assert_eq!(forge.menu, None);
    });
    assert_eq!(
        focused(&cx).last().map(String::as_str),
        Some("forge-ref-picker")
    );
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    assert!(cx.interactivity(&items[1]).aria.active_descendant);
    cx.simulate_focus("forge-ref-picker-menu");
    cx.simulate_key_down("forge-ref-picker-menu", "up");
    cx.simulate_key_down("forge-ref-picker-menu", "enter");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().rev.as_deref(),
            Some(b"refs/heads/main".as_slice())
        );
        assert_eq!(forge.menu, None);
    });
}

#[test]
fn escape_in_a_menu_closes_it_once() {
    let (mut cx, _view) = opened("default");
    let back_to_picker = |cx: &TestAppContext| -> usize {
        cx.host()
            .requests::<view::methods::HostWidget>()
            .iter()
            .filter(|command| {
                matches!(command, wire::WidgetCommand::Focus { target }
                    if target.last() == Some(&wire::ElementIdWire::Name("forge-ref-picker".into())))
            })
            .count()
    };
    for list in [false, true] {
        cx.simulate_click("forge-ref-picker");
        cx.run_until_parked();
        let before = back_to_picker(&cx);
        match list {
            false => cx.simulate_field_key("forge-ref-picker-filter", "escape"),
            true => {
                cx.simulate_focus("forge-ref-picker-menu");
                cx.simulate_key_down("forge-ref-picker-menu", "escape");
            }
        }
        cx.run_until_parked();
        assert!(cx.find("forge-ref-picker-menu").is_none());
        assert_eq!(back_to_picker(&cx) - before, 1, "closed once");
    }
}

#[test]
fn a_dropdown_leaves_the_window_where_it_was() {
    fn paths(
        node: &wire::Node,
        above: &mut Vec<wire::ElementIdWire>,
        out: &mut Vec<Vec<wire::ElementIdWire>>,
    ) {
        let id = node.identity().cloned();
        if let Some(id) = &id {
            above.push(id.clone());
            out.push(above.clone());
        }
        node.children()
            .iter()
            .for_each(|child| paths(child, above, out));
        if id.is_some() {
            above.pop();
        }
    }
    let window = |cx: &TestAppContext| {
        let mut out = Vec::new();
        paths(cx.root(), &mut Vec::new(), &mut out);
        let root = wire::ElementIdWire::Name("forge".into());
        out.retain(|path| path.contains(&root));
        out
    };
    let (mut cx, _view) = opened("default");
    let closed = window(&cx);
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    assert!(
        cx.find("forge-ref-picker-menu").is_some(),
        "the menu is open"
    );
    assert_eq!(window(&cx), closed);
    cx.simulate_field_key("forge-ref-picker-filter", "escape");
    cx.run_until_parked();
    assert!(
        cx.find("forge-ref-picker-menu").is_none(),
        "the menu is closed"
    );
    assert_eq!(window(&cx), closed);
}

#[test]
fn the_tree_claims_a_row_on_entry_and_end_reaches_the_last() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.simulate_click("forge-ref-refs/heads/clean");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    let tree = cx.interactivity("forge-tree-rows");
    assert_eq!(tree.role, Some(view::Role::Tree));
    assert!(tree.focusable && tree.tab_stop == Some(true));
    let rows = view.read(|forge| forge.tree_rows());
    let first = format!("forge-tree-{}", path_of(&rows[0].path));
    let last = format!("forge-tree-{}", path_of(&rows[rows.len() - 1].path));
    view.read(|forge| assert_eq!(forge.nav().cursor, None));
    assert!(cx.interactivity(&first).aria.active_descendant);
    cx.simulate_focus("forge-tree-rows");
    cx.simulate_key_down("forge-tree-rows", "end");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().cursor.as_deref(),
            Some(rows[rows.len() - 1].path.as_slice())
        )
    });
    assert!(cx.interactivity(&last).aria.active_descendant);
    assert!(!cx.interactivity(&first).aria.active_descendant);
    cx.simulate_key_down("forge-tree-rows", "home");
    cx.run_until_parked();
    assert!(cx.interactivity(&first).aria.active_descendant);
}

#[test]
fn no_tree_row_is_lit_until_the_arrows_walk_it() {
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    let lit = |cx: &TestAppContext| lit_rows(cx, "forge-tree-rows", Role::TreeItem);
    assert_eq!(lit(&cx), Vec::<String>::new(), "at rest");
    cx.simulate_click("forge-tree-src");
    cx.run_until_parked();
    assert_eq!(lit(&cx), Vec::<String>::new(), "after a press on a folder");
    cx.simulate_focus("forge-tree-rows");
    cx.simulate_key_down("forge-tree-rows", "down");
    assert_eq!(lit(&cx).len(), 1, "the arrows lit the row they walked to");
}

#[test]
fn the_keys_go_on_from_the_tree_row_pressed() {
    use crate::tree::Key;
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-code");
    cx.run_until_parked();
    let press = |cx: &mut TestAppContext, key| {
        cx.update(&view, |forge, _, cx| forge.tree_key(key, cx));
        cx.run_until_parked();
    };
    press(&mut cx, Key::End);
    cx.simulate_click("forge-tree-src");
    cx.run_until_parked();
    press(&mut cx, Key::Down);
    let cursor = view.read(|forge| forge.nav().cursor.clone().map(|path| path_of(&path)));
    assert!(
        cursor
            .as_deref()
            .is_some_and(|path| path.starts_with("src/")),
        "{cursor:?}"
    );
}

#[test]
fn a_forge_link_opens_its_repository() {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    let props = cx.host().stream::<HostSession>();
    let routes = cx.host().stream::<view::methods::HostRoute>();
    let view = cx.open::<Forge>();
    cx.run_until_parked();
    props.send(seat(b"", None));
    cx.run_until_parked();
    routes.send("project".into());
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().repo.as_deref(), Some("project")));
    routes.send("project/1".into());
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            (forge.nav().repo.as_deref(), forge.nav().change),
            (Some("project"), Some(1))
        )
    });
    routes.send("project/src/lib.rs".into());
    cx.run_until_parked();
    view.read(|forge| {
        let open = forge.nav().blob.as_ref().map(|(path, _)| path.clone());
        assert_eq!(
            (forge.nav().tab, open),
            (RepoTab::Code, Some(b"src/lib.rs".to_vec()))
        )
    });
    routes.send(String::new());
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().repo.is_none()));
}

#[test]
fn the_commit_list_is_a_list_box_whose_enter_opens_the_active_commit() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    let list = cx.interactivity("forge-log-list");
    assert_eq!(list.role, Some(view::Role::ListBox));
    assert!(list.focusable && list.tab_stop == Some(true));
    let first = cx.interactivity("forge-commit-26607f522099476177a45a8058a93108fba5a84d");
    assert_eq!(first.role, Some(view::Role::ListBoxOption));
    assert!(!first.focusable && first.aria.active_descendant);
    cx.simulate_focus("forge-log-list");
    cx.simulate_key_down("forge-log-list", "down");
    let second: String = cx
        .find("forge-log")
        .expect("the log")
        .children()
        .iter()
        .filter_map(|row| row.key())
        .nth(1)
        .expect("a second commit")
        .to_owned();
    assert!(cx.interactivity(&second).aria.active_descendant);
    cx.simulate_key_down("forge-log-list", "enter");
    cx.run_until_parked();
    let oid = second.trim_start_matches("forge-commit-").to_owned();
    view.read(|forge| assert_eq!(forge.nav().commit.as_deref(), Some(oid.as_str())));
}

fn lit_rows(cx: &TestAppContext, root: &str, role: Role) -> Vec<String> {
    fn walk(cx: &TestAppContext, node: &wire::Node, role: Role, out: &mut Vec<String>) {
        if let Some(key) = node.key()
            && node.interactivity().is_some_and(|it| it.role == Some(role))
            && node.style().is_some()
            && cx.style(key).background.is_some()
        {
            out.push(key.to_owned());
        }
        for child in node.children() {
            walk(cx, child, role, out);
        }
    }
    let mut out = Vec::new();
    walk(cx, cx.node(root), role, &mut out);
    out
}

#[test]
fn no_list_row_is_lit_until_the_arrows_walk_it() {
    let (cx, _) = booted("default");
    assert_eq!(
        lit_rows(&cx, "forge-repos-list", Role::Row),
        Vec::<String>::new(),
        "the repositories at rest"
    );
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    let rows = |cx: &TestAppContext| lit_rows(cx, "forge-log-list", Role::ListBoxOption);
    assert_eq!(rows(&cx), Vec::<String>::new(), "the commits at rest");
    cx.simulate_focus("forge-log-list");
    cx.simulate_key_down("forge-log-list", "down");
    let lit = rows(&cx);
    assert_eq!(lit.len(), 1, "{lit:?}");
}

fn list_box<'a>(cx: &'a TestAppContext, id: &str) -> &'a view::StyleRefinement {
    assert!(
        matches!(cx.find(id), Some(wire::Node::Container(_))),
        "no list {id}"
    );
    cx.style(id)
}

#[test]
fn the_commit_list_leaves_the_scrolling_and_the_gutter_to_its_rows() {
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    let list = list_box(&cx, "forge-log-list");
    assert_eq!((list.overflow.y, list.padding.right), (None, None));
    assert!(
        matches!(cx.find("forge-log"), Some(wire::Node::UniformList { .. })),
        "the commits are a virtual list"
    );
    let rows = cx.style("forge-log");
    assert_eq!(rows.padding.right, None, "the gutter is the host's");
}

#[test]
fn an_arrow_on_the_commit_list_scrolls_its_rows_to_the_commit_moved_to() {
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-tab-commits");
    cx.run_until_parked();
    cx.simulate_focus("forge-log-list");
    cx.simulate_key_down("forge-log-list", "down");
    let Some(wire::Node::UniformList { scroll_request, .. }) = cx.find("forge-log") else {
        panic!("the commits are a virtual list");
    };
    assert_eq!(scroll_request.map(|request| request.index), Some(1));
}

#[test]
fn the_change_list_leaves_the_scrolling_to_its_rows() {
    let (mut cx, _) = opened("many-changes");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    let list = list_box(&cx, "forge-changes-list");
    assert_eq!(list.overflow.y, None);
    cx.simulate_focus("forge-changes-list");
    cx.simulate_key_down("forge-changes-list", "down");
    let Some(wire::Node::UniformList { scroll_request, .. }) = cx.find("forge-changes-rows") else {
        panic!("the changes are a virtual list");
    };
    assert_eq!(scroll_request.map(|request| request.index), Some(1));
}

#[test]
fn a_long_change_list_draws_the_changes_on_screen_and_opens_one_by_keyboard() {
    let (mut cx, view) = opened("many-changes");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    let Some(wire::Node::UniformList {
        count, children, ..
    }) = cx.find("forge-changes-rows")
    else {
        panic!("the changes are a virtual list");
    };
    assert_eq!(*count, 300, "every change is listed");
    assert!(
        children.len() < 64,
        "{} of 300 changes drawn",
        children.len()
    );
    assert!(
        cx.style("forge-change-1").size.width.is_some(),
        "a row is as wide as the list: its counts on the right, its rule across"
    );
    cx.simulate_focus("forge-changes-list");
    for _ in 0..250 {
        cx.simulate_key_down("forge-changes-list", "down");
    }
    cx.run_until_parked();
    cx.simulate_viewport(12);
    assert!(
        cx.interactivity("forge-change-251").aria.active_descendant,
        "the change walked to is drawn and claimed"
    );
    cx.simulate_key_down("forge-changes-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().change, Some(251)));
}

#[test]
fn the_ref_list_leaves_the_scrolling_to_its_rows() {
    let (mut cx, _) = opened("default");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let grid = list_box(&cx, "forge-refs-list");
    assert_eq!(grid.overflow.y, None);
    assert!(
        matches!(
            cx.find("forge-refs-rows"),
            Some(wire::Node::UniformList { .. })
        ),
        "the refs are a virtual list"
    );
    cx.simulate_focus("forge-refs-list");
    cx.simulate_key_down("forge-refs-list", "down");
    let Some(wire::Node::UniformList { scroll_request, .. }) = cx.find("forge-refs-rows") else {
        panic!("the refs are a virtual list");
    };
    assert_eq!(scroll_request.map(|request| request.index), Some(1));
}

#[test]
fn a_long_ref_list_draws_the_refs_on_screen_and_picks_one_by_keyboard() {
    let (mut cx, view) = opened("many-refs");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let total = view.read(|forge| forge.refs().map_or(0, <[_]>::len));
    assert!(total > 120, "{total} refs");
    let drawn = ref_rows(&cx).len();
    assert!(drawn > 0 && drawn < 64, "{drawn} of {total} refs drawn");
    cx.simulate_focus("forge-refs-list");
    for _ in 0..110 {
        cx.simulate_key_down("forge-refs-list", "down");
    }
    cx.run_until_parked();
    cx.simulate_viewport(20);
    let name = view.read(|forge| forge.refs().unwrap()[110].name.clone());
    let label = crate::ui::components::ref_label(&name);
    assert!(
        cx.interactivity(&format!("forge-ref-row-{label}-open"))
            .aria
            .active_descendant,
        "the ref walked to is drawn and claimed"
    );
    cx.simulate_key_down("forge-refs-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.head_name(), name));
    assert!(ref_rows(&cx).len() < 64);
}

fn ref_rows(cx: &TestAppContext) -> Vec<String> {
    cx.find("forge-refs-rows")
        .expect("the refs")
        .children()
        .iter()
        .filter_map(|slot| slot.children().first()?.key().map(str::to_owned))
        .filter(|key| key.starts_with("forge-ref-row-"))
        .collect()
}

#[test]
fn the_change_list_is_a_list_box_whose_enter_opens_the_active_change() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    let list = cx.interactivity("forge-changes-list");
    assert_eq!(list.role, Some(view::Role::ListBox));
    assert!(list.focusable && list.tab_stop == Some(true));
    let row = cx.interactivity("forge-change-1");
    assert_eq!(row.role, Some(view::Role::ListBoxOption));
    assert!(!row.focusable && row.aria.active_descendant);
    cx.simulate_focus("forge-changes-list");
    cx.simulate_key_down("forge-changes-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().change, Some(1)));
}

#[test]
fn the_ref_list_is_a_grid_whose_right_reaches_compare() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let grid = cx.interactivity("forge-refs-list");
    assert_eq!(grid.role, Some(view::Role::Grid));
    assert!(grid.focusable && grid.tab_stop == Some(true));
    let rows = ref_rows(&cx);
    assert!(
        cx.interactivity(&format!("{}-open", rows[0]))
            .aria
            .active_descendant
    );
    cx.simulate_focus("forge-refs-list");
    cx.simulate_key_down("forge-refs-list", "down");
    let branch = &rows[1];
    let label = &branch["forge-ref-row-".len()..];
    assert!(
        cx.interactivity(&format!("{branch}-open"))
            .aria
            .active_descendant
    );
    cx.simulate_key_down("forge-refs-list", "right");
    let compare = cx.interactivity(&format!("forge-compare-{label}"));
    assert!(compare.aria.active_descendant && !compare.focusable);
    cx.simulate_key_down("forge-refs-list", "enter");
    cx.run_until_parked();
    view.read(|forge| {
        let form = forge.form.as_ref().expect("a change draft");
        assert_eq!(form.from, format!("refs/heads/{label}").into_bytes());
    });
}

#[test]
fn a_reader_who_may_not_write_has_no_compare_cell() {
    let (mut cx, view) = seated(b"stranger", None);
    view.read(|forge| assert!(!forge.may_write()));
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let rows = ref_rows(&cx);
    cx.simulate_focus("forge-refs-list");
    cx.simulate_key_down("forge-refs-list", "down");
    cx.simulate_key_down("forge-refs-list", "right");
    let branch = &rows[1];
    let label = &branch["forge-ref-row-".len()..];
    assert!(
        cx.interactivity(&format!("{branch}-open"))
            .aria
            .active_descendant,
        "the ref stays the active cell"
    );
    let compare = cx.interactivity(&format!("forge-compare-{label}"));
    assert!(compare.aria.disabled == Some(true) && !compare.aria.active_descendant);
    cx.simulate_key_down("forge-refs-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.form.is_none(), "no draft for a reader"));
}

#[test]
fn a_ref_without_compare_is_a_grid_row_of_one_cell() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let rows = ref_rows(&cx);
    for label in ["main", "v1"] {
        let key = format!("forge-ref-row-{label}");
        assert_eq!(cx.interactivity(&key).role, Some(view::Role::Row), "{key}");
        assert_eq!(
            cx.interactivity(&format!("{key}-open-cell")).role,
            Some(view::Role::GridCell),
            "{key}"
        );
        assert_eq!(
            cx.interactivity(&format!("{key}-open")).role,
            Some(view::Role::Button),
            "{key}"
        );
        assert!(cx.find(&format!("forge-compare-{label}")).is_none());
    }
    assert_eq!(
        cx.interactivity("forge-ref-row-main-open").aria.current,
        Some(view::accesskit::AriaCurrent::True)
    );
    let at = |label: &str| {
        rows.iter()
            .position(|key| *key == format!("forge-ref-row-{label}"))
            .unwrap_or_else(|| panic!("{label} is listed: {rows:?}"))
    };
    cx.simulate_focus("forge-refs-list");
    for _ in 0..at("main") {
        cx.simulate_key_down("forge-refs-list", "down");
    }
    cx.simulate_key_down("forge-refs-list", "right");
    assert!(
        cx.interactivity("forge-ref-row-main-open")
            .aria
            .active_descendant
    );
    for _ in at("main")..at("v1") {
        cx.simulate_key_down("forge-refs-list", "down");
    }
    cx.simulate_key_down("forge-refs-list", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.head_name(), b"refs/tags/v1".to_vec()));
}

fn kind(query: &Query) -> String {
    let text = format!("{query:?}");
    text.split([' ', '(', '{']).next().unwrap_or("?").to_owned()
}

struct Costed {
    cx: TestAppContext,
    heads: Heads,
    reply_bytes: std::rc::Rc<std::cell::RefCell<usize>>,
}

fn costed() -> Costed {
    let (cx, heads) = followed("default");
    let reply_bytes = std::rc::Rc::new(std::cell::RefCell::new(0usize));
    let counted = reply_bytes.clone();
    cx.host().handle::<Ask>(move |query| {
        let reply = answer(&query, "default");
        *counted.borrow_mut() += borsh::to_vec(&reply).unwrap().len();
        Ok(reply)
    });
    Costed {
        cx,
        heads,
        reply_bytes,
    }
}

fn costs(
    costed: &mut Costed,
    label: &str,
    send: impl FnOnce(&Costed),
) -> std::collections::BTreeMap<String, usize> {
    let before = costed.cx.host().requests::<Ask>().len();
    let bytes_before = *costed.reply_bytes.borrow();
    send(costed);
    costed.cx.run_until_parked();
    let asked = costed.cx.host().requests::<Ask>();
    let mut by_kind = std::collections::BTreeMap::new();
    for query in &asked[before..] {
        *by_kind.entry(kind(query)).or_insert(0) += 1;
    }
    eprintln!(
        "AUDIT {label}: {} program.query asks, {} reply bytes, by kind {by_kind:?}",
        asked.len() - before,
        *costed.reply_bytes.borrow() - bytes_before
    );
    by_kind
}

#[test]
fn a_block_costs_the_reads_of_the_tables_it_wrote_to() {
    let mut costed = costed();
    let review = costs(&mut costed, "Readme tab, a review record written", |c| {
        c.heads.forge.send(block(100, vec![review_key()]))
    });
    assert_eq!(
        review.keys().collect::<Vec<_>>(),
        ["Judgment"],
        "the badge's read follows a review: {review:?}"
    );
    let real_review = costs(&mut costed, "Readme tab, a real review block", |c| {
        c.heads.forge.send(block(100, real_review_keys()))
    });
    assert_eq!(
        real_review.keys().collect::<Vec<_>>(),
        ["Activity", "Changes", "Judgment", "Repo", "Repos"],
        "{real_review:?}"
    );
    let pushed = costs(&mut costed, "Readme tab, a ref moved", |c| {
        c.heads.forge.send(block(101, vec![ref_key()]))
    });
    assert_eq!(
        pushed.keys().collect::<Vec<_>>(),
        ["Blob", "Judgment", "Refs", "Tree"],
        "{pushed:?}"
    );
    let repo = forge::tables::REPOS.key(&"project".to_owned());
    let configured = costs(&mut costed, "Readme tab, the repo record written", |c| {
        c.heads.forge.send(block(102, vec![repo]))
    });
    assert_eq!(
        configured.keys().collect::<Vec<_>>(),
        ["Activity", "Repo", "Repos"],
        "{configured:?}"
    );
    let chat = costs(&mut costed, "Readme tab, one Changes<chat> item", |c| {
        c.heads.chat.send(block(103, vec![root_key("general")]))
    });
    assert!(chat.is_empty(), "{chat:?}");
    let names = costed
        .cx
        .host()
        .requests::<ProgramQuery<::chat::Chat>>()
        .len();
    let identity = costs(&mut costed, "Readme tab, one Changes<identity> item", |c| {
        c.heads.identity.send(block(104, Vec::new()))
    });
    assert!(identity.is_empty(), "{identity:?}");
    assert!(
        costed
            .cx
            .host()
            .requests::<ProgramQuery<::chat::Chat>>()
            .len()
            > names,
        "the names re-read"
    );
    let all = costs(&mut costed, "Readme tab, a reopened link (None)", |c| {
        c.heads.forge.send(None)
    });
    assert_eq!(
        all.keys().collect::<Vec<_>>(),
        [
            "Activity", "Archived", "Blob", "Changes", "Judgment", "Refs", "Repo", "Repos", "Tree"
        ],
        "{all:?}"
    );
    assert_eq!(all.values().sum::<usize>(), 11);
    let archived = forge::tables::ARCHIVED.key(&"project".to_owned());
    let archiving = costs(&mut costed, "Readme tab, the repo archived", |c| {
        c.heads.forge.send(block(105, vec![archived]))
    });
    assert_eq!(
        archiving.keys().collect::<Vec<_>>(),
        ["Archived"],
        "the badge's read follows an archive: {archiving:?}"
    );
    costed.cx.simulate_click("forge-tab-changes");
    costed.cx.run_until_parked();
    costed.cx.simulate_click("forge-filter-judgment");
    costed.cx.run_until_parked();
    let judged = costs(
        &mut costed,
        "Judgment filter, a real review block, the forge item",
        |c| c.heads.forge.send(block(106, real_review_keys())),
    );
    assert_eq!(
        judged.keys().collect::<Vec<_>>(),
        ["Activity", "Changes", "Judgment", "Repo", "Repos"],
        "{judged:?}"
    );
    let posted = costs(
        &mut costed,
        "Judgment filter, a real review block, the chat item",
        |c| {
            c.heads
                .chat
                .send(block(106, posted_line_keys("forge:project:1")))
        },
    );
    assert_eq!(
        posted.keys().collect::<Vec<_>>(),
        ["Judgment"],
        "{posted:?}"
    );
}

#[test]
fn a_chat_block_that_answered_a_thread_re_reads_the_judgment() {
    let (mut cx, heads) = followed("judgment");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    let judgments = |cx: &TestAppContext| {
        cx.host()
            .requests::<Ask>()
            .iter()
            .filter(|query| matches!(query, Query::Judgment { .. }))
            .count()
    };
    let asked = judgments(&cx);
    assert!(asked > 0, "the filter asked");
    heads
        .chat
        .send(block(100, vec![member_key("forge:project:1")]));
    cx.run_until_parked();
    assert_eq!(judgments(&cx), asked, "a member seated moves no judgment");
    heads
        .chat
        .send(block(101, vec![answered_key("forge:project:1")]));
    cx.run_until_parked();
    assert_eq!(judgments(&cx), asked + 1, "a thread answered");
    heads.chat.send(None);
    cx.run_until_parked();
    assert_eq!(judgments(&cx), asked + 2, "a reopened chat link");
}

#[test]
fn the_window_is_named_for_the_screen() {
    use view::methods::HostTitle;
    let (mut cx, _) = opened("default");
    assert_eq!(
        cx.host().requests::<HostTitle>(),
        ["Repositories", "project"]
    );
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.host().never::<Ask>();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<HostTitle>(),
        ["Repositories", "project", "project · #1"]
    );
    let (cx, _) = change_screen("default", ChangeTab::Conversation);
    assert_eq!(
        cx.host().requests::<HostTitle>(),
        [
            "Repositories",
            "project",
            "project · #1",
            "project · #1 Review this change"
        ]
    );
}

fn picker_items(cx: &TestAppContext) -> Vec<String> {
    fn gather(node: &wire::Node, items: &mut Vec<String>) {
        if node
            .interactivity()
            .is_some_and(|item| item.role == Some(view::Role::MenuItemRadio))
        {
            items.push(node.key().unwrap_or_default().to_owned());
        }
        node.children()
            .iter()
            .for_each(|child| gather(child, items));
    }
    let mut items = Vec::new();
    if let Some(menu) = cx.find("forge-ref-picker-menu") {
        gather(menu, &mut items);
    }
    items
}

#[test]
fn the_ref_picker_filters_its_branches_and_tags() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    let branch = |name: &str| format!("forge-ref-refs/heads/{name}");
    assert_eq!(
        picker_items(&cx),
        ["main", "clean", "conflict", "feature", "unrelated"].map(branch)
    );
    assert!(cx.has_text("Branches 5") && cx.has_text("Tags 1"));
    cx.simulate_input("forge-ref-picker-filter", "CON");
    cx.run_until_parked();
    assert_eq!(picker_items(&cx), [branch("conflict")]);
    assert!(cx.has_text("Branches 1") && cx.has_text("Tags 0"));
    cx.simulate_input("forge-ref-picker-filter", "zzz");
    cx.run_until_parked();
    assert!(picker_items(&cx).is_empty());
    assert!(cx.has_text("No branches match"), "{:?}", cx.texts());
    cx.simulate_input("forge-ref-picker-filter", "");
    cx.simulate_click("forge-ref-picker-tags");
    cx.run_until_parked();
    assert_eq!(picker_items(&cx), ["forge-ref-refs/tags/v1"]);
    let Some(wire::Node::Field { placeholder, .. }) = cx.find("forge-ref-picker-filter") else {
        panic!("the filter box");
    };
    assert_eq!(placeholder, "Filter tags");
    cx.simulate_input("forge-ref-picker-filter", "v");
    cx.simulate_submit("forge-ref-picker-filter");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.nav().rev.as_deref(), Some(b"refs/tags/v1".as_slice()));
        assert_eq!(forge.menu, None);
    });
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    assert_eq!(picker_items(&cx), ["forge-ref-refs/tags/v1"]);
    assert_eq!(
        cx.interactivity("forge-ref-refs/tags/v1").aria.toggled,
        Some(view::Toggled::True)
    );
}

#[test]
fn the_ref_pickers_tabs_keep_their_arrows_and_a_press_goes_back_to_the_filter() {
    let (mut cx, _view) = opened("default");
    let focused = |cx: &TestAppContext| cx.focused().and_then(wire::Node::key).map(str::to_owned);
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    cx.simulate_focus("forge-ref-picker-kind");
    cx.simulate_key_down("forge-ref-picker-kind", "right");
    cx.run_until_parked();
    assert_eq!(picker_items(&cx), ["forge-ref-refs/tags/v1"]);
    assert_eq!(
        focused(&cx).as_deref(),
        Some("forge-ref-picker-kind"),
        "the arrows keep the keys on the tabs"
    );
    cx.simulate_click("forge-ref-picker-branches");
    cx.run_until_parked();
    assert_eq!(picker_items(&cx).len(), 5);
    assert_eq!(focused(&cx).as_deref(), Some("forge-ref-picker-filter"));
}

#[test]
fn a_long_ref_list_keeps_the_picker_in_the_window() {
    use view::px;
    let rows = |cx: &TestAppContext| {
        let height = cx.style("forge-ref-picker-rows").size.height;
        let (row, inset) = (design::height::ROW as f32, design::spacing::XXS as f32 * 2.);
        (1..=12).find(|rows| height == Some(px(*rows as f32 * row + inset).into()))
    };
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    assert_eq!(rows(&cx), Some(6), "a tall window shows every ref");
    cx.simulate_click("forge-ref-refs/heads/unrelated");
    cx.simulate_resize(1180., 200.);
    cx.simulate_click("forge-ref-picker");
    cx.run_until_parked();
    let shown = rows(&cx).expect("the list is as tall as whole rows");
    assert!(
        shown < 5,
        "a short window shows fewer than the branches: {shown}"
    );
    let mut list = None;
    fn find_list<'a>(node: &'a wire::Node, list: &mut Option<&'a wire::Node>) {
        if matches!(node, wire::Node::UniformList { .. }) {
            *list = Some(node);
        }
        node.children()
            .iter()
            .for_each(|child| find_list(child, list));
    }
    find_list(cx.node("forge-ref-picker-menu"), &mut list);
    let Some(wire::Node::UniformList {
        count,
        scroll_request,
        ..
    }) = list
    else {
        panic!("the items are a virtual list");
    };
    assert_eq!(*count, 5, "every branch is in the list");
    assert_eq!(
        scroll_request
            .as_ref()
            .map(|request| (request.index, request.strategy)),
        Some((4, wire::list::UniformListScrollStrategy::Center)),
        "scrolled to unrelated, the last, with its neighbours"
    );
    view.read(|forge| assert_eq!(forge.menu_cursor, 4));
}

#[test]
fn a_long_ref_name_is_cut_in_the_middle_and_whole_on_hover() {
    use crate::ui::components::{name_cell, whole_name};
    let long = "people/ada/a-rather-long-branch-name-about-the-perf-work-35";
    let (node, styles, _) = view::testing::lower::<Forge, _>(|_| {
        div()
            .id("names")
            .child(whole_name(
                name_cell(long).id("long"),
                long,
                &Theme::default(),
            ))
            .child(whole_name(
                name_cell("main").id("short"),
                "main",
                &Theme::default(),
            ))
    });
    let [long_cell, short_cell] = node.children() else {
        panic!("two cells: {node:?}");
    };
    assert!(long_cell.interactivity().unwrap().tooltip.is_some());
    assert!(
        short_cell
            .interactivity()
            .is_none_or(|it| it.tooltip.is_none())
    );
    let overflow = &styles[long_cell.style().unwrap()].text.text_overflow;
    assert!(
        format!("{overflow:?}").starts_with("Some(TruncateMiddle("),
        "{overflow:?}"
    );
}

#[test]
fn an_archived_repository_wears_its_badge_and_a_live_one_none() {
    let (cx, _) = opened("archived");
    assert!(cx.find("forge-repo-archived").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("Archived"), "{:?}", cx.texts());
    assert!(cx.has_text("project"), "a link still opens it");
    let (cx, _) = opened("default");
    assert!(cx.find("forge-repo-archived").is_none());
}

// The Refs tab of a repository with more than 120 branches reads every branch's standing in one
// Compares, and asks no Compare.
#[test]
fn the_refs_tab_reads_every_branch_standing_in_one_ask() {
    let (mut cx, view) = opened("many-refs");
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let branches = view.read(|forge| forge.branches().len());
    assert!(branches > 120, "{branches} branches");
    let asked = cx.host().requests::<Ask>();
    let count = |kind: fn(&Query) -> bool| asked.iter().filter(|query| kind(query)).count();
    let compares = count(|query| matches!(query, Query::Compares { .. }));
    let compare = count(|query| matches!(query, Query::Compare { .. }));
    assert_eq!((compares, compare), (1, 0));
}

// A refused read stays refused, saying why, until what it reads moves: blocks that wrote none of
// its tables do not ask it again, and a ref moved does.
#[test]
fn a_refused_read_is_asked_again_only_when_what_it_reads_moves() {
    let (mut cx, heads) = followed("default");
    cx.host().handle::<Ask>(|query| match query {
        Query::Compares { .. } => Err(refusal("refused-capacity")),
        query => Ok(answer(&query, "default")),
    });
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    let compares = |cx: &TestAppContext| {
        cx.host()
            .requests::<Ask>()
            .iter()
            .filter(|query| matches!(query, Query::Compares { .. }))
            .count()
    };
    assert_eq!(compares(&cx), 1);
    heads.forge.send(block(100, vec![review_key()]));
    cx.run_until_parked();
    heads.chat.send(block(101, vec![root_key("general")]));
    cx.run_until_parked();
    assert_eq!(compares(&cx), 1, "blocks that moved no ref");
    heads.forge.send(block(102, vec![ref_key()]));
    cx.run_until_parked();
    assert_eq!(compares(&cx), 2, "a ref moved");
}

// A pane out of sight asks nothing: blocks of every program it follows land and none is read,
// until it shows again, when what is on screen is read once.
#[test]
fn a_hidden_pane_reads_nothing_until_it_shows_again() {
    let (mut cx, heads) = followed("default");
    heads.visible.send(false);
    cx.run_until_parked();
    let asked = cx.host().requests::<Ask>().len();
    heads.forge.send(block(100, vec![ref_key()]));
    cx.run_until_parked();
    heads.forge.send(None);
    cx.run_until_parked();
    heads
        .chat
        .send(block(101, vec![answered_key("forge:project:1")]));
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Ask>().len(), asked);
    heads.visible.send(true);
    cx.run_until_parked();
    assert!(cx.host().requests::<Ask>().len() > asked);
}
