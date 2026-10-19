use super::*;

// a second registered service the form can pick, served by one node
fn with_ccs() -> Net {
    let mut net = Net::default();
    net.services.push(egress::ServiceRow {
        name: "ccs-b".into(),
        owner: ADA,
        nodes: vec![vec![3]],
        registered: 6,
    });
    net.serving.push(("ccs-b".into(), 1));
    net
}

fn takes(cx: &TestAppContext) {
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
}

fn open_sonnet(cx: &mut TestAppContext) {
    cx.simulate_click("agents-models");
    cx.run_until_parked();
    cx.simulate_click("model-sonnet");
    cx.run_until_parked();
}

fn sent(cx: &TestAppContext) -> Vec<harness::Op> {
    cx.host().requests::<Submit<harness::Harness>>()
}

#[test]
fn an_edit_sends_only_what_moved_after_a_yes() {
    let (mut cx, _) = seated(with_ccs(), EDDY);
    takes(&cx);
    open_sonnet(&mut cx);
    assert!(cx.has_text("Edit sonnet"), "{:?}", cx.texts());
    assert!(cx.find("model-id").is_none(), "an open record keeps its id");
    cx.simulate_click("service-ccs-b");
    cx.simulate_input("model-compact-at", "650000");
    cx.simulate_click("model-next");
    cx.run_until_parked();
    for text in [
        "Save sonnet",
        "Reach it through anthropic, then ccs-b",
        "Compact its conversations at 650,000 tokens",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(sent(&cx).is_empty());
    cx.simulate_click("model-set");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [
            harness::Op::SetServices {
                id: SONNET.into(),
                services: vec!["anthropic".into(), "ccs-b".into()],
            },
            harness::Op::SetCompactAt {
                id: SONNET.into(),
                compact_at: Some(650_000),
            },
        ]
    );
    // a saved record closes its form
    assert!(cx.find("model-compact-at").is_none(), "{:?}", cx.texts());
}

#[test]
fn a_record_reached_through_more_services_lists_them_in_order() {
    let mut net = with_ccs();
    net.model_services = vec![(SONNET.into(), vec!["ccs-b".into()])];
    let (mut cx, _) = seated(net, EDDY);
    takes(&cx);
    open_sonnet(&mut cx);
    for text in [" · then ccs-b", "first · the record's", "then, 2"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    // dropping the first makes the next one the record's
    cx.simulate_click("service-anthropic");
    cx.simulate_click("model-next");
    cx.run_until_parked();
    cx.simulate_click("model-set");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [harness::Op::SetServices {
            id: SONNET.into(),
            services: vec!["ccs-b".into()],
        }]
    );
}

#[test]
fn an_empty_compact_at_clears_the_default_and_no_change_sends_nothing() {
    let mut net = Net::default();
    net.models[0].compact_at = Some(650_000);
    let (mut cx, _) = seated(net, EDDY);
    takes(&cx);
    open_sonnet(&mut cx);
    cx.simulate_click("model-next");
    cx.run_until_parked();
    assert!(cx.has_text("Nothing changed."), "{:?}", cx.texts());
    assert!(cx.find("model-set").is_none());
    cx.simulate_input("model-compact-at", "");
    cx.simulate_click("model-next");
    cx.run_until_parked();
    let clears = "Clear its compact_at: a conversation compacts when its window fills, unless its agent names one";
    assert!(cx.has_text(clears), "{:?}", cx.texts());
    cx.simulate_click("model-set");
    cx.run_until_parked();
    assert_eq!(
        sent(&cx),
        [harness::Op::SetCompactAt {
            id: SONNET.into(),
            compact_at: None,
        }]
    );
}

#[test]
fn removing_asks_once_and_names_the_agents_that_still_use_it() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    takes(&cx);
    open_sonnet(&mut cx);
    cx.simulate_click("model-remove");
    cx.run_until_parked();
    for text in [
        "Remove sonnet?",
        "Scout and Atlas still name it in their configuration: each asks no model until it names another.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(sent(&cx).is_empty());
    cx.simulate_click("model-remove-yes");
    cx.run_until_parked();
    assert_eq!(sent(&cx), [harness::Op::RemoveModel { id: SONNET.into() }]);
}

#[test]
fn a_refusal_shows_harnesss_sentence_and_keeps_the_form() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    let refusal = "account 7 may not write models/sonnet: its owner is account 8";
    cx.host()
        .refuse::<Submit<harness::Harness>>("unauthorized", refusal);
    open_sonnet(&mut cx);
    cx.simulate_input("model-compact-at", "650000");
    cx.simulate_click("model-next");
    cx.run_until_parked();
    cx.simulate_click("model-set");
    cx.run_until_parked();
    let said = format!("That didn’t go through: {refusal}");
    assert!(cx.has_text(&said), "{:?}", cx.texts());
    assert!(cx.find("model-compact-at").is_some(), "{:?}", cx.texts());
}
