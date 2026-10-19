//! Files in the composer: picked, pasted or dropped, put at once and
//! committed under the draft's message, carried by the post; and the files
//! a message carries, drawn, and saved through the host.
use super::*;
use files::BlobId;
use view::methods::{
    BlobGet, ClipboardFiles, FilePick, FileRelease, FileSave, OpPut, Pasted, Picked, Putting,
    SaveFile,
};

const SNAPSHOT: [u8; 32] = [5; 32];
const OBJECT: [u8; 32] = [6; 32];
const CHUNK: [u8; 32] = [9; 32];
const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

fn picked(name: &str, size: u64, file: u64) -> Picked {
    Picked {
        name: name.into(),
        size,
        file,
    }
}

// what files answers: each path a file of 3 bytes at one snapshot, its
// object one chunk, its bytes a picture's
fn files_host(cx: &mut TestAppContext) {
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host()
        .handle::<Submit<files::Files>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.host()
        .handle::<Submit<access::Access>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.host()
        .handle::<Ask<files::Files>>(|query| Ok(files_reply(query)));
    let object = files::FileObj {
        size: 3,
        chunks: vec![BlobId::Sha256(CHUNK)],
        meta: Default::default(),
    };
    cx.host()
        .handle::<BlobGet>(move |_| Ok(Some(abi::encode(&object))));
}

// every put held at once
fn holds_puts(cx: &mut TestAppContext) {
    cx.host()
        .ends::<OpPut>(|put| Ok(Putting::Held(vec![CHUNK; put.blobs.len()])));
}

fn files_reply(query: files::Query) -> files::Reply {
    match query {
        files::Query::Stat { path, .. } => {
            let node = files::Node::File {
                file: BlobId::Sha256(OBJECT),
                size: 3,
                exec: false,
            };
            files::Reply::Stat {
                snapshot: Some(BlobId::Sha256(SNAPSHOT)),
                entry: Some(files::EntryInfo {
                    path,
                    node,
                    meta: Default::default(),
                }),
            }
        }
        files::Query::Read { .. } => files::Reply::Contents(files::Contents {
            snapshot: BlobId::Sha256(SNAPSHOT),
            size: PNG.len() as u64,
            offset: 0,
            bytes: PNG.to_vec(),
            eof: true,
        }),
        files::Query::Head => files::Reply::Head(Some(files::SnapshotRow {
            id: BlobId::Sha256(SNAPSHOT),
            parent: None,
            root: BlobId::Sha256(OBJECT),
            author: Principal::Account(7),
            message: "head".into(),
            seq: 1,
            height: 1,
            time: 0,
        })),
        query => panic!("unexpected files query: {query:?}"),
    }
}

// what the files commits put: each path, and the message its meta names
fn puts(cx: &TestAppContext) -> Vec<(String, String)> {
    let commits = cx.host().requests::<Submit<files::Files>>();
    commits
        .into_iter()
        .flat_map(|op| match op {
            files::Op::Commit { changes, .. } => changes,
            op => panic!("a commit: {op:?}"),
        })
        .filter_map(|change| match change {
            files::Change::Put { path, meta, .. } => Some((path, meta["source.message"].clone())),
            _ => None,
        })
        .collect()
}

fn carried(blocks: &[chat::Block]) -> Vec<&str> {
    blocks
        .iter()
        .filter_map(|block| match block {
            chat::Block::Attachment(file) => Some(file.path.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn picked_files_go_up_at_once_under_the_drafts_message_and_the_post_carries_them() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host().handle::<FilePick>(|pick| {
        assert!(pick.multiple);
        Ok(vec![picked("notes.txt", 3, 1), picked("notes.txt", 3, 2)])
    });
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    let folder = "/chat/general/message-1";
    let first = format!("{folder}/notes.txt");
    let second = format!("{folder}/notes (2).txt");
    let message = String::from("message-1");
    assert_eq!(
        puts(&cx),
        [(first.clone(), message.clone()), (second.clone(), message)]
    );
    let commits = cx.host().requests::<Submit<files::Files>>();
    let files::Op::Commit { base, changes, .. } = &commits[0] else {
        panic!("a commit")
    };
    let files::Change::Put { meta, .. } = &changes[0] else {
        panic!("a put")
    };
    assert_eq!(*base, None, "an upload only creates");
    assert_eq!(meta["uploader"], "7");
    assert_eq!(meta["source.program"], "chat");
    assert_eq!(meta["source.channel"], "general");
    assert_eq!(meta["content-type"], "text/plain");
    assert_eq!(meta["name"], "notes.txt");
    // the host lets each file go once its chunks are held
    assert_eq!(cx.host().requests::<FileRelease>(), [vec![1], vec![2]]);
    assert!(cx.has_text("notes (2).txt"), "{:?}", cx.texts());
    cx.simulate_input("draft-general/editor", "two files");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let posted = cx.host().requests::<Submit<::chat::Chat>>();
    let Some(Op::PostMessage {
        message_id, blocks, ..
    }) = posted.last()
    else {
        panic!("a post")
    };
    assert_eq!(message_id, "message-1");
    assert_eq!(carried(blocks), [first.as_str(), second.as_str()]);
    view.read(|chat| assert!(chat.drafts["draft-general"].attachments.is_empty()));
}

#[test]
fn a_send_waits_for_a_file_still_going_up_and_its_chip_says_how_far() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    let putting = cx.host().stream::<OpPut>();
    let size = 3 * files::CHUNK_SIZE;
    cx.host()
        .handle::<FilePick>(move |_| Ok(vec![picked("big.bin", size, 1)]));
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    let all = crate::composer::attachments::framed(size);
    let sent = all / 3;
    putting.send(Putting::Sent(sent));
    cx.run_until_parked();
    let said = format!("Uploading {}%", sent * 100 / all);
    assert!(cx.has_text(&said), "{:?}", cx.texts());
    cx.simulate_input("draft-general/editor", "wait for it");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let posts = || cx.host().requests::<Submit<::chat::Chat>>();
    assert!(posts().is_empty(), "the send waits for the file");
    putting.send(Putting::Held(vec![CHUNK; 3]));
    cx.run_until_parked();
    let posted = cx.host().requests::<Submit<::chat::Chat>>();
    let [Op::PostMessage { blocks, .. }] = posted.as_slice() else {
        panic!("one post: {posted:?}")
    };
    assert_eq!(carried(blocks), ["/chat/general/message-1/big.bin"]);
    assert_eq!(blocks.len(), 2, "the text, then the file");
}

#[test]
fn a_file_that_fails_says_why_and_holds_the_send_back() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    cx.host()
        .refuse::<OpPut>("node_failed", "the node went away");
    cx.host()
        .handle::<FilePick>(|_| Ok(vec![picked("a.txt", 3, 1)]));
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    assert!(cx.has_text("the node went away"), "{:?}", cx.texts());
    view.read(|chat| assert!(!chat.drafts["draft-general"].can_send()));
    cx.simulate_click("draft-general/file/1/remove");
    cx.run_until_parked();
    view.read(|chat| assert!(chat.drafts["draft-general"].attachments.is_empty()));
}

#[test]
fn the_paste_key_attaches_the_clipboards_files_else_puts_its_text_over_the_selection() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    let mut pastes = vec![
        Pasted {
            files: Vec::new(),
            text: "pasted".into(),
        },
        Pasted {
            files: vec![picked("image.png", 3, 4)],
            text: String::new(),
        },
    ];
    cx.host()
        .handle::<ClipboardFiles>(move |()| Ok(pastes.pop().unwrap()));
    cx.simulate_focus("draft-general/editor");
    cx.simulate_field_key("draft-general/editor", "cmd-v");
    cx.run_until_parked();
    assert_eq!(
        puts(&cx),
        [(
            "/chat/general/message-1/image.png".into(),
            "message-1".into()
        )]
    );
    cx.simulate_field_key("draft-general/editor", "cmd-v");
    cx.run_until_parked();
    view.read(|chat| assert_eq!(chat.drafts["draft-general"].field.text(), "pasted"));
}

#[test]
fn files_dropped_on_the_composer_attach_and_one_taken_out_is_removed_again() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.simulate_file_drop("draft-general/drop", vec![picked("drop.txt", 3, 5)]);
    cx.run_until_parked();
    let path = String::from("/chat/general/message-1/drop.txt");
    assert_eq!(puts(&cx), [(path.clone(), "message-1".into())]);
    cx.simulate_click("draft-general/file/1/remove");
    cx.run_until_parked();
    let commits = cx.host().requests::<Submit<files::Files>>();
    let Some(files::Op::Commit { base, changes, .. }) = commits.last() else {
        panic!("a commit")
    };
    assert_eq!(*base, Some(BlobId::Sha256(SNAPSHOT)));
    assert_eq!(changes, &[files::Change::Remove { path }]);
}

#[test]
fn a_message_draws_its_picture_and_saves_its_other_files_through_the_host() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    cx.host().handle::<FileSave>(|_| Ok(true));
    let file = |name: &str, content_type: &str| {
        chat::Block::Attachment(chat::message::Attachment {
            path: format!("/chat/general/m3/{name}"),
            snapshot: SNAPSHOT,
            name: name.into(),
            size: 3,
            content_type: content_type.into(),
        })
    };
    let blocks = vec![
        file("tape.png", "image/png"),
        file("plan.pdf", "application/pdf"),
    ];
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![
                    row(1, 7, "hello"),
                    MsgRow {
                        blocks: blocks.clone(),
                        ..row(3, 8, "")
                    },
                ])),
                query => chat_reply(query),
            })
        }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    view.read(|chat| {
        let shown = |name: &str| {
            let key = (format!("/chat/general/m3/{name}"), SNAPSHOT);
            chat.shown_files[&key].clone()
        };
        assert!(shown("tape.png").picture.is_some());
        assert!(shown("plan.pdf").picture.is_none());
    });
    assert!(cx.find("chat-message-m3-block-0/save").is_none());
    cx.simulate_click("chat-message-m3-block-1/save");
    cx.run_until_parked();
    let save = SaveFile {
        name: "plan.pdf".into(),
        blobs: vec![CHUNK],
    };
    assert_eq!(cx.host().requests::<FileSave>(), [save]);
}

// A phone's tap on a picture opens it: the host is handed its chunks, as
// Save hands them, for the system's viewer (J-075).
#[test]
fn a_tap_on_a_picture_on_a_phone_hands_it_to_the_host_to_open() {
    let mut cx = TestAppContext::new();
    configure(&mut cx);
    let _view = cx.open::<Chat>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    seat(&mut cx);
    files_host(&mut cx);
    cx.host().handle::<FileSave>(|_| Ok(true));
    let picture = chat::Block::Attachment(chat::message::Attachment {
        path: "/chat/general/m3/tape.png".into(),
        snapshot: SNAPSHOT,
        name: "tape.png".into(),
        size: 3,
        content_type: "image/png".into(),
    });
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![MsgRow {
                    blocks: vec![picture.clone()],
                    ..row(3, 8, "")
                }])),
                query => chat_reply(query),
            })
        }));
    cx.simulate_click("chat-rooms-channel-general");
    cx.run_until_parked();
    cx.simulate_click("chat-message-m3-block-0");
    cx.run_until_parked();
    let opened = SaveFile {
        name: "tape.png".into(),
        blobs: vec![CHUNK],
    };
    assert_eq!(cx.host().requests::<FileSave>(), [opened]);
}

// A message names the snapshot the node predicted as its file went up; the
// block that carried the commit minted another (#duckpond 73's picture).
// The file is read where it stands, and drawn (J-075).
#[test]
fn a_picture_is_read_where_it_stands_whatever_snapshot_its_message_names() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    cx.host().handle::<Ask<files::Files>>(|query| match &query {
        files::Query::Stat { at: Some(_), .. } | files::Query::Read { at: Some(_), .. } => Err(
            view::host::Error::new("not_found", "files minted no snapshot"),
        ),
        _ => Ok(files_reply(query)),
    });
    let picture = chat::Block::Attachment(chat::message::Attachment {
        path: "/chat/general/m3/image.png".into(),
        snapshot: SNAPSHOT,
        name: "image.png".into(),
        size: 3,
        content_type: "image/png".into(),
    });
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![MsgRow {
                    blocks: vec![picture.clone()],
                    ..row(3, 8, "")
                }])),
                query => chat_reply(query),
            })
        }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    view.read(|chat| {
        let key = ("/chat/general/m3/image.png".to_owned(), SNAPSHOT);
        let shown = &chat.shown_files[&key];
        assert!(shown.picture.is_some(), "drawn");
        assert_eq!(shown.chunks.as_deref(), Some(&[CHUNK][..]), "and saved");
    });
}

// A file that goes up is its uploader's alone from then on: the message
// reads it where it stands, so no one else may write it again (J-075).
#[test]
fn a_file_that_goes_up_is_listed_its_uploaders_alone() {
    let (mut cx, _) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host()
        .handle::<FilePick>(|_| Ok(vec![picked("tape.png", 3, 1)]));
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    let put = puts(&cx);
    let [(path, _)] = put.as_slice() else {
        panic!("one file put: {put:?}")
    };
    let owned = access::Op::Set {
        program: files::PROGRAM.into(),
        key: path.clone(),
        list: Some(::program::Ownership::Owner),
    };
    assert_eq!(cx.host().requests::<Submit<access::Access>>(), [owned]);
}

// A click on a picture opens it whole over the room: Save hands the host
// its chunks, Open in Files opens its path in Files and closes it, and a
// press outside it closes it too.
#[test]
fn a_click_on_a_picture_opens_it_whole_with_save_and_open_in_files() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    cx.host().handle::<FileSave>(|_| Ok(true));
    let picture = chat::Block::Attachment(chat::message::Attachment {
        path: "/chat/general/m3/tape.png".into(),
        snapshot: SNAPSHOT,
        name: "tape.png".into(),
        size: 3,
        content_type: "image/png".into(),
    });
    cx.host()
        .handle::<Ask<::chat::Chat>>(rooms_alone(move |query| {
            Ok(match query {
                Query::Roots { .. } => Reply::Roots(page(vec![MsgRow {
                    blocks: vec![picture.clone()],
                    ..row(3, 8, "")
                }])),
                query => chat_reply(query),
            })
        }));
    cx.host()
        .stream::<Changes<::chat::Chat>>()
        .send(block(4, posted("general")));
    cx.run_until_parked();
    assert!(cx.find("chat-picture").is_none(), "closed until clicked");
    cx.simulate_click("chat-message-m3-block-0");
    cx.run_until_parked();
    assert!(cx.find("chat-picture/image").is_some(), "it opens whole");
    cx.simulate_click("chat-picture/save");
    cx.run_until_parked();
    let save = SaveFile {
        name: "tape.png".into(),
        blobs: vec![CHUNK],
    };
    assert_eq!(cx.host().requests::<FileSave>(), [save]);
    cx.simulate_click("chat-picture/open");
    cx.run_until_parked();
    let opened = cx.host().requests::<view::methods::LinkOpen>();
    let files = "duck://testnet-0a1b2c3d/files/chat/general/m3/tape.png";
    assert_eq!(opened.last().map(String::as_str), Some(files));
    assert!(
        cx.find("chat-picture").is_none(),
        "Files opens in its place"
    );
    cx.simulate_click("chat-message-m3-block-0");
    cx.run_until_parked();
    assert!(cx.find("chat-picture").is_some());
    cx.simulate_dismiss("chat-picture-overlay");
    cx.run_until_parked();
    assert!(
        cx.find("chat-picture").is_none(),
        "a press outside closes it"
    );
    view.read(|chat| assert!(chat.enlarged.is_none()));
}
