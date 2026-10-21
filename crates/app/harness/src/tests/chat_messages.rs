use super::*;

fn read(text: &str) -> (Vec<String>, bool) {
    let answer: Value = serde_json::from_str(text).unwrap();
    let messages = answer["messages"].as_array().unwrap();
    let texts = messages
        .iter()
        .map(|message| message["text"].as_str().unwrap().to_owned())
        .collect();
    (texts, !answer["next"].is_null())
}

#[test]
fn chat_messages_with_a_limit_reads_the_newest_that_many_and_its_cursor_reads_on() {
    let world = world();
    let chain = &world.chain;
    let create = chat::Op::CreateChannel {
        channel_id: "tail".into(),
        name: "tail".into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &create).unwrap();
    for (id, text) in [("t1", "first"), ("t2", "second"), ("t3", "third")] {
        world.post(ADA, "tail", id, text);
    }
    let thread = world.opened("m1", "read the tail", Some(WINDOW));
    chain.at(3, 0);
    let asked = chat::Query::Roots {
        channel_id: "tail".into(),
        viewer: Vec::new(),
        page: PageRequest::first(2),
    };
    let chat::Reply::Roots(page) = chain.query(chat::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    let cursor = abi::hex(&page.next.unwrap());
    let calls = json!([
        tool_use(
            "toolu_tail",
            "chat_messages",
            json!({ "channel": "tail", "limit": 2 })
        ),
        tool_use(
            "toolu_on",
            "chat_messages",
            json!({ "channel": "tail", "limit": 2, "cursor": cursor })
        ),
        tool_use("toolu_all", "chat_messages", json!({ "channel": "tail" })),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_tail", "toolu_on", "toolu_all"] {
        relay_back(chain, call);
    }
    let answer = |call| {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{text}");
        read(&text)
    };
    let texts =
        |words: &[&str]| -> Vec<String> { words.iter().map(|word| word.to_string()).collect() };
    assert_eq!(answer("toolu_tail"), (texts(&["third", "second"]), true));
    assert_eq!(answer("toolu_on"), (texts(&["first"]), false));
    let all = texts(&["third", "second", "first"]);
    assert_eq!(answer("toolu_all"), (all, false));
}
