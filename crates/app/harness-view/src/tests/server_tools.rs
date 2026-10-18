use super::*;
use serde_json::json;

fn searched_and_fetched() -> Vec<Said> {
    let search = json!({
        "type": "web_search_tool_result",
        "tool_use_id": "srvtoolu_01",
        "content": [
            {
                "type": "web_search_result",
                "url": "https://status.example.com",
                "title": "Status",
                "encrypted_content": "e1",
            },
            {
                "type": "web_search_result",
                "url": "https://blog.example.com/timeouts",
                "title": "Timeouts",
                "encrypted_content": "e2",
            },
        ],
    });
    let fetch = json!({
        "type": "web_fetch_tool_result",
        "tool_use_id": "srvtoolu_02",
        "content": {
            "type": "web_fetch_result",
            "url": "https://blog.example.com/timeouts",
            "content": {
                "type": "document",
                "source": { "type": "text", "media_type": "text/plain", "data": "Fail fast." },
                "title": "Timeouts",
            },
            "retrieved_at": "2026-10-18T10:00:00Z",
        },
    });
    let citation = json!({
        "type": "web_search_result_location",
        "url": "https://blog.example.com/timeouts",
        "title": "Timeouts",
        "encrypted_index": "i1",
        "cited_text": "Fail fast.",
    });
    vec![
        Said::ServerToolUse {
            id: "srvtoolu_01".into(),
            name: "web_search".into(),
            input: r#"{"query":"payments timeout"}"#.into(),
        },
        Said::ServerToolResult(search.to_string()),
        Said::ServerToolUse {
            id: "srvtoolu_02".into(),
            name: "web_fetch".into(),
            input: r#"{"url":"https://blog.example.com/timeouts"}"#.into(),
        },
        Said::ServerToolResult(fetch.to_string()),
        Said::Cited {
            text: "A timeout under a second fails fast.".into(),
            citations: vec![citation.to_string()],
        },
    ]
}

#[test]
fn a_search_and_a_fetch_read_as_what_they_did_and_found() {
    let mut net = Net::default();
    let turn = Body::Turn {
        model: "claude-sonnet-5-5".into(),
        config: SNAPSHOT,
        said: searched_and_fetched(),
        stop: Stop::EndTurn,
        usage: Usage::default(),
    };
    net.frames.push((7, frame(4, turn)));
    net.threads[0].frames = 4;
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    for words in [
        "Searched the web: payments timeout",
        "2 results",
        "Fetched https://blog.example.com/timeouts",
        "“Timeouts”",
        "A timeout under a second fails fast.",
        "Sources: Timeouts",
    ] {
        assert!(cx.has_text(words), "{words}: {:?}", cx.texts());
    }
    assert!(!cx.has_text("Not placed"), "{:?}", cx.texts());
    assert!(
        !cx.has_text("Result of srvtoolu_01 · 2 results"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("server-srvtoolu_01");
    cx.run_until_parked();
    assert!(
        cx.has_text("Status · https://status.example.com"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_burst_says_it_searched_the_web_and_fetched_pages() {
    let said = vec![
        Said::ToolUse {
            id: "toolu_09".into(),
            name: "chat_post".into(),
            input: "{}".into(),
        },
        Said::ServerToolUse {
            id: "srvtoolu_01".into(),
            name: "web_search".into(),
            input: r#"{"query":"q"}"#.into(),
        },
        Said::ServerToolUse {
            id: "srvtoolu_02".into(),
            name: "web_fetch".into(),
            input: r#"{"url":"https://a.example"}"#.into(),
        },
        Said::ServerToolUse {
            id: "srvtoolu_03".into(),
            name: "web_fetch".into(),
            input: r#"{"url":"https://b.example"}"#.into(),
        },
    ];
    let turn = frame(
        1,
        Body::Turn {
            model: "m".into(),
            config: SNAPSHOT,
            said,
            stop: Stop::ToolUse,
            usage: Usage::default(),
        },
    );
    let frames = [turn];
    let entries = timeline::entries(&frames);
    let [timeline::Entry::Worked(work)] = &entries[..] else {
        panic!("one burst");
    };
    assert_eq!(
        work.calls().count(),
        1,
        "the vendor's calls are no harness calls"
    );
    assert_eq!(
        work.summary(),
        "Posted in chat, searched the web, fetched 2 pages"
    );
}
