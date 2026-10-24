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
                json!({ "text": "ferry", "limit": u64::MAX })
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
    let capped = answer("toolu_large");
    assert_eq!(seqs(&capped), (3..=22).rev().collect::<Vec<_>>());
    assert!(capped["next"].is_string());
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

#[test]
fn chat_search_bounds_actual_result_bytes_without_skipping_unreturned_hits() {
    let world = world();
    let chain = &world.chain;
    world.post(ADA, "general", "small", "ferry oldest");
    let repeated = "ferry \"\\\t😀 ".repeat(3000);
    for n in 1..=24 {
        world.post(
            ADA,
            "general",
            &format!("large{n}"),
            &format!("ferry {n} {repeated}"),
        );
    }
    let thread = world.opened("m1", "find a message", Some(WINDOW));
    let mut cursor = Value::Null;
    let mut seen = Vec::new();
    for page in 0..25 {
        let call = format!("toolu_bounded_{page}");
        chain.at(3 + page * 2, 0);
        model_says(
            chain,
            json!([tool_use(
                &call,
                "chat_search",
                json!({
                    "text": "ferry", "channel": "general", "limit": u64::MAX,
                    "around": if page == 1 { 0 } else { usize::MAX }, "cursor": cursor,
                })
            )]),
            "tool_use",
        );
        chain.at(4 + page * 2, 0);
        relay_back(chain, &call);
        let (text, failed) = result_of(chain, thread, &call);
        assert!(!failed, "{text}");
        assert!(text.len() <= 32_768, "returned {} bytes", text.len());
        let answer: Value = serde_json::from_str(&text).unwrap();
        let hits = answer["hits"].as_array().unwrap();
        assert!(!hits.is_empty(), "a continuation page must make progress");
        if page == 0 {
            assert!(hits.len() < 20, "the byte bound must shorten this page");
        }
        for hit in hits {
            assert_eq!(hit["channel"], "general");
            let seq = hit["seq"].as_u64().unwrap();
            seen.push(seq);
            assert!(hit["snippets"].to_string().len() <= 4096);
            if seq > 1 {
                assert_eq!(hit["snippets_truncated"], true);
                assert!(hit["snippets"].to_string().contains("ferry"));
            }
        }
        cursor = answer["next"].clone();
        if cursor.is_null() {
            break;
        }
    }
    assert!(cursor.is_null(), "the complete search must finish");
    assert_eq!(seen, (1..=25).rev().collect::<Vec<_>>());
}

#[test]
fn chat_search_excerpts_a_huge_sentence_around_its_late_match() {
    let world = world();
    let chain = &world.chain;
    let text = format!("{}ferry 마지막", "unrelated 🦆 ".repeat(12_000));
    world.post(ADA, "general", "late", &text);
    let thread = world.opened("m1", "find a message", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use(
            "toolu_late",
            "chat_search",
            json!({ "text": "ferry", "channel": "general" })
        )]),
        "tool_use",
    );
    chain.at(4, 0);
    relay_back(chain, "toolu_late");
    let (answer, failed) = result_of(chain, thread, "toolu_late");
    assert!(!failed, "{answer}");
    assert!(answer.len() <= 32_768, "returned {} bytes", answer.len());
    let answer: Value = serde_json::from_str(&answer).unwrap();
    let hit = &answer["hits"][0];
    assert_eq!(hit["channel"], "general");
    assert_eq!(hit["seq"], 1);
    assert_eq!(hit["snippets_truncated"], true);
    assert!(hit["snippets"].to_string().len() <= 4096);
    assert!(hit["snippets"].to_string().contains("ferry 마지막"));
    assert!(answer["next"].is_null());
}

#[test]
fn chat_search_refuses_a_cursor_that_cannot_fit_without_advancing() {
    let world = world();
    let chain = &world.chain;
    let term = "x".repeat(20_000);
    world.post(ADA, "general", "long1", &term);
    world.post(ADA, "general", "long2", &term);
    let thread = world.opened("m1", "find a long term", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use(
            "toolu_cursor",
            "chat_search",
            json!({ "text": term, "channel": "general" })
        )]),
        "tool_use",
    );
    chain.at(4, 0);
    relay_back(chain, "toolu_cursor");
    let (text, failed) = result_of(chain, thread, "toolu_cursor");
    assert!(
        failed,
        "oversized cursor metadata must not escape the bound"
    );
    assert!(text.len() <= 32_768);
    assert!(text.contains("metadata"), "{text}");
}
