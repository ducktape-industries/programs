//! Tools over any program of the network by the schema its code carries: the shapes of its ops,
//! queries and replies, a query asked and its reply read back, and an op sent as the agent.
use ::program::{Error, ExecCtx, code, invalid};
use abi::role::egress::{self, http};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Reading, Run, Seat, Tool, Write, object};
use crate::codec::{self, Fault};
use crate::schema::{Interface, schema_of};

pub(crate) fn program_schema() -> Value {
    json!({ "type": "string", "description": "a program id, such as chat or files" })
}

fn refused(fault: Fault) -> Error {
    invalid(fault.to_string())
}

pub(crate) fn encoded(ctx: &ExecCtx, program: &str, op: &Value) -> Result<Vec<u8>, Error> {
    let interface = schema_of(ctx, program)?;
    codec::encode(&interface.schema, &interface.op, "op", op).map_err(refused)
}

#[derive(Deserialize)]
pub(crate) struct SchemaArgs {
    program: String,
}

pub(crate) struct Schema;

impl Tool for Schema {
    const NAME: &'static str = "network_schema";
    const DESCRIPTION: &'static str = "The shapes of a program's ops, queries and replies, from the schema its code carries: write a query for network_query and an op for network_call in them. `op`, `query` and `reply` are shapes, `returns` (where the schema has it) gives, for each op that returns something, the shape of what it returns, and each type they name is under `types`. A shape is a type name; \"null\"; \"bool\"; \"string\"; an integer type such as \"u64\", written as a JSON number (u128 and i128 as decimal strings); \"hex\", bytes as hex ({\"hex\": N}: exactly N bytes); {\"option\": shape}, null or the value; {\"vec\": shape}, an array (with \"len\", exactly that many), and a map is a vec of [key, value] tuples; {\"tuple\": [shapes]}, an array; {\"struct\": {field: shape}}, an object, where a field that is an option may be left out; {\"enum\": {Variant: shape}}, one variant, written \"Variant\" when its shape is null and {\"Variant\": value} otherwise. network_query with program `programs` and query \"Roster\" lists the programs that run.";
    type Args = SchemaArgs;

    fn schema() -> Value {
        object(json!({ "program": program_schema() }), &["program"])
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: SchemaArgs) -> Result<Run, Error> {
        let interface = schema_of(ctx, &args.program)?;
        let roots = [
            ("op", interface.op.as_str()),
            ("query", interface.query.as_str()),
            ("reply", interface.reply.as_str()),
        ];
        let returns: Option<Vec<(&str, &str)>> = interface.returns.as_ref().map(|returns| {
            let named = returns.iter().map(|(op, of)| (op.as_str(), of.as_str()));
            named.collect()
        });
        let mut shape = codec::shape(&interface.schema, &roots, returns.as_deref())
            .map_err(|fault| Error::new(code::CORRUPT, fault.to_string()))?;
        shape["program"] = json!(args.program);
        Ok(Run::Done(shape))
    }
}

#[derive(Deserialize)]
pub(crate) struct QueryArgs {
    program: String,
    query: Value,
}

pub(crate) struct Query;

impl Tool for Query {
    const NAME: &'static str = "network_query";
    const DESCRIPTION: &'static str = "Ask a program a query, written in the query shape network_schema gives for it, and read its reply in its reply shape. A query changes nothing. An answer that is not the program's reply comes back as its bytes in hex, with why it did not read.";
    type Args = QueryArgs;

    fn schema() -> Value {
        object(
            json!({
                "program": program_schema(),
                "query": { "description": "the query, in the program's query shape" },
            }),
            &["program", "query"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: QueryArgs) -> Result<Run, Error> {
        let interface = schema_of(ctx, &args.program)?;
        let asked = codec::encode(&interface.schema, &interface.query, "query", &args.query)
            .map_err(refused)?;
        let answer = ctx.query_raw(args.program, asked)?;
        let read = codec::decode(&interface.schema, &interface.reply, "reply", &answer);
        Ok(Run::Done(match read {
            Ok(reply) => json!({ "reply": reply }),
            Err(fault) => json!({ "bytes": abi::hex(&answer), "unread": fault.to_string() }),
        }))
    }
}

#[derive(Deserialize)]
pub(crate) struct CallArgs {
    program: String,
    op: Value,
}

pub(crate) struct Call;

impl Tool for Call {
    const NAME: &'static str = "network_call";
    const DESCRIPTION: &'static str = "Send a program an op, written in the op shape network_schema gives for it, as your own account. The op is yours: the program applies it where it allows your account and refuses it otherwise, with its reason. An HTTP request the program bound to the egress role answers whole comes back as the answer's `status`, `headers` and `body`: JSON where the body parses as JSON, else text where it is UTF-8, else hex, as `body_as` says. Any other output comes back in `returned`: as JSON in the shape network_schema's `returns` gives for the op, else as hex, and then `unread` says why it reads as nothing more.";
    const WRITES: bool = true;
    type Args = CallArgs;

    fn schema() -> Value {
        object(
            json!({
                "program": program_schema(),
                "op": { "description": "the op, in the program's op shape" },
            }),
            &["program", "op"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: CallArgs) -> Result<Run, Error> {
        let interface = schema_of(ctx, &args.program)?;
        let payload =
            codec::encode(&interface.schema, &interface.op, "op", &args.op).map_err(refused)?;
        let read: Reading = match answers_http(ctx, &args.program, &payload) {
            true => Box::new(response),
            false => returned(&args.program, interface, &payload),
        };
        let writes = vec![Write {
            target: args.program,
            payload,
        }];
        Ok(Run::Read { writes, read })
    }
}

fn answers_http(ctx: &ExecCtx, program: &str, payload: &[u8]) -> bool {
    let to_egress = ctx
        .env()
        .roles
        .get(abi::role::EGRESS)
        .is_some_and(|bound| bound == program);
    to_egress
        && matches!(
            abi::decode::<egress::Op>(payload),
            Ok(egress::Op::Request {
                request: egress::Request::Http { .. },
                delivery: egress::Delivery::Whole,
                ..
            })
        )
}

// What the op's output reads as: JSON in the type its program's schema names for the op, else
// its bytes as hex and why.
fn returned(program: &str, interface: Interface, payload: &[u8]) -> Reading {
    let op = codec::variant(&interface.schema, &interface.op, payload);
    let named = match &op {
        Some(op) => format!("its op {op}"),
        None => "this op".to_owned(),
    };
    let declared = match (&interface.returns, &op) {
        (Some(returns), Some(op)) => returns
            .iter()
            .find(|(returning, _)| returning == op)
            .map(|(_, of)| of.clone()),
        _ => None,
    };
    let undeclared = match &interface.returns {
        Some(_) => format!(
            "the schema of program {program} names no type for what {named} returns, so this is the output's bytes"
        ),
        None => format!(
            "the schema of program {program} names no type for what its ops return, so this is the output's bytes"
        ),
    };
    let program = program.to_owned();
    Box::new(move |output| {
        if output.is_empty() {
            return Ok(json!({ "ok": true }));
        }
        let Some(of) = declared else {
            return Ok(unread(output, &undeclared));
        };
        Ok(
            match codec::decode(&interface.schema, &of, "returned", output) {
                Ok(value) => json!({ "ok": true, "returned": value }),
                Err(fault) => {
                    let why = format!(
                        "the output does not read as {of}, the type the schema of program {program} names for {named}: {fault}"
                    );
                    unread(output, &why)
                }
            },
        )
    })
}

fn unread(output: &[u8], why: &str) -> Value {
    json!({ "ok": true, "returned": abi::hex(output), "unread": why })
}

fn response(output: &[u8]) -> Result<Value, Error> {
    let response = match abi::decode::<http::Response>(output) {
        Ok(response) => response,
        Err(fault) => {
            let unread = format!("the answer is no HTTP response: {}", fault.sentence);
            return Ok(json!({ "ok": true, "returned": abi::hex(output), "unread": unread }));
        }
    };
    let (body_as, body) = body(response.body);
    Ok(json!({
        "ok": true,
        "status": response.status,
        "headers": response.headers,
        "body": body,
        "body_as": body_as,
    }))
}

fn body(bytes: Vec<u8>) -> (&'static str, Value) {
    if let Ok(value) = serde_json::from_slice(&bytes) {
        return ("json", value);
    }
    match String::from_utf8(bytes) {
        Ok(text) => ("text", json!(text)),
        Err(fault) => ("hex", json!(abi::hex(fault.as_bytes()))),
    }
}
