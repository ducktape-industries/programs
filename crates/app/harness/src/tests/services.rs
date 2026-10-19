use super::*;
use crate::state::MODEL_SERVICES;

// a second egress service the node serves, beside the model's first
const TWO: &str = "ccs-two";
// a reset long after every block these tests make, in seconds and as ccs writes it
const FAR: &str = "4000000000";
const FAR_AT: &str = "2096-01-01T00:00:00Z";

// the node serves both services, and the model is reached through both, `ccs` first
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
    let set = Op::SetServices {
        id: MODEL.into(),
        services: vec![SERVICE.into(), TWO.into()],
    };
    chain.submit(signed(ADA), PROGRAM, &set).unwrap();
}

// the one request open on `service` at `target`, its head, and the account it is pinned to
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

// ccs's status of `service`'s accounts, each with its 5-hour window at a percent, read at
// `height` and answered a block later
fn status(chain: &MockChain, height: u64, service: &str, accounts: &[(&str, u64)]) {
    chain.at(height, 0);
    let op = Op::ReadBudget {
        service: service.into(),
    };
    chain.submit(signed(ADA), PROGRAM, &op).unwrap();
    let (request, _, pinned) = open(chain, service, "/ccs/status");
    assert_eq!(pinned, None, "a status read is pinned to no account");
    chain.at(height + 1, 0);
    answer(chain, request.id, Answer::Output(read(accounts)));
}

fn read(accounts: &[(&str, u64)]) -> Vec<u8> {
    let accounts: Vec<Value> = accounts
        .iter()
        .map(|(label, percent)| {
            json!({
                "label": label,
                "provider": "anthropic",
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

// the model's window, learned through `service` from `account`
fn learned(chain: &MockChain, service: &str, account: Option<&str>) {
    let (request, _, pinned) = open(chain, service, "/v1/models/claude-test");
    assert_eq!(pinned.as_deref(), account);
    let info = json!({ "type": "model", "id": "claude-test", "max_input_tokens": WINDOW });
    answer(chain, request.id, Answer::Output(response(200, info)));
}

// how `account` answers the turn asked of it through `service`
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

fn says(chain: &MockChain, service: &str, account: &str, said: Says) {
    let (request, _, pinned) = open(chain, service, "/v1/messages");
    assert_eq!(
        pinned.as_deref(),
        Some(account),
        "pinned to the account picked"
    );
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

fn services(chain: &MockChain) -> Reply {
    let asked = Query::Services {
        model: MODEL.into(),
    };
    chain.query::<_, Reply>(PROGRAM, &asked).unwrap()
}

fn configured(chain: &MockChain, more: &str) {
    let text = scout()
        .write()
        .replace("max_tokens = 1024\n", &format!("max_tokens = 1024\n{more}"));
    commit(chain, ADA, CONFIG, &text);
}

#[test]
fn a_new_conversation_goes_to_the_account_with_the_most_room_and_stays_on_it() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 80), ("b", 90)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, TWO, Some("c"));
    chain.at(7, 0);
    says(chain, TWO, "c", FINE);
    assert_eq!(route(chain, thread), on(TWO, "c"));
    // c fills past a and b, short of the whole: the conversation stays where its cache is
    status(chain, 8, TWO, &[("c", 95)]);
    world.mention("m2", "again");
    chain.at(10, 0);
    says(
        chain,
        TWO,
        "c",
        Says {
            used: "0.96",
            ..FINE
        },
    );
    let moved: Vec<bool> = answerers(chain, thread)
        .iter()
        .map(|row| row.moved)
        .collect();
    assert_eq!(moved, [false, false]);
    // a new conversation goes where the room is now
    let fresh = Op::NewThread { agent: world.agent };
    chain.submit(signed(ADA), PROGRAM, &fresh).unwrap();
    let second = world.mention("m3", "a new one");
    assert_ne!(second, thread);
    chain.at(11, 0);
    says(chain, SERVICE, "a", FINE);
    assert_eq!(route(chain, second), on(SERVICE, "a"));
}

#[test]
fn a_429_moves_the_turn_to_another_account_at_once_and_the_move_shows_its_cache_write() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, TWO, Some("c"));
    chain.at(7, 0);
    let written = |cache_write| Says {
        cache_write,
        ..FINE
    };
    says(chain, TWO, "c", written(1000));
    world.mention("m2", "and now?");
    chain.at(8, 0);
    says(chain, TWO, "c", LIMITED);
    let told = notices(chain, thread);
    let again =
        "harness asks the model again at block 9 on another of its model's accounts (attempt 1)";
    assert!(told.last().unwrap().ends_with(again), "{told:?}");
    chain.at(9, 0);
    chain.at(10, 0);
    assert!(none_open(chain, TWO, "/v1/messages"));
    says(chain, SERVICE, "a", written(1200));
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
    let rows: Vec<(String, Option<String>, bool)> = answerers(chain, thread)
        .into_iter()
        .map(|row| (row.service, row.account, row.moved))
        .collect();
    assert_eq!(
        rows,
        [
            (TWO.to_owned(), Some("c".to_owned()), false),
            (SERVICE.to_owned(), Some("a".to_owned()), true)
        ]
    );
    assert_eq!(cache_writes(chain, thread), [1000, 1200]);
    // the 429 holds c for every conversation, not only this one
    let fresh = Op::NewThread { agent: world.agent };
    chain.submit(signed(ADA), PROGRAM, &fresh).unwrap();
    let second = world.mention("m3", "a new one");
    chain.at(11, 0);
    says(chain, SERVICE, "a", FINE);
    assert_eq!(route(chain, second), on(SERVICE, "a"));
}

#[test]
fn a_failed_answer_moves_on_but_a_call_cut_off_by_an_update_stays() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, TWO, Some("c"));
    chain.at(7, 0);
    says(chain, TWO, "c", FINE);
    world.mention("m2", "and now?");
    chain.at(8, 0);
    let (request, _, _) = open(chain, TWO, "/v1/messages");
    answer(
        chain,
        request.id,
        Answer::Failed("the service is down".into()),
    );
    chain.at(9, 0);
    chain.at(10, 0);
    says(chain, SERVICE, "a", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
    world.mention("m3", "once more");
    let (request, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("a"));
    chain.cancel_parked(PROGRAM);
    let deadline = request.made + egress::DEFAULT_WAIT;
    chain.at(deadline + 1, 0);
    chain.at(deadline + 2, 0);
    chain.at(deadline + 3, 0);
    assert!(
        none_open(chain, TWO, "/v1/messages"),
        "the cache stays on a"
    );
    says(chain, SERVICE, "a", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
}

#[test]
fn a_pinned_account_is_asked_alone_and_its_429_is_waited_out() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    configured(chain, "account = \"a\"\n");
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "hello");
    chain.at(6, 0);
    learned(chain, SERVICE, Some("a"));
    chain.at(7, 0);
    says(chain, SERVICE, "a", LIMITED);
    let told = notices(chain, thread);
    let wait = "once the model's wait ends at 4000000000 (seconds since the epoch) (attempt 1)";
    assert!(told.last().unwrap().ends_with(wait), "{told:?}");
    chain.at(8, 0);
    chain.at(9, 0);
    assert!(none_open(chain, TWO, "/v1/messages"), "no move off a pin");
    assert!(
        none_open(chain, SERVICE, "/v1/messages"),
        "held until the reset"
    );
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
    learned(chain, SERVICE, Some("b"));
    chain.at(7, 0);
    says(chain, SERVICE, "b", FINE);
    configured(chain, "service = \"elsewhere\"\n");
    world.mention("m2", "and now?");
    let told = notices(chain, thread);
    let why = "the agent's configuration pins service elsewhere, which model claude-test is not reached through (ccs, ccs-two)";
    assert_eq!(told.last().map(String::as_str), Some(why));
    assert!(none_open(chain, SERVICE, "/v1/messages"));
    assert!(none_open(chain, TWO, "/v1/messages"));
}

#[test]
fn an_old_one_service_row_is_asked_as_before_unpinned() {
    let world = world();
    let chain = &world.chain;
    let listed = services(chain);
    assert_eq!(listed, Reply::Services(Some(vec![SERVICE.to_owned()])));
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
fn a_forced_move_near_a_compaction_compacts_first_on_the_new_account() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    // a context of 1,015 tokens: under 2,000, but not with one more answer of up to 1,024
    configured(chain, "compact_at = 2000\n");
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "remember the plan");
    chain.at(6, 0);
    learned(chain, TWO, Some("c"));
    chain.at(7, 0);
    says(
        chain,
        TWO,
        "c",
        Says {
            cache_write: 1000,
            ..FINE
        },
    );
    assert_eq!(thread_row(chain, thread).context, Context::Measured(1015));
    status(chain, 8, TWO, &[("c", 100)]);
    world.mention("m2", "and now?");
    let (request, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("a"));
    assert_eq!(
        body(chain, &request)["compaction"],
        json!({ "type": "summarize" })
    );
    let whole = abi::role::egress::Delivery::Whole;
    assert_eq!(awaited(&request.request, &whole), Some(Awaited::Summary));
}

#[test]
fn only_the_models_owner_sets_its_services_and_budgets_read_every_one() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    let set = |services: &[&str]| Op::SetServices {
        id: MODEL.into(),
        services: services.iter().map(|service| service.to_string()).collect(),
    };
    assert!(chain.submit(signed(BO), PROGRAM, &set(&[TWO])).is_err());
    let both = Reply::Services(Some(vec![SERVICE.to_owned(), TWO.to_owned()]));
    assert_eq!(services(chain), both);
    chain
        .submit(signed(ADA), PROGRAM, &set(&[TWO, TWO, SERVICE]))
        .unwrap();
    let turned = Reply::Services(Some(vec![TWO.to_owned(), SERVICE.to_owned()]));
    assert_eq!(services(chain), turned);
    let Reply::Model(Some(row)) = chain
        .query(PROGRAM, &Query::Model { id: MODEL.into() })
        .unwrap()
    else {
        panic!("the model has a record")
    };
    assert_eq!(row.service, TWO);
    assert!(chain.submit(signed(ADA), PROGRAM, &set(&[])).is_err());
    let key = MODEL_SERVICES.key(&MODEL.to_owned());
    assert_eq!(written(&key), Some(Written::Model));
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
    let text = scout().write().replace(
        "max_tokens = 1024\n",
        "max_tokens = 1024\nservice = \"ccs\"\naccount = \"\"\n",
    );
    let (config, pin) = Config::parse_pinned(text.as_bytes()).unwrap();
    assert_eq!(config, scout());
    let pinned = crate::config::Pin {
        service: Some(SERVICE.into()),
        account: None,
    };
    assert_eq!(pin, pinned);
}

// `account` answers through `service` with a search: its use, its result with content only that
// account reads, and a cited answer
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

#[test]
fn a_search_one_account_answered_reaches_another_as_plain_text_after_a_429() {
    let world = world();
    let chain = &world.chain;
    two_services(&world);
    status(chain, 2, SERVICE, &[("a", 50)]);
    status(chain, 4, TWO, &[("c", 20)]);
    let thread = world.mention("m1", "look it up");
    chain.at(6, 0);
    learned(chain, TWO, Some("c"));
    chain.at(7, 0);
    searched(chain, TWO, "c");
    world.mention("m2", "and now?");
    chain.at(8, 0);
    let (own, _, _) = open(chain, TWO, "/v1/messages");
    let sent = body(chain, &own).to_string();
    assert!(
        sent.contains("EqgfCioIARgB") && sent.contains("server_tool_use"),
        "the account that minted it reads it as sent: {sent}"
    );
    says(chain, TWO, "c", LIMITED);
    chain.at(9, 0);
    chain.at(10, 0);
    let (moved, _, pinned) = open(chain, SERVICE, "/v1/messages");
    assert_eq!(pinned.as_deref(), Some("a"));
    let sent = body(chain, &moved).to_string();
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
    says(chain, SERVICE, "a", FINE);
    assert_eq!(route(chain, thread), on(SERVICE, "a"));
}
