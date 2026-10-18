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
