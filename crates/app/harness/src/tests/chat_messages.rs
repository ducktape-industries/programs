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

#[test]
fn chat_messages_without_a_limit_reads_the_newest_twenty_and_its_cursor_reads_the_older_ones() {
    let world = world();
    let chain = &world.chain;
    let create = chat::Op::CreateChannel {
        channel_id: "long".into(),
        name: "long".into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain.submit(signed(ADA), chat::PROGRAM, &create).unwrap();
    for n in 1..=22 {
        world.post(ADA, "long", &format!("p{n}"), &format!("post {n}"));
    }
    let thread = world.opened("m1", "read the room", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use(
            "toolu_newest",
            "chat_messages",
            json!({ "channel": "long" })
        ),
        tool_use(
            "toolu_none",
            "chat_messages",
            json!({ "channel": "long", "limit": 0 })
        ),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_newest", "toolu_none"] {
        relay_back(chain, call);
    }
    let (text, failed) = result_of(chain, thread, "toolu_newest");
    assert!(!failed, "{text}");
    let answer: Value = serde_json::from_str(&text).unwrap();
    let (texts, more) = read(&text);
    let newest: Vec<String> = (3..=22).rev().map(|n| format!("post {n}")).collect();
    assert_eq!((texts, more), (newest, true));
    let (refused, failed) = result_of(chain, thread, "toolu_none");
    assert!(failed && refused.contains("from 1"), "{refused}");

    let cursor = answer["next"].as_str().unwrap().to_owned();
    let calls = json!([tool_use(
        "toolu_older",
        "chat_messages",
        json!({ "channel": "long", "cursor": cursor })
    )]);
    chain.at(5, 0);
    model_says(chain, calls, "tool_use");
    chain.at(6, 0);
    relay_back(chain, "toolu_older");
    let (text, failed) = result_of(chain, thread, "toolu_older");
    assert!(!failed, "{text}");
    let older = vec!["post 2".to_owned(), "post 1".to_owned()];
    assert_eq!(read(&text), (older, false));
}
