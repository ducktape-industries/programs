//! A picture posted from the composer over the fake node, the real chat, files and ownership
//! programs behind it, its commit answered with a prediction before its block lands: the post
//! names the snapshot files minted, and every app draws the picture from that one files address.
#![cfg(not(feature = "notifier"))]
use chat::{Block, Op, PostPolicy, Query, Reply};
use chat_view::Chat;
use std::cell::RefCell;
use std::rc::Rc;
use view::methods::{HostId, HostSession, HostVisible, StoreGet, Submit, Submitted};
use view::rig::Network;
use view::testing::TestAppContext;

const PNG: [u8; 24] = [
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R', 0, 0, 0,
    96, 0, 0, 0, 96,
];

fn app(net: &Network) -> TestAppContext {
    let mut cx = TestAppContext::new();
    net.back(&cx.host());
    cx.host().handle::<StoreGet>(|_| Ok(None));
    let mut minted = 0;
    cx.host().handle::<HostId>(move |prefix| {
        minted += 1;
        Ok(format!("{prefix}-{minted}"))
    });
    let session = cx.host().stream::<HostSession>();
    let visible = cx.host().stream::<HostVisible>();
    cx.open::<Chat>();
    cx.run_until_parked();
    session.send(net.session());
    visible.send(true);
    cx.run_until_parked();
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    cx
}

#[test]
fn a_posted_picture_names_a_snapshot_files_minted_and_every_app_draws_it_from_there() {
    let net = Network::new();
    net.seat::<chat::Chat>(chat::PROGRAM);
    net.seat::<files::Files>(files::PROGRAM);
    net.seat::<ownership::Ownership>(ownership::PROGRAM);
    let me = net.me();
    let room = Op::CreateChannel {
        channel_id: "general".into(),
        name: "General".into(),
        post_policy: PostPolicy::Open,
    };
    net.submit(me, chat::PROGRAM, &room).unwrap();
    let listen = Op::Listen {
        channel_id: "general".into(),
        listening: true,
    };
    net.submit(me, chat::PROGRAM, &listen).unwrap();
    net.device(&[("tape.png", &PNG)]);
    let held: Rc<RefCell<Vec<files::Op>>> = Rc::default();
    let holding = held.clone();
    let next = net.height() + 1;
    net.handle::<Submit<files::Files>>(move |op| {
        holding.borrow_mut().push(op);
        Ok(Submitted::Applied {
            frame: [0; 32],
            until: next,
            output: abi::encode(&files::BlobId::Sha256([4; 32])),
        })
    });

    let mut sender = app(&net);
    sender.simulate_click("draft-general/attach");
    sender.run_until_parked();
    sender.simulate_focus("draft-general/editor");
    sender.simulate_input("draft-general/editor", "a picture");
    sender.simulate_field_key("draft-general/editor", "enter");
    sender.run_until_parked();
    for op in held.take() {
        net.submit(me, files::PROGRAM, &op)
            .expect("the commit lands");
    }
    sender.simulate_resize(1000., 700.);
    sender.run_until_parked();

    let roots = Query::Roots {
        channel_id: "general".into(),
        viewer: vec![],
        page: chat::PageRequest {
            after: None,
            limit: None,
        },
    };
    let Reply::Roots(page) = net.query(chat::PROGRAM, &roots).unwrap() else {
        panic!("roots")
    };
    let [row] = page.items.as_slice() else {
        panic!("one post: {:?}", page.items)
    };
    let Some(file) = row.blocks.iter().find_map(|block| match block {
        Block::Attachment(file) => Some(file.clone()),
        _ => None,
    }) else {
        panic!("the post carries the picture: {:?}", row.blocks)
    };
    let named = files::Query::Stat {
        path: file.path.clone(),
        at: Some(files::BlobId::Sha256(file.snapshot)),
    };
    let held: files::Reply = net
        .query(files::PROGRAM, &named)
        .expect("files minted the snapshot the post names");
    let files::Reply::Stat { entry, .. } = held else {
        panic!("a stat")
    };
    assert!(entry.is_some(), "files holds the file at that snapshot");

    let picture = format!("chat-message-{}-block-1/picture", row.message_id);
    assert!(
        sender.find(&picture).is_some(),
        "the sender's own post: {:?}",
        sender.texts()
    );
    let other = app(&net);
    assert!(
        other.find(&picture).is_some(),
        "another app: {:?}",
        other.texts()
    );
}
