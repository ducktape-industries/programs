//! Chat over the fake node: the chain seeded through chat's own ops, what the
//! person sees read off it, another's post reaching the open room through
//! `program.changes`, and a composer post landing as the signed-in account.
// the view's tests: the notifier build (`--features notifier`) has no view
#![cfg(not(feature = "notifier"))]
use chat::{Block, Op, PostPolicy, Query, Reply};
use chat_view::Chat;
use view::methods::{HostId, HostSession, HostVisible, StoreGet};
use view::rig::Network;
use view::testing::TestAppContext;

/// The time of the test's `n`th block (unix ms), a minute apart: pinned
/// with `Network::at`, so the times the room draws are the same every run.
fn minute(n: u64) -> u64 {
    1_790_000_000_000 + n * 60_000
}

fn post(net: &Network, who: u64, id: &str, text: &str) {
    net.submit(
        who,
        chat::PROGRAM,
        &Op::PostMessage {
            channel_id: "general".into(),
            message_id: id.into(),
            blocks: vec![Block::paragraph(text)],
            thread: None,
        },
    )
    .unwrap();
}

#[test]
fn a_seeded_room_is_read_off_the_chain_and_a_post_comes_back_through_it() {
    let net = Network::new();
    net.seat::<chat::Chat>(chat::PROGRAM);
    net.at(minute(0));
    let ada = net.person("ada");
    net.at(minute(1));
    net.submit(
        ada,
        chat::PROGRAM,
        &Op::CreateChannel {
            channel_id: "general".into(),
            name: "General".into(),
            post_policy: PostPolicy::Open,
        },
    )
    .unwrap();
    net.at(minute(2));
    post(&net, ada, "m1", "hello from ada");
    net.at(minute(3));
    post(&net, net.me(), "m2", "and from me");

    let mut cx = TestAppContext::new();
    net.back(&cx.host());
    // the host's own methods stay the test's: no store, ids minted here
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
    assert!(cx.has_text("General"), "{:?}", cx.texts());
    cx.simulate_click("chat-sidebar-channel-general");
    cx.run_until_parked();
    assert!(cx.has_text("hello from ada"), "{:?}", cx.texts());
    assert!(cx.has_text("and from me"));
    assert!(cx.has_text("ada"), "the author's name is identity's");

    // ada posts while the room is open: the view asked nothing, so her
    // post reaches it only as the block's `program.changes` item and the
    // re-read that item causes
    net.at(minute(4));
    post(&net, ada, "m3", "ada again, while the room is open");
    cx.run_until_parked();
    assert!(
        cx.has_text("ada again, while the room is open"),
        "{:?}",
        cx.texts()
    );

    // a post through the composer: chat's rules run it, as the signed-in
    // account
    cx.simulate_input("draft-general/editor", "typed in the live test");
    net.at(minute(5));
    cx.simulate_click("draft-general/send");
    cx.run_until_parked();
    assert!(cx.has_text("typed in the live test"), "{:?}", cx.texts());
    let Reply::Roots(page) = net
        .query(
            chat::PROGRAM,
            &Query::Roots {
                channel_id: "general".into(),
                viewer: vec![],
                page: chat::PageRequest {
                    after: None,
                    limit: None,
                },
            },
        )
        .unwrap()
    else {
        panic!("roots")
    };
    assert_eq!(page.items.len(), 4);
    assert_eq!(page.items[0].text, "typed in the live test");
    assert_eq!(page.items[0].author, chat::Principal::Account(net.me()));
}
