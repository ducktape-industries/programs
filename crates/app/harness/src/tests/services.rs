use super::*;

const TWO: &str = "ccs-two";
const FAR: &str = "4000000000";
const FAR_AT: &str = "2096-01-01T00:00:00Z";

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
    chain
        .submit(signed(&NODE), egress::PROGRAM, &serve)
        .unwrap();
    let both = [
        (SERVICE, Wire::AnthropicMessages),
        (TWO, Wire::AnthropicMessages),
    ];
    chain
        .submit(signed(ADA), PROGRAM, &set_route(&both))
        .unwrap();
}

fn open(
    chain: &MockChain,
    service: &str,
    target: &str,
) -> (RequestRow, http::Head, Option<String>) {
    let rows: Vec<(RequestRow, http::Head)> = assigned(chain, service)
        .into_iter()
        .filter_map(|row| match row.request.clone() {
            Request::Http { head, .. } if head.target == target => Some((row, head)),
            _ => None,
        })
        .collect();
    let count = rows.len();
    let Ok([(row, head)]) = <[(RequestRow, http::Head); 1]>::try_from(rows) else {
        panic!("{count} requests open on {service} at {target}");
    };
    let pinned = head
        .headers
        .iter()
        .find(|(key, _)| key == "x-ccs-account")
        .map(|(_, value)| value.clone());
    (row, head, pinned)
}

fn none_open(chain: &MockChain, service: &str, target: &str) -> bool {
    let at = |row: &RequestRow| matches!(&row.request, Request::Http { head, .. } if head.target == target);
    !assigned(chain, service).iter().any(at)
}

fn status(chain: &MockChain, height: u64, service: &str, accounts: &[(&str, u64)]) {
    status_of(chain, height, service, "claude", accounts);
}

fn status_of(
    chain: &MockChain,
    height: u64,
    service: &str,
    provider: &str,
    accounts: &[(&str, u64)],
) {
    chain.at(height, 0);
    let op = Op::ReadBudget {
        service: service.into(),
    };
    chain.submit(signed(ADA), PROGRAM, &op).unwrap();
    let (request, _, pinned) = open(chain, service, "/ccs/status");
    assert_eq!(pinned, None, "a status read is pinned to no account");
    chain.at(height + 1, 0);
    answer(
        chain,
        request.id,
        Answer::Output(read_of(provider, accounts)),
    );
}

fn read(accounts: &[(&str, u64)]) -> Vec<u8> {
    read_of("claude", accounts)
}

fn read_of(provider: &str, accounts: &[(&str, u64)]) -> Vec<u8> {
    let accounts: Vec<Value> = accounts
        .iter()
        .map(|(label, percent)| {
            json!({
                "label": label,
                "provider": provider,
                "read_at": FAR_AT,
                "five_hour": { "percent": percent, "resets_at": FAR_AT },
                "weekly": null,
                "models": [],
                "error": null
            })
        })
        .collect();
    response(200, json!({ "accounts": accounts }))
}

fn learned(chain: &MockChain, service: &str) {
    let (request, _, pinned) = open(chain, service, "/v1/models/claude-test");
    assert_eq!(pinned, None, "a new conversation names no account");
    let info = json!({ "type": "model", "id": "claude-test", "max_input_tokens": WINDOW });
    answer(chain, request.id, Answer::Output(response(200, info)));
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
    let (request, _, sent) = open(chain, service, "/v1/messages");
    assert_eq!(sent.as_deref(), pinned, "the account the call names");
    let body = match said.status {
        200 => json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "claude-test",
            "content": [{ "type": "text", "text": "hello back" }],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "cache_creation_input_tokens": said.cache_write
            }
        }),
        _ => {
            json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "limited" } })
        }
    };
    let headers = [
        ("x-ccs-account", account),
        ("anthropic-ratelimit-unified-5h-utilization", said.used),
        ("anthropic-ratelimit-unified-5h-reset", FAR),
    ];
    let response = http::Response {
        status: said.status,
        headers: headers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        body: body.to_string().into_bytes(),
    };
    answer(chain, request.id, Answer::Output(abi::encode(&response)));
}

fn refused(chain: &MockChain, pinned: Option<&str>, message: &str) {
    let (request, _, sent) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(sent.as_deref(), pinned, "the account the call names");
    let error = json!({ "type": "invalid_request_error", "message": message });
    let body = json!({ "type": "error", "error": error });
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
fn a_new_conversation_names_no_account_and_stays_on_the_one_ccs_answered_with_whatever_its_budget_reads()
 {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 80), ("b", 10)]);
    status(chain, 4, TWO, &[("c", 0)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    status(chain, 8, SERVICE, &[("a", 10), ("b", 100)]);
    world.mention("m2", "again");
    chain.at(10, 0);
    says(chain, SERVICE, Some("b"), "b", Says { used: "1", ..FINE });
    let moved: Vec<bool> = answerers(chain, thread)
        .iter()
        .map(|row| row.moved)
        .collect();
    assert_eq!(moved, [false, false]);
    let fresh = Op::NewThread { agent: world.agent };
    chain.submit(signed(ADA), PROGRAM, &fresh).unwrap();
    let second = world.mention("m3", "a new one");
    assert_ne!(second, thread);
    chain.at(11, 0);
    says(chain, SERVICE, None, "a", FINE);
    assert_eq!(route(chain, second), on(SERVICE, "a"));
}

#[test]
fn a_429_on_the_threads_account_asks_again_next_block_naming_none_and_the_thread_stays_where_ccs_answered()
 {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
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
    assert!(none_open(chain, TWO, "/v1/messages"), "ccs picks first");
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
    assert!(none_open(chain, SERVICE, "/v1/messages"), "held");
    assert!(none_open(chain, TWO, "/v1/messages"), "held");
    let held = wakes_of(chain, thread);
    let until = 4_000_000_000_000;
    let named =
        |wake: &Wake| matches!(wake, Wake::RetryAfter { until: reset, .. } if *reset == until);
    assert!(held.iter().any(named), "the wait names its reset: {held:?}");
    world.mention("m2", "still there?");
    let (_, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned, None, "a new input ends the wait");
}

#[test]
fn a_status_read_that_shows_an_account_with_room_ends_a_wait_for_the_reset() {
    let world = world();
    let chain = &world.chain;
    status(chain, 2, SERVICE, &[("a", 100)]);
    let thread = world.mention("m1", "hello");
    chain.at(4, 0);
    learned(chain, SERVICE);
    chain.at(5, 0);
    says(chain, SERVICE, None, "a", LIMITED);
    chain.at(6, 0);
    assert!(none_open(chain, SERVICE, "/v1/messages"), "held");
    status(chain, 7, SERVICE, &[("a", 100)]);
    chain.at(9, 0);
    assert!(
        none_open(chain, SERVICE, "/v1/messages"),
        "no account has room"
    );
    status_of(chain, 10, SERVICE, "codex", &[("x", 10)]);
    chain.at(12, 0);
    assert!(
        none_open(chain, SERVICE, "/v1/messages"),
        "another provider's room is not this call's"
    );
    status(chain, 13, SERVICE, &[("a", 100), ("b", 40)]);
    chain.at(16, 0);
    let (_, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned, None, "ccs picks the account with room");
    let wakes = wakes_of(chain, thread);
    let held = |wake: &Wake| matches!(wake, Wake::RetryAfter { .. });
    assert!(!wakes.iter().any(held), "{wakes:?}");
}

#[test]
fn a_pin_to_an_account_ccs_no_longer_holds_asks_again_naming_none_and_only_an_unpinned_refusal_is_final()
 {
    let world = world();
    let chain = &world.chain;
    status(chain, 2, SERVICE, &[("a", 50)]);
    let thread = world.mention("m1", "hello");
    chain.at(4, 0);
    learned(chain, SERVICE);
    chain.at(5, 0);
    says(chain, SERVICE, None, "a", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
    world.mention("m2", "and now?");
    chain.at(6, 0);
    refused(chain, Some("a"), "no such account for claude: a");
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
        none_open(chain, SERVICE, "/v1/messages"),
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
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "a", FINE);
    world.mention("m2", "and now?");
    chain.at(8, 0);
    let (request, _, pinned) = open(chain, SERVICE, "/v1/messages");
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
    let (request, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("b"));
    chain.cancel_parked(PROGRAM);
    let deadline = request.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    chain.at(deadline + 2, 0);
    chain.at(deadline + 3, 0);
    assert!(
        none_open(chain, TWO, "/v1/messages"),
        "the cache stays on b"
    );
    says(chain, SERVICE, Some("b"), "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
}

#[test]
fn a_pinned_service_keeps_to_its_accounts_and_one_the_model_lists_not_gives_up() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    configured(chain, &format!("service = \"{SERVICE}\"\n"));
    status(chain, 2, SERVICE, &[("a", 50), ("b", 30)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    says(chain, SERVICE, None, "a", FINE);
    configured(chain, "service = \"elsewhere\"\n");
    world.mention("m2", "and now?");
    let told = notices(chain, thread);
    let why = "the agent's configuration pins service elsewhere, which provider anthropic's route is not reached through (ccs, ccs-two)";
    assert_eq!(told.last().map(String::as_str), Some(why));
    assert!(none_open(chain, SERVICE, "/v1/messages"));
    assert!(none_open(chain, TWO, "/v1/messages"));
}

#[test]
fn a_one_service_route_is_asked_unpinned() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    let (_, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned, None);
    model_says(chain, json!([{ "type": "text", "text": "hi" }]), "end_turn");
    assert_eq!(route(chain, thread), Some((SERVICE.to_owned(), None)));
    let [only]: [Answerer; 1] = answerers(chain, thread).try_into().unwrap();
    assert!(!only.moved);
}

#[test]
fn a_call_moved_near_the_threshold_does_not_compact_on_the_account_ccs_picks() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    configured(chain, "compact_at = 2000\n");
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
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
    let (request, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned, None);
    let sent = body(chain, &request);
    assert!(
        sent["compaction"].is_null(),
        "a turn, not a summary: {sent}"
    );
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "back again");
    let (_, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("b"), "it stays where ccs answered");
}

#[test]
fn a_route_lists_its_services_in_order_once_each_and_budgets_read_every_one() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let set = |services: &[&str]| {
        let listed: Vec<(&str, Wire)> = services
            .iter()
            .map(|service| (*service, Wire::AnthropicMessages))
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
    chain.at(2, 0);
    chain.submit(signed(BO), PROGRAM, &Op::ReadBudgets).unwrap();
    let (first, _, _) = open(chain, SERVICE, "/ccs/status");
    let (second, _, _) = open(chain, TWO, "/ccs/status");
    chain.at(3, 0);
    answer(chain, first.id, Answer::Output(read(&[("a", 10)])));
    answer(chain, second.id, Answer::Output(read(&[("c", 20)])));
    let Reply::Budget(report) = chain.query(PROGRAM, &Query::Budget).unwrap() else {
        panic!("the budget answers as one")
    };
    let mut read: Vec<String> = report.reads.into_iter().map(|read| read.service).collect();
    read.sort();
    assert_eq!(read, [SERVICE, TWO]);
}

#[test]
fn a_pin_reads_from_the_front_matter_and_leaves_the_config_as_it_was() {
    let text = md(&scout()).replace(
        "max_tokens = 1024\n",
        "max_tokens = 1024\nservice = \"ccs\"\n",
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
    let (request, _, _) = open(chain, service, "/v1/messages");
    let body = json!({
        "id": "msg_s",
        "type": "message",
        "role": "assistant",
        "model": "claude-test",
        "content": [
            {
                "type": "server_tool_use",
                "id": "srvtoolu_1",
                "name": "web_search",
                "input": { "query": "ducktape" },
            },
            {
                "type": "web_search_tool_result",
                "tool_use_id": "srvtoolu_1",
                "content": [{
                    "type": "web_search_result",
                    "url": "https://ducktape.example",
                    "title": "Ducktape",
                    "encrypted_content": "EqgfCioIARgB",
                }],
            },
            {
                "type": "text",
                "text": "Ducktape is a network.",
                "citations": [{
                    "type": "web_search_result_location",
                    "url": "https://ducktape.example",
                    "title": "Ducktape",
                    "encrypted_index": "Eo8BCioIAhgB",
                    "cited_text": "Ducktape is a network.",
                }],
            },
        ],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 10, "output_tokens": 5 }
    });
    let headers = [
        ("x-ccs-account", account),
        ("anthropic-ratelimit-unified-5h-utilization", "0.5"),
        ("anthropic-ratelimit-unified-5h-reset", FAR),
    ];
    let response = http::Response {
        status: 200,
        headers: headers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        body: body.to_string().into_bytes(),
    };
    answer(chain, request.id, Answer::Output(abi::encode(&response)));
}

fn minted_absent(sent: &str) {
    for minted in [
        "EqgfCioIARgB",
        "Eo8BCioIAhgB",
        r#""type":"server_tool_use""#,
        r#""type":"web_search_tool_result""#,
    ] {
        assert!(
            !sent.contains(minted),
            "{minted} reached another account: {sent}"
        );
    }
    assert!(sent.contains("Ducktape https://ducktape.example"), "{sent}");
    assert!(sent.contains("Ducktape is a network."), "{sent}");
}

#[test]
fn a_search_one_account_answered_goes_as_plain_text_once_a_429_leaves_the_account_to_ccs() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "look it up");
    chain.at(6, 0);
    learned(chain, SERVICE);
    chain.at(7, 0);
    searched(chain, SERVICE, "a");
    world.mention("m2", "and now?");
    chain.at(8, 0);
    let (own, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("a"));
    let sent = body(chain, &own).to_string();
    assert!(
        sent.contains("EqgfCioIARgB") && sent.contains("server_tool_use"),
        "the account that minted it reads it as sent: {sent}"
    );
    says(chain, SERVICE, Some("a"), "a", LIMITED);
    chain.at(9, 0);
    chain.at(10, 0);
    let (unnamed, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned, None);
    minted_absent(&body(chain, &unnamed).to_string());
    says(chain, SERVICE, None, "b", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "b"));
    world.mention("m3", "back again");
    let (next, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("b"));
    minted_absent(&body(chain, &next).to_string());
}
