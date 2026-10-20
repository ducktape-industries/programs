//! Each account's picture is its face wherever forge draws one, read once per blob from
//! identity's cards: owners, a change's timeline and reviewers, the settings' access rows; the
//! phone's root avatar is the reader's.
use super::*;
use view::methods::BlobGet;
use view::wire::Node;

// Ada (1), Rae (2), Tal (4) and Wren (9), each with a picture.
fn pictured(cx: &mut TestAppContext) {
    let account = |number: u64| identity::Account {
        number,
        card: identity::Card {
            name: String::new(),
            avatar: Some(abi::BlobId::Sha256([number as u8; 32])),
            bio: None,
            updated_at: 0,
        },
        control: identity::Control::User { keys: Vec::new() },
    };
    cx.host().handle::<ProgramQuery<Identity>>(move |query| {
        Ok(match query {
            identity::Query::List { .. } => identity::Reply::Accounts(identity::PageResponse {
                height: 1,
                items: [1, 2, 4, 9].map(account).into(),
                next: None,
            }),
            identity::Query::Profile { .. } => identity::Reply::Profile(None),
            query => panic!("unexpected identity query: {query:?}"),
        })
    });
    cx.host()
        .handle::<BlobGet>(|_| Ok(Some(b"\x89PNG\r\n\x1a\nface".to_vec())));
}

fn holds_picture(node: &Node) -> bool {
    matches!(node, Node::Image { .. }) || node.children().iter().any(holds_picture)
}

fn pictured_at(cx: &TestAppContext, key: &str) -> bool {
    cx.find(key).is_some_and(holds_picture)
}

fn read_once(cx: &TestAppContext) {
    let reads = cx.host().requests::<BlobGet>();
    let blobs: std::collections::BTreeSet<&String> = reads.iter().collect();
    assert!(!reads.is_empty(), "a picture was read");
    assert_eq!(
        blobs.len(),
        reads.len(),
        "each picture read once: {reads:?}"
    );
}

// Forge seated as Ada, `project`'s owner, under a pointer or (`touch`) a finger.
fn seated(touch: bool) -> TestAppContext {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "reviewed");
    pictured(&mut cx);
    let props = cx.host().stream::<HostSession>();
    cx.open::<Forge>();
    match touch {
        true => {
            cx.simulate_touch(true);
            cx.simulate_resize(390., 844.);
        }
        false => cx.simulate_resize(1180., 760.),
    }
    cx.run_until_parked();
    props.send(seat(b"owner", Some(1)));
    cx.run_until_parked();
    cx
}

fn press(cx: &mut TestAppContext, keys: &[&str]) {
    for key in keys {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
}

#[test]
fn an_accounts_picture_is_its_face_on_the_desktop() {
    let mut cx = seated(false);
    assert!(pictured_at(&cx, "forge-repo-project"), "{:?}", cx.texts());
    press(&mut cx, &["forge-repo-project-open", "forge-tab-settings"]);
    assert!(pictured_at(&cx, "forge-writer-owner"), "{:?}", cx.texts());
    press(&mut cx, &["forge-tab-changes", "forge-change-1"]);
    assert!(pictured_at(&cx, "forge-conversation"), "{:?}", cx.texts());
    assert!(pictured_at(&cx, "forge-dock"), "the reviewers");
    read_once(&cx);
}

#[test]
fn on_a_phone_the_root_leads_with_the_readers_picture_and_the_change_with_its_people() {
    let mut cx = seated(true);
    assert!(
        pictured_at(&cx, "forge-page-repos-account"),
        "{:?}",
        cx.texts()
    );
    press(
        &mut cx,
        &[
            "forge-repo-project",
            "forge-tab-changes",
            "forge-change-project-1",
        ],
    );
    assert!(pictured_at(&cx, "forge-conversation"), "{:?}", cx.texts());
    press(&mut cx, &["forge-change-tab-details"]);
    assert!(pictured_at(&cx, "forge-details"), "the reviewers");
    read_once(&cx);
}
