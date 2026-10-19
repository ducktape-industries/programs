//! Each account's picture is its face wherever agents draw one, read once per blob from the
//! roster's identity read: the list's rows, an agent's head, a conversation's authors and its
//! agent, input waiting behind the loop; the phone's root avatar is the reader's.
use super::*;
use view::methods::BlobGet;
use view::wire::Node;

fn blob(number: u64) -> String {
    format!("sha256:{}", format!("{:02x}", number as u8).repeat(32))
}

// The suite's network with everyone but Atlas holding a picture.
fn pictured() -> Net {
    let mut net = Net::default();
    for account in &mut net.accounts {
        if account.number != ATLAS {
            account.card.avatar = Some(BlobId::Sha256([account.number as u8; 32]));
        }
    }
    net
}

fn holds_picture(node: &Node) -> bool {
    matches!(node, Node::Image { .. }) || node.children().iter().any(holds_picture)
}

fn pictured_at(cx: &TestAppContext, key: &str) -> bool {
    cx.find(key).is_some_and(holds_picture)
}

fn read_once(cx: &TestAppContext, accounts: &[u64]) {
    let mut reads = cx.host().requests::<BlobGet>();
    reads.sort();
    let pictures: Vec<String> = accounts.iter().map(|number| blob(*number)).collect();
    assert_eq!(reads, pictures, "each picture read once");
}

#[test]
fn an_accounts_picture_is_its_face_in_the_list_the_agent_and_the_conversation() {
    let (mut cx, _) = seated(pictured(), EDDY);
    assert!(pictured_at(&cx, "agent-12"), "{:?}", cx.texts());
    assert!(
        pictured_at(&cx, "unscripted-19"),
        "Pager, not handed to harness yet"
    );
    assert!(!pictured_at(&cx, "agent-24"), "Atlas keeps its initial");
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "agent-head"), "{:?}", cx.texts());
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "entry-1"), "Ada's post");
    assert!(pictured_at(&cx, "entry-2"), "Scout's work");
    read_once(&cx, &[ADA, SCOUT, PAGER]);
}

#[test]
fn on_a_phone_the_root_leads_with_the_readers_picture_and_waiting_input_with_its_authors() {
    let mut net = pictured();
    net.threads[0].held = 1;
    net.held = vec![(
        7,
        Body::Post {
            author: Principal::Account(ADA),
            text: "Ada mentioned you in chat channel general:\nAnd the deploy?".into(),
        },
    )];
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    assert!(pictured_at(&cx, "agents-page-account"), "{:?}", cx.texts());
    assert!(pictured_at(&cx, "agent-12") && pictured_at(&cx, "unscripted-19"));
    assert!(!pictured_at(&cx, "agent-24"), "Atlas keeps its initial");
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "agent-head"), "{:?}", cx.texts());
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(pictured_at(&cx, "held-0"), "{:?}", cx.texts());
    assert!(pictured_at(&cx, "entry-2"), "Scout's work");
    read_once(&cx, &[EDDY, ADA, SCOUT, PAGER]);
}
