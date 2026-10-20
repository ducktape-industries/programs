use super::*;

fn routed(chain: &MockChain, provider: &str, services: &[(&str, Wire)]) -> Result<Vec<u8>, Error> {
    let services = services
        .iter()
        .map(|(service, wire)| RouteService {
            service: (*service).into(),
            wire: *wire,
        })
        .collect();
    let op = Op::SetRoute {
        provider: provider.into(),
        services,
    };
    chain.submit(signed(ADA), PROGRAM, &op)
}

fn provided(chain: &MockChain, provider: &str) {
    commit(chain, ADA, CONFIG, &scout().write(&pin_of(provider)));
}

fn configured(chain: &MockChain, agent: AccountNumber) -> Configured {
    let asked = Query::Agent { agent };
    let Reply::Agent(Some(setup)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("the agent has a setup")
    };
    setup.configured
}

#[test]
fn an_agent_naming_its_provider_is_reached_through_that_route_with_no_model_record() {
    let world = world();
    let chain = &world.chain;
    routed(chain, "anthropic", &[(SERVICE, Wire::AnthropicMessages)]).unwrap();
    provided(chain, "anthropic");
    let unasked = configured(chain, world.agent);
    let routed_so = matches!(
        &unasked,
        Configured::Routed { provider, route, window: Window::Unasked, .. }
            if provider == "anthropic" && route.len() == 1
    );
    assert!(routed_so, "{unasked:?}");
    let thread = world.opened("m1", "hello", Some(WINDOW));
    let (request, _) = open_request(chain);
    assert_eq!(body(chain, &request)["model"], MODEL);
    let learnt = configured(chain, world.agent);
    let windowed = matches!(
        learnt,
        Configured::Routed {
            window: Window::Tokens(WINDOW),
            ..
        }
    );
    assert!(windowed, "{learnt:?}");
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    let turned = frames(chain, thread)
        .iter()
        .any(|body| matches!(body, Body::Turn { .. }));
    assert!(turned, "the model answered through the route");
}

#[test]
fn an_agent_naming_a_provider_with_no_route_asks_nothing_and_says_so() {
    let world = world();
    let chain = &world.chain;
    provided(chain, "openai");
    let read = configured(chain, world.agent);
    let unrouted = matches!(&read, Configured::NoRoute { provider, .. } if provider == "openai");
    assert!(unrouted, "{read:?}");
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    assert!(assigned(chain, SERVICE).is_empty(), "no model is asked");
    let told = notices(chain, thread);
    let said = told
        .iter()
        .any(|text| text.ends_with("names provider openai, which harness has no route for"));
    assert!(said, "{told:?}");
}

#[test]
fn a_route_lists_each_registered_service_once_and_the_anthropic_wire_only_for_anthropic() {
    let world = world();
    let chain = &world.chain;
    let anthropic_wire = [(SERVICE, Wire::AnthropicMessages)];
    assert!(routed(chain, "openai", &anthropic_wire).is_err());
    assert!(routed(chain, "anthropic", &[]).is_err());
    assert!(routed(chain, "anthropic", &[(RELAY, Wire::Harness)]).is_err());
    let twice = [(SERVICE, Wire::Harness), (SERVICE, Wire::Harness)];
    routed(chain, "openai", &twice).unwrap();
    let asked = Query::ProviderRoute {
        provider: "openai".into(),
    };
    let Reply::ProviderRoute(Some(row)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("the route is set")
    };
    assert_eq!((row.owner, row.services.len()), (world.ada, 1));
    let listed = Query::ProviderRoutes {
        page: PageRequest::default(),
    };
    let Reply::ProviderRoutes(page) = chain.query(PROGRAM, &listed).unwrap() else {
        panic!("routes answer as a page")
    };
    assert!(page.items.contains(&row), "{:?}", page.items);
    let remove = Op::RemoveRoute {
        provider: "openai".into(),
    };
    chain.submit(signed(ADA), PROGRAM, &remove).unwrap();
    let Reply::ProviderRoute(None) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("the route is removed")
    };
    assert!(chain.submit(signed(ADA), PROGRAM, &remove).is_err());
}

#[test]
fn a_service_that_answers_for_another_provider_than_its_route_asks_no_turn() {
    let world = world();
    let chain = &world.chain;
    routed(chain, "anthropic", &[(SERVICE, Wire::Harness)]).unwrap();
    provided(chain, "anthropic");
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), "/models/claude-test");
    let model = json!({ "provider": "openai", "window": WINDOW });
    answer(chain, request.id, Answer::Output(response(200, model)));
    chain.at(3, 0);
    assert!(assigned(chain, SERVICE).is_empty(), "no turn is asked");
    let told = notices(chain, thread);
    let said = told
        .iter()
        .any(|text| text.starts_with("service ccs answers for provider openai, not anthropic"));
    assert!(said, "{told:?}");
}
