//! Tools over any program of the network by the schema and tools sections its code carries: the
//! programs that run, a program's index, one op or query unfolded with an example to edit, a query
//! asked and its reply read back, and an op sent as the agent.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, QueryCtx, code, invalid};
use abi::role::egress::{self, http};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{Reading, Run, Seat, Tool, Write, object};
use borsh_json::tools::{Described, Told};
use borsh_json::{shape, unread, variant_example, variant_shape, variants};

use crate::calls::refusal;
use crate::codec::{Fault, spoken};
use crate::schema::{Interface, code_of, running, schema_in, schema_of, told_in};

pub(crate) fn program_schema() -> Value {
    json!({ "type": "string", "description": "a program id, such as chat or files" })
}

fn refused(fault: Fault) -> Error {
    invalid(fault.to_string())
}

pub(crate) fn encoded(ctx: &ExecCtx, program: &str, op: &Value) -> Result<Vec<u8>, Error> {
    let interface = schema_of(ctx, program)?;
    interface.encode_op(&spoken(op)).map_err(refused)
}

#[derive(Deserialize)]
pub(crate) struct SchemaArgs {
    program: Option<String>,
    op: Option<String>,
    query: Option<String>,
    whole: Option<bool>,
}

pub(crate) struct Schema;

impl Tool for Schema {
    const NAME: &'static str = "network_schema";
    const DESCRIPTION: &'static str = "What a program's ops and queries are and how to write them, from the code it runs. With no program: every program that runs, each with what it is for (`about`). With a program alone: its index, `about` and each op and query by name with what it does. With a program and an `op` (or a `query`), a variant's name: what it does (`description`), an `example` to edit and send with network_call (or network_query), its `shape`, the shape of what it `returns` (what network_call reads an op's output as, a query's reply), and each type those name under `types`. With `whole`: every shape of the program at once (`op`, `query`, `reply`, `returns` and `types`). A shape is a type name; \"null\"; \"bool\"; \"string\"; an integer type such as \"u64\", written as a JSON number (u128 and i128 as decimal strings); \"hex\", bytes as hex ({\"hex\": N}: exactly N bytes), which an op or a query may also write as {\"utf8\": text} for bytes that are text, such as forge's ref names and paths (a reply always reads as hex); {\"option\": shape}, null or the value; {\"vec\": shape}, an array (with \"len\", exactly that many), and a map is a vec of [key, value] tuples; {\"tuple\": [shapes]}, an array; {\"struct\": {field: shape}}, an object, where a field that is an option may be left out; {\"enum\": {Variant: shape}}, one variant, written \"Variant\" when its shape is null and {\"Variant\": value} otherwise.";
    type Args = SchemaArgs;

    fn schema() -> Value {
        object(
            json!({
                "program": {
                    "type": "string",
                    "description": "a program id, such as chat or files; omitted, every program that runs"
                },
                "op": {
                    "type": "string",
                    "description": "one op of the program, by its variant's name, such as PostMessage"
                },
                "query": {
                    "type": "string",
                    "description": "one query of the program, by its variant's name, such as Roots"
                },
                "whole": {
                    "type": "boolean",
                    "description": "every shape of the program at once"
                },
            }),
            &[],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: SchemaArgs) -> Result<Run, Error> {
        let whole = args.whole == Some(true);
        let Some(program) = args.program else {
            if args.op.is_some() || args.query.is_some() || whole {
                return Err(invalid(
                    "op, query and whole tell of one program: give its program too",
                ));
            }
            return Ok(Run::Done(roster(ctx)?));
        };
        let code = code_of(ctx, &program)?;
        let interface = schema_in(ctx, &program, code)?;
        let answer = match (args.op, args.query, whole) {
            (None, None, false) => index(&program, &interface, &told_in(ctx, &program, code)?),
            (None, None, true) => whole_of(&program, &interface)?,
            (Some(op), None, false) => {
                let told = told_in(ctx, &program, code)?;
                unfold(&program, &interface, Side::Op, &op, told.ops.get(&op))?
            }
            (None, Some(query), false) => {
                let told = told_in(ctx, &program, code)?;
                unfold(
                    &program,
                    &interface,
                    Side::Query,
                    &query,
                    told.queries.get(&query),
                )?
            }
            _ => {
                return Err(invalid(
                    "network_schema gives a program's index, one op, one query or the whole program: give at most one of op, query and whole",
                ));
            }
        };
        Ok(Run::Done(answer))
    }
}

#[derive(Clone, Copy)]
enum Side {
    Op,
    Query,
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Op => "op",
            Side::Query => "query",
        }
    }

    fn plural(self) -> &'static str {
        match self {
            Side::Op => "ops",
            Side::Query => "queries",
        }
    }

    fn declaration(self, interface: &Interface) -> &str {
        match self {
            Side::Op => &interface.op,
            Side::Query => &interface.query,
        }
    }
}

fn corrupt(fault: Fault) -> Error {
    Error::new(code::CORRUPT, fault.to_string())
}

fn said(text: &str) -> Value {
    match text.trim().is_empty() {
        true => Value::Null,
        false => json!(text),
    }
}

// Every program that runs, each with what its code says it is for, or why that does not read.
fn roster(ctx: &QueryCtx) -> Result<Value, Error> {
    let listed: Vec<Value> = running(ctx)?
        .into_iter()
        .map(|entry| match told_in(ctx, &entry.program, entry.code) {
            Ok(told) => json!({ "program": entry.program, "about": said(&told.about) }),
            Err(error) => json!({ "program": entry.program, "unread": refusal(&error) }),
        })
        .collect();
    Ok(json!({ "programs": listed }))
}

// A program's index: what it is for, and each op and query by name with what it does, null where
// its code says nothing of one.
fn index(program: &str, interface: &Interface, told: &Described) -> Value {
    let listed = |side: Side, group: &BTreeMap<String, Told>| {
        let each = variants(&interface.schema, side.declaration(interface))
            .into_iter()
            .map(|name| {
                let description = group
                    .get(&name)
                    .map_or(Value::Null, |one| said(&one.description));
                (name, description)
            });
        Value::Object(each.collect())
    };
    let mut index = json!({
        "program": program,
        "about": said(&told.about),
        "ops": listed(Side::Op, &told.ops),
        "queries": listed(Side::Query, &told.queries),
    });
    if told.ops.is_empty() && told.queries.is_empty() {
        index["note"] = json!(format!(
            "program {program}'s code describes none of its ops and queries; network_schema with one of them still gives its shape and an example made from it"
        ));
    }
    index
}

// One op or query: what it does, an example (the code's own, else one made from its shape), its
// shape, the shape of what it returns, and each type those name.
fn unfold(
    program: &str,
    interface: &Interface,
    side: Side,
    name: &str,
    told: Option<&Told>,
) -> Result<Value, Error> {
    let schema = &interface.schema;
    let declaration = side.declaration(interface);
    let known = variants(schema, declaration);
    if !known.iter().any(|known| known == name) {
        return Err(invalid(format!(
            "program {program} has no {} {name}; its {} are {}",
            side.word(),
            side.plural(),
            known.join(", ")
        )));
    }
    let (shape, mut types) = apart(variant_shape(schema, declaration, name).map_err(corrupt)?);
    let mut answer = Map::new();
    answer.insert("program".into(), json!(program));
    answer.insert(side.word().into(), json!(name));
    if let Some(told) = told.filter(|told| !told.description.trim().is_empty()) {
        answer.insert("description".into(), json!(told.description));
    }
    let example = match told.and_then(|told| told.example.clone()) {
        Some(example) => Some(example),
        None => variant_example(schema, declaration, name).map_err(corrupt)?,
    };
    if let Some(example) = example {
        answer.insert("example".into(), example);
    }
    answer.insert("shape".into(), shape);
    let returns = match side {
        Side::Op => returned_by(interface, name)?,
        Side::Query => told.and_then(|told| told.returns.clone()),
    };
    if let Some(returns) = returns {
        let (returns, reached) = apart(returns);
        types.extend(reached);
        answer.insert("returns".into(), returns);
    }
    answer.insert("types".into(), Value::Object(types));
    Ok(Value::Object(answer))
}

fn returned_by(interface: &Interface, op: &str) -> Result<Option<Value>, Error> {
    let declared = interface
        .returns
        .iter()
        .flatten()
        .find(|(returning, _)| returning == op);
    let Some((_, of)) = declared else {
        return Ok(None);
    };
    let shaped = shape(&interface.schema, &[("returns", of.as_str())], None).map_err(corrupt)?;
    Ok(Some(shaped))
}

// A shape written as `{"returns": shape, "types": {..}}`, as its shape and its types.
fn apart(shaped: Value) -> (Value, Map<String, Value>) {
    match shaped {
        Value::Object(mut parts) if parts.contains_key("returns") => {
            let types = match parts.remove("types") {
                Some(Value::Object(types)) => types,
                _ => Map::new(),
            };
            (parts.remove("returns").unwrap_or_default(), types)
        }
        other => (other, Map::new()),
    }
}

fn whole_of(program: &str, interface: &Interface) -> Result<Value, Error> {
    let mut shape = interface.shape().map_err(corrupt)?;
    shape["program"] = json!(program);
    Ok(shape)
}

#[derive(Deserialize)]
pub(crate) struct QueryArgs {
    program: String,
    query: Value,
}

pub(crate) struct Query;

impl Tool for Query {
    const NAME: &'static str = "network_query";
    const DESCRIPTION: &'static str = "Ask a program a query, written in the query shape network_schema gives for it (network_schema with the program and the query's name gives an example to edit), and read its reply in its reply shape. A query changes nothing. An answer that is not the program's reply comes back as its bytes in hex, with why it did not read.";
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
        let query = spoken(&args.query);
        let asked = interface.encode_query(&query).map_err(refused)?;
        let answer = ctx.query_raw(args.program, asked)?;
        Ok(Run::Done(match interface.decode_reply(&answer) {
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
    const DESCRIPTION: &'static str = "Send a program an op, written in the op shape network_schema gives for it (network_schema with the program and the op's name gives an example to edit), as your own account. The op is yours: the program applies it where it allows your account and refuses it otherwise, with its reason. An HTTP request the program bound to the egress role answers whole comes back as the answer's `status`, `headers` and `body`: JSON where the body parses as JSON, else text where it is UTF-8, else hex, as `body_as` says. Any other output comes back in `returned`: as JSON in the shape network_schema's `returns` gives for the op, else as hex, and then `unread` says why it reads as nothing more.";
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
        let op = spoken(&args.op);
        let payload = interface.encode_op(&op).map_err(refused)?;
        let read = reading(ctx, &args.program, interface, &payload);
        let writes = vec![Write {
            target: args.program,
            payload,
        }];
        Ok(Run::Read { writes, read })
    }
}

// How the output of an op sent to `program` reads: an HTTP answer egress delivers whole as its
// status, headers and body, any other output as `returned` says.
pub(crate) fn reading(
    ctx: &QueryCtx,
    program: &str,
    interface: Interface,
    payload: &[u8],
) -> Reading {
    match answers_http(ctx, program, payload) {
        true => Box::new(response),
        false => returned(program, interface, payload),
    }
}

// What an op sent outside a tool call (a booked run) answered, read as network_call reads it,
// else its bytes as hex and why.
pub(crate) fn answered(ctx: &QueryCtx, program: &str, payload: &[u8], output: &[u8]) -> Value {
    let read = schema_of(ctx, program)
        .and_then(|interface| reading(ctx, program, interface, payload)(output));
    match read {
        Ok(value) => value,
        Err(_) if output.is_empty() => json!({ "ok": true }),
        Err(error) => unread(output, &refusal(&error)),
    }
}

fn answers_http(ctx: &QueryCtx, program: &str, payload: &[u8]) -> bool {
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
    let (program, payload) = (program.to_owned(), payload.to_vec());
    Box::new(move |output| Ok(interface.returned(&program, &payload, output)))
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
