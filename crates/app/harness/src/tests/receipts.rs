use super::network::{pods_get, served};
use super::*;

const PODS: &str = "pods";

fn pods_request(chain: &MockChain) -> RequestRow {
    let [row]: [RequestRow; 1] = assigned(chain, PODS).try_into().unwrap();
    row
}

fn last_message(chain: &MockChain) -> Value {
    let [asked]: [RequestRow; 1] = assigned(chain, SERVICE)
        .try_into()
        .expect("the model is asked");
    let sent = provider_body(chain, &asked);
    sent["messages"].as_array().unwrap().last().unwrap().clone()
}

fn tool_results(message: &Value) -> Vec<(String, String)> {
    let blocks = message["content"].as_array().unwrap();
    blocks
        .iter()
        .filter(|block| block["type"] == "tool_result")
        .map(|block| {
            let call = block["tool_use_id"].as_str().unwrap().to_owned();
            (call, block["content"].to_string())
        })
        .collect()
}

#[test]
fn all_calls_are_acked_before_any_relay_and_every_result_arrives_as_new_input() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    served(chain);
    let thread = world.opened("m1", "list the pods and the files", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use("n1", "network_call", pods_get("/v1/pods")),
        tool_use("l1", "files_list", json!({ "path": "/shared" })),
    ]);
    model_says(chain, calls, "tool_use");
    let immediate = tool_results(&last_message(chain));
    for call in ["n1", "l1"] {
        let result = immediate.iter().find(|(id, _)| id == call).unwrap();
        assert!(result.1.contains(&format!("Accepted as call {call}")));
    }
    chain.at(4, 0);
    relay_back(chain, "n1");
    relay_back(chain, "l1");
    let request = pods_request(chain);
    assert_eq!(assigned(chain, SERVICE).len(), 1);
    let landed = held(chain, thread);
    assert!(
        landed
            .iter()
            .any(|body| matches!(body, Body::Arrived { call, .. } if call == "l1"))
    );

    chain.at(5, 0);
    let results = tool_results(&last_message(chain));
    let of = |call: &str| {
        let found = results.iter().find(|(id, _)| id == call);
        found.expect("the call is answered").1.clone()
    };
    let receipt = of("n1");
    assert!(receipt.contains("Accepted as call n1"), "{receipt}");
    let listed = of("l1");
    assert!(
        listed.contains("Accepted as call l1"),
        "a fast call receives an acknowledgement too: {listed}"
    );
    model_says(
        chain,
        json!([{ "type": "text", "text": "Waiting on the pods." }]),
        "end_turn",
    );
    assert_eq!(assigned(chain, SERVICE).len(), 1);
    assert_eq!(
        thread_row(chain, thread).asking,
        Asking::Asked {
            upto: thread_row(chain, thread).frames
        }
    );
    let text = last_message(chain).to_string();
    assert!(text.contains("Result of tool call l1"), "{text}");

    chain.at(6, 0);
    let out = json!({ "pods": ["duckpond-7"] });
    answer(chain, request.id, Answer::Output(response(200, out)));
    assert!(
        held(chain, thread)
            .iter()
            .any(|body| matches!(body, Body::Arrived { call, .. } if call == "n1"))
    );
    model_says(
        chain,
        json!([{ "type": "text", "text": "I am still working." }]),
        "end_turn",
    );
    let last = last_message(chain);
    assert_eq!(last["role"], "user");
    assert!(tool_results(&last).is_empty(), "{last}");
    let text = last["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Result of tool call n1"), "{text}");
    assert!(text.contains("duckpond-7"), "{text}");
}

fn open_calls(chain: &MockChain, thread: u64) -> u32 {
    let Reply::Wakes(wakes) = chain.query(PROGRAM, &Query::Wakes { thread }).unwrap() else {
        panic!("harness answers wakes")
    };
    wakes
        .into_iter()
        .find_map(|wake| match wake {
            Wake::Calls { open } => Some(open),
            _ => None,
        })
        .unwrap_or(0)
}

#[test]
fn another_reply_can_start_calls_while_an_earlier_result_is_still_pending() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    served(chain);
    let thread = world.opened("m1", "keep working while pods answers", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("n1", "network_call", pods_get("/v1/pods"))]),
        "tool_use",
    );
    assert!(
        last_message(chain)
            .to_string()
            .contains("Accepted as call n1")
    );
    assert_eq!(open_calls(chain, thread), 1);

    chain.at(4, 0);
    relay_back(chain, "n1");
    let request = pods_request(chain);
    model_says(
        chain,
        json!([tool_use("l2", "files_list", json!({"path":"/shared"}))]),
        "tool_use",
    );
    assert!(
        last_message(chain)
            .to_string()
            .contains("Accepted as call l2")
    );
    assert_eq!(open_calls(chain, thread), 2);

    chain.at(5, 0);
    relay_back(chain, "l2");
    assert_eq!(open_calls(chain, thread), 1);
    model_says(
        chain,
        json!([{ "type":"text", "text":"I can use the files while pods is running." }]),
        "end_turn",
    );
    let last = last_message(chain).to_string();
    assert!(last.contains("Result of tool call l2"), "{last}");
    assert!(!last.contains("Result of tool call n1"), "{last}");

    chain.at(6, 0);
    model_says(
        chain,
        json!([{ "type":"text", "text":"My independent work is finished." }]),
        "end_turn",
    );
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert_eq!(open_calls(chain, thread), 1);

    chain.at(7, 0);
    answer(
        chain,
        request.id,
        Answer::Output(response(200, json!({"pods":["duckpond-7"]}))),
    );
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "a result to an idle conversation waits out the next block"
    );
    chain.at(9, 0);
    assert!(
        last_message(chain)
            .to_string()
            .contains("Result of tool call n1")
    );
    assert_eq!(open_calls(chain, thread), 0);
    for call in ["n1", "l2"] {
        let count = frames(chain, thread)
            .into_iter()
            .chain(held(chain, thread))
            .filter(|body| matches!(body, Body::Arrived { call: done, .. } if done == call))
            .count();
        assert_eq!(count, 1, "{call} arrives once");
    }
}

#[test]
fn a_wait_ends_on_its_acknowledgement_without_waiting_for_its_own_result() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "wake at block ten", Some(WINDOW));
    chain.at(3, 0);
    model_says(
        chain,
        json!([tool_use("w1", "wait_for", json!({"event":{"height":10}}))]),
        "tool_use",
    );
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    assert!(
        frames(chain, thread)
            .iter()
            .any(|body| matches!(body, Body::Receipt { call, .. } if call == "w1"))
    );
    assert_eq!(open_calls(chain, thread), 1);

    chain.at(4, 0);
    relay_back(chain, "w1");
    assert!(!result_of(chain, thread, "w1").1);
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(open_calls(chain, thread), 0);

    chain.at(11, 0);
    assert_eq!(assigned(chain, SERVICE).len(), 1);
}

#[test]
fn results_landing_in_one_block_on_an_idle_conversation_ride_one_ask() {
    let world = super::schedule::schemed();
    let chain = &world.chain;
    served(chain);
    let thread = world.opened("m1", "list the pods three times", Some(WINDOW));
    chain.at(3, 0);
    let calls = json!([
        tool_use("p1", "network_call", pods_get("/v1/pods")),
        tool_use("p2", "network_call", pods_get("/v1/pods")),
        tool_use("p3", "network_call", pods_get("/v1/pods")),
    ]);
    model_says(chain, calls, "tool_use");
    chain.at(4, 0);
    for call in ["p1", "p2", "p3"] {
        relay_back(chain, call);
    }
    model_says(
        chain,
        json!([{ "type": "text", "text": "Waiting on the pods." }]),
        "end_turn",
    );
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
    let requests = assigned(chain, PODS);
    assert_eq!(requests.len(), 3);

    chain.at(5, 0);
    for (n, request) in requests.iter().enumerate() {
        let out = json!({ "pods": [format!("duckpond-{n}")] });
        answer(chain, request.id, Answer::Output(response(200, out)));
    }
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the block's results wait for the block to end"
    );
    let Reply::Settling(window) = chain.query(PROGRAM, &Query::Settling { thread }).unwrap() else {
        panic!("harness answers settling")
    };
    assert!(
        window.is_some(),
        "the first result opens a one-block window"
    );
    chain.at(7, 0);
    let [asked]: [RequestRow; 1] = assigned(chain, SERVICE)
        .try_into()
        .expect("the window's end asks once");
    let sent = provider_body(chain, &asked).to_string();
    for n in 0..3 {
        let pods = format!("duckpond-{n}");
        assert!(sent.contains(&pods), "one ask hears {pods}");
    }
    model_says(
        chain,
        json!([{ "type": "text", "text": "All three are in." }]),
        "end_turn",
    );
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "nothing is left for a second ask"
    );
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);
}
