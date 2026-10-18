use super::*;
use crate::state::{CALLS, CURRENT, FRAMES, HELD, MODELS, TALLIES, THREADS};

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
    assert_eq!(written(&CURRENT.key(&12u64)), Some(Written::Agent(12)));
    assert_eq!(written(&MODELS.key(&"m".to_owned())), Some(Written::Model));
    assert_eq!(written(b"count"), None);
}
