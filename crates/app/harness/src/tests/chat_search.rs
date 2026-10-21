use super::*;

fn found_texts(text: &str) -> Vec<String> {
    let answer: Value = serde_json::from_str(text).unwrap();
    let messages = answer["messages"].as_array().unwrap();
    messages
        .iter()
        .map(|message| message["text"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn chat_search_answers_every_message_holding_each_word_newest_first() {
    let world = world();
    let chain = &world.chain;
    world.post(ADA, "general", "s1", "the ferry leaves at noon");
    world.post(BO, "general", "s2", "unrelated words");
    world.post(ADA, "general", "s3", "a second Ferry at NOON");
    let thread = world.opened("m1", "find the ferry", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use(
            "toolu_room",
            "chat_search",
            json!({ "text": "ferry noon", "channel": "general" })
        ),
        tool_use("toolu_all", "chat_search", json!({ "text": "ferry noon" })),
        tool_use("toolu_none", "chat_search", json!({ "text": "submarine" })),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_room", "toolu_all", "toolu_none"] {
        relay_back(chain, call);
    }
    let answer = |call| {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{text}");
        found_texts(&text)
    };
    let both = ["a second Ferry at NOON", "the ferry leaves at noon"];
    assert_eq!(answer("toolu_room"), both);
    assert_eq!(answer("toolu_all"), both);
    assert!(answer("toolu_none").is_empty());
}
