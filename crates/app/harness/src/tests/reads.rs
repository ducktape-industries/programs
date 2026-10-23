use super::*;
use crate::state::{CALLS, CURRENT, FRAMES, HELD, PROVIDER_ROUTES, SETTLING, TALLIES, THREADS};

fn tally(chain: &MockChain, thread: u64) -> Tally {
    let Reply::Tally(tally) = chain.query(PROGRAM, &Query::Tally { thread }).unwrap() else {
        panic!()
    };
    tally
}

fn added_up(chain: &MockChain, thread: u64) -> Tally {
    let mut tally = Tally::default();
    for body in frames(chain, thread) {
        tally.count(&body);
    }
    tally
}

fn last_turn(chain: &MockChain, thread: u64) -> Option<u64> {
    let asked = Query::LastTurn { thread };
    let Reply::LastTurn(height) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    height
}

fn newest(chain: &MockChain, thread: u64) -> Body {
    frames(chain, thread).pop().expect("a frame")
}

fn topic_of(chain: &MockChain, thread: u64) -> Option<String> {
    let Reply::Topic(topic) = chain.query(PROGRAM, &Query::Topic { thread }).unwrap() else {
        panic!()
    };
    topic
}

fn conversations(world: &World) -> Vec<Conversation> {
    let asked = Query::Conversations {
        agent: world.agent,
        page: PageRequest::default(),
    };
    let Reply::Conversations(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

fn span(chain: &MockChain, thread: u64, from: u64, page: PageRequest) -> PageResponse<Frame> {
    let asked = Query::FramesFrom { thread, from, page };
    let Reply::Frames(span) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    span
}

fn seqs(span: &PageResponse<Frame>) -> Vec<u64> {
    span.items.iter().map(|frame| frame.seq).collect()
}

#[test]
fn a_tally_keeps_what_the_turns_used_and_each_summary_as_they_are_recorded() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    assert_eq!(tally(chain, thread), Tally::default());
    chain.at(3, 0);
    model_says_after(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
        1000,
    );
    let one = tally(chain, thread);
    assert_eq!(one.usage.input, 1000);
    assert_eq!(one, added_up(chain, thread));
    world.mention("m2", "and now?");
    chain.at(4, 0);
    summarized(chain, "Ada set out the plan.");
    let two = tally(chain, thread);
    assert_eq!((two.usage, two.summaries), (one.usage, 1));
    assert_eq!(two, added_up(chain, thread));
}

#[test]
fn conversations_answers_each_row_with_its_topic_and_tally_in_the_order_threads_does() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_t", "thread_topic", json!({ "topic": "the plan" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_t");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Noted." }]),
        "end_turn",
    );
    let fork = Op::ForkThread { agent: world.agent };
    let output = chain.submit(signed(ADA), PROGRAM, &fork).unwrap();
    let forked: u64 = abi::decode(&output).unwrap();
    let read = conversations(&world);
    let rows: Vec<ThreadRow> = read.iter().map(|each| each.row.clone()).collect();
    let ids: Vec<u64> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids, [forked, thread]);
    assert_eq!(rows, world.threads());
    for each in &read {
        let id = each.row.id;
        assert_eq!(each.tally, Some(tally(chain, id)), "conversation {id}");
        assert_eq!(each.topic, topic_of(chain, id), "conversation {id}");
    }
    let first = &read[1];
    assert_eq!(first.topic.as_deref(), Some("the plan"));
    let kept = first.tally.expect("a conversation that turned is tallied");
    assert!(kept.usage.input > 0, "{kept:?}");
}

#[test]
fn only_a_turn_moves_last_turn_never_a_heard_post_a_result_a_notice_or_a_summary() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "remember the plan", Some(2_000));
    assert!(matches!(newest(chain, thread), Body::Post { .. }));
    assert_eq!(last_turn(chain, thread), None, "a heard post is no turn");
    chain.at(3, 0);
    let call = tool_use("toolu_t", "thread_topic", json!({ "topic": "the plan" }));
    model_says(chain, json!([call]), "tool_use");
    assert!(matches!(newest(chain, thread), Body::Turn { .. }));
    assert_eq!(last_turn(chain, thread), Some(3));
    chain.at(4, 0);
    relay_back(chain, "toolu_t");
    assert!(matches!(newest(chain, thread), Body::Result { .. }));
    assert_eq!(
        last_turn(chain, thread),
        Some(3),
        "a tool result is no turn"
    );
    chain.at(5, 0);
    let (request, _) = open_request(chain);
    let overloaded = json!({ "type": "error", "error": { "type": "overloaded_error", "message": "Overloaded" } });
    answer(chain, request.id, Answer::Output(response(529, overloaded)));
    assert!(matches!(newest(chain, thread), Body::Notice { .. }));
    assert_eq!(last_turn(chain, thread), Some(3), "a notice is no turn");
    chain.at(6, 0);
    world.mention("m2", "and now?");
    assert!(matches!(newest(chain, thread), Body::Post { .. }));
    assert_eq!(last_turn(chain, thread), Some(3), "a heard post is no turn");
    chain.at(7, 0);
    let noted = json!([{ "type": "text", "text": "Noted." }]);
    model_says_after(chain, noted, "end_turn", 1000);
    assert_eq!(last_turn(chain, thread), Some(7));
    world.mention("m3", "what next?");
    chain.at(8, 0);
    summarized(chain, "Ada set out the plan.");
    let summary = |body: &Body| matches!(body, Body::Compaction { .. });
    assert!(frames(chain, thread).iter().any(summary));
    assert_eq!(last_turn(chain, thread), Some(7), "a summary is no turn");
    let fork = Op::ForkThread { agent: world.agent };
    let output = chain.submit(signed(ADA), PROGRAM, &fork).unwrap();
    let forked: u64 = abi::decode(&output).unwrap();
    assert_eq!(
        last_turn(chain, forked),
        Some(7),
        "a fork keeps the turn it copies"
    );
}

fn adopt_tallies(chain: &MockChain, after: Option<Vec<u8>>, limit: u64) -> Option<Vec<u8>> {
    let adopt = Op::AdoptTallies { after, limit };
    abi::decode(&chain.submit(signed(BO), PROGRAM, &adopt).unwrap()).unwrap()
}

#[test]
fn adopt_tallies_tallies_each_untallied_conversation_once_and_conversations_reads_rows_alone() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    let fork = Op::ForkThread { agent: world.agent };
    let output = chain.submit(signed(ADA), PROGRAM, &fork).unwrap();
    let forked: u64 = abi::decode(&output).unwrap();
    let kept: Vec<Option<Tally>> = conversations(&world)
        .into_iter()
        .map(|each| each.tally)
        .collect();
    assert_eq!(
        kept,
        [Some(tally(chain, forked)), Some(tally(chain, thread))],
        "a fork is tallied as it copies"
    );
    for id in [forked, thread] {
        chain
            .host(PROGRAM)
            .borrow_mut()
            .state
            .remove(&TALLIES.key(&id));
    }
    let untallied: Vec<Option<Tally>> = conversations(&world)
        .into_iter()
        .map(|each| each.tally)
        .collect();
    assert_eq!(
        untallied,
        [None, None],
        "an untallied conversation adds up no frames"
    );
    let mut after = adopt_tallies(chain, None, 10);
    let one_run: Vec<bool> = conversations(&world)
        .into_iter()
        .map(|each| each.tally.is_some())
        .collect();
    assert_eq!(
        one_run,
        [false, true],
        "a run tallies one conversation, the oldest, however many it may walk"
    );
    assert!(after.is_some(), "it resumes after the one it tallied");
    let mut runs = 1;
    while after.is_some() {
        after = adopt_tallies(chain, after, 10);
        runs += 1;
    }
    assert_eq!(runs, 3, "one run each, then one that finds none left");
    let adopted: Vec<Option<Tally>> = conversations(&world)
        .into_iter()
        .map(|each| each.tally)
        .collect();
    let added = [Some(added_up(chain, forked)), Some(added_up(chain, thread))];
    assert_eq!(adopted, added);
    let marked = Tally {
        summaries: 7,
        ..Tally::default()
    };
    chain
        .host(PROGRAM)
        .borrow_mut()
        .state
        .insert(TALLIES.key(&thread), abi::encode(&marked));
    assert_eq!(adopt_tallies(chain, None, 10), None);
    let again: Vec<Option<Tally>> = conversations(&world)
        .into_iter()
        .map(|each| each.tally)
        .collect();
    let kept = [Some(added_up(chain, forked)), Some(marked)];
    assert_eq!(
        again, kept,
        "a second backfill leaves every kept row as it is"
    );
}

fn adopting(chain: &MockChain, height: u64) -> usize {
    let asked = scheduler::Query::Due {
        height,
        after: None,
        limit: None,
    };
    let scheduler::Reply::Due { entries, .. } = chain.query(scheduler::PROGRAM, &asked).unwrap()
    else {
        panic!("the scheduler answers what is due")
    };
    entries
        .iter()
        .filter(|entry| {
            matches!(
                abi::decode::<Op>(&entry.payload),
                Ok(Op::AdoptTallies { .. })
            )
        })
        .count()
}

fn chained(chains: usize) -> (Vec<u64>, u64) {
    let world = world();
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    for _ in 0..2 {
        let fork = Op::ForkThread { agent: world.agent };
        chain.submit(signed(ADA), PROGRAM, &fork).unwrap();
    }
    let mut ids: Vec<u64> = world.threads().iter().map(|row| row.id).collect();
    ids.sort();
    assert_eq!(ids.len(), 3);
    for id in &ids {
        chain
            .host(PROGRAM)
            .borrow_mut()
            .state
            .remove(&TALLIES.key(id));
    }
    for _ in 0..chains {
        let next = adopt_tallies(chain, None, 10);
        assert!(next.is_some(), "a run that tallied one goes on after it");
    }
    let mut height = chain.env(PROGRAM).height;
    let mut blocks = 0;
    let mut marked: std::collections::BTreeMap<u64, Tally> = Default::default();
    loop {
        for each in conversations(&world) {
            let id = each.row.id;
            let Some(kept) = each.tally else {
                continue;
            };
            if let Some(mark) = marked.get(&id) {
                assert_eq!(kept, *mark, "conversation {id} was tallied again");
                continue;
            }
            assert_eq!(kept, added_up(chain, id), "conversation {id}");
            let mark = Tally {
                summaries: kept.summaries + 100,
                ..kept
            };
            chain
                .host(PROGRAM)
                .borrow_mut()
                .state
                .insert(TALLIES.key(&id), abi::encode(&mark));
            marked.insert(id, mark);
        }
        height += 1;
        if adopting(chain, height) == 0 {
            break;
        }
        chain.at(height + 1, 0);
        blocks += 1;
    }
    let tallied: Vec<u64> = marked.keys().copied().collect();
    assert_eq!(tallied, ids, "every conversation is tallied");
    (ids, blocks)
}

#[test]
fn one_adopt_tallies_call_books_its_own_runs_and_tallies_every_conversation_one_a_block() {
    let (ids, blocks) = chained(1);
    assert_eq!(
        blocks,
        ids.len() as u64,
        "one more block per conversation after the call's own, the last finding none left"
    );
}

#[test]
fn two_adopt_tallies_chains_started_together_never_tally_a_conversation_twice() {
    let (ids, blocks) = chained(2);
    assert!(
        blocks < ids.len() as u64,
        "two chains share the work: {blocks}"
    );
}

#[test]
fn a_fork_tallies_the_frames_it_copied_by_adding_them_up() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );
    let fork = Op::ForkThread { agent: world.agent };
    let output = chain.submit(signed(ADA), PROGRAM, &fork).unwrap();
    let forked: u64 = abi::decode(&output).unwrap();
    assert_ne!(forked, thread);
    assert_eq!(tally(chain, forked), tally(chain, thread));
    assert_eq!(tally(chain, forked), added_up(chain, forked));
}

#[test]
fn frames_from_reads_a_span_from_a_seq_on_and_a_call_reads_alone() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "look", Some(WINDOW));
    chain.at(3, 0);
    let call = tool_use("toolu_l", "files_list", json!({ "path": "/" }));
    model_says(chain, json!([call]), "tool_use");
    chain.at(4, 0);
    relay_back(chain, "toolu_l");
    let whole = frames(chain, thread);
    assert_eq!(whole.len(), 3);

    let one = span(chain, thread, 2, PageRequest::first(1));
    assert_eq!(seqs(&one), [2]);
    assert!(one.next.is_some(), "frame 3 follows");
    let rest = span(chain, thread, 2, PageRequest::resume(one.next, 1));
    assert_eq!((seqs(&rest), rest.next), (vec![3], None));
    let all = span(chain, thread, 2, PageRequest::default());
    let bodies: Vec<Body> = all.items.into_iter().map(|frame| frame.body).collect();
    assert_eq!(bodies, whole[1..]);
    let past = span(chain, thread, 9, PageRequest::default());
    assert!(past.items.is_empty());

    let asked = Query::Call {
        thread,
        call: "toolu_l".into(),
    };
    let Reply::Call(Some(row)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert!(matches!(row.state, CallState::Done { failed: false, .. }));
    assert!(
        row.input.is_empty(),
        "the turn's frame alone keeps the input"
    );
}

#[test]
fn a_written_key_names_the_thread_or_agent_it_belongs_to() {
    assert_eq!(written(&THREADS.key(&7u64)), Some(Written::Thread(7)));
    assert_eq!(
        written(&FRAMES.key(&(7u64, 3u64))),
        Some(Written::Frame { thread: 7, seq: 3 })
    );
    let call = Written::Call {
        thread: 7,
        call: "toolu_l".into(),
    };
    assert_eq!(
        written(&CALLS.key(&(7u64, "toolu_l".to_owned()))),
        Some(call)
    );
    assert_eq!(written(&HELD.key(&(7u64, 0u64))), Some(Written::Held(7)));
    assert_eq!(written(&TALLIES.key(&7u64)), Some(Written::Tally(7)));
    assert_eq!(written(&SETTLING.key(&7u64)), Some(Written::Settling(7)));
    assert_eq!(written(&CURRENT.key(&12u64)), Some(Written::Agent(12)));
    let route = PROVIDER_ROUTES.key(&"anthropic".to_owned());
    assert_eq!(written(&route), Some(Written::Route));
    assert_eq!(written(b"count"), None);
}
