use super::*;

fn found_snippets(text: &str) -> Vec<String> {
    let answer: Value = serde_json::from_str(text).unwrap();
    let hits = answer["hits"].as_array().unwrap();
    hits.iter()
        .inspect(|hit| assert_eq!(hit["channel"], "general", "{hit}"))
        .map(|hit| {
            let snippets = hit["snippets"].as_array().unwrap();
            let snippets: Vec<&str> = snippets.iter().map(|s| s.as_str().unwrap()).collect();
            snippets.join(" | ")
        })
        .collect()
}

#[test]
fn chat_search_answers_the_sentences_holding_each_word_newest_first() {
    let world = world();
    let chain = &world.chain;
    world.post(ADA, "general", "s1", "the ferry leaves at noon");
    world.post(BO, "general", "s2", "unrelated words");
    world.post(
        ADA,
        "general",
        "s3",
        "Long post first. Then a second Ferry at NOON. And more after.",
    );
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
        tool_use(
            "toolu_around",
            "chat_search",
            json!({ "text": "ferry", "channel": "general", "around": 3 })
        ),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_room", "toolu_all", "toolu_none", "toolu_around"] {
        relay_back(chain, call);
    }
    let answer = |call| {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{text}");
        found_snippets(&text)
    };
    let both = ["Then a second Ferry at NOON.", "the ferry leaves at noon"];
    assert_eq!(answer("toolu_room"), both);
    assert_eq!(answer("toolu_all"), both);
    assert!(answer("toolu_none").is_empty());
    assert_eq!(
        answer("toolu_around"),
        ["he ferry", "nd Ferry at", "he ferry le"]
    );
}
