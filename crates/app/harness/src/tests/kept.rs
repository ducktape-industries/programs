use ::program::schema::interface;
use programs::Programs;

use super::network::served;
use super::*;
use crate::schema::tests::{code_with, listed, module};
use crate::state::MODEL_KEPT;

const NEXT: &str = "claude-next";

fn switched(chain: &MockChain, model: &str, provider: &str) {
    let config = Config {
        model: model.into(),
        max_tokens: 2048,
        thinking: Some(Thinking::High),
        compact_at: Some(1000),
        ..scout()
    };
    commit(chain, ADA, CONFIG, &config.write(&pin_of(provider)));
}

fn asked(chain: &MockChain) -> [Value; 4] {
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), model_wire::TURN, "a turn, not a model read");
    let sent = provider_body(chain, &request);
    [
        sent["model"].clone(),
        sent["max_tokens"].clone(),
        sent["thinking"].clone(),
        sent["compaction"].clone(),
    ]
}

fn kept() -> [Value; 4] {
    [json!(MODEL), json!(1024), Value::Null, Value::Null]
}

fn front_now() -> [Value; 4] {
    [
        json!(NEXT),
        json!(2048),
        json!({ "type": "adaptive" }),
        Value::Null,
    ]
}

fn learn_next(chain: &MockChain) {
    let (request, head) = open_request(chain);
    assert_eq!(path(&head), model_wire::model_path(NEXT));
    let info = json!({ "type": "model", "id": NEXT, "max_input_tokens": WINDOW });
    answer(chain, request.id, Answer::Output(response(200, info)));
}

fn done(chain: &MockChain) {
    model_says(
        chain,
        json!([{ "type": "text", "text": "Done." }]),
        "end_turn",
    );
}

fn bind(world: &World, channel: &str) -> u64 {
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: channel.into(),
        },
    };
    abi::decode(&world.chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap()
}

fn room(world: &World, channel: &str) {
    let op = chat::Op::CreateChannel {
        channel_id: channel.into(),
        name: channel.into(),
        post_policy: chat::PostPolicy::Open,
    };
    world.chain.submit(signed(ADA), chat::PROGRAM, &op).unwrap();
}

fn codes(build: &[u8]) -> [(&'static str, Vec<u8>); 3] {
    [
        (PROGRAM, module(&[("ducktape.build", build)])),
        (egress::PROGRAM, code_with(&interface::<Egress>())),
        ("valset", module(&[])),
    ]
}

fn from_before_kept_models(chain: &MockChain, threads: &[u64]) {
    let host = chain.host(PROGRAM);
    let mut host = host.borrow_mut();
    for thread in threads {
        host.state.remove(&MODEL_KEPT.key(thread));
    }
}

fn card_model(chain: &MockChain, thread: u64) -> Option<(String, Option<u64>)> {
    let Reply::Agent(card) = chain.query(PROGRAM, &Query::ThreadCard { thread }).unwrap() else {
        panic!("a thread card answers as an agent's")
    };
    let configured = card?.configured;
    let model = configured
        .config()
        .map(|(_, config)| config.model.clone())?;
    Some((model, configured.compact_at()))
}

fn models(chain: &MockChain, agent: AccountNumber) -> Vec<(u64, Option<String>, Window)> {
    let asked = Query::ConversationModels {
        agent,
        page: PageRequest::default(),
    };
    let Reply::ConversationModels(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("conversation models answer as a page")
    };
    let rows = page.items.into_iter();
    rows.map(|row| (row.thread, row.kept.map(|kept| kept.model), row.window))
        .collect()
}

#[test]
fn an_open_conversation_keeps_the_model_it_started_with_and_a_new_one_takes_the_front_as_it_is() {
    let world = world();
    let chain = &world.chain;
    let open = world.opened("m1", "remember the plan", Some(WINDOW));
    chain.at(3, 0);
    let said = json!([{ "type": "text", "text": "Noted." }]);
    model_says_after(chain, said, "end_turn", 1000);
    assert_eq!(thread_row(chain, open).context, Context::Measured(1008));
    switched(chain, NEXT, PROVIDER);
    world.mention("m2", "and now?");
    assert_eq!(
        asked(chain),
        kept(),
        "its model, max_tokens, thinking and compact_at are the ones it started with"
    );
    assert_eq!(card_model(chain, open), Some((MODEL.to_owned(), None)));
    chain.at(4, 0);
    done(chain);
    new_thread_retiring_the_current_one(&world);
    let fresh = world.mention("m3", "a new one");
    assert_ne!(fresh, open);
    chain.at(5, 0);
    learn_next(chain);
    assert_eq!(
        asked(chain),
        front_now(),
        "a new conversation takes the front"
    );
    assert_eq!(
        card_model(chain, fresh),
        Some((NEXT.to_owned(), Some(1000)))
    );
    assert_eq!(
        models(chain, world.agent),
        [
            (fresh, Some(NEXT.to_owned()), Window::Tokens(WINDOW)),
            (open, Some(MODEL.to_owned()), Window::Tokens(WINDOW)),
        ]
    );
}

#[test]
fn a_conversation_opened_before_the_front_changed_keeps_its_provider_though_it_never_asked() {
    let world = world();
    let chain = &world.chain;
    let bound = bind(&world, "general");
    switched(chain, NEXT, "openai");
    let heard = world.mention("m1", "hello");
    assert_eq!(heard, bound);
    chain.at(2, 0);
    learn(chain, Some(WINDOW));
    assert_eq!(asked(chain), kept(), "anthropic's route, its model");
    assert!(
        notices(chain, bound).is_empty(),
        "{:?}",
        notices(chain, bound)
    );
}

#[test]
fn conversations_open_when_new_harness_code_first_runs_keep_the_front_that_run_reads() {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    listed(&world.chain, &codes(b"1"));
    served(&world.chain);
    let world = world;
    let chain = &world.chain;
    let canonical = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    done(chain);
    room(&world, "side");
    let side = bind(&world, "side");
    from_before_kept_models(chain, &[canonical, side]);
    chain.at(4, 0);
    listed(chain, &codes(b"2"));
    world.mention("m2", "still there?");
    assert_eq!(asked(chain), kept());
    chain.at(5, 0);
    done(chain);
    switched(chain, NEXT, PROVIDER);
    world.post(ADA, "side", "m3", &format!("<@{}> and here?", world.agent));
    assert_eq!(
        asked(chain),
        kept(),
        "it never asked under the new code, and keeps the front that code's first run read"
    );
    chain.at(6, 0);
    done(chain);
    new_thread_retiring_the_current_one(&world);
    world.mention("m4", "a new one");
    chain.at(7, 0);
    learn_next(chain);
    assert_eq!(asked(chain), front_now());
}

#[test]
fn a_window_no_service_reported_is_asked_again_once_new_harness_code_first_runs() {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    listed(&world.chain, &codes(b"1"));
    served(&world.chain);
    let world = world;
    let chain = &world.chain;
    world.opened("m1", "hello", None);
    chain.at(3, 0);
    done(chain);
    world.mention("m2", "again");
    chain.at(4, 0);
    let (_, head) = open_request(chain);
    assert_eq!(
        path(&head),
        model_wire::TURN,
        "the same code asks no window again"
    );
    done(chain);
    chain.at(5, 0);
    listed(chain, &codes(b"2"));
    world.mention("m3", "and now?");
    chain.at(6, 0);
    let (_, head) = open_request(chain);
    assert_eq!(
        path(&head),
        model_wire::model_path(MODEL),
        "new code asks the unreported window once"
    );
    learn(chain, Some(WINDOW));
    let Reply::Agent(Some(card)) = chain
        .query(PROGRAM, &Query::Card { agent: world.agent })
        .unwrap()
    else {
        panic!("the agent has a card")
    };
    let window = card.configured.reached().map(|reached| reached.window);
    assert_eq!(window, Some(Window::Tokens(WINDOW)));
}
