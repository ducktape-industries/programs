//! What harness shows of the accounts a model's service answers from: the budget row each answer
//! writes, the account a conversation stays on, and a wait for a reset that another answer ends.
use super::*;
use model_wire::{Block, Class, Event, Lane, Stop};

const FAR: u64 = 4_000_000_000;
const TWO: &str = "two";
const OPENAI: &str = "openai";

type Window = (String, String, u64, u64, u64);
type Row = (String, String, Option<String>, u64, u64, Vec<Window>);
type By = (String, Option<String>, bool);

pub(super) fn harness_wired(world: &World) {
    let route = set_route(&[(SERVICE, Wire::Harness)]);
    world.chain.submit(signed(ADA), PROGRAM, &route).unwrap();
}

pub(super) fn from_node(chain: &MockChain, op: &egress::Op) {
    chain.submit(signed(&NODE), egress::PROGRAM, op).unwrap();
}

fn asked_at(chain: &MockChain, service: &str, target: &str) -> Vec<RequestRow> {
    let at = |row: &RequestRow| matches!(&row.request, Request::Http { head, .. } if head.target == target);
    assigned(chain, service).into_iter().filter(at).collect()
}

pub(super) fn model_read(chain: &MockChain) {
    model_read_on(chain, SERVICE, MODEL, PROVIDER);
}

pub(super) fn model_read_on(chain: &MockChain, service: &str, name: &str, provider: &str) {
    let path = model_wire::model_path(name);
    let [request]: [RequestRow; 1] = asked_at(chain, service, &path)
        .try_into()
        .expect("one model read open");
    let model = model_wire::Model {
        provider: provider.into(),
        window: Some(WINDOW),
        pictures: None,
    };
    let body = serde_json::to_value(&model).unwrap();
    answer(chain, request.id, Answer::Output(response(200, body)));
}

pub(super) fn turns_open(chain: &MockChain, thread: u64) -> Vec<RequestRow> {
    turns_open_on(chain, SERVICE, thread)
}

fn turns_open_on(chain: &MockChain, service: &str, thread: u64) -> Vec<RequestRow> {
    let deliver = egress::Delivery::Events {
        deliver: streamed_as(thread),
    };
    let open = asked_at(chain, service, model_wire::TURN).into_iter();
    open.filter(|row| row.delivery == deliver).collect()
}

pub(super) fn turn_open(chain: &MockChain, thread: u64) -> (RequestRow, Option<String>) {
    turn_open_on(chain, SERVICE, thread)
}

fn turn_open_on(chain: &MockChain, service: &str, thread: u64) -> (RequestRow, Option<String>) {
    let open = turns_open_on(chain, service, thread);
    let count = open.len();
    let Ok([row]) = <[RequestRow; 1]>::try_from(open) else {
        panic!("{count} turns open for conversation {thread} on {service}");
    };
    let pinned = pinned_to(chain, &row);
    (row, pinned)
}

pub(super) fn pinned_to(chain: &MockChain, row: &RequestRow) -> Option<String> {
    let Request::Http { head, .. } = &row.request else {
        return None;
    };
    let unnamed = head.headers.iter().all(|(key, _)| key == "content-type");
    assert!(
        unnamed,
        "a call names its account only in its body: {:?}",
        head.headers
    );
    if row.request.parts().is_empty() {
        return None;
    }
    let target = body(chain, row)["target"].clone();
    match target["kind"].as_str() {
        Some("pinned") => {
            assert_eq!(target["service"], row.request.service(), "{target}");
            target["account"].as_str().map(str::to_owned)
        }
        Some("unpinned") => None,
        _ => panic!("a call's body names where it goes: {target}"),
    }
}

pub(super) fn answer_headers(account: &str, used: &str, status: &str) -> Vec<(String, String)> {
    let reset = FAR.to_string();
    let headers = [
        ("x-harness-account", account),
        ("x-harness-window-5h-used", used),
        ("x-harness-window-5h-reset", reset.as_str()),
        ("x-harness-window-5h-status", status),
        ("x-harness-status", status),
    ];
    let each = headers.iter();
    each.map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

pub(super) fn limited_headers(account: &str) -> Vec<(String, String)> {
    let mut headers = answer_headers(account, "1", "rejected");
    headers.push(("retry-after".to_owned(), FAR.to_string()));
    headers
}

pub(super) fn replied(
    chain: &MockChain,
    request: &RequestRow,
    said: Vec<Block>,
    usage: model_wire::Usage,
    headers: Vec<(String, String)>,
) {
    let reply = model_wire::Reply {
        model: MODEL.into(),
        lane: Lane::of(PROVIDER, MODEL),
        said,
        stop: Stop::EndTurn,
        usage,
        prompt: None,
        context: None,
    };
    let event = egress::Event {
        id: request.id,
        seq: 0,
        data: model_wire::sse(&Event::Reply(reply)).into_bytes(),
    };
    from_node(chain, &egress::Op::Events(vec![event]));
    let end = egress::StreamEnd::Complete {
        status: 200,
        headers,
    };
    let ended = egress::Op::EndStream {
        id: request.id,
        events: 1,
        end,
    };
    from_node(chain, &ended);
}

pub(super) fn says_as(chain: &MockChain, thread: u64, account: &str, used: &str) {
    says_on(chain, SERVICE, thread, account, used);
}

pub(super) fn says_on(chain: &MockChain, service: &str, thread: u64, account: &str, used: &str) {
    let (request, _) = turn_open_on(chain, service, thread);
    let said = vec![Block::Text {
        text: "hello back".into(),
        citations: Vec::new(),
    }];
    let usage = model_wire::Usage {
        input: 10,
        output: 5,
        cache_read: 0,
        cache_write: 0,
    };
    let headers = answer_headers(account, used, "allowed");
    replied(chain, &request, said, usage, headers);
}

fn limited_as(chain: &MockChain, thread: u64, account: &str) {
    limited_on(chain, SERVICE, thread, account);
}

fn limited_on(chain: &MockChain, service: &str, thread: u64, account: &str) {
    let (request, _) = turn_open_on(chain, service, thread);
    let failure = model_wire::Failure {
        class: Class::Limited,
        message: "limited".into(),
        reset: Some(FAR),
    };
    let limited = http::Response {
        status: 429,
        headers: limited_headers(account),
        body: serde_json::to_vec(&failure).unwrap(),
    };
    answer(chain, request.id, Answer::Output(abi::encode(&limited)));
}

fn budget_now(chain: &MockChain) -> BudgetReport {
    let Reply::BudgetFacts(report) = chain.query(PROGRAM, &Query::BudgetFacts).unwrap() else {
        panic!("the budget answers as one")
    };
    report.budget
}

fn rows(report: BudgetReport) -> Vec<Row> {
    let row = |account: AccountBudget| {
        let windows = account.windows.into_iter().map(|window| {
            let used = (window.utilization, window.used);
            (window.name, used.0, used.1, window.reset, window.since)
        });
        let at = (account.height, account.time);
        let named = (account.service, account.account, account.label);
        (named.0, named.1, named.2, at.0, at.1, windows.collect())
    };
    report.accounts.into_iter().map(row).collect()
}

fn row(
    account: &str,
    (height, time): (u64, u64),
    (used, millionths): (&str, u64),
    since: u64,
) -> Row {
    let window = ("5h".to_owned(), used.to_owned(), millionths, FAR, since);
    let named = Some(account.to_owned());
    (
        SERVICE.to_owned(),
        account.to_owned(),
        named,
        height,
        time,
        vec![window],
    )
}

pub(super) fn route_now(chain: &MockChain, thread: u64) -> Option<(String, Option<String>)> {
    let Reply::Route(route) = chain.query(PROGRAM, &Query::Route { thread }).unwrap() else {
        panic!("a route answers as one")
    };
    route.map(|route| (route.service, route.account))
}

fn routes_now(chain: &MockChain) -> Vec<(u64, String, Option<String>)> {
    let asked = Query::Routes {
        page: PageRequest::default(),
    };
    let Reply::Routes(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("routes answer as a page")
    };
    let each = page.items.into_iter();
    each.map(|routed| (routed.thread, routed.route.service, routed.route.account))
        .collect()
}

fn answerers_now(chain: &MockChain, thread: u64) -> Vec<By> {
    let asked = Query::Answerers {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Answerers(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("answerers answer as a page")
    };
    let each = page.items.into_iter();
    each.map(|row| (row.service, row.account, row.moved))
        .collect()
}

fn by(account: &str, moved: bool) -> By {
    (SERVICE.to_owned(), Some(account.to_owned()), moved)
}

pub(super) fn on(account: &str) -> Option<(String, Option<String>)> {
    Some((SERVICE.to_owned(), Some(account.to_owned())))
}

fn second_service(world: &World) {
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
    let both = set_route(&[(SERVICE, Wire::Harness), (TWO, Wire::Harness)]);
    chain.submit(signed(ADA), PROGRAM, &both).unwrap();
    let openai = Op::SetRoute {
        provider: OPENAI.into(),
        services: vec![RouteService {
            service: SERVICE.into(),
            wire: Wire::Harness,
        }],
    };
    chain.submit(signed(ADA), PROGRAM, &openai).unwrap();
}

pub(super) fn configured_as(
    chain: &MockChain,
    path: &str,
    model: &str,
    service: &str,
    provider: &str,
) {
    let pin = Pin {
        service: Some(service.into()),
        provider: Some(provider.into()),
    };
    let config = Config {
        model: model.into(),
        ..scout()
    };
    commit(chain, ADA, path, &config.write(&pin));
}

pub(super) fn agent_on(
    world: &World,
    name: &str,
    model: &str,
    service: &str,
    provider: &str,
) -> AccountNumber {
    let path = format!("/home/Ada/agents/{}.md", name.to_lowercase());
    configured_as(&world.chain, &path, model, service, provider);
    list(&world.chain, ADA, &path, ::program::Ownership::Owner);
    let create = identity::Op::CreateAgent { name: name.into() };
    let agent = number(identity_op(&world.chain, ADA, create).unwrap());
    scripted(&world.chain, agent, &path);
    agent
}

pub(super) fn mention_of(world: &World, agent: AccountNumber, id: &str, text: &str) -> u64 {
    world.post(BO, "general", id, &format!("<@{agent}> {text}"));
    let asked = Query::Threads {
        agent,
        page: PageRequest::default(),
    };
    let Reply::Threads(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("threads answer as a page")
    };
    page.items[0].id
}

fn held_for_the_reset(chain: &MockChain, thread: u64) -> bool {
    let Reply::Wakes(wakes) = chain.query(PROGRAM, &Query::Wakes { thread }).unwrap() else {
        panic!("harness answers wakes")
    };
    wakes
        .iter()
        .any(|wake| matches!(wake, Wake::RetryAfter { .. }))
}

#[test]
fn each_answer_writes_the_budget_row_of_the_account_its_service_names() {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    let first = world.mention("m1", "hello");
    chain.at(2, 10_000);
    model_read(chain);
    chain.at(3, 60_000);
    says_as(chain, first, "a", "0.25");
    let one = row("a", (3, 60_000), ("0.25", 250_000), 60_000);
    assert_eq!(rows(budget_now(chain)), [one]);
    world.mention("m2", "again");
    chain.at(4, 120_000);
    says_as(chain, first, "a", "0.5");
    new_thread_retiring_the_current_one(&world);
    let second = world.mention("m3", "a new one");
    assert_ne!(second, first);
    chain.at(5, 150_000);
    says_as(chain, second, "b", "0.1");
    assert_eq!(
        rows(budget_now(chain)),
        [
            row("a", (4, 120_000), ("0.5", 500_000), 60_000),
            row("b", (5, 150_000), ("0.1", 100_000), 150_000)
        ]
    );
}

#[test]
fn the_budget_reply_says_each_services_pace_and_each_accounts_windows_and_when_they_run_out() {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    let thread = world.mention("m1", "hello");
    chain.at(2, 10_000);
    model_read(chain);
    chain.at(3, 60_000);
    says_as(chain, thread, "a", "0.25");
    world.mention("m2", "again");
    chain.at(4, 90_000);
    says_as(chain, thread, "a", "0.5");
    chain.at(5, 100_000);
    let window = WindowOutlook {
        name: "5h".into(),
        utilization: "0.5".into(),
        used: 500_000,
        reset: FAR,
        status: Some("allowed".into()),
        since: 60_000,
        runs_out: Some(150_000),
    };
    let account = AccountBudget {
        service: SERVICE.into(),
        account: "a".into(),
        label: Some("a".into()),
        organization: None,
        status: Some("allowed".into()),
        fallback: None,
        error: None,
        height: 4,
        time: 90_000,
        windows: vec![window],
    };
    let pace = ServicePace {
        service: SERVICE.into(),
        last_minute: 1,
        this_minute: 2,
    };
    let expected = BudgetReport {
        time: 100_000,
        turns: 0,
        services: vec![pace],
        accounts: vec![account],
        reads: Vec::new(),
    };
    assert_eq!(budget_now(chain), expected);
}

#[test]
fn a_conversation_never_names_the_account_that_answered_it_and_after_a_429_its_service_picks_another()
 {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    model_read(chain);
    chain.at(3, 0);
    let (_, pinned) = turn_open(chain, thread);
    assert_eq!(pinned, None, "a new conversation names no account");
    says_as(chain, thread, "a", "0.25");
    assert_eq!(route_now(chain, thread), on("a"));
    world.mention("m2", "and now?");
    chain.at(4, 0);
    let (_, pinned) = turn_open(chain, thread);
    assert_eq!(pinned, None, "not even the account that answered it");
    limited_as(chain, thread, "a");
    assert!(
        held_for_the_reset(chain, thread),
        "its service fell over on every account it holds"
    );
    assert_eq!(
        route_now(chain, thread),
        on("a"),
        "a refusal moves no route"
    );
    world.mention("m3", "once more");
    chain.at(5, 0);
    let (_, pinned) = turn_open(chain, thread);
    assert_eq!(
        pinned, None,
        "a new input asks at once, and its service picks"
    );
    says_as(chain, thread, "b", "0.1");
    assert_eq!(route_now(chain, thread), on("b"));
    let routed = (thread, SERVICE.to_owned(), Some("b".to_owned()));
    assert_eq!(routes_now(chain), [routed]);
    assert_eq!(
        answerers_now(chain, thread),
        [by("b", true), by("a", false)]
    );
    let accounts = budget_now(chain).accounts.into_iter();
    let used: Vec<(String, u64)> = accounts
        .map(|row| (row.account, row.windows[0].used))
        .collect();
    let read = [("a".to_owned(), 1_000_000), ("b".to_owned(), 100_000)];
    assert_eq!(used, read);
    world.mention("m4", "and again");
    chain.at(6, 0);
    let (_, pinned) = turn_open(chain, thread);
    assert_eq!(pinned, None, "nor the one that answered it last");
}

#[test]
fn a_conversation_held_for_its_reset_asks_again_once_a_turn_on_its_route_and_provider_is_answered()
{
    let world = world();
    let chain = &world.chain;
    second_service(&world);
    configured_as(chain, CONFIG, MODEL, SERVICE, PROVIDER);
    let held = world.mention("m1", "hello");
    chain.at(2, 0);
    model_read(chain);
    chain.at(3, 0);
    limited_as(chain, held, "a");
    chain.at(4, 0);
    chain.at(5, 0);
    let moved = turns_open_on(chain, TWO, held).len();
    assert_eq!(moved, 1, "the retry moves on: {:?}", notices(chain, held));
    limited_on(chain, TWO, held, "c");
    chain.at(6, 0);
    chain.at(7, 0);
    assert!(turns_open(chain, held).is_empty(), "held");
    assert!(turns_open_on(chain, TWO, held).is_empty(), "held");
    assert!(held_for_the_reset(chain, held));
    let ranger = agent_on(&world, "Ranger", MODEL, SERVICE, PROVIDER);
    let ranging = mention_of(&world, ranger, "m2", "anyone there?");
    chain.at(8, 0);
    limited_on(chain, SERVICE, ranging, "b");
    assert!(held_for_the_reset(chain, held), "a 429 shows no room");
    let reader = agent_on(&world, "Reader", "claude-other", SERVICE, PROVIDER);
    mention_of(&world, reader, "m3", "which model are you?");
    chain.at(9, 0);
    model_read_on(chain, SERVICE, "claude-other", PROVIDER);
    assert!(
        held_for_the_reset(chain, held),
        "a model read shows no room"
    );
    let scribe = agent_on(&world, "Scribe", "gpt-test", SERVICE, OPENAI);
    let scribing = mention_of(&world, scribe, "m4", "and you?");
    chain.at(10, 0);
    model_read_on(chain, SERVICE, "gpt-test", OPENAI);
    chain.at(11, 0);
    says_on(chain, SERVICE, scribing, "d", "0.1");
    assert!(
        held_for_the_reset(chain, held),
        "another provider's room is not its own"
    );
    let tracker = agent_on(&world, "Tracker", MODEL, TWO, PROVIDER);
    let tracking = mention_of(&world, tracker, "m5", "and you?");
    chain.at(12, 0);
    says_on(chain, SERVICE, tracking, "e", "0.1");
    assert!(
        !held_for_the_reset(chain, held),
        "room on any service of its route is its own"
    );
    chain.at(13, 0);
    chain.at(14, 0);
    let open = [SERVICE, TWO].map(|service| turns_open_on(chain, service, held).len());
    assert_eq!(open, [1, 0], "{:?}", notices(chain, held));
    let (_, pinned) = turn_open_on(chain, SERVICE, held);
    assert_eq!(pinned, None, "its service picks an account with room");
}

#[test]
fn a_conversation_waiting_out_its_backoff_asks_again_once_a_turn_on_its_service_and_provider_is_answered()
 {
    let world = world();
    let chain = &world.chain;
    harness_wired(&world);
    configured_as(chain, CONFIG, MODEL, SERVICE, PROVIDER);
    let waiting = world.mention("m1", "hello");
    chain.at(2, 0);
    model_read(chain);
    for height in [3, 5, 8] {
        chain.at(height, 0);
        let (request, _) = turn_open(chain, waiting);
        answer(
            chain,
            request.id,
            Answer::Failed("the service is down".into()),
        );
    }
    let told = notices(chain, waiting);
    let again = "again at block 12 (attempt 3)";
    assert!(told.last().unwrap().ends_with(again), "{told:?}");
    let ranger = agent_on(&world, "Ranger", MODEL, SERVICE, PROVIDER);
    let ranging = mention_of(&world, ranger, "m2", "anyone there?");
    chain.at(9, 0);
    says_on(chain, SERVICE, ranging, "a", "0.1");
    chain.at(11, 0);
    let (_, pinned) = turn_open(chain, waiting);
    assert_eq!(pinned, None, "pulled in before block 12, its service picks");
}
