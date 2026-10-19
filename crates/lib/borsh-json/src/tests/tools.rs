use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use program::schema::interface;
use program::tools::{Tools, op, query};
use program::{Error, Program, QueryCtx};
use serde_json::json;

use super::*;
use crate::tools::{Described, check, read};

#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
enum PollOp {
    Open { id: String, choices: Vec<String> },
    Close(u64),
}

#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
enum PollQuery {
    Poll { id: String },
}

struct Poll;

impl Program for Poll {
    const NAME: &'static str = "poll";
    type Op = PollOp;
    type Query = PollQuery;
    type Reply = ();
    const TOOLS: Tools = Tools::new(
        "Polls anyone opens and closes.",
        &[
            op("Open", "Open a poll with its choices.")
                .example(r#"{"Open":{"id":"lunch","choices":["noodles","rice"]}}"#),
            op("Close", "Close a poll by number.").example(r#"{"Close":7}"#),
            query("Poll", "One poll by id.").example(r#"{"Poll":{"id":"lunch"}}"#),
        ],
    );

    fn query(_: &QueryCtx, _: PollQuery) -> Result<(), Error> {
        Ok(())
    }
}

fn poll() -> Interface {
    Interface::read(&borsh::to_vec(&interface::<Poll>()).unwrap()).unwrap()
}

fn problems(section: serde_json::Value) -> Vec<String> {
    check(&poll(), &read(section.to_string().as_bytes()).unwrap())
}

#[test]
fn a_program_that_describes_every_op_and_query_has_no_problem() {
    let described = read(&program::tools::json(&Poll::TOOLS)).unwrap();
    assert_eq!(described.about, "Polls anyone opens and closes.");
    assert_eq!(described.ops["Close"].example, Some(json!({ "Close": 7 })));
    assert_eq!(check(&poll(), &described), Vec::<String>::new());
}

#[test]
fn every_variant_left_out_or_unknown_or_silent_is_a_problem() {
    let section = json!({
        "about": "Polls.",
        "ops": { "Open": { "says": " " }, "Vote": { "says": "Vote." } },
        "queries": {},
    });
    assert_eq!(
        problems(section),
        [
            "op Close is not described",
            "op Open says nothing",
            "op Vote is described, and the op has no variant Vote",
            "query Poll is not described",
        ]
    );
    let silent = check(&poll(), &Described::default());
    assert_eq!(silent.len(), 3, "{silent:?}");
    let about = json!({
        "ops": { "Open": { "says": "Open." }, "Close": { "says": "Close." } },
        "queries": { "Poll": { "says": "One." } },
    });
    assert_eq!(
        problems(about),
        ["about says nothing of what the program is for"]
    );
}

#[test]
fn an_example_is_a_value_of_its_own_variant_written_as_it_reads_back() {
    let section = json!({
        "about": "Polls.",
        "ops": {
            "Open": { "says": "Open.", "example": { "Close": 1 } },
            "Close": { "says": "Close.", "example": { "Close": "7" } },
        },
        "queries": { "Poll": { "says": "One.", "example": { "Poll": { "name": "x" } } } },
    });
    assert_eq!(
        problems(section),
        [
            "op Close's example reads back as {\"Close\":7}: write it so",
            "op Open's example is a Close",
            "query Poll's example does not encode: 2 problems: at query.Poll: there is no field name; the fields are id; at query.Poll: field id is missing",
        ]
    );
}

#[test]
fn a_section_is_read_by_the_keys_it_knows_and_refused_when_it_is_no_object() {
    let grown = json!({
        "about": "Polls.",
        "since": 3,
        "ops": { "Open": { "says": "Open.", "cost": 1 } },
    });
    let described = read(grown.to_string().as_bytes()).unwrap();
    assert_eq!(described.ops["Open"].says, "Open.");
    assert!(described.queries.is_empty());
    assert_eq!(read(b"{}").unwrap(), Described::default());
    assert!(read(b"[]").is_err());
    assert!(read(br#"{"ops":{"Open":{}}}"#).is_err());
}
