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

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    bytes.extend(13u32.to_be_bytes());
    bytes.extend(b"IHDR");
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes
}

fn picked(name: &str, size: u64, file: u64) -> Picked {
    Picked {
        name: name.into(),
        size,
        file,
    }
}

fn files_host(cx: &mut TestAppContext) {
    cx.host().handle::<HostId>(|kind| Ok(format!("{kind}-1")));
    cx.host()
        .handle::<Submit<files::Files>>(|_| Ok(view::testing::applied(Vec::new())));
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
        files::Query::Read { .. } => contents(png(96, 96)),
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
    assert_eq!(cx.host().requests::<FileRelease>(), [vec![1], vec![2]]);
    assert!(cx.has_text("notes (2).txt"), "{:?}", cx.texts());
    cx.simulate_input("draft-general/editor", "two files");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let posted = posts_of(&cx);
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
    assert!(posts_of(&cx).is_empty(), "the send waits for the file");
    putting.send(Putting::Held(vec![CHUNK; 3]));
    cx.run_until_parked();
    let posted = posts_of(&cx);
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

fn contents(bytes: Vec<u8>) -> files::Reply {
    files::Reply::Contents(files::Contents {
        snapshot: BlobId::Sha256(SNAPSHOT),
        size: bytes.len() as u64,
        offset: 0,
        bytes,
        eof: true,
    })
}

fn picture_room(cx: &mut TestAppContext, size: (u32, u32)) {
    files_host(cx);
    cx.host().handle::<Ask<files::Files>>(move |query| {
        Ok(match query {
            files::Query::Read { .. } => contents(png(size.0, size.1)),
            query => files_reply(query),
        })
    });
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
}

fn drawn_at(cx: &TestAppContext, key: &str, (width, height): (f32, f32)) {
    let size = &cx.style(key).size;
    let named = (size.width, size.height);
    assert_eq!(
        named,
        (Some(px(width).into()), Some(px(height).into())),
        "{key}"
    );
}

#[test]
fn a_picture_in_a_message_draws_at_its_own_size_up_to_its_bound() {
    let sizes = [
        ((96, 96), (96., 96.)),
        ((4000, 1000), (360., 90.)),
        ((1000, 4000), (60., 240.)),
    ];
    for (size, at) in sizes {
        let (mut cx, _view) = opened();
        picture_room(&mut cx, size);
        drawn_at(&cx, "chat-message-m3-block-0/picture", at);
        let button = cx.style("chat-message-m3-block-0");
        let aligned = format!("{:?}", button.align_self);
        assert_eq!(aligned, "Some(Start)", "at the start of its column");
    }
}

#[test]
fn a_picture_bigger_than_the_window_opens_whole_inside_it() {
    let inset = 2. * f32::from(crate::ui::dialogs::INSET);
    let bar = f32::from(view::design::space::SM) + f32::from(view::design::POINTER.hit);
    let room = (800. - inset, 600. - inset - bar);
    for size in [(4000, 1000), (1000, 4000)] {
        let (mut cx, _view) = opened();
        cx.simulate_resize(800., 600.);
        picture_room(&mut cx, size);
        cx.simulate_click("chat-message-m3-block-0");
        cx.run_until_parked();
        let (width, height) = view::picture::fit(size, room);
        assert!(width <= room.0 && height <= room.1, "{size:?} fits");
        assert!(width == room.0 || height == room.1, "{size:?} fills a side");
        drawn_at(&cx, "chat-picture/image", (width, height));
    }
}

fn sends_the_picture(cx: &mut TestAppContext) {
    cx.run_until_parked();
    let path = "/chat/general/message-1/image.png";
    let commits = cx.host().requests::<Submit<files::Files>>();
    let [
        files::Op::Commit {
            base: None,
            changes,
            ..
        },
    ] = commits.as_slice()
    else {
        panic!("one commit that only creates: {commits:?}")
    };
    let put = matches!(changes.as_slice(), [files::Change::Put { path: at, .. }] if at == path);
    assert!(put, "{changes:?}");
    let listed = cx.host().requests::<Submit<access::Access>>();
    assert!(listed.is_empty(), "nothing else asked: {listed:?}");
    cx.simulate_input("draft-general/editor", "look");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let posted = posts_of(cx);
    let [
        Op::PostMessage {
            message_id, blocks, ..
        },
    ] = posted.as_slice()
    else {
        panic!("one post: {posted:?}")
    };
    assert_eq!(message_id, "message-1");
    assert_eq!(carried(blocks), [path]);
}

#[test]
fn a_picked_picture_is_committed_and_posted() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host()
        .handle::<FilePick>(|_| Ok(vec![picked("image.png", 3, 1)]));
    cx.simulate_click("draft-general/attach");
    sends_the_picture(&mut cx);
}

#[test]
fn a_pasted_picture_is_committed_and_posted_as_a_picked_one_is() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host().handle::<ClipboardFiles>(|()| {
        Ok(Pasted {
            files: vec![picked("image.png", 3, 1)],
            text: String::new(),
        })
    });
    cx.simulate_focus("draft-general/editor");
    cx.simulate_field_key("draft-general/editor", "cmd-v");
    sends_the_picture(&mut cx);
}

#[test]
fn a_dropped_picture_is_committed_and_posted_as_a_picked_one_is() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.simulate_file_drop("draft-general/drop", vec![picked("image.png", 3, 1)]);
    sends_the_picture(&mut cx);
}

#[test]
fn a_name_filed_under_the_message_stays_taken_after_its_chip_is_gone() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    let mut first = true;
    cx.host().handle::<Submit<files::Files>>(move |_| {
        Ok(match std::mem::take(&mut first) {
            true => view::methods::Submitted::InDoubt {
                frame: [0; 32],
                until: 9,
            },
            false => view::testing::applied(Vec::new()),
        })
    });
    cx.simulate_file_drop("draft-general/drop", vec![picked("image.png", 3, 1)]);
    cx.run_until_parked();
    cx.simulate_click("draft-general/file/1/remove");
    cx.run_until_parked();
    cx.simulate_file_drop("draft-general/drop", vec![picked("image.png", 3, 2)]);
    cx.run_until_parked();
    let paths: Vec<String> = puts(&cx).into_iter().map(|(path, _)| path).collect();
    let folder = "/chat/general/message-1";
    let named = [
        format!("{folder}/image.png"),
        format!("{folder}/image (2).png"),
    ];
    assert_eq!(paths, named);
}

#[test]
fn a_refused_message_id_fails_the_files_once_and_asks_for_none_again() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host().never::<HostId>();
    cx.simulate_file_drop("draft-general/drop", vec![picked("image.png", 3, 1)]);
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<HostId>().len(), 1, "one id asked");
    cx.host().refuse::<HostId>("invalid", "no ids here");
    cx.host().answer_held::<HostId>();
    cx.host().never::<HostId>();
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<HostId>().len(),
        1,
        "a refusal asks for no other"
    );
    assert!(cx.has_text("no ids here"), "{:?}", cx.texts());
}

#[test]
fn a_long_refusal_shows_whole_on_the_files_chip() {
    let (mut cx, _view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    let path = "/chat/general/message-1/image.png";
    let said = format!("{path} changed since the base snapshot: read it again and retry");
    cx.host().handle::<Submit<files::Files>>(move |_| {
        Ok(view::methods::Submitted::Refused {
            frame: [0; 32],
            until: 9,
            refusal: view::host::Error::new("stale", said.clone()),
        })
    });
    cx.simulate_file_drop("draft-general/drop", vec![picked("image.png", 3, 1)]);
    cx.run_until_parked();
    let whole = "That didn’t go through: this file changed since the base snapshot: \
                 read it again and retry";
    assert!(cx.has_text(whole), "{:?}", cx.texts());
    let status = cx.style("draft-general/file/1/status");
    assert_eq!(
        status.text.text_overflow, None,
        "never cut with an ellipsis"
    );
}

const PREDICTED: [u8; 32] = [4; 32];

type There = std::rc::Rc<std::cell::Cell<bool>>;
type Touched = std::rc::Rc<std::cell::RefCell<Vec<String>>>;

fn files_until(cx: &TestAppContext, there: There, touched: Touched) {
    cx.host()
        .handle::<Ask<files::Files>>(move |query| match query {
            files::Query::Stat { .. } if !there.get() => Ok(files::Reply::Stat {
                snapshot: Some(BlobId::Sha256(PREDICTED)),
                entry: None,
            }),
            files::Query::Diff { .. } => {
                let written = touched.borrow();
                let entries = written.iter().map(|path| files::DiffEntry {
                    path: path.clone(),
                    kind: files::DiffKind::Modified,
                });
                Ok(files::Reply::Diff(entries.collect()))
            }
            query => Ok(files_reply(query)),
        });
}

fn applied_until(until: u64) -> view::methods::Submitted {
    view::methods::Submitted::Applied {
        frame: [0; 32],
        until,
        output: abi::encode(&BlobId::Sha256(PREDICTED)),
    }
}

fn chip(view: &Entity<Chat>) -> (bool, Option<String>) {
    view.read(|chat| {
        let file = &chat.drafts["draft-general"].attachments[0];
        (
            file.landed().is_some(),
            file.failed().map(|why| why.to_string()),
        )
    })
}

#[test]
fn a_chip_waits_through_other_files_blocks_and_says_so_once_its_frame_has_lapsed() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host()
        .handle::<Submit<files::Files>>(|_| Ok(applied_until(9)));
    let there = There::default();
    files_until(&cx, there.clone(), Touched::default());
    let moved = cx.host().stream::<Changes<files::Files>>();
    cx.host()
        .handle::<FilePick>(|_| Ok(vec![picked("image.png", 3, 1)]));
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    moved.send(block(5, Vec::new()));
    cx.run_until_parked();
    assert_eq!(chip(&view), (false, None), "another's block: still waiting");
    moved.send(block(10, Vec::new()));
    cx.run_until_parked();
    let lapsed = Some(crate::words::NOT_LANDED.to_owned());
    assert_eq!(chip(&view), (false, lapsed), "past its frame's last height");
}

#[test]
fn a_commit_whose_answer_was_lost_lands_once_files_holds_it() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    cx.host().handle::<Submit<files::Files>>(|_| {
        Ok(view::methods::Submitted::InDoubt {
            frame: [0; 32],
            until: 9,
        })
    });
    let there = There::default();
    files_until(&cx, there.clone(), Touched::default());
    let moved = cx.host().stream::<Changes<files::Files>>();
    cx.host()
        .handle::<FilePick>(|_| Ok(vec![picked("image.png", 3, 1)]));
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    assert_eq!(chip(&view), (false, None), "waiting, not failed");
    there.set(true);
    moved.send(block(6, Vec::new()));
    cx.run_until_parked();
    assert_eq!(chip(&view), (true, None), "landed");
}

#[test]
fn a_file_lands_on_the_snapshot_its_block_minted_and_the_post_waits_for_that_block() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    holds_puts(&mut cx);
    let predicted = BlobId::Sha256(PREDICTED);
    cx.host().handle::<Submit<files::Files>>(move |_| {
        Ok(view::testing::applied(abi::encode(&predicted)))
    });
    let there = There::default();
    files_until(&cx, there.clone(), Touched::default());
    let moved = cx.host().stream::<Changes<files::Files>>();
    cx.host()
        .handle::<FilePick>(|_| Ok(vec![picked("image.png", 3, 1)]));
    cx.simulate_click("draft-general/attach");
    cx.run_until_parked();
    assert!(!cx.has_text(crate::words::NOT_THERE), "{:?}", cx.texts());
    cx.simulate_focus("draft-general/editor");
    cx.simulate_input("draft-general/editor", "look");
    cx.simulate_field_key("draft-general/editor", "enter");
    cx.run_until_parked();
    let posts = |cx: &TestAppContext| posts_of(cx);
    assert!(posts(&cx).is_empty(), "the post waits for the block");
    there.set(true);
    moved.send(block(5, Vec::new()));
    cx.run_until_parked();
    let posted = posts(&cx);
    let [Op::PostMessage { blocks, .. }] = posted.as_slice() else {
        panic!("one post: {posted:?}")
    };
    let named: Vec<[u8; 32]> = blocks
        .iter()
        .filter_map(|block| match block {
            chat::Block::Attachment(file) => Some(file.snapshot),
            _ => None,
        })
        .collect();
    assert_eq!(named, [SNAPSHOT], "the snapshot files holds it at");
    view.read(|chat| assert!(chat.drafts["draft-general"].attachments.is_empty()));
}

const M3: &str = "/chat/general/m3/tape.png";

fn lands_a_picture(cx: &TestAppContext) {
    let picture = chat::Block::Attachment(chat::message::Attachment {
        path: M3.into(),
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
}

fn drawn(view: &Entity<Chat>) -> bool {
    view.read(|chat| {
        let key = (M3.to_owned(), SNAPSHOT);
        chat.shown_files
            .get(&key)
            .is_some_and(|shown| shown.picture.is_some())
    })
}

fn stats(cx: &TestAppContext) -> usize {
    let asked = cx.host().requests::<Ask<files::Files>>();
    asked
        .iter()
        .filter(|query| matches!(query, files::Query::Stat { .. }))
        .count()
}

#[test]
fn a_picture_files_holds_nothing_at_yet_is_read_again_once_a_block_writes_its_path() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    let there = There::default();
    let touched = Touched::default();
    files_until(&cx, there.clone(), touched.clone());
    let moved = cx.host().stream::<Changes<files::Files>>();
    lands_a_picture(&cx);
    cx.run_until_parked();
    assert!(!drawn(&view), "files holds nothing there yet");
    assert!(cx.find("chat-message-m3-block-0/picture").is_none());
    let asked = stats(&cx);
    touched
        .borrow_mut()
        .push("/chat/general/m9/other.png".into());
    moved.send(block(5, Vec::new()));
    cx.run_until_parked();
    assert_eq!(
        stats(&cx),
        asked,
        "a block that wrote elsewhere reads nothing again"
    );
    there.set(true);
    touched.borrow_mut().push(M3.into());
    moved.send(block(6, Vec::new()));
    cx.run_until_parked();
    assert!(drawn(&view), "read again on the block that wrote it");
    assert!(cx.find("chat-message-m3-block-0/picture").is_some());
}

#[test]
fn a_picture_whose_read_failed_is_read_again_on_the_next_files_block() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    let failing = There::default();
    failing.set(true);
    let object = files::FileObj {
        size: 3,
        chunks: vec![BlobId::Sha256(CHUNK)],
        meta: Default::default(),
    };
    let fails = failing.clone();
    cx.host().handle::<BlobGet>(move |_| match fails.get() {
        true => Err(view::host::Error::new(
            "unavailable",
            "the read was dropped",
        )),
        false => Ok(Some(abi::encode(&object))),
    });
    let moved = cx.host().stream::<Changes<files::Files>>();
    lands_a_picture(&cx);
    cx.run_until_parked();
    assert!(!drawn(&view), "its read failed");
    failing.set(false);
    moved.send(block(5, Vec::new()));
    cx.run_until_parked();
    assert!(drawn(&view), "read again on the next files block");
}

#[test]
fn a_block_that_lands_a_file_while_it_is_read_again_is_heard() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    let there = There::default();
    let touched = Touched::default();
    files_until(&cx, there.clone(), touched.clone());
    let moved = cx.host().stream::<Changes<files::Files>>();
    lands_a_picture(&cx);
    cx.run_until_parked();
    touched.borrow_mut().push(M3.into());
    cx.host().never::<Ask<files::Files>>();
    moved.send(block(5, Vec::new()));
    cx.run_until_parked();
    assert!(moved.subscribed(), "followed while its read is out");
    there.set(true);
    moved.send(block(6, Vec::new()));
    files_until(&cx, there.clone(), touched.clone());
    cx.host().answer_held::<Ask<files::Files>>();
    cx.run_until_parked();
    assert!(drawn(&view), "the landing block was heard");
}

fn posts_of(cx: &TestAppContext) -> Vec<Op> {
    let ops = cx.host().requests::<Submit<::chat::Chat>>();
    let posts = ops
        .into_iter()
        .filter(|op| matches!(op, Op::PostMessage { .. }));
    posts.collect()
}

#[test]
fn a_read_that_keeps_failing_is_read_again_once_then_waits_for_a_reopen() {
    let (mut cx, view) = opened();
    files_host(&mut cx);
    cx.host().handle::<BlobGet>(|_| {
        Err(view::host::Error::new(
            "unavailable",
            "the read was dropped",
        ))
    });
    let moved = cx.host().stream::<Changes<files::Files>>();
    lands_a_picture(&cx);
    cx.run_until_parked();
    let gets = |cx: &TestAppContext| cx.host().requests::<BlobGet>().len();
    assert_eq!(gets(&cx), 1, "read once");
    moved.send(block(5, Vec::new()));
    cx.run_until_parked();
    assert_eq!(gets(&cx), 2, "read again on the next files block");
    moved.send(block(6, Vec::new()));
    cx.run_until_parked();
    assert_eq!(gets(&cx), 2, "then on no files block");
    moved.send(None);
    cx.run_until_parked();
    assert_eq!(gets(&cx), 3, "read again once the link reopens");
    assert!(!drawn(&view));
}
