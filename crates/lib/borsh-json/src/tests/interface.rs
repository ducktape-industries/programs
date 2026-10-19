use ::program::schema::{Return, interface, returns};
use ::program::{Error, Program, QueryCtx};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
use serde_json::json;

use super::*;

#[derive(Debug, BorshSerialize, BorshDeserialize, BorshSchema)]
enum Op {
    Count { by: u64 },
    Reset,
    Name(String),
}

struct Counter;

impl Program for Counter {
    const NAME: &'static str = "counter";
    type Op = Op;
    type Query = u8;
    type Reply = Option<u64>;
    const RETURNS: &'static [Return] = &[returns::<u64>("Count"), returns::<()>("Reset")];

    fn query(_: &QueryCtx, _: u8) -> Result<Option<u64>, Error> {
        Ok(None)
    }
}

fn counter() -> Interface {
    Interface::read(&abi::encode(&interface::<Counter>())).unwrap()
}

#[test]
fn a_section_reads_as_its_roots_and_what_each_op_returns() {
    let read = counter();
    assert_eq!(read.schema, interface::<Counter>());
    let roots = (read.op.as_str(), read.query.as_str(), read.reply.as_str());
    assert_eq!(roots, ("Op", "u8", "Option<u64>"));
    let returns = vec![
        ("Count".to_owned(), "u64".to_owned()),
        ("Reset".to_owned(), "()".to_owned()),
    ];
    assert_eq!(read.returns, Some(returns));
}

#[test]
fn a_section_that_is_no_interface_says_why() {
    assert_eq!(Interface::read(b"\x05"), Err(Unreadable::NotBorsh));
    let plain = BorshSchemaContainer::for_type::<u64>();
    assert_eq!(
        Interface::read(&abi::encode(&plain)),
        Err(Unreadable::NoRoots)
    );
}

#[test]
fn ops_queries_and_replies_go_both_ways_through_the_interface() {
    let read = counter();
    let op = json!({ "Count": { "by": 3 } });
    let bytes = read.encode_op(&op).unwrap();
    assert_eq!(bytes, abi::encode(&Op::Count { by: 3 }));
    assert_eq!(read.decode_op(&bytes).unwrap(), op);
    assert_eq!(read.encode_query(&json!(7)).unwrap(), [7]);
    let reply = abi::encode(&Some(9u64));
    assert_eq!(read.decode_reply(&reply).unwrap(), json!(9));
    let missing = read.encode_op(&json!({ "Count": {} })).unwrap_err();
    assert!(
        matches!(&missing, Fault::MissingField { field, .. } if field == "by"),
        "{missing}"
    );
    let shape = read.shape().unwrap();
    assert_eq!(shape["returns"]["Count"], json!("u64"));
}

#[test]
fn an_output_reads_in_the_type_its_op_returns_else_as_hex_and_why() {
    let read = counter();
    let count = abi::encode(&Op::Count { by: 1 });
    assert_eq!(read.returned("counter", &count, &[]), json!({ "ok": true }));
    let seven = abi::encode(&7u64);
    assert_eq!(
        read.returned("counter", &count, &seven),
        json!({ "ok": true, "returned": 7 })
    );
    let short = read.returned("counter", &count, &[1, 2]);
    assert_eq!(short["returned"], json!("0102"));
    let why = short["unread"].as_str().unwrap();
    assert!(why.starts_with("the output does not read as u64, the type the schema of program counter names for its op Count: "), "{why}");
    let name = abi::encode(&Op::Name("x".into()));
    let unnamed = read.returned("counter", &name, &[1]);
    assert_eq!(
        unnamed["unread"],
        json!(
            "the schema of program counter names no type for what its op Name returns, so this is the output's bytes"
        )
    );
}
