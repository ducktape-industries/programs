//! Wait-list tools page both the conversation's open waits and the agent's history.
use super::schedule::{answered, refused, schemed, turn};
use super::*;

#[test]
fn wait_list_pages_this_conversations_open_waits() {
    pages_waits(false);
}

#[test]
fn wait_list_pages_the_agents_wait_history() {
    pages_waits(true);
}

fn ids(page: &Value) -> Vec<u64> {
    page["waits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["wait"].as_u64().unwrap())
        .collect()
}

fn pages_waits(all: bool) {
    let world = schemed();
    let chain = &world.chain;
    let thread = world.opened("m1", "make waits", Some(WINDOW));
    let names: Vec<String> = (0..23).map(|i| format!("w{i}")).collect();
    let calls: Vec<(&str, &str, Value)> = names
        .iter()
        .map(|name| {
            (
                name.as_str(),
                "wait_for",
                json!({ "event": { "height": 1000 } }),
            )
        })
        .collect();
    turn(&world, 3, &calls);
    let expected: Vec<u64> = names
        .iter()
        .map(|name| answered(chain, thread, name)["wait"].as_u64().unwrap())
        .collect();
    chain.at(6, 0);
    world.mention("m2", "list waits");
    turn(
        &world,
        7,
        &[
            ("first", "wait_list", json!({ "all": all })),
            ("small", "wait_list", json!({ "all": all, "limit": 7 })),
            (
                "whole",
                "wait_list",
                json!({ "all": all, "limit": expected.len() }),
            ),
            ("zero", "wait_list", json!({ "all": all, "limit": 0 })),
            (
                "bad",
                "wait_list",
                json!({ "all": all, "cursor": "not hex" }),
            ),
        ],
    );
    let first = answered(chain, thread, "first");
    assert_eq!(ids(&first), expected[..20]);
    assert!(first["next"].is_string());
    let small = answered(chain, thread, "small");
    assert_eq!(ids(&small), expected[..7]);
    assert!(small["next"].is_string());
    let whole = answered(chain, thread, "whole");
    assert_eq!(ids(&whole), expected);
    assert!(whole["next"].is_null());
    refused(chain, thread, "zero");
    refused(chain, thread, "bad");
    turn(
        &world,
        10,
        &[
            (
                "next",
                "wait_list",
                json!({ "all": all, "cursor": first["next"] }),
            ),
            (
                "small_next",
                "wait_list",
                json!({ "all": all, "limit": 7, "cursor": small["next"] }),
            ),
        ],
    );
    let next = answered(chain, thread, "next");
    assert_eq!(ids(&next), expected[20..]);
    assert!(next["next"].is_null());
    let small_next = answered(chain, thread, "small_next");
    assert_eq!(ids(&small_next), expected[7..14]);
    assert!(small_next["next"].is_string());
}
