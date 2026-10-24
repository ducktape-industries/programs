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

#[test]
fn chat_search_pages_matching_messages_and_keeps_every_hit_reachable() {
    let world = world();
    let chain = &world.chain;
    for n in 1..=22 {
        world.post(ADA, "general", &format!("search{n}"), &format!("ferry {n}"));
    }
    world.post(BO, "general", "unrelated", "nothing to find");
    let thread = world.opened("m1", "find a message", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([
            tool_use(
                "toolu_default",
                "chat_search",
                json!({ "text": "ferry", "around": 180 })
            ),
            tool_use(
                "toolu_two",
                "chat_search",
                json!({ "text": "ferry", "limit": 2 })
            ),
            tool_use(
                "toolu_large",
                "chat_search",
                json!({ "text": "ferry", "limit": 30 })
            ),
            tool_use(
                "toolu_zero",
                "chat_search",
                json!({ "text": "ferry", "limit": 0 })
            ),
        ]),
        "tool_use",
    );
    chain.at(4, 0);
    for call in ["toolu_default", "toolu_two", "toolu_large", "toolu_zero"] {
        relay_back(chain, call);
    }
    let answer = |call| {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{text}");
        serde_json::from_str::<Value>(&text).unwrap()
    };
    let seqs = |answer: &Value| {
        answer["hits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hit| hit["seq"].as_u64().unwrap())
            .collect::<Vec<_>>()
    };
    let first = answer("toolu_default");
    assert_eq!(seqs(&first), (3..=22).rev().collect::<Vec<_>>());
    assert!(first["next"].is_string());
    let two = answer("toolu_two");
    assert_eq!(seqs(&two), vec![22, 21]);
    assert!(two["next"].is_string());
    let all = answer("toolu_large");
    assert_eq!(seqs(&all), (1..=22).rev().collect::<Vec<_>>());
    assert!(all["next"].is_null());
    let (text, failed) = result_of(chain, thread, "toolu_zero");
    assert!(failed, "{text}");
    assert!(text.contains("from 1"), "{text}");

    chain.at(5, 0);
    model_says(
        chain,
        json!([tool_use(
            "toolu_rest",
            "chat_search",
            json!({
                "text": "ferry", "around": 180, "cursor": first["next"],
            })
        )]),
        "tool_use",
    );
    chain.at(6, 0);
    relay_back(chain, "toolu_rest");
    let rest = answer("toolu_rest");
    assert_eq!(seqs(&rest), vec![2, 1]);
    assert!(rest["next"].is_null());
}
