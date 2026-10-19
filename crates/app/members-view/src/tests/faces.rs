//! Each account's picture is its face wherever the roster draws one, read
//! once per blob from identity's cards, on both designs; the phone's root
//! avatar is the reader's.
use super::*;
use view::methods::BlobGet;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nface";

fn blob(byte: u8) -> String {
    format!("sha256:{}", format!("{byte:02x}").repeat(32))
}

fn pictured(mut account: identity::Account) -> identity::Account {
    account.card.avatar = Some(abi::BlobId::Sha256([account.number as u8; 32]));
    account
}

// The suite's network with eddy (#7) and scout (#9) each holding a picture,
// seated as eddy; on a phone with `touch`.
fn faces(touch: bool) -> TestAppContext {
    let mut cx = TestAppContext::new();
    respond(&mut cx);
    cx.host().handle::<Query<Identity>>(|query| {
        assert!(matches!(query, identity::Query::List { .. }));
        Ok(identity::Reply::Accounts(page(vec![
            pictured(person(7, "eddy", vec![key(EDDY, "laptop")])),
            pictured(agent(
                9,
                "scout",
                7,
                identity::Life::Active {
                    keys: vec![key(SCOUT, "sandbox")],
                },
            )),
            person(11, "ada", vec![key(b"\x0b", "phone")]),
        ])))
    });
    cx.host().handle::<BlobGet>(|_| Ok(Some(PNG.to_vec())));
    cx.open::<Members>();
    cx.host().stream::<HostSession>().send(Session {
        account: Some(7),
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    });
    if touch {
        // a phone titles the activity it lists by what chat says it did
        cx.host().handle::<ProgramDescribe>(|_: (String, Vec<u8>)| {
            Ok(Some(view::methods::Description {
                title: "Post in #general".into(),
                fields: Vec::new(),
            }))
        });
        cx.simulate_touch(true);
        cx.simulate_resize(390., 844.);
    }
    cx.run_until_parked();
    cx
}

fn holds_picture(node: &Node) -> bool {
    matches!(node, Node::Image { .. }) || node.children().iter().any(holds_picture)
}

fn pictured_at(cx: &TestAppContext, key: &str) -> bool {
    cx.find(key).is_some_and(holds_picture)
}

fn read_once(cx: &TestAppContext) {
    let mut reads = cx.host().requests::<BlobGet>();
    reads.sort();
    assert_eq!(reads, [blob(7), blob(9)], "each picture read once");
}

#[test]
fn an_accounts_picture_is_its_face_in_the_roster_and_its_detail() {
    let mut cx = faces(false);
    assert!(pictured_at(&cx, "members-row-7") && pictured_at(&cx, "members-row-9"));
    assert!(!pictured_at(&cx, "members-row-11"), "ada keeps her initial");
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "members-detail-head"), "{:?}", cx.texts());
    assert!(
        pictured_at(&cx, "members-manages-9"),
        "the agent eddy manages"
    );
    read_once(&cx);
}

#[test]
fn on_a_phone_the_roster_leads_with_the_readers_picture() {
    let mut cx = faces(true);
    assert!(
        pictured_at(&cx, "members-roster-account"),
        "{:?}",
        cx.texts()
    );
    assert!(pictured_at(&cx, "members-row-9"));
    cx.simulate_click("members-row-7");
    cx.run_until_parked();
    assert!(
        pictured_at(&cx, "members-manages-9"),
        "the agent eddy manages"
    );
    read_once(&cx);
}
