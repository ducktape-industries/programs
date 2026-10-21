use super::*;

fn sse(event: Value) -> Vec<u8> {
    let kind = event["type"].as_str().unwrap().to_owned();
    format!("event: {kind}\r\ndata: {event}").into_bytes()
}

fn began() -> Vec<u8> {
    sse(json!({
        "type": "message_start",
        "message": {
            "id": "msg_1", "type": "message", "role": "assistant", "content": [],
            "model": "claude-sonnet-4-5", "stop_reason": null, "stop_sequence": null,
            "usage": { "input_tokens": 10, "output_tokens": 1,
                       "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0 }
        }
    }))
}

fn calls(index: u64, id: &str, name: &str) -> Vec<u8> {
    sse(json!({
        "type": "content_block_start", "index": index,
        "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} }
    }))
}

fn writes(index: u64, text: &str) -> Vec<u8> {
    sse(json!({
        "type": "content_block_start", "index": index,
        "content_block": { "type": "text", "text": text }
    }))
}

fn input(index: u64, json: &str) -> Vec<u8> {
    sse(json!({
        "type": "content_block_delta", "index": index,
        "delta": { "type": "input_json_delta", "partial_json": json }
    }))
}

fn stops(index: u64) -> Vec<u8> {
    sse(json!({ "type": "content_block_stop", "index": index }))
}

fn ended(stop: &str) -> [Vec<u8>; 2] {
    [
        sse(json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop, "stop_sequence": null },
            "usage": { "output_tokens": 5 }
        })),
        sse(json!({ "type": "message_stop" })),
    ]
}

fn through(chain: &MockChain, request: &RequestRow, events: &[Vec<u8>], upto: usize) {
    let seqs: Vec<u64> = (0..upto as u64).collect();
    stream(chain, request, events, &seqs);
}

fn from(chain: &MockChain, request: &RequestRow, events: &[Vec<u8>], start: usize) {
    let seqs: Vec<u64> = (start as u64..events.len() as u64).collect();
    stream(chain, request, events, &seqs);
}

fn ran(chain: &MockChain, call: &str) {
    chain.at(4, 0);
    relay_back(chain, call);
}

fn message(chain: &MockChain, message_id: &str) -> Option<chat::MsgRow> {
    let asked = chat::Query::MessageById {
        message_id: message_id.into(),
    };
    let chat::Reply::Message(row) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    row
}

fn text_of(row: &chat::MsgRow) -> String {
    chat::plain_text(&row.blocks)
}

fn posts_by(chain: &MockChain, author: AccountNumber) -> Vec<chat::MsgRow> {
    let asked = chat::Query::Roots {
        channel_id: "general".into(),
        viewer: Vec::new(),
        page: PageRequest::default(),
    };
    let chat::Reply::Roots(page) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
        .into_iter()
        .filter(|row| row.author == Principal::Account(author) && !row.deleted)
        .collect()
}

fn call_state(chain: &MockChain, thread: u64, call: &str) -> Option<CallState> {
    let asked = Query::Calls {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Calls(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
        .into_iter()
        .find(|row| row.id == call)
        .map(|row| row.state)
}

fn post_events(call: &str, pieces: &[&str]) -> Vec<Vec<u8>> {
    let mut events = vec![
        began(),
        writes(0, "On it."),
        stops(0),
        calls(1, call, "chat_post"),
    ];
    events.extend(pieces.iter().map(|piece| input(1, piece)));
    events.push(stops(1));
    events.extend(ended("tool_use"));
    events
}

#[test]
fn a_call_runs_once_its_block_is_whole_and_the_model_is_asked_once_the_turn_ends_and_every_result_is_back()
 {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = [
        vec![
            began(),
            calls(0, "toolu_c", "chat_channels"),
            input(0, "{}"),
            stops(0),
        ],
        vec![writes(1, "Looking.")],
        vec![stops(1)],
        ended("tool_use").to_vec(),
    ]
    .concat();
    through(chain, &request, &events, 4);
    assert_eq!(
        call_state(chain, thread, "toolu_c"),
        Some(CallState::Relaying)
    );
    ran(chain, "toolu_c");
    assert!(matches!(
        call_state(chain, thread, "toolu_c"),
        Some(CallState::Done { failed: false, .. })
    ));
    assert!(turns(chain, thread).is_empty(), "no frame before the turn");
    assert!(
        matches!(thread_row(chain, thread).asking, Asking::Asked { .. }),
        "the turn has not ended"
    );
    from(chain, &request, &events, 4);
    end(chain, &request, events.len() as u64);
    let bodies = frames(chain, thread);
    let [.., Body::Turn { .. }, Body::Result { call, failed, .. }] = &bodies[..] else {
        panic!("the turn, then its result: {bodies:?}");
    };
    assert_eq!((call.as_str(), *failed), ("toolu_c", false));
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    assert_eq!(
        thread_row(chain, thread).asking,
        Asking::Asked {
            upto: thread_row(chain, thread).frames
        }
    );
}

#[test]
fn a_call_still_running_when_the_turn_ends_is_recorded_after_it_and_then_the_model_is_asked() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = [
        vec![
            began(),
            calls(0, "toolu_c", "chat_channels"),
            input(0, "{}"),
            stops(0),
        ],
        ended("tool_use").to_vec(),
    ]
    .concat();
    from(chain, &request, &events, 0);
    end(chain, &request, events.len() as u64);
    assert_eq!(thread_row(chain, thread).asking, Asking::Running);
    assert!(assigned(chain, SERVICE).is_empty());
    world.mention("m2", "and this");
    ran(chain, "toolu_c");
    let bodies = frames(chain, thread);
    let [
        ..,
        Body::Turn { .. },
        Body::Result { .. },
        Body::Post { text, .. },
    ] = &bodies[..]
    else {
        panic!("the turn, its result, then the held input: {bodies:?}");
    };
    assert!(text.contains("and this"), "{text}");
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

#[test]
fn a_streamed_post_lands_once_whole_and_is_never_edited_while_the_draft_shows_its_text() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = post_events(
        "toolu_p",
        &[
            r#"{"channel":"gen"#,
            r#"eral","text":"Hel"#,
            r#"lo **wor"#,
            r#"ld**"}"#,
        ],
    );
    let id = format!("harness-{thread}-toolu_p");
    through(chain, &request, &events, 6);
    assert_eq!(drafts(chain), [writing("Hel")], "the post's text alone");
    from(chain, &request, &events, 6);
    end(chain, &request, events.len() as u64);
    assert!(
        message(chain, &id).is_none(),
        "nothing is posted while the call forms"
    );
    assert_eq!(drafts(chain), [writing("Hello **world**")]);
    ran(chain, "toolu_p");
    let [posted] = posts_by(chain, world.agent).try_into().unwrap();
    assert_eq!(posted.message_id, id);
    assert_eq!(posted.author, Principal::Account(world.agent));
    assert_eq!(posted.blocks, chat::parse_message("Hello **world**"));
    assert_eq!(
        (posted.rev, posted.edited),
        (0, false),
        "one PostMessage with the whole text, and no EditMessage"
    );
    assert!(message(chain, &format!("{id}-moved")).is_none());
    let (text, failed) = result_of(chain, thread, "toolu_p");
    assert!(!failed, "{text}");
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

#[test]
fn a_reaction_is_added_once_its_input_is_whole_and_never_twice() {
    let world = world();
    let chain = &world.chain;
    world.post(BO, "general", "b1", "ship it");
    let (thread, request) = asked(&world);
    let seq = message(chain, "b1").unwrap().seq;
    let events = [
        vec![
            began(),
            calls(0, "toolu_r", "chat_react"),
            input(0, r#"{"channel":"general","seq":"#),
            input(0, &format!(r#"{seq},"emoji":"ey"#)),
            input(0, r#"es"}"#),
            stops(0),
        ],
        ended("tool_use").to_vec(),
    ]
    .concat();
    through(chain, &request, &events, 4);
    assert!(message(chain, "b1").unwrap().reactions.is_empty());
    through(chain, &request, &events, 5);
    let reacted = message(chain, "b1").unwrap().reactions;
    assert_eq!(reacted.len(), 1);
    assert_eq!((reacted[0].emoji.as_str(), reacted[0].count), ("eyes", 1));
    from(chain, &request, &events, 5);
    end(chain, &request, events.len() as u64);
    ran(chain, "toolu_r");
    let reacted = message(chain, "b1").unwrap().reactions;
    assert_eq!(reacted.len(), 1);
    assert_eq!(reacted[0].count, 1);
    assert!(!result_of(chain, thread, "toolu_r").1);
}

#[test]
fn a_post_the_target_refuses_becomes_the_calls_failed_result() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = post_events("toolu_p", &[r#"{"channel":"nowhere","text":"Hi"}"#]);
    from(chain, &request, &events, 0);
    end(chain, &request, events.len() as u64);
    ran(chain, "toolu_p");
    let (text, failed) = result_of(chain, thread, "toolu_p");
    assert!(failed, "{text}");
    assert!(text.contains("nowhere"), "{text}");
    assert!(
        assigned(chain, RELAY).is_empty(),
        "a refused call is not run again"
    );
    assert!(posts_by(chain, world.agent).is_empty());
    let (_, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
}

#[test]
fn a_reply_that_starts_over_before_its_post_is_whole_posts_only_the_new_one() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let first = post_events("toolu_a", &[r#"{"channel":"general","text":"Draft one"#]);
    through(chain, &request, &first, 5);
    let old = format!("harness-{thread}-toolu_a");
    assert!(message(chain, &old).is_none());
    assert_eq!(drafts(chain), [writing("Draft one")]);
    let second = post_events("toolu_b", &[r#"{"channel":"general","text":"Final"}"#]);
    from(chain, &request, &second, 0);
    end(chain, &request, second.len() as u64);
    assert_eq!(drafts(chain), [writing("Final")]);
    assert_eq!(call_state(chain, thread, "toolu_a"), None);
    ran(chain, "toolu_b");
    let [posted] = posts_by(chain, world.agent).try_into().unwrap();
    assert_eq!(posted.message_id, format!("harness-{thread}-toolu_b"));
    assert_eq!(text_of(&posted), "Final");
    assert!(message(chain, &old).is_none());
    let [Body::Turn { said, .. }] = &turns(chain, thread)[..] else {
        panic!("one turn");
    };
    assert!(
        said.iter()
            .all(|said| !matches!(said, Said::ToolUse { id, .. } if id == "toolu_a"))
    );
}

#[test]
fn a_reply_that_starts_over_after_a_call_ran_says_so_and_a_post_it_made_is_taken_back() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let first = post_events("toolu_a", &[r#"{"channel":"general","text":"Once"}"#]);
    through(chain, &request, &first, 6);
    ran(chain, "toolu_a");
    let old = format!("harness-{thread}-toolu_a");
    assert_eq!(text_of(&message(chain, &old).unwrap()), "Once");
    let second = [
        vec![began(), writes(0, "Done.")],
        vec![stops(0)],
        ended("end_turn").to_vec(),
    ]
    .concat();
    from(chain, &request, &second, 0);
    end(chain, &request, second.len() as u64);
    assert!(message(chain, &old).unwrap().deleted);
    assert!(posts_by(chain, world.agent).is_empty());
    assert!(
        !frames(chain, thread)
            .iter()
            .any(|body| matches!(body, Body::Result { .. }))
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
}

#[test]
fn a_stream_that_breaks_while_a_post_forms_posts_nothing() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = post_events(
        "toolu_p",
        &[r#"{"channel":"general","text":"Half"#, r#" done"}"#],
    );
    through(chain, &request, &events, 5);
    let id = format!("harness-{thread}-toolu_p");
    assert!(message(chain, &id).is_none());
    let mut broken = events.clone();
    broken[5] = b"event: content_block_delta\r\ndata: not json".to_vec();
    stream(chain, &request, &broken, &[5, 6, 7]);
    end(chain, &request, 8);
    assert!(message(chain, &id).is_none());
    let said = notices(chain, thread);
    assert!(said.iter().any(|text| text.contains("broke")), "{said:?}");
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert!(assigned(chain, RELAY).is_empty());
}

#[test]
fn a_post_still_forming_when_its_thread_closes_never_posts_and_fails() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = post_events(
        "toolu_p",
        &[r#"{"channel":"general","text":"Hi"#, r#" there"}"#],
    );
    through(chain, &request, &events, 5);
    chain
        .submit(signed(ADA), PROGRAM, &Op::Close { thread })
        .unwrap();
    from(chain, &request, &events, 5);
    end(chain, &request, events.len() as u64);
    ran(chain, "toolu_p");
    let id = format!("harness-{thread}-toolu_p");
    assert!(message(chain, &id).is_none(), "nothing was posted");
    let (text, failed) = result_of(chain, thread, "toolu_p");
    assert!(failed && text.contains("closed"), "{text}");
    assert!(assigned(chain, SERVICE).is_empty());
}

#[test]
fn a_mention_in_a_post_still_forming_is_heard_once_the_post_is_whole() {
    let world = world();
    let chain = &world.chain;
    let create = identity::Op::CreateAgent {
        name: "Echo".into(),
    };
    let echo = number(identity_op(chain, ADA, create).unwrap());
    scripted(chain, echo, CONFIG);
    let (_, request) = asked(&world);
    let opening = format!(r#"{{"channel":"general","text":"<@{echo}> pl"#);
    let events = post_events("toolu_p", &[&opening, r#"ease check"}"#]);
    through(chain, &request, &events, 5);
    let echoes = |chain: &MockChain| {
        let asked = Query::Threads {
            agent: echo,
            page: PageRequest::default(),
        };
        let Reply::Threads(page) = chain.query(PROGRAM, &asked).unwrap() else {
            panic!()
        };
        page.items
    };
    assert!(
        echoes(chain).is_empty(),
        "a post still forming wakes no one"
    );
    from(chain, &request, &events, 5);
    end(chain, &request, events.len() as u64);
    ran(chain, "toolu_p");
    let [heard] = echoes(chain).try_into().unwrap();
    let posts: Vec<String> = frames(chain, heard.id)
        .into_iter()
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    let [post] = &posts[..] else {
        panic!("heard once: {posts:?}");
    };
    assert!(post.contains("please check"), "{post}");
}

fn forming(chain: &MockChain, thread: u64) -> Option<String> {
    let partial = partial(chain, thread)?;
    match (partial.forming, partial.said.last()) {
        (Forming::Kept { .. }, Some(Said::ToolUse { input, .. })) => Some(input.clone()),
        _ => None,
    }
}

#[test]
fn a_commits_input_grows_in_the_reply_as_the_model_writes_it_and_the_call_is_placed_once_whole() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(FORGE_COMMIT);
    let pieces = pieces(FORGE_COMMIT);
    through(chain, &request, &events, 2);
    for count in 1..=pieces.len() {
        stream(chain, &request, &events, &[count as u64 + 1]);
        assert_eq!(forming(chain, thread), Some(pieces[..count].concat()));
        assert_eq!(call_state(chain, thread, "toolu_1"), None);
    }
    stream(chain, &request, &events, &[pieces.len() as u64 + 2]);
    assert_eq!(
        call_state(chain, thread, "toolu_1"),
        Some(CallState::Relaying)
    );
}

#[test]
fn a_call_cut_off_mid_input_is_refused_with_its_input_as_written_and_the_model_is_asked_again() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(CUT_OFF);
    from(chain, &request, &events, 0);
    end(chain, &request, events.len() as u64);
    let written = pieces(CUT_OFF).concat();
    let refusal = json!({ "INVALID_JSON": written }).to_string();
    let bodies = frames(chain, thread);
    let [
        ..,
        Body::Turn { stop, .. },
        Body::Refused {
            call,
            tool,
            problem,
        },
    ] = &bodies[..]
    else {
        panic!("the turn, then its call refused: {bodies:?}");
    };
    assert_eq!(*stop, Stop::MaxTokens);
    assert_eq!((call.as_str(), tool.as_str()), ("toolu_1", "forge_commit"));
    assert_eq!(problem, &refusal);
    assert!(notices(chain, thread).is_empty());
    assert_eq!(
        partial(chain, thread),
        Some(Partial::new(Wire::AnthropicMessages))
    );
    assert!(assigned(chain, RELAY).is_empty());
    let (next, head) = open_request(chain);
    assert_eq!(path(&head), "/v1/messages");
    let sent = body(chain, &next);
    let messages = sent["messages"].as_array().unwrap();
    let [.., said, answered] = &messages[..] else {
        panic!("the turn and its refusal are sent: {messages:?}");
    };
    assert_eq!(said["content"][0]["type"], "tool_use");
    assert_eq!(said["content"][0]["input"], json!({}));
    assert_eq!(answered["content"][0]["type"], "tool_result");
    assert_eq!(answered["content"][0]["is_error"], true);
    assert_eq!(answered["content"][0]["content"], refusal);
}

fn call_row(chain: &MockChain, thread: u64, call: &str) -> CallRow {
    let asked = Query::Call {
        thread,
        call: call.into(),
    };
    let Reply::Call(Some(row)) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("call {call} is placed")
    };
    row
}

#[test]
fn a_post_that_ran_is_taken_back_when_the_reply_starts_over_though_its_call_keeps_no_input() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let first = post_events("toolu_a", &[r#"{"channel":"general","text":"Once"}"#]);
    through(chain, &request, &first, 6);
    ran(chain, "toolu_a");
    let row = call_row(chain, thread, "toolu_a");
    assert!(matches!(row.state, CallState::Done { failed: false, .. }));
    assert_eq!(row.input, "", "a call that ran keeps no input of its own");
    let old = format!("harness-{thread}-toolu_a");
    assert!(!message(chain, &old).unwrap().deleted);
    let again = post_events("toolu_b", &[r#"{"channel":"general","text":"Twice"}"#]);
    stream(chain, &request, &again, &[0]);
    assert!(
        message(chain, &old).unwrap().deleted,
        "what it did is still known, so it is taken back"
    );
    assert_eq!(
        call_state(chain, thread, "toolu_a"),
        Some(CallState::Voided { height: 4 })
    );
}

#[test]
fn a_stream_the_node_starts_again_mid_commit_places_its_call_once() {
    let world = world();
    let chain = &world.chain;
    let (thread, request) = asked(&world);
    let events = events(FORGE_COMMIT);
    through(chain, &request, &events, 4);
    assert!(forming(chain, thread).is_some(), "the commit is forming");
    assert_eq!(call_state(chain, thread, "toolu_1"), None);
    from(chain, &request, &events, 0);
    end(chain, &request, events.len() as u64);
    assert_eq!(
        call_state(chain, thread, "toolu_1"),
        Some(CallState::Relaying)
    );
    assert_eq!(assigned(chain, RELAY).len(), 1, "one relay, once");
    let [Body::Turn { said, .. }] = &turns(chain, thread)[..] else {
        panic!("one turn");
    };
    let calls: Vec<&str> = said
        .iter()
        .filter_map(|said| match said {
            Said::ToolUse { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["toolu_1"]);
    assert!(notices(chain, thread).is_empty());
    assert_eq!(thread_row(chain, thread).asking, Asking::Running);
}

fn drafts(chain: &MockChain) -> Vec<(String, String)> {
    let asked = chat::Query::Drafts {
        channel_id: "general".into(),
    };
    let chat::Reply::Drafts(drafts) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    drafts
        .into_iter()
        .map(|draft| (draft.doing, draft.text))
        .collect()
}

fn draft(doing: &str, text: &str) -> (String, String) {
    (doing.into(), text.into())
}

fn writing(text: &str) -> (String, String) {
    draft("writing an answer", text)
}

// each draft in #general: the thread root it answers under, and its text
fn placed(chain: &MockChain) -> Vec<(Option<u64>, String)> {
    let asked = chat::Query::Drafts {
        channel_id: "general".into(),
    };
    let chat::Reply::Drafts(drafts) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    drafts
        .into_iter()
        .map(|draft| (draft.thread, draft.text))
        .collect()
}

#[test]
fn a_turn_that_posts_nothing_drafts_nothing() {
    let world = world();
    let chain = &world.chain;
    let (_, request) = asked(&world);
    assert!(drafts(chain).is_empty(), "nothing while the model is asked");
    let events = [
        vec![
            began(),
            writes(0, "Looking"),
            stops(0),
            calls(1, "toolu_c", "chat_channels"),
            input(1, "{}"),
            stops(1),
        ],
        ended("tool_use").to_vec(),
    ]
    .concat();
    from(chain, &request, &events, 0);
    end(chain, &request, events.len() as u64);
    assert!(
        drafts(chain).is_empty(),
        "its text and its calls draft nothing"
    );
    ran(chain, "toolu_c");
    assert!(drafts(chain).is_empty(), "the next model call");
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Nothing to post." }]),
        "end_turn",
    );
    assert!(drafts(chain).is_empty(), "the turn is over");
}

#[test]
fn a_post_at_the_rooms_foot_drafts_there_once_its_text_begins_and_the_post_takes_its_place() {
    let world = world();
    let chain = &world.chain;
    let (_, request) = asked(&world);
    let posting = post_events(
        "toolu_p",
        &[r#"{"channel":"general","#, r#""text":"Do"#, r#"ne"}"#],
    );
    through(chain, &request, &posting, 5);
    assert!(
        drafts(chain).is_empty(),
        "the room alone does not say whether it answers in a thread"
    );
    stream(chain, &request, &posting, &[5]);
    assert_eq!(
        placed(chain),
        [(None, "Do".to_owned())],
        "at the room's foot"
    );
    assert_eq!(drafts(chain), [writing("Do")]);
    from(chain, &request, &posting, 6);
    end(chain, &request, posting.len() as u64);
    assert_eq!(drafts(chain), [writing("Done")]);
    ran(chain, "toolu_p");
    let [posted] = posts_by(chain, world.agent).try_into().unwrap();
    assert_eq!(text_of(&posted), "Done");
    assert!(drafts(chain).is_empty(), "the post took the draft's place");
}

#[test]
fn a_reply_in_a_thread_drafts_under_its_root_as_reply_to_arrives_before_any_text() {
    let world = world();
    let chain = &world.chain;
    let (_, request) = asked(&world);
    let posting = post_events(
        "toolu_t",
        &[
            r#"{"channel":"general","#,
            r#""reply_to":1,"#,
            r#""text":"In the"#,
            r#" thread"}"#,
        ],
    );
    through(chain, &request, &posting, 5);
    assert!(placed(chain).is_empty(), "the room alone");
    stream(chain, &request, &posting, &[5]);
    assert_eq!(
        placed(chain),
        [(Some(1), String::new())],
        "the thread opens as reply_to arrives, before any text"
    );
    assert_eq!(drafts(chain), [writing("")]);
    stream(chain, &request, &posting, &[6]);
    assert_eq!(placed(chain), [(Some(1), "In the".to_owned())]);
    from(chain, &request, &posting, 7);
    end(chain, &request, posting.len() as u64);
    assert_eq!(placed(chain), [(Some(1), "In the thread".to_owned())]);
    ran(chain, "toolu_t");
    assert!(placed(chain).is_empty(), "the reply took the draft's place");
}

#[test]
fn a_turn_waiting_to_ask_its_model_again_drafts_nothing() {
    let world = world();
    let chain = &world.chain;
    let (_, request) = asked(&world);
    let limited =
        json!({ "type": "error", "error": { "type": "rate_limit_error", "message": "Slow down" } });
    answer(chain, request.id, Answer::Output(response(429, limited)));
    assert!(drafts(chain).is_empty());
}

#[test]
fn a_mention_heard_while_a_reaction_shows_reaches_its_conversation_once() {
    let world = world();
    let chain = &world.chain;
    world.post(BO, "general", "b1", "ship it");
    let (thread, request) = asked(&world);
    let seq = message(chain, "b1").unwrap().seq;
    let reaction = format!(r#"{{"channel":"general","seq":{seq},"emoji":"eyes"}}"#);
    let events = [
        vec![
            began(),
            calls(0, "toolu_r", "chat_react"),
            input(0, &reaction),
            stops(0),
        ],
        ended("tool_use").to_vec(),
    ]
    .concat();
    through(chain, &request, &events, 3);
    assert_eq!(
        call_state(chain, thread, "toolu_r"),
        Some(CallState::Showing)
    );
    world.mention("m2", "and this");
    from(chain, &request, &events, 3);
    end(chain, &request, events.len() as u64);
    ran(chain, "toolu_r");
    let heard = |chain: &MockChain| {
        frames(chain, thread)
            .into_iter()
            .filter(|body| matches!(body, Body::Post { text, .. } if text.contains("and this")))
            .count()
    };
    assert_eq!(heard(chain), 1, "{:?}", frames(chain, thread));
    chain.at(5, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Seen." }]),
        "end_turn",
    );
    assert_eq!(heard(chain), 1, "{:?}", frames(chain, thread));
}
