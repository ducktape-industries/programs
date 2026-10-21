//! A program's ops and queries as an agent finds and uses them, whichever harness asks: the
//! programs that run, one program's index, one op or query unfolded, or every shape at once; and
//! the words and inputs of the tools that answer them, so every harness says the same.
use std::collections::BTreeMap;

use abi::role::egress::{self, http};
use borsh::schema::BorshSchemaContainer;
use serde_json::{Map, Value, json};

use crate::tools::{Described, Told};
use crate::{
    Fault, Interface, decode, encode, shape, spelled, variant_example, variant_named,
    variant_shape, variants,
};

pub const SCHEMA: &str = "network_schema";
pub const QUERY: &str = "network_query";
pub const CALL: &str = "network_call";

pub const SCHEMA_DESCRIPTION: &str = "What a program's ops and queries are and how to write them, from the code it runs. With no program: every program that runs, each with what it is for (`about`). With a program alone: its index, `about` and each op and query by name with what it does. With a program and an `op` (or a `query`), a variant's name: what it does (`description`), an `example` to edit and send with network_call (or network_query), its `shape`, the shape of what it `returns` (what network_call reads an op's output as, a query's reply), and each type those name under `types`. With `whole`: every shape of the program at once (`op`, `query`, `reply`, `returns` and `types`). A shape is a type name; \"null\"; \"bool\"; \"string\"; an integer type such as \"u64\", written as a JSON number (u128 and i128 as decimal strings); \"hex\", bytes as hex ({\"hex\": N}: exactly N bytes), which an op or a query may also write as {\"utf8\": text} for bytes that are text, such as forge's ref names and paths (a reply always reads as hex); {\"option\": shape}, null or the value; {\"vec\": shape}, an array (with \"len\", exactly that many), and a map is a vec of [key, value] tuples; {\"tuple\": [shapes]}, an array; {\"struct\": {field: shape}}, an object, where a field that is an option may be left out; {\"enum\": {variant: shape}}, one variant by its name in snake_case, written \"variant\" when its shape is null and {\"variant\": value} otherwise.";

pub const QUERY_DESCRIPTION: &str = "Ask a program a query, written in the query shape network_schema gives for it (network_schema with the program and the query's name gives an example to edit), and read its reply in its reply shape. A query changes nothing. An answer that is not the program's reply comes back as its bytes in hex, with why it did not read.";

pub const CALL_DESCRIPTION: &str = "Send a program an op, written in the op shape network_schema gives for it (network_schema with the program and the op's name gives an example to edit), as your own account. The op is yours: the program applies it where it allows your account and refuses it otherwise, with its reason. An HTTP request the program bound to the egress role answers whole comes back as the answer's `status`, `headers` and `body`: JSON where the body parses as JSON, else text where it is UTF-8, else hex, as `body_as` says. Any other output comes back in `returned`: as JSON in the shape network_schema's `returns` gives for the op, else as hex, and then `unread` says why it reads as nothing more.";

fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

pub fn program_input() -> Value {
    json!({ "type": "string", "description": "a program id, such as chat or files" })
}

pub fn schema_input() -> Value {
    object(
        json!({
            "program": {
                "type": "string",
                "description": "a program id, such as chat or files; omitted, every program that runs"
            },
            "op": {
                "type": "string",
                "description": "one op of the program, by its variant's name, such as post_message"
            },
            "query": {
                "type": "string",
                "description": "one query of the program, by its variant's name, such as roots"
            },
            "whole": {
                "type": "boolean",
                "description": "every shape of the program at once"
            },
        }),
        &[],
    )
}

pub fn query_input() -> Value {
    object(
        json!({
            "program": program_input(),
            "query": { "description": "the query, in the program's query shape" },
        }),
        &["program", "query"],
    )
}

pub fn call_input() -> Value {
    object(
        json!({
            "program": program_input(),
            "op": { "description": "the op, in the program's op shape" },
        }),
        &["program", "op"],
    )
}

// What a network_schema call asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    Roster,
    Index(String),
    Whole(String),
    Op(String, String),
    Query(String, String),
}

impl Ask {
    pub fn of(
        program: Option<String>,
        op: Option<String>,
        query: Option<String>,
        whole: Option<bool>,
    ) -> Result<Ask, String> {
        let whole = whole == Some(true);
        let Some(program) = program else {
            return match op.is_some() || query.is_some() || whole {
                true => Err("op, query and whole tell of one program: give its program too".into()),
                false => Ok(Ask::Roster),
            };
        };
        match (op, query, whole) {
            (None, None, false) => Ok(Ask::Index(program)),
            (None, None, true) => Ok(Ask::Whole(program)),
            (Some(op), None, false) => Ok(Ask::Op(program, op)),
            (None, Some(query), false) => Ok(Ask::Query(program, query)),
            _ => Err("network_schema gives a program's index, one op, one query or the whole program: give at most one of op, query and whole".into()),
        }
    }
}

// Why an answer could not be given: what was asked does not exist, or the code's sections do not
// read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Asked(String),
    Corrupt(String),
}

fn corrupt(fault: Fault) -> Refusal {
    Refusal::Corrupt(fault.to_string())
}

fn said(text: &str) -> Value {
    match text.trim().is_empty() {
        true => Value::Null,
        false => json!(text),
    }
}

// Every program that runs, each with what its code says it is for, or why that does not read.
pub fn roster(programs: Vec<(String, Result<Described, String>)>) -> Value {
    let listed: Vec<Value> = programs
        .into_iter()
        .map(|(program, told)| match told {
            Ok(told) => json!({ "program": program, "about": said(&told.about) }),
            Err(why) => json!({ "program": program, "unread": why }),
        })
        .collect();
    json!({ "programs": listed })
}

#[derive(Clone, Copy)]
pub enum Side {
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

    fn told(self, described: &Described) -> &BTreeMap<String, Told> {
        match self {
            Side::Op => &described.ops,
            Side::Query => &described.queries,
        }
    }
}

// A program's index: what it is for, and each op and query by name with what it does, null where
// its code says nothing of one.
pub fn index(program: &str, interface: &Interface, told: &Described) -> Value {
    let listed = |side: Side| {
        let group = side.told(told);
        let each = variants(&interface.schema, side.declaration(interface))
            .into_iter()
            .map(|name| {
                let description = group
                    .get(&name)
                    .map_or(Value::Null, |one| said(&one.description));
                (spelled(&name), description)
            });
        Value::Object(each.collect())
    };
    let mut index = json!({
        "program": program,
        "about": said(&told.about),
        "ops": listed(Side::Op),
        "queries": listed(Side::Query),
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
pub fn unfold(
    program: &str,
    interface: &Interface,
    told: &Described,
    side: Side,
    name: &str,
) -> Result<Value, Refusal> {
    let schema = &interface.schema;
    let declaration = side.declaration(interface);
    let Some(variant) = variant_named(schema, declaration, name) else {
        let known: Vec<String> = variants(schema, declaration)
            .iter()
            .map(String::as_str)
            .map(spelled)
            .collect();
        return Err(Refusal::Asked(format!(
            "program {program} has no {} {name}; its {} are {}",
            side.word(),
            side.plural(),
            known.join(", ")
        )));
    };
    let told = side.told(told).get(&variant);
    let (shape, mut types) = apart(variant_shape(schema, declaration, &variant).map_err(corrupt)?);
    let mut answer = Map::new();
    answer.insert("program".into(), json!(program));
    answer.insert(side.word().into(), json!(spelled(&variant)));
    if let Some(told) = told.filter(|told| !told.description.trim().is_empty()) {
        answer.insert("description".into(), json!(told.description));
    }
    let example = match told.and_then(|told| told.example.clone()) {
        Some(example) => Some(respelled(schema, declaration, side.word(), example)),
        None => variant_example(schema, declaration, &variant).map_err(corrupt)?,
    };
    if let Some(example) = example {
        answer.insert("example".into(), example);
    }
    answer.insert("shape".into(), shape);
    let returns = match side {
        Side::Op => returned_by(interface, &variant)?,
        Side::Query => told
            .and_then(|told| told.returns.clone())
            .map(|stored| reshaped(interface, stored)),
    };
    if let Some(returns) = returns {
        let (returns, reached) = apart(returns);
        types.extend(reached);
        answer.insert("returns".into(), returns);
    }
    answer.insert("types".into(), Value::Object(types));
    Ok(Value::Object(answer))
}

fn respelled(
    schema: &BorshSchemaContainer,
    declaration: &str,
    root: &str,
    example: Value,
) -> Value {
    let read = encode(schema, declaration, root, &example)
        .and_then(|bytes| decode(schema, declaration, root, &bytes));
    read.unwrap_or(example)
}

fn reshaped(interface: &Interface, stored: Value) -> Value {
    let named = match &stored["returns"] {
        Value::String(name) => Some(name.clone()),
        Value::Object(one) if one.len() == 1 => one.keys().next().cloned(),
        _ => None,
    };
    let fresh =
        named.and_then(|name| variant_shape(&interface.schema, &interface.reply, &name).ok());
    fresh.unwrap_or(stored)
}

fn returned_by(interface: &Interface, op: &str) -> Result<Option<Value>, Refusal> {
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

pub fn whole(program: &str, interface: &Interface) -> Result<Value, Refusal> {
    let mut shape = interface.shape().map_err(corrupt)?;
    shape["program"] = json!(program);
    Ok(shape)
}

// What answers a query: its reply in the reply's shape, else its bytes and why they do not read.
pub fn replied(interface: &Interface, answer: &[u8]) -> Value {
    match interface.decode_reply(answer) {
        Ok(reply) => json!({ "reply": reply }),
        Err(fault) => json!({ "bytes": abi::hex(answer), "unread": fault.to_string() }),
    }
}

// Whether `payload` is egress's request for an HTTP answer delivered whole, which reads as
// `http_answer` reads it when sent to the program the egress role binds.
pub fn asks_http(payload: &[u8]) -> bool {
    matches!(
        abi::decode::<egress::Op>(payload),
        Ok(egress::Op::Request {
            request: egress::Request::Http { .. },
            delivery: egress::Delivery::Whole,
            ..
        })
    )
}

// An HTTP answer egress delivered whole: its status, headers and body, the body as JSON where it
// parses, else text where it is UTF-8, else hex.
pub fn http_answer(output: &[u8]) -> Value {
    let response = match abi::decode::<http::Response>(output) {
        Ok(response) => response,
        Err(fault) => {
            let unread = format!("the answer is no HTTP response: {}", fault.sentence);
            return json!({ "ok": true, "returned": abi::hex(output), "unread": unread });
        }
    };
    let (body_as, body) = body(response.body);
    json!({
        "ok": true,
        "status": response.status,
        "headers": response.headers,
        "body": body,
        "body_as": body_as,
    })
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
