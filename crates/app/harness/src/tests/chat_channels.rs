use super::*;

fn ids(text: &str) -> Vec<String> {
    let answer: Value = serde_json::from_str(text).unwrap();
    assert_eq!(answer["next"], Value::Null, "{text}");
    let channels = answer["channels"].as_array().unwrap();
    let mut ids: Vec<String> = channels
        .iter()
        .map(|channel| channel["id"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids
}

#[test]
fn chat_channels_with_a_name_lists_only_the_channels_whose_name_or_id_holds_it_in_any_case() {
    let world = world();
    let chain = &world.chain;
    for (id, name) in [("j-071", "j-071 · efficiency"), ("eng-room", "Engineering")] {
        let create = chat::Op::CreateChannel {
            channel_id: id.into(),
            name: name.into(),
            post_policy: chat::PostPolicy::Open,
        };
        chain.submit(signed(ADA), chat::PROGRAM, &create).unwrap();
    }
    let thread = world.opened("m1", "which room is j-071?", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use("toolu_id", "chat_channels", json!({ "name": "J-071" })),
        tool_use("toolu_name", "chat_channels", json!({ "name": "engineer" })),
        tool_use("toolu_none", "chat_channels", json!({ "name": "nowhere" })),
        tool_use("toolu_all", "chat_channels", json!({})),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["toolu_id", "toolu_name", "toolu_none", "toolu_all"] {
        relay_back(chain, call);
    }
    let answer = |call| {
        let (text, failed) = result_of(chain, thread, call);
        assert!(!failed, "{text}");
        ids(&text)
    };
    assert_eq!(answer("toolu_id"), ["j-071"]);
    assert_eq!(answer("toolu_name"), ["eng-room"]);
    assert!(answer("toolu_none").is_empty());
    assert_eq!(answer("toolu_all"), ["eng-room", "general", "j-071"]);
}
