use super::schedule::{answered, refused, turn};
use super::*;
use crate::promises::REPEAT;

fn promise(text: &str, blocks: u64) -> Value {
    json!({ "text": text, "channel": "general", "seq": 1, "in": blocks })
}

fn promised(chain: &MockChain, thread: u64, call: &str) -> u64 {
    answered(chain, thread, call)["promise"].as_u64().unwrap()
}

fn told(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Post {
                author: Principal::Root,
                text,
            } => Some(text),
            _ => None,
        })
        .collect()
}

fn open(chain: &MockChain, agent: AccountNumber) -> Vec<PromiseRow> {
    let asked = Query::Promises {
        agent,
        page: PageRequest::default(),
    };
    let Reply::Promises(page) = chain.query(&harness(), &asked).unwrap() else {
        panic!("harness answers promises")
    };
    page.items
}

fn asked(chain: &MockChain, agent: AccountNumber) -> Vec<u64> {
    let asked = Query::Asked {
        agent,
        page: PageRequest::default(),
    };
    let Reply::Promises(page) = chain.query(&harness(), &asked).unwrap() else {
        panic!("harness answers asks")
    };
    page.items.iter().map(|row| row.id).collect()
}

fn owed(chain: &MockChain, agent: AccountNumber) -> Vec<u64> {
    open(chain, agent).iter().map(|row| row.id).collect()
}

fn ask(text: &str, owed_by: AccountNumber, blocks: u64) -> Value {
    let mut asked = promise(text, blocks);
    asked["owed_by"] = json!(owed_by);
    asked
}

fn row(chain: &MockChain, promise: u64) -> Option<PromiseRow> {
    let asked = Query::Promise { promise };
    let Reply::Promise(found) = chain.query(&harness(), &asked).unwrap() else {
        panic!("harness answers a promise")
    };
    found
}

fn said(chain: &MockChain, at: u64, text: &str) {
    chain.at(at, 0);
    model_says(chain, json!([{ "type": "text", "text": text }]), "end_turn");
}

#[test]
fn a_promise_wakes_its_conversation_when_due_and_again_each_interval_until_it_is_kept() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "report back in a while", Some(WINDOW));
    turn(
        &world,
        3,
        &[("p1", "promise", promise("post the report", 7))],
    );
    let id = promised(chain, thread, "p1");
    said(chain, 5, "Promised.");
    let [made] = open(chain, world.agent)
        .try_into()
        .expect("one open promise");
    assert_eq!(
        (made.id, made.owed_by, made.asked_by, made.thread),
        (id, world.agent, world.agent, thread)
    );
    assert_eq!((made.channel.as_str(), made.seq), ("general", 1));
    assert_eq!(made.text, "post the report");
    assert_eq!(
        (made.due, made.next, made.closed.clone()),
        (made.made + 7, made.made + 7, None)
    );
    assert_eq!(row(chain, id), Some(made.clone()));

    chain.at(made.due - 1, 0);
    assert!(
        told(chain, thread).is_empty(),
        "nothing wakes before it is due"
    );
    chain.at(made.due + 1, 0);
    let [first] = told(chain, thread).try_into().expect("one wake");
    let due = format!("Promise {id} is due: post the report");
    assert!(first.starts_with(&due), "{first}");
    let again = format!("comes back at block {}", made.due + REPEAT);
    assert!(first.contains(&again), "{first}");
    assert!(first.contains("Your open promises:"), "{first}");
    assert_eq!(row(chain, id).unwrap().next, made.due + REPEAT);
    said(chain, made.due + 2, "Not yet.");

    chain.at(made.due + REPEAT - 1, 0);
    let wakes = told(chain, thread);
    assert_eq!(
        wakes.len(),
        1,
        "nothing comes back before the repeat: {wakes:?}"
    );
    chain.at(made.due + REPEAT + 1, 0);
    let wakes = told(chain, thread);
    assert_eq!(wakes.len(), 2, "it comes back while it is open: {wakes:?}");
    assert!(wakes[1].starts_with(&due), "{}", wakes[1]);
    let keep = json!({ "promise": id, "kept": true, "how": "posted it in general" });
    let kept_at = made.due + REPEAT + 2;
    turn(&world, kept_at, &[("k1", "promise_keep", keep)]);
    assert_eq!(answered(chain, thread, "k1"), json!({ "ok": true }));
    said(chain, kept_at + 2, "Kept.");
    let closed = row(chain, id).unwrap().closed.expect("it is closed");
    assert!(closed.kept, "{closed:?}");
    assert_eq!(closed.how, "posted it in general");
    assert!(open(chain, world.agent).is_empty());

    chain.at(made.due + 2 * REPEAT + 1, 0);
    assert_eq!(told(chain, thread).len(), 2, "a kept promise wakes no one");
}

#[test]
fn only_its_agent_closes_a_promise_and_only_harness_own_entry_makes_it_due() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "promise something", Some(WINDOW));
    turn(&world, 3, &[("p1", "promise", promise("ship it", 20))]);
    let id = promised(chain, thread, "p1");
    let keep = Op::Keep {
        promise: id,
        kept: false,
        how: "not mine".into(),
    };
    let error = chain.submit(signed(ADA), PROGRAM, &keep).unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    for origin in [signed(ADA), Origin::Program(PROGRAM.into())] {
        let due = Op::PromiseDue { promise: id };
        let error = chain.submit(origin, PROGRAM, &due).unwrap_err();
        assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    }
    assert_eq!(row(chain, id).unwrap().closed, None);
    assert!(told(chain, thread).is_empty());
}

#[test]
fn a_promise_due_now_or_before_is_refused_and_records_nothing() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "promise the past", Some(WINDOW));
    let past = json!({ "text": "too late", "channel": "general", "seq": 1, "height": 2 });
    turn(&world, 3, &[("p1", "promise", past)]);
    let text = refused(chain, thread, "p1");
    assert!(text.starts_with(code::INVALID_INPUT), "{text}");
    assert!(open(chain, world.agent).is_empty());
}

#[test]
fn a_promise_whose_conversation_closed_wakes_the_agents_current_one_and_moves_there() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "promise then go", Some(WINDOW));
    turn(&world, 3, &[("p1", "promise", promise("follow up", 20))]);
    let id = promised(chain, thread, "p1");
    said(chain, 5, "Promised.");
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    chain.at(6, 0);
    let fresh = world.mention("m2", "a fresh start");
    assert_ne!(fresh, thread);
    let due = row(chain, id).unwrap().due;
    chain.at(due + 1, 0);
    assert!(told(chain, thread).is_empty(), "{:?}", told(chain, thread));
    let heard = told(chain, fresh);
    let [handover, wake] = &heard[..] else {
        panic!("m1 handed over, then the promise's wake: {heard:?}")
    };
    let carried = "notice queue Note: 1 unanswered\n\n";
    assert!(handover.starts_with(carried), "{handover}");
    assert!(handover.ends_with("promise then go"), "{handover}");
    let due = format!("Promise {id} is due: follow up");
    assert!(wake.starts_with(&due), "{wake}");
    assert_eq!(row(chain, id).unwrap().thread, fresh);
}

#[test]
fn the_agent_lists_its_open_promises_oldest_first() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "promise twice", Some(WINDOW));
    let two = [
        ("p1", "promise", promise("one", 30)),
        ("p2", "promise", promise("two", 40)),
    ];
    turn(&world, 3, &two);
    let (one, two) = (promised(chain, thread, "p1"), promised(chain, thread, "p2"));
    turn(&world, 5, &[("l1", "promise_list", json!({}))]);
    let listed = answered(chain, thread, "l1");
    let rows = listed["promises"].as_array().unwrap();
    let ids: Vec<u64> = rows
        .iter()
        .map(|row| row["promise"].as_u64().unwrap())
        .collect();
    assert_eq!(ids, [one.min(two), one.max(two)]);
    assert_eq!(rows[0]["conversation"], json!(thread));
    assert_eq!(listed["next"], Value::Null);
}

#[test]
fn an_ask_of_a_person_wakes_the_asker_and_only_that_person_closes_it() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "ask Ada to rule", Some(WINDOW));
    turn(
        &world,
        3,
        &[("a1", "promise", ask("rule on X", world.ada, 20))],
    );
    let id = promised(chain, thread, "a1");
    said(chain, 5, "Asked.");
    let made = row(chain, id).expect("the ask is kept");
    assert_eq!(
        (made.owed_by, made.asked_by, made.thread),
        (world.ada, world.agent, thread)
    );
    assert_eq!(owed(chain, world.ada), [id], "Ada owes it");
    assert_eq!(asked(chain, world.agent), [id], "the agent asked it");
    assert!(
        owed(chain, world.agent).is_empty(),
        "the agent owes nothing"
    );

    world.mention("m2", "close it yourself");
    let keep = json!({ "promise": id, "kept": true, "how": "ruled" });
    turn(&world, 7, &[("k1", "promise_keep", keep)]);
    let text = refused(chain, thread, "k1");
    assert!(text.starts_with(code::UNAUTHORIZED), "{text}");
    said(chain, 9, "Only Ada closes it.");

    chain.at(made.due + 1, 0);
    let [wake] = told(chain, thread).try_into().expect("one wake");
    let due = format!("Ask {id} of account {} is due: rule on X", world.ada);
    assert!(wake.starts_with(&due), "{wake}");

    let keep = Op::Keep {
        promise: id,
        kept: true,
        how: "ruled: X stands".into(),
    };
    chain.submit(signed(ADA), PROGRAM, &keep).unwrap();
    let closed = row(chain, id).unwrap().closed.expect("Ada closed it");
    assert_eq!(
        (closed.kept, closed.how.as_str()),
        (true, "ruled: X stands")
    );
    assert!(owed(chain, world.ada).is_empty());
    assert!(asked(chain, world.agent).is_empty());
}

#[test]
fn an_ask_of_another_agent_wakes_that_agents_conversation_and_the_askers() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "ask Ranger to check", Some(WINDOW));
    let create = identity::Op::CreateAgent {
        name: "Ranger".into(),
    };
    let ranger = number(identity_op(chain, ADA, create).unwrap());
    scripted(chain, ranger, CONFIG);
    let check = ask("check the pods", ranger, 20);
    turn(&world, 3, &[("a1", "promise", check)]);
    let id = promised(chain, thread, "a1");
    said(chain, 5, "Asked.");
    assert_eq!(owed(chain, ranger), [id]);

    chain.at(6, 0);
    world.post(ADA, "general", "m9", &format!("<@{ranger}> hello"));
    let asked_of = Query::Threads {
        agent: ranger,
        page: PageRequest::default(),
    };
    let Reply::Threads(page) = chain.query(&harness(), &asked_of).unwrap() else {
        panic!("harness answers threads")
    };
    let [theirs] = page.items.try_into().expect("Ranger has one conversation");

    chain.at(row(chain, id).unwrap().due + 1, 0);
    let [owing] = told(chain, theirs.id)
        .try_into()
        .expect("Ranger is woken once");
    let owes = format!(
        "Ask {id} is due: account {} asked you: check the pods",
        world.agent
    );
    assert!(owing.starts_with(&owes), "{owing}");
    let [asking] = told(chain, thread)
        .try_into()
        .expect("the asker is woken once");
    let chase = format!("Ask {id} of account {ranger} is due: check the pods");
    assert!(asking.starts_with(&chase), "{asking}");
}

#[test]
fn a_compacted_conversation_hears_its_agents_open_promises_next() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    turn(
        &world,
        3,
        &[("p1", "promise", promise("post the plan", 50))],
    );
    let id = promised(chain, thread, "p1");
    chain.at(5, 0);
    let words = json!([{ "type": "text", "text": "Promised." }]);
    model_says_after(chain, words, "end_turn", 1000);
    world.mention("m2", "and now?");
    chain.at(6, 0);
    summarized(chain, "Ada set out the plan.");
    let messages = sent_messages(chain);
    let last = messages.last().unwrap()["content"].to_string();
    let listed = last.contains("Your open promises:")
        && last.contains(&format!("promise {id}"))
        && last.contains("post the plan");
    assert!(listed, "{last}");
}
