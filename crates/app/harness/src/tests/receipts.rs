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
fn a_call_still_open_a_block_after_its_turn_is_acked_and_its_result_arrives_as_new_input() {
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
    chain.at(4, 0);
    relay_back(chain, "n1");
    relay_back(chain, "l1");
    let request = pods_request(chain);
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "a call has its turn's block to end in"
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
        !listed.contains("Accepted"),
        "a call that ended within the block keeps its result: {listed}"
    );
    model_says(
        chain,
        json!([{ "type": "text", "text": "Waiting on the pods." }]),
        "end_turn",
    );
    assert!(assigned(chain, SERVICE).is_empty());
    assert_eq!(thread_row(chain, thread).asking, Asking::Idle);

    chain.at(6, 0);
    let out = json!({ "pods": ["duckpond-7"] });
    answer(chain, request.id, Answer::Output(response(200, out)));
    let last = last_message(chain);
    assert_eq!(last["role"], "user");
    assert!(tool_results(&last).is_empty(), "{last}");
    let text = last["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Result of tool call n1"), "{text}");
    assert!(text.contains("duckpond-7"), "{text}");
}
