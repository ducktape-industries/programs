use super::*;

fn searcher() -> Config {
    Config {
        server_tools: vec![ServerTool::WebSearch, ServerTool::WebFetch],
        ..scout()
    }
}

fn searched_and_fetched() -> Value {
    json!([
        { "type": "text", "text": "Looking it up." },
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
                "page_age": "April 30, 2025",
            }],
        },
        {
            "type": "server_tool_use",
            "id": "srvtoolu_2",
            "name": "web_fetch",
            "input": { "url": "https://ducktape.example" },
        },
        {
            "type": "web_fetch_tool_result",
            "tool_use_id": "srvtoolu_2",
            "content": {
                "type": "web_fetch_result",
                "url": "https://ducktape.example",
                "content": {
                    "type": "document",
                    "source": {
                        "type": "text",
                        "media_type": "text/plain",
                        "data": "Ducktape is a network.",
                    },
                    "title": "Ducktape",
                },
                "retrieved_at": "2026-10-18T10:00:00Z",
            },
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
    ])
}

fn typed(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .filter(|tool| tool.get("type").is_some())
        .cloned()
        .collect()
}

#[test]
fn server_tools_are_offered_only_to_an_agent_that_lists_them() {
    let world = world();
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    let (request, head) = open_request(chain);
    let tools = body(chain, &request)["tools"].as_array().unwrap().clone();
    assert_eq!(tools.len(), tools::CATALOG.len());
    assert!(typed(&tools).is_empty(), "{tools:?}");
    chain.at(3, 0);
    model_says(
        chain,
        json!([{ "type": "text", "text": "Hi." }]),
        "end_turn",
    );

    commit(chain, ADA, CONFIG, &searcher().write());
    world.mention("m2", "look it up");
    let (request, searching) = open_request(chain);
    let tools = body(chain, &request)["tools"].as_array().unwrap().clone();
    assert_eq!(tools.len(), tools::CATALOG.len() + 2);
    assert_eq!(
        typed(&tools),
        [
            json!({ "type": "web_search_20250305", "name": "web_search" }),
            json!({
                "type": "web_fetch_20250910",
                "name": "web_fetch",
                "cache_control": { "type": "ephemeral" },
            }),
        ]
    );
    assert_eq!(
        head.headers, searching.headers,
        "no beta header of their own"
    );
}

#[test]
fn a_search_and_a_fetch_are_kept_as_sent_never_run_and_sent_back() {
    let world = world();
    let chain = &world.chain;
    commit(chain, ADA, CONFIG, &searcher().write());
    let thread = world.opened("m1", "what is ducktape?", Some(WINDOW));
    chain.at(3, 0);
    model_says_after(chain, searched_and_fetched(), "end_turn", 40);

    let asked = Query::Calls {
        thread,
        page: PageRequest::default(),
    };
    let Reply::Calls(calls) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    assert!(calls.items.is_empty(), "{:?}", calls.items);
    assert!(assigned(chain, RELAY).is_empty(), "nothing relays");
    assert!(assigned(chain, SERVICE).is_empty(), "no request runs them");
    let Some(Body::Turn { said, .. }) = frames(chain, thread).pop() else {
        panic!("the reply is the last frame");
    };
    assert_eq!(said.len(), 6);
    assert!(matches!(said[5], Said::Cited { .. }), "{said:?}");
    assert_eq!(
        thread_row(chain, thread).context,
        Context::Measured(40 + 3 + 5),
        "their tokens count as the reply's usage says"
    );

    world.mention("m2", "and then?");
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    assert_eq!(sent["messages"][1]["role"], "assistant");
    assert_eq!(sent["messages"][1]["content"], searched_and_fetched());
}

#[test]
fn input_heard_while_a_turn_pauses_mid_fetch_waits_until_the_continuation_is_answered() {
    let world = world();
    let chain = &world.chain;
    commit(chain, ADA, CONFIG, &searcher().write());
    let thread = world.opened("m1", "fetch it", Some(WINDOW));
    world.mention("m2", "and this too");
    chain.at(3, 0);
    let all = searched_and_fetched();
    let blocks = all.as_array().unwrap();
    let paused = json!(blocks[3..4].to_vec());
    model_says(chain, paused, "pause_turn");
    assert_eq!(
        held(chain, thread).len(),
        1,
        "the post waits behind the paused turn"
    );
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(
        messages.last().unwrap()["role"],
        "assistant",
        "{messages:?}"
    );
    assert!(
        !sent["messages"].to_string().contains("and this too"),
        "nothing follows the server tool's use: {messages:?}"
    );
    chain.at(4, 0);
    model_says(chain, json!(blocks[4..].to_vec()), "end_turn");
    assert!(
        held(chain, thread).is_empty(),
        "the continuation is answered"
    );
    let bodies = frames(chain, thread);
    let [
        ..,
        Body::Turn { stop: first, .. },
        Body::Turn { stop: last, .. },
        Body::Post { text, .. },
    ] = &bodies[..]
    else {
        panic!("the paused turn, its continuation, then the post: {bodies:?}");
    };
    assert_eq!((*first, *last), (Stop::Pause, Stop::EndTurn));
    assert!(text.contains("and this too"), "{text}");
    let (request, _) = open_request(chain);
    assert!(
        body(chain, &request)["messages"]
            .to_string()
            .contains("and this too")
    );
}

fn continuation_refused(chain: &MockChain) {
    let (request, _) = open_request(chain);
    let refused = json!({
        "type": "error",
        "error": { "type": "invalid_request_error", "message": "continuation refused" },
    });
    answer(chain, request.id, Answer::Output(response(400, refused)));
}

#[test]
fn a_paused_turn_given_up_keeps_no_unanswered_server_tool_use_ahead_of_the_input_held_behind_it() {
    let world = world();
    let chain = &world.chain;
    commit(chain, ADA, CONFIG, &searcher().write());
    let thread = world.opened("m1", "search and fetch it", Some(WINDOW));
    world.mention("m2", "and this too");
    chain.at(3, 0);
    let all = searched_and_fetched();
    let blocks = all.as_array().unwrap();
    model_says(chain, json!(blocks[..4].to_vec()), "pause_turn");
    assert_eq!(held(chain, thread).len(), 1);
    chain.at(4, 0);
    continuation_refused(chain);
    assert!(held(chain, thread).is_empty(), "the turn was given up");
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(messages[1]["role"], "assistant", "{messages:?}");
    assert_eq!(
        messages[1]["content"],
        json!(blocks[..3].to_vec()),
        "the search its result answers stays, the fetch nothing answered goes"
    );
    assert!(
        messages[2].to_string().contains("and this too"),
        "{messages:?}"
    );
    assert!(!sent["messages"].to_string().contains("srvtoolu_2"));
}

#[test]
fn a_paused_turn_given_up_with_nothing_but_an_unanswered_use_is_left_out_as_a_notice() {
    let world = world();
    let chain = &world.chain;
    commit(chain, ADA, CONFIG, &searcher().write());
    let thread = world.opened("m1", "fetch it", Some(WINDOW));
    world.mention("m2", "and this too");
    chain.at(3, 0);
    let all = searched_and_fetched();
    let blocks = all.as_array().unwrap();
    model_says(chain, json!(blocks[3..4].to_vec()), "pause_turn");
    chain.at(4, 0);
    continuation_refused(chain);
    let (request, _) = open_request(chain);
    let sent = body(chain, &request);
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(
        messages.len(),
        1,
        "both inputs are one message: {messages:?}"
    );
    let text = messages[0].to_string();
    assert!(
        text.contains("fetch it") && text.contains("and this too"),
        "{text}"
    );
    assert!(!sent["messages"].to_string().contains("server_tool_use"));
    let bodies = frames(chain, thread);
    assert!(
        !bodies.iter().any(|body| matches!(body, Body::Turn { .. })),
        "{bodies:?}"
    );
}
