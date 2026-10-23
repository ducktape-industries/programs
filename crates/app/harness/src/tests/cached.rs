//! What one conversation's calls on the Codex surface share for its provider's prompt cache: the
//! conversation's cache key, and every byte of the call before as the head of the next.
use model_wire::{Block, codex};
use serde_json::{Value, json};

use super::accounts::{
    answer_headers, configured_as, harness_wired, model_read_on, replied, turn_open,
};
use super::*;

const GPT: &str = "gpt-test";

fn codex_wired(world: &World) {
    harness_wired(world);
    let openai = Op::SetRoute {
        provider: codex::PROVIDER.into(),
        services: vec![RouteService {
            service: SERVICE.into(),
            wire: Wire::Harness,
        }],
    };
    world.chain.submit(signed(ADA), PROGRAM, &openai).unwrap();
    configured_as(&world.chain, CONFIG, GPT, SERVICE, codex::PROVIDER);
}

fn sent_to_codex(chain: &MockChain, row: &RequestRow) -> Value {
    let asked: model_wire::Request =
        serde_json::from_value(body(chain, row)).expect("a model call is the harness wire");
    codex::body(&asked)
}

fn asked_and_answered_by_a(chain: &MockChain, thread: u64, turn: u64) -> Value {
    let (row, _) = turn_open(chain, thread);
    let sent = sent_to_codex(chain, &row);
    let item =
        json!({ "type": "reasoning", "id": format!("rs_{turn}"), "encrypted_content": "sealed" });
    let said = vec![
        Block::Reasoning {
            text: "weighing it".into(),
            sealed: Some(item.to_string()),
        },
        Block::Text {
            text: format!("answer {turn}"),
            citations: Vec::new(),
        },
    ];
    let usage = model_wire::Usage {
        input: 10,
        output: 5,
        cache_read: 0,
        cache_write: 0,
    };
    replied(
        chain,
        &row,
        said,
        usage,
        answer_headers("a", "0.1", "allowed"),
    );
    sent
}

fn head(sent: &Value, items: usize) -> String {
    let input = sent["input"]
        .as_array()
        .expect("a Codex body has its input");
    assert!(
        input.len() >= items,
        "a later call holds every item of the one before"
    );
    let fixed = [
        "model",
        "instructions",
        "tools",
        "reasoning",
        "prompt_cache_key",
    ];
    let fixed: Vec<(&str, &Value)> = fixed.iter().map(|key| (*key, &sent[*key])).collect();
    json!({ "fixed": fixed, "input": &input[..items] }).to_string()
}

fn opens_with(before: &Value, after: &Value) {
    let items = before["input"]
        .as_array()
        .expect("a Codex body has its input")
        .len();
    assert_eq!(
        head(after, items),
        head(before, items),
        "the call before is the head of the next"
    );
}

#[test]
fn a_codex_conversations_calls_share_its_cache_key_and_each_opens_with_every_byte_of_the_one_before()
 {
    let world = world();
    let chain = &world.chain;
    codex_wired(&world);
    let thread = world.mention("m1", "hello");
    chain.at(2, 0);
    model_read_on(chain, SERVICE, GPT, codex::PROVIDER);
    chain.at(3, 0);
    let first = asked_and_answered_by_a(chain, thread, 1);
    world.mention("m2", "and now?");
    chain.at(4, 0);
    let second = asked_and_answered_by_a(chain, thread, 2);
    world.mention("m3", "once more");
    chain.at(5, 0);
    let third = asked_and_answered_by_a(chain, thread, 3);
    let key = &first["prompt_cache_key"];
    assert_eq!(
        *key,
        json!(thread.to_string()),
        "a Codex call names its conversation as its cache key"
    );
    assert_eq!(second["prompt_cache_key"], *key);
    assert_eq!(third["prompt_cache_key"], *key);
    opens_with(&first, &second);
    opens_with(&second, &third);
}
