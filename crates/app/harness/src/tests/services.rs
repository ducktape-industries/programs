//! Which service and account a conversation's calls go to: none named at first, the account that
//! answered after, and how a refused, limited or failed call moves on, on the harness wire.
use super::accounts::{
    answer_headers, from_node, harness_wired, limited_headers, pinned_to, replied,
};
use super::*;
use model_wire::{Block, Class, Usage};

const TWO: &str = "models-two";

fn two_services(world: &World) {
    let chain = &world.chain;
    let register = egress::Op::Register {
        service: TWO.into(),
    };
    let assign = egress::Op::Assign {
        service: TWO.into(),
        nodes: vec![NODE.to_vec()],
    };
    for op in [register, assign] {
        chain.submit(signed(ADA), egress::PROGRAM, &op).unwrap();
    }
    let serve = egress::Op::Serve {
        services: vec![SERVICE.into(), RELAY.into(), TWO.into()],
    };
    from_node(chain, &serve);
    let both = [(SERVICE, Wire::Harness), (TWO, Wire::Harness)];
    chain
        .submit(signed(ADA), PROGRAM, &set_route(&both))
        .unwrap();
}

fn requests_at(chain: &MockChain, service: &str, target: &str) -> Vec<RequestRow> {
    let at = |row: &RequestRow| matches!(&row.request, Request::Http { head, .. } if head.target == target);
    assigned(chain, service).into_iter().filter(at).collect()
}

fn open(chain: &MockChain, service: &str, target: &str) -> (RequestRow, Option<String>) {
    let rows = requests_at(chain, service, target);
    let count = rows.len();
    let Ok([row]) = <[RequestRow; 1]>::try_from(rows) else {
        panic!("{count} requests open on {service} at {target}");
    };
    let pinned = pinned_to(chain, &row);
    (row, pinned)
}

fn none_open(chain: &MockChain, service: &str, target: &str) -> bool {
    requests_at(chain, service, target).is_empty()
}

fn learned(chain: &MockChain, service: &str) {
    let (request, pinned) = open(chain, service, &model_wire::model_path(MODEL));
    assert_eq!(pinned, None, "a model read names no account");
    let model = model_wire::Model {
        provider: PROVIDER.into(),
        window: Some(WINDOW),
        pictures: None,
    };
    let body = serde_json::to_value(&model).unwrap();
    answer(chain, request.id, Answer::Output(response(200, body)));
}

struct Says {
    status: u16,
    used: &'static str,
    cache_write: u64,
}

const FINE: Says = Says {
    status: 200,
    used: "0.5",
    cache_write: 0,
};

const LIMITED: Says = Says {
    status: 429,
    used: "1",
    cache_write: 0,
};

fn says(chain: &MockChain, service: &str, pinned: Option<&str>, account: &str, said: Says) {
    let (request, sent) = open(chain, service, model_wire::TURN);
    assert_eq!(sent.as_deref(), pinned, "the account the call names");
    if said.status == LIMITED.status {
        let failure = model_wire::Failure::new(Class::Limited, "limited");
        let limited = http::Response {
            status: said.status,
            headers: limited_headers(account),
            body: serde_json::to_vec(&failure).unwrap(),
        };
        return answer(chain, request.id, Answer::Output(abi::encode(&limited)));
    }
    let text = Block::Text {
        text: "hello back".into(),
        citations: Vec::new(),
    };
    let usage = Usage {
        input: 10,
        output: 5,
        cache_read: 0,
        cache_write: said.cache_write,
    };
    let headers = answer_headers(account, said.used, "allowed");
    replied(chain, &request, vec![text], usage, headers);
}

fn refused(chain: &MockChain, pinned: Option<&str>, message: &str) {
    let (request, sent) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(sent.as_deref(), pinned, "the account the call names");
    let failure = model_wire::Failure::new(Class::Provider, message);
    let body = serde_json::to_value(&failure).unwrap();
    answer(chain, request.id, Answer::Output(response(400, body)));
}

fn route(chain: &MockChain, thread: u64) -> Option<(String, Option<String>)> {
    let Reply::Route(route) = chain.query(PROGRAM, &Query::Route { thread }).unwrap() else {
        panic!("a route answers as one")
    };
    route.map(|route| (route.service, route.account))
}

fn on(service: &str, account: &str) -> Option<(String, Option<String>)> {
    Some((service.to_owned(), Some(account.to_owned())))
}

fn answerers(chain: &MockChain, thread: u64) -> Vec<Answerer> {
    let asked = Query::Answerers {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Answerers(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("answerers answer as a page")
    };
    page.items
}

fn cache_writes(chain: &MockChain, thread: u64) -> Vec<u64> {
    let turns = frames(chain, thread)
        .into_iter()
        .filter_map(|body| match body {
            Body::Turn { usage, .. } => Some(usage.cache_write),
            _ => None,
        });
    turns.collect()
}

fn configured(chain: &MockChain, more: &str) {
    let text = md(&scout()).replace("max_tokens = 1024\n", &format!("max_tokens = 1024\n{more}"));
    commit(chain, ADA, CONFIG, &text);
}

fn wakes_of(chain: &MockChain, thread: u64) -> Vec<Wake> {
    let Reply::Wakes(wakes) = chain.query(PROGRAM, &Query::Wakes { thread }).unwrap() else {
        panic!("harness answers wakes")
    };
    wakes
}

#[test]
fn a_new_conversation_names_no_account_and_stays_on_the_account_that_answered_it() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m2", "again");
    chain.at(10, 0);
    says(chain, SERVICE, Some("b"), "b", Says { used: "1", ..FINE });
    let moved: Vec<bool> = answerers(chain, thread)
        .iter()
        .map(|row| row.moved)
        .collect();
    assert_eq!(moved, [false, false]);
    new_thread_retiring_the_current_one(&world);
    let second = world.mention("m3", "a new one");
    assert_ne!(second, thread);
    chain.at(11, 0);
    says(chain, SERVICE, None, "a", FINE);
    assert_eq!(route(chain, second), on(SERVICE, "a"));
}

#[test]
fn a_429_on_the_threads_account_asks_again_next_block_naming_none_and_the_thread_stays_where_it_was_answered()
 {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    let written = |cache_write| Says {
        cache_write,
        ..FINE
    };
    says(chain, SERVICE, None, "a", written(1000));
    world.mention("m2", "and now?");
    chain.at(8, 0);
    says(chain, SERVICE, Some("a"), "a", LIMITED);
    let told = notices(chain, thread);
    let again = "harness asks the model again at block 9 with no account named, so its service picks one (attempt 1)";
    assert!(told.last().unwrap().ends_with(again), "{told:?}");
    chain.at(9, 0);
    chain.at(10, 0);
    assert!(
        none_open(chain, TWO, model_wire::TURN),
        "its service picks first"
    );
    says(chain, SERVICE, None, "b", written(1200));
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "once more");
    chain.at(11, 0);
    says(chain, SERVICE, Some("b"), "b", written(0));
    let rows: Vec<(String, Option<String>, bool)> = answerers(chain, thread)
        .into_iter()
        .map(|row| (row.service, row.account, row.moved))
        .collect();
    assert_eq!(
        rows,
        [
            (SERVICE.to_owned(), Some("a".to_owned()), false),
            (SERVICE.to_owned(), Some("b".to_owned()), true),
            (SERVICE.to_owned(), Some("b".to_owned()), false)
        ]
    );
    assert_eq!(cache_writes(chain, thread), [1000, 1200, 0]);
}

#[test]
fn a_429_with_no_account_named_moves_to_the_next_service_and_one_on_every_service_waits_for_the_reset_until_an_input()
 {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "a", LIMITED);
    let told = notices(chain, thread);
    let again = "at block 8 with no account named, so its service picks one (attempt 1)";
    assert!(told.last().unwrap().ends_with(again), "{told:?}");
    chain.at(8, 0);
    chain.at(9, 0);
    says(chain, TWO, None, "c", LIMITED);
    let told = notices(chain, thread);
    let wait = "once the model's wait ends at 4000000000 (seconds since the epoch) (attempt 2)";
    assert!(told.last().unwrap().ends_with(wait), "{told:?}");
    chain.at(10, 0);
    chain.at(11, 0);
    assert!(none_open(chain, SERVICE, model_wire::TURN), "held");
    assert!(none_open(chain, TWO, model_wire::TURN), "held");
    let held = wakes_of(chain, thread);
    let until = 4_000_000_000_000;
    let named =
        |wake: &Wake| matches!(wake, Wake::RetryAfter { until: reset, .. } if *reset == until);
    assert!(held.iter().any(named), "the wait names its reset: {held:?}");
    world.mention("m2", "still there?");
    let (_, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(pinned, None, "a new input ends the wait");
}

#[test]
fn a_pin_to_an_account_its_service_no_longer_holds_asks_again_naming_none_and_only_an_unpinned_refusal_is_final()
 {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    let thread = world.mention("m1", "hello");
    chain.at(4, 0);
    learned(chain, SERVICE);
    chain.at(5, 0);
    says(chain, SERVICE, None, "a", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
    world.mention("m2", "and now?");
    chain.at(6, 0);
    refused(chain, Some("a"), "no such account: a");
    let told = notices(chain, thread);
    let again = "harness asks the model again at block 7 with no account named, so its service picks one (attempt 1)";
    assert!(told.last().unwrap().ends_with(again), "{told:?}");
    chain.at(7, 0);
    chain.at(8, 0);
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "a request no account takes");
    chain.at(9, 0);
    refused(chain, Some("b"), "bad request");
    chain.at(10, 0);
    chain.at(11, 0);
    refused(chain, None, "bad request");
    chain.at(12, 0);
    chain.at(13, 0);
    assert!(
        none_open(chain, SERVICE, model_wire::TURN),
        "an unpinned refusal is final"
    );
    let told = notices(chain, thread);
    let last = told.last().unwrap();
    let gave_up = last.contains("bad request") && !last.contains("asks the model again");
    assert!(gave_up, "{told:?}");
}

#[test]
fn a_failed_answer_asks_again_naming_no_account_but_a_call_cut_off_by_an_update_stays() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "a", FINE);
    world.mention("m2", "and now?");
    chain.at(8, 0);
    let (request, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(pinned.as_deref(), Some("a"));
    answer(
        chain,
        request.id,
        Answer::Failed("the service is down".into()),
    );
    chain.at(9, 0);
    chain.at(10, 0);
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "once more");
    let (request, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(pinned.as_deref(), Some("b"));
    chain.cancel_parked(PROGRAM);
    let deadline = request.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    chain.at(deadline + 2, 0);
    chain.at(deadline + 3, 0);
    assert!(
        none_open(chain, TWO, model_wire::TURN),
        "the cache stays on b"
    );
    says(chain, SERVICE, Some("b"), "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
}

#[test]
fn a_pinned_service_keeps_to_its_accounts_and_one_the_route_lists_not_gives_up() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    configured(chain, &format!("service = \"{SERVICE}\"\n"));
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "a", FINE);
    configured(chain, "service = \"elsewhere\"\n");
    world.mention("m2", "and now?");
    let told = notices(chain, thread);
    let why = format!(
        "the agent's configuration pins service elsewhere, which provider anthropic's route is not reached through ({SERVICE}, {TWO})"
    );
    assert_eq!(told.last(), Some(&why));
    assert!(none_open(chain, SERVICE, model_wire::TURN));
    assert!(none_open(chain, TWO, model_wire::TURN));
}

#[test]
fn a_one_service_route_is_asked_unpinned() {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    learned(chain, SERVICE);
    chain.at(3, 0);
    says(chain, SERVICE, None, "", FINE);
    assert_eq!(route(chain, thread), Some((SERVICE.to_owned(), None)));
    let [only]: [Answerer; 1] = answerers(chain, thread).try_into().unwrap();
    assert!(!only.moved);
}

#[test]
fn the_anthropic_wire_names_no_account_even_once_one_answered() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (request, _) = open_request(chain);
    let said = reply(json!([{ "type": "text", "text": "hi" }]), "end_turn", 10);
    let mut said: http::Response = abi::decode(&said).unwrap();
    said.headers = answer_headers("a", "0.5", "allowed");
    answer(chain, request.id, Answer::Output(abi::encode(&said)));
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
    world.mention("m2", "and now?");
    chain.at(4, 0);
    let (request, head) = open_request(chain);
    let named = head.headers.iter().any(|(key, _)| key.contains("account"));
    assert!(!named, "{:?}", head.headers);
    let sent = body(chain, &request);
    assert!(sent.get("target").is_none(), "{sent}");
}

#[test]
fn a_call_moved_near_the_threshold_does_not_compact_on_the_account_its_service_picks() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    configured(chain, "compact_at = 2000\n");
    let thread = world.mention("m1", "remember the plan");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(
        chain,
        SERVICE,
        None,
        "a",
        Says {
            cache_write: 1000,
            ..FINE
        },
    );
    assert_eq!(thread_row(chain, thread).context, Context::Measured(1015));
    world.mention("m2", "and now?");
    chain.at(8, 0);
    says(chain, SERVICE, Some("a"), "a", LIMITED);
    chain.at(9, 0);
    chain.at(10, 0);
    assert!(
        none_open(chain, SERVICE, model_wire::SUMMARY),
        "a turn, not a summary"
    );
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "back again");
    let (_, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(
        pinned.as_deref(),
        Some("b"),
        "it stays where it was answered"
    );
}

#[test]
fn a_route_lists_its_services_in_order_once_each() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let set = |services: &[&str]| {
        let listed: Vec<(&str, Wire)> = services
            .iter()
            .map(|service| (*service, Wire::Harness))
            .collect();
        set_route(&listed)
    };
    let listed = || {
        let asked = Query::ProviderRoute {
            provider: PROVIDER.into(),
        };
        let Reply::ProviderRoute(Some(row)) = chain.query(PROGRAM, &asked).unwrap() else {
            panic!("the route is set")
        };
        let services = row.services.into_iter().map(|entry| entry.service);
        services.collect::<Vec<String>>()
    };
    assert_eq!(listed(), [SERVICE, TWO]);
    chain
        .submit(signed(ADA), PROGRAM, &set(&[TWO, TWO, SERVICE]))
        .unwrap();
    assert_eq!(listed(), [TWO, SERVICE]);
    assert!(chain.submit(signed(ADA), PROGRAM, &set(&[])).is_err());
    let key = crate::state::PROVIDER_ROUTES.key(&PROVIDER.to_owned());
    assert_eq!(written(&key), Some(Written::Route));
}

#[test]
fn reading_a_services_status_is_retired_and_always_refused() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let one = Op::ReadBudget {
        service: SERVICE.into(),
    };
    for op in [one, Op::ReadBudgets] {
        let error = chain.submit(signed(BO), PROGRAM, &op).unwrap_err();
        assert_eq!(error.code, code::INVALID_INPUT, "{op:?}");
        let said = "an idle account's windows are its service's own page";
        assert!(error.message.contains(said), "{}", error.message);
        assert!(describe(&op).title.starts_with("Retired, always refused: "));
    }
    for service in [SERVICE, TWO] {
        assert!(
            assigned(chain, service).is_empty(),
            "nothing asks {service}"
        );
    }
}

#[test]
fn a_pin_reads_from_the_front_matter_and_leaves_the_config_as_it_was() {
    let text = md(&scout()).replace(
        "max_tokens = 1024\n",
        &format!("max_tokens = 1024\nservice = \"{SERVICE}\"\n"),
    );
    let (config, pin) = Config::parse_pinned(text.as_bytes()).unwrap();
    assert_eq!(config, scout());
    let pinned = crate::config::Pin {
        service: Some(SERVICE.into()),
        provider: Some(PROVIDER.into()),
    };
    assert_eq!(pin, pinned);
}

fn searched(chain: &MockChain, service: &str, account: &str) {
    let (request, _) = open(chain, service, model_wire::TURN);
    let result = json!({
        "type": "web_search_tool_result",
        "tool_use_id": "srvtoolu_1",
        "content": [{
            "type": "web_search_result",
            "url": "https://ducktape.example",
            "title": "Ducktape",
            "encrypted_content": "EqgfCioIARgB",
        }],
    });
    let citation = json!({
        "type": "web_search_result_location",
        "url": "https://ducktape.example",
        "title": "Ducktape",
        "encrypted_index": "Eo8BCioIAhgB",
        "cited_text": "Ducktape is a network.",
    });
    let said = vec![
        Block::ServerToolUse {
            id: "srvtoolu_1".into(),
            name: "web_search".into(),
            input: json!({ "query": "ducktape" }).to_string(),
        },
        Block::ServerToolResult {
            sealed: result.to_string(),
        },
        Block::Text {
            text: "Ducktape is a network.".into(),
            citations: vec![citation.to_string()],
        },
    ];
    let usage = Usage {
        input: 10,
        output: 5,
        ..Usage::default()
    };
    let headers = answer_headers(account, "0.5", "allowed");
    replied(chain, &request, said, usage, headers);
}

fn minted_absent(sent: &str) {
    for minted in [
        "EqgfCioIARgB",
        "Eo8BCioIAhgB",
        r#""kind":"server_tool_use""#,
        r#""kind":"server_tool_result""#,
    ] {
        assert!(
            !sent.contains(minted),
            "{minted} reached another account: {sent}"
        );
    }
    assert!(sent.contains("Ducktape https://ducktape.example"), "{sent}");
    assert!(sent.contains("Ducktape is a network."), "{sent}");
}

fn assistant_lanes(sent: &Value) -> Vec<Value> {
    let messages = sent["messages"].as_array().expect("a request's messages");
    let spoken = messages
        .iter()
        .filter(|message| message["role"] == "assistant");
    spoken.map(|message| message["lane"].clone()).collect()
}

#[test]
fn a_search_one_account_answered_goes_as_plain_text_once_a_429_leaves_the_account_to_its_service() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let thread = world.mention("m1", "look it up");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    searched(chain, SERVICE, "a");
    world.mention("m2", "and now?");
    chain.at(8, 0);
    let (own, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(pinned.as_deref(), Some("a"));
    let sent = body(chain, &own);
    let lane = json!({ "provider": PROVIDER, "model": MODEL, "service": SERVICE, "account": "a" });
    assert_eq!(
        assistant_lanes(&sent),
        std::slice::from_ref(&lane),
        "the lane names who answered, so the account pinned reads its own blocks"
    );
    let text = sent.to_string();
    assert!(
        text.contains("EqgfCioIARgB") && text.contains(r#""kind":"server_tool_use""#),
        "the account that minted it reads it as sent: {text}"
    );
    says(chain, SERVICE, Some("a"), "a", LIMITED);
    chain.at(9, 0);
    chain.at(10, 0);
    let (unnamed, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(pinned, None);
    let sent = body(chain, &unnamed);
    assert_eq!(assistant_lanes(&sent), [lane]);
    minted_absent(&sent.to_string());
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "back again");
    let (next, pinned) = open(chain, SERVICE, model_wire::TURN);
    assert_eq!(pinned.as_deref(), Some("b"));
    minted_absent(&body(chain, &next).to_string());
}
