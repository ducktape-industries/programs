use super::*;

fn with_a_second_service() -> Net {
    let mut net = Net::default();
    net.services.push(egress::ServiceRow {
        name: "models-b".into(),
        owner: ADA,
        nodes: vec![vec![3]],
        registered: 6,
    });
    net.serving.push(("models-b".into(), 1));
    net
}

fn takes(cx: &TestAppContext) {
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
}

fn open_anthropic(cx: &mut TestAppContext) {
    cx.simulate_click("agents-routes");
    cx.run_until_parked();
    cx.simulate_click("route-anthropic");
    cx.run_until_parked();
}

fn sent(cx: &TestAppContext) -> Vec<harness::Op> {
    cx.host().requests::<Submit<harness::Harness>>()
}

fn reached(services: &[&str]) -> Vec<RouteService> {
    let services = services.iter().map(|service| RouteService {
        service: (*service).into(),
        wire: Wire::AnthropicMessages,
    });
    services.collect()
}

#[test]
fn the_routes_page_lists_each_providers_route_and_sets_one_after_a_yes() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    takes(&cx);
    cx.simulate_click("agents-routes");
    cx.run_until_parked();
    for text in [
        "Routes",
        "anthropic",
        "anthropic (Anthropic Messages) · ",
        "1 node serving",
        "Ada",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(
        cx.find("route-provider").is_none(),
        "closed until Set a route"
    );
    cx.simulate_click("routes-set");
    cx.run_until_parked();
    cx.simulate_input("route-provider", "claude-direct");
    cx.simulate_click("service-anthropic");
    cx.simulate_click("route-next");
    cx.run_until_parked();
    for text in [
        "Save claude-direct's route",
        "Reach its models through anthropic (Anthropic Messages)",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(sent(&cx).is_empty());
    cx.simulate_click("route-set");
    cx.run_until_parked();
    let set = harness::Op::SetRoute {
        provider: "claude-direct".into(),
        services: reached(&["anthropic"]),
    };
    assert_eq!(sent(&cx), [set]);
}

#[test]
fn an_edit_sends_the_whole_route_in_its_new_order_after_a_yes() {
    let (mut cx, _) = seated(with_a_second_service(), EDDY);
    takes(&cx);
    open_anthropic(&mut cx);
    for text in ["Edit anthropic's route", "first · Anthropic Messages"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(
        cx.find("route-provider").is_none(),
        "a route keeps its provider"
    );
    cx.simulate_click("service-models-b");
    cx.simulate_click("route-next");
    cx.run_until_parked();
    let reach = "Reach its models through anthropic (Anthropic Messages), then models-b (Anthropic Messages)";
    for text in ["Save anthropic's route", reach] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(sent(&cx).is_empty());
    cx.simulate_click("route-set");
    cx.run_until_parked();
    let set = harness::Op::SetRoute {
        provider: ANTHROPIC.into(),
        services: reached(&["anthropic", "models-b"]),
    };
    assert_eq!(sent(&cx), [set]);
    assert!(cx.find("route-next").is_none(), "{:?}", cx.texts());
}

#[test]
fn no_change_sends_nothing() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    takes(&cx);
    open_anthropic(&mut cx);
    cx.simulate_click("route-next");
    cx.run_until_parked();
    assert!(cx.has_text("Nothing changed."), "{:?}", cx.texts());
    assert!(cx.find("route-set").is_none());
    assert!(sent(&cx).is_empty());
}

#[test]
fn removing_asks_once_and_names_the_agents_that_name_the_provider() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    takes(&cx);
    open_anthropic(&mut cx);
    cx.simulate_click("route-remove");
    cx.run_until_parked();
    for text in [
        "Remove anthropic's route?",
        "Scout and Atlas still name it in their configuration: each asks no model until a route reaches it again.",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    assert!(sent(&cx).is_empty());
    cx.simulate_click("route-remove-yes");
    cx.run_until_parked();
    let removed = harness::Op::RemoveRoute {
        provider: ANTHROPIC.into(),
    };
    assert_eq!(sent(&cx), [removed]);
}

#[test]
fn a_refusal_shows_harnesss_sentence_and_keeps_the_form() {
    let (mut cx, _) = seated(with_a_second_service(), EDDY);
    let refusal = "account 7 may not write provider-route/anthropic: its owner is account 8";
    cx.host()
        .refuse::<Submit<harness::Harness>>("unauthorized", refusal);
    open_anthropic(&mut cx);
    cx.simulate_click("service-models-b");
    cx.simulate_click("route-next");
    cx.run_until_parked();
    cx.simulate_click("route-set");
    cx.run_until_parked();
    let said = format!("That didn’t go through: {refusal}");
    assert!(cx.has_text(&said), "{:?}", cx.texts());
    assert!(cx.find("route-next").is_some(), "{:?}", cx.texts());
}
