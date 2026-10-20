//! Each account's picture is its face wherever the explorer draws one: the
//! Accounts list and an account's page, on both designs, and the phone's root
//! avatar; each picture read once however often it is drawn.
use super::*;
use view::methods::BlobGet;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nface";

fn blob(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

fn pictured() -> TestAppContext {
    let mut cx = TestAppContext::new();
    node(&mut cx, Rc::new(RefCell::new(12)));
    cx.host().handle::<Query<Identity>>(|query| match query {
        identity::Query::List { .. } => {
            let (mut ada, mut scout) = (ada(), scout());
            ada.card.avatar = Some(BlobId::Sha256([7; 32]));
            scout.card.avatar = Some(BlobId::Sha256([8; 32]));
            Ok(identity::Reply::Accounts(programs::PageResponse {
                height: 1,
                items: vec![ada, scout, forge()],
                next: None,
            }))
        }
        other => panic!("unexpected identity query: {other:?}"),
    });
    cx.host().handle::<BlobGet>(|_| Ok(Some(PNG.to_vec())));
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx
}

fn drawn(cx: &TestAppContext) -> usize {
    let mut drawn = 0;
    cx.whole_frame()
        .root
        .expect("a tree")
        .for_each_mut(&mut |node| {
            if matches!(node, view::wire::Node::Image { .. }) {
                drawn += 1;
            }
        });
    drawn
}

#[test]
fn an_accounts_picture_is_its_face_on_both_designs() {
    for touch in [false, true] {
        let mut cx = pictured();
        if touch {
            cx.simulate_touch(true);
            cx.simulate_resize(390., 844.);
            cx.run_until_parked();
        }
        cx.simulate_click("explorer-tab-accounts");
        cx.run_until_parked();
        let listed = drawn(&cx);
        assert_eq!(
            listed, 2,
            "Ada's and Scout's, touch {touch}; forge's stays a letter"
        );
        cx.simulate_click("explorer-account-5");
        cx.run_until_parked();
        assert!(drawn(&cx) >= 1, "Scout's page leads with it, touch {touch}");
        let reads = cx.host().requests::<BlobGet>();
        assert_eq!(reads, [blob(7), blob(8)], "each read once, touch {touch}");
    }
}

#[test]
fn on_a_phone_the_root_leads_with_the_readers_picture() {
    let mut cx = pictured();
    cx.host().stream::<HostSession>().send(Session {
        connected: true,
        chain_id: "test#1".into(),
        signer: abi::hex(&ADA),
        account: Some(3),
        endpoint: String::new(),
    });
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    let root = cx
        .find("explorer-root-account")
        .expect("the reader's avatar");
    let mut pictured = false;
    root.clone().for_each_mut(&mut |node| {
        pictured |= matches!(node, view::wire::Node::Image { .. });
    });
    assert!(pictured, "Ada's picture leads the root: {:?}", cx.texts());
    assert_eq!(cx.host().requests::<BlobGet>(), [blob(7)], "read once");
}
