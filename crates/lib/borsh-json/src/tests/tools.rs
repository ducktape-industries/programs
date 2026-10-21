use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use program::schema::{Return, interface};
use program::tools::{Described as _, Tools};
use program::{Error, Program, QueryCtx};
use serde_json::json;

use super::*;
use crate::tools::{Described, check, read, section};

mod examples {
    use super::PollOp;

    pub fn open() -> PollOp {
        PollOp::Open {
            id: "lunch".into(),
            choices: vec!["noodles".into(), "rice".into()],
        }
    }

    pub fn shut() -> PollOp {
        PollOp::Close(7)
    }
}

#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema, program::Described)]
#[tool_about("Polls anyone opens and closes.")]
enum PollOp {
    #[tool_op(
        description = "Open a poll with its choices.",
        returns = u64,
        example = examples::open
    )]
    Open { id: String, choices: Vec<String> },
    #[tool_op(description = "Close a poll by number.", example = PollOp::Close(3))]
    Close(u64),
    #[tool_op(description = "Vote.", example = examples::shut)]
    Vote,
}

#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema, program::Described)]
enum PollQuery {
    #[tool_query(description = "One poll by id.")]
    Poll {
        id: String,
    },
    Polls,
}

#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
enum PollReply {
    Poll(Option<u64>),
}

struct Poll;

impl Program for Poll {
    const NAME: &'static str = "poll";
    type Op = PollOp;
    type Query = PollQuery;
    type Reply = PollReply;
    const RETURNS: &'static [Return] = PollOp::RETURNS;

    fn query(_: &QueryCtx, _: PollQuery) -> Result<PollReply, Error> {
        Ok(PollReply::Poll(None))
    }

    fn tools() -> Tools {
        Tools::of::<PollOp, PollQuery>()
    }
}

fn poll() -> Interface {
    Interface::read(&borsh::to_vec(&interface::<Poll>()).unwrap()).unwrap()
}

fn problems(section: serde_json::Value) -> Vec<String> {
    check(&poll(), &read(section.to_string().as_bytes()).unwrap())
}

#[test]
fn the_derive_lists_every_variant_in_order_with_what_its_attribute_tells() {
    let tools = Poll::tools();
    assert_eq!(tools.about, "Polls anyone opens and closes.");
    let ops: Vec<(&str, &str)> = tools
        .ops
        .iter()
        .map(|tool| (tool.variant, tool.description))
        .collect();
    assert_eq!(
        ops,
        [
            ("Open", "Open a poll with its choices."),
            ("Close", "Close a poll by number."),
            ("Vote", "Vote."),
        ]
    );
    assert_eq!(
        tools.ops[1].example,
        Some(borsh::to_vec(&PollOp::Close(3)).unwrap())
    );
    assert_eq!(tools.queries[1].description, "");
    let returned = poll().returns.unwrap();
    assert_eq!(returned, [("Open".to_owned(), "u64".to_owned())]);
}

#[test]
fn the_section_writes_each_example_as_json_and_each_return_as_its_shape() {
    let written = section(&poll(), &Poll::tools()).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&written).unwrap();
    assert_eq!(
        value["ops"]["Open"],
        json!({
            "description": "Open a poll with its choices.",
            "example": { "open": { "id": "lunch", "choices": ["noodles", "rice"] } },
            "returns": { "returns": "u64", "types": {} },
        })
    );
    assert_eq!(value["ops"]["Close"]["example"], json!({ "close": 3 }));
    assert_eq!(value["queries"]["Polls"], json!({ "example": "polls" }));
    assert_eq!(
        check(&poll(), &read(&written).unwrap()),
        [
            "op Vote's example is a Close",
            "query Polls has no description",
        ]
    );
    assert_eq!(section(&poll(), &Tools::default()).unwrap(), b"{}");
}

#[test]
fn a_query_returns_the_reply_variant_its_ask_names() {
    #[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema, program::Described)]
    enum Asked {
        #[tool_query(description = "One poll.")]
        #[ask(PollReply::Poll(Option<u64>))]
        Poll { id: String },
    }
    let told = Asked::tools();
    assert_eq!(told[0].reply, "Poll");
    let reply = crate::variant_shape(&poll().schema, &poll().reply, "Poll").unwrap();
    assert_eq!(
        reply,
        json!({ "returns": { "poll": { "option": "u64" } }, "types": {} })
    );
}

#[test]
fn every_variant_left_out_or_unknown_or_without_a_description_is_a_problem() {
    let section = json!({
        "about": "Polls.",
        "ops": {
            "Open": { "description": " ", "example": { "Open": { "id": "", "choices": [] } } },
            "Close": { "example": { "Close": 1 } },
            "Shut": { "description": "Shut." },
        },
        "queries": {},
    });
    assert_eq!(
        problems(section),
        [
            "op Vote is not described",
            "op Close has no description",
            "op Open has no description",
            "op Shut is described, and the op has no variant Shut",
            "query Poll is not described",
            "query Polls is not described",
        ]
    );
    let silent = check(&poll(), &Described::default());
    assert_eq!(silent.len(), 5, "{silent:?}");
    let about = json!({
        "ops": {
            "Open": { "description": "Open.", "example": { "Open": { "id": "", "choices": [] } } },
            "Close": { "description": "Close.", "example": { "Close": "7" } },
            "Vote": { "description": "Vote.", "example": "Vote" },
        },
        "queries": {
            "Poll": { "description": "One.", "example": { "Poll": { "id": { "utf8": "x" } } } },
            "Polls": { "description": "All.", "example": "Polls" },
        },
    });
    let left = problems(about);
    assert_eq!(left.len(), 2, "{left:?}");
    assert_eq!(left[0], "about says nothing of what the program is for");
    assert!(
        left[1].starts_with("query Poll's example does not encode"),
        "{left:?}"
    );
}

#[test]
fn a_section_is_read_by_the_keys_it_knows_and_refused_when_it_is_no_object() {
    let grown = json!({
        "about": "Polls.",
        "since": 3,
        "ops": { "Open": { "description": "Open.", "cost": 1 }, "Vote": {} },
    });
    let described = read(grown.to_string().as_bytes()).unwrap();
    assert_eq!(described.ops["Open"].description, "Open.");
    assert_eq!(described.ops["Vote"].description, "");
    assert!(described.queries.is_empty());
    assert_eq!(read(b"{}").unwrap(), Described::default());
    assert!(read(b"[]").is_err());
    assert!(read(br#"{"ops":{"Open":{"description":1}}}"#).is_err());
    assert!(read(br#"{"ops":{"Open":[]}}"#).is_err());
}

#[test]
fn one_variant_is_named_and_made_alone() {
    let poll = poll();
    let ops = crate::variants(&poll.schema, &poll.op);
    assert_eq!(ops, ["Open", "Close", "Vote"]);
    assert!(crate::variants(&poll.schema, "u64").is_empty());
    let made = |name: &str| crate::variant_example(&poll.schema, &poll.op, name).unwrap();
    assert_eq!(made("Close"), Some(json!({ "close": 0 })));
    assert_eq!(made("Vote"), Some(json!("vote")));
    assert_eq!(made("Shut"), None);
}

#[test]
fn a_network_schema_ask_is_one_of_five_and_a_mixed_one_says_what_to_give() {
    use crate::discover::Ask;
    let some = |text: &str| Some(text.to_owned());
    assert_eq!(Ask::of(None, None, None, None), Ok(Ask::Roster));
    assert_eq!(Ask::of(None, None, None, Some(false)), Ok(Ask::Roster));
    assert_eq!(
        Ask::of(some("poll"), None, None, None),
        Ok(Ask::Index("poll".into()))
    );
    assert_eq!(
        Ask::of(some("poll"), None, None, Some(true)),
        Ok(Ask::Whole("poll".into()))
    );
    assert_eq!(
        Ask::of(some("poll"), some("Open"), None, None),
        Ok(Ask::Op("poll".into(), "Open".into()))
    );
    assert_eq!(
        Ask::of(some("poll"), None, some("Poll"), None),
        Ok(Ask::Query("poll".into(), "Poll".into()))
    );
    assert!(Ask::of(None, some("Open"), None, None).is_err());
    assert!(Ask::of(some("poll"), some("Open"), None, Some(true)).is_err());
    assert!(Ask::of(some("poll"), some("Open"), some("Poll"), None).is_err());
}

#[test]
fn the_index_and_an_unfolded_op_read_the_section_and_the_schema_together() {
    use crate::discover::{Refusal, Side, index, roster, unfold};
    let poll = poll();
    let told = read(&section(&poll, &Poll::tools()).unwrap()).unwrap();
    let listed = index("poll", &poll, &told);
    assert_eq!(listed["about"], json!("Polls anyone opens and closes."));
    assert_eq!(
        listed["ops"]["open"],
        json!("Open a poll with its choices.")
    );
    assert_eq!(listed["queries"]["polls"], serde_json::Value::Null);
    assert!(listed.get("note").is_none(), "{listed}");
    let silent = index("poll", &poll, &Described::default());
    assert!(silent["note"].as_str().unwrap().contains("describes none"));

    let open = unfold("poll", &poll, &told, Side::Op, "Open").unwrap();
    assert_eq!(
        open["example"],
        json!({ "open": { "id": "lunch", "choices": ["noodles", "rice"] } })
    );
    assert_eq!(open["returns"], json!("u64"));
    assert_eq!(
        open["shape"],
        json!({ "open": { "struct": { "id": "string", "choices": { "vec": "string" } } } })
    );
    let made = unfold("poll", &poll, &Described::default(), Side::Op, "Close").unwrap();
    assert_eq!(made["example"], json!({ "close": 0 }));
    assert!(made.get("description").is_none(), "{made}");
    let Err(Refusal::Asked(why)) = unfold("poll", &poll, &told, Side::Query, "Open") else {
        panic!("a query poll lacks is refused")
    };
    assert_eq!(
        why,
        "program poll has no query Open; its queries are poll, polls"
    );

    let rows = roster(vec![
        ("poll".into(), Ok(told)),
        ("odd".into(), Err("no JSON".into())),
    ]);
    assert_eq!(
        rows,
        json!({ "programs": [
            { "program": "poll", "about": "Polls anyone opens and closes." },
            { "program": "odd", "unread": "no JSON" },
        ] })
    );
}

#[test]
fn an_http_answer_reads_as_its_status_headers_and_body() {
    use crate::discover::http_answer;
    let answer = |body: &[u8]| {
        let response = abi::role::egress::http::Response {
            status: 200,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: body.to_vec(),
        };
        http_answer(&abi::encode(&response))
    };
    assert_eq!(answer(br#"{"a":1}"#)["body"], json!({ "a": 1 }));
    assert_eq!(answer(b"plain")["body_as"], json!("text"));
    assert_eq!(answer(&[0xff])["body"], json!("ff"));
    let broken = http_answer(b"no");
    assert!(
        broken["unread"]
            .as_str()
            .unwrap()
            .starts_with("the answer is no HTTP response")
    );
}

#[test]
fn an_unfolded_variant_is_asked_and_answered_in_snake_case_over_a_section_packed_before_it() {
    use crate::discover::{Refusal, Side, index, unfold};
    let poll = poll();
    let packed = json!({
        "about": "Polls.",
        "ops": { "Open": {
            "description": "Open.",
            "example": { "Open": { "id": "x", "choices": [] } },
        } },
        "queries": { "Poll": {
            "description": "One.",
            "returns": { "returns": { "Poll": { "option": "u64" } }, "types": {} },
        } },
    });
    let told = read(packed.to_string().as_bytes()).unwrap();
    assert_eq!(index("poll", &poll, &told)["ops"]["open"], json!("Open."));
    for asked in ["open", "Open"] {
        let open = unfold("poll", &poll, &told, Side::Op, asked).unwrap();
        assert_eq!(open["op"], json!("open"));
        assert_eq!(
            open["example"],
            json!({ "open": { "id": "x", "choices": [] } })
        );
        assert_eq!(
            open["shape"],
            json!({ "open": { "struct": { "id": "string", "choices": { "vec": "string" } } } })
        );
    }
    let asked = unfold("poll", &poll, &told, Side::Query, "poll").unwrap();
    assert_eq!(asked["returns"], json!({ "poll": { "option": "u64" } }));
    let Err(Refusal::Asked(why)) = unfold("poll", &poll, &told, Side::Query, "open") else {
        panic!("a query poll lacks is refused")
    };
    assert_eq!(
        why,
        "program poll has no query open; its queries are poll, polls"
    );
}
