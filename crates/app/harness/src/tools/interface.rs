//! Tools over any program of the network by the schema its code carries: the shapes of its ops,
//! queries and replies, a query asked and its reply read back, and an op sent as the agent.
use ::program::{Error, ExecCtx, code, invalid};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Run, Seat, Tool, object, write};
use crate::codec::{self, Fault};
use crate::schema::schema_of;

fn program_schema() -> Value {
    json!({ "type": "string", "description": "a program id, such as chat or files" })
}

fn refused(fault: Fault) -> Error {
    invalid(fault.to_string())
}

#[derive(Deserialize)]
pub(crate) struct SchemaArgs {
    program: String,
}

pub(crate) struct Schema;

impl Tool for Schema {
    const NAME: &'static str = "network_schema";
    const DESCRIPTION: &'static str = "The shapes of a program's ops, queries and replies, from the schema its code carries: write a query for network_query and an op for network_call in them. `op`, `query` and `reply` are shapes, and each type they name is under `types`. A shape is a type name; \"null\"; \"bool\"; \"string\"; an integer type such as \"u64\", written as a JSON number (u128 and i128 as decimal strings); \"hex\", bytes as hex ({\"hex\": N}: exactly N bytes); {\"option\": shape}, null or the value; {\"vec\": shape}, an array (with \"len\", exactly that many), and a map is a vec of [key, value] tuples; {\"tuple\": [shapes]}, an array; {\"struct\": {field: shape}}, an object, where a field that is an option may be left out; {\"enum\": {Variant: shape}}, one variant, written \"Variant\" when its shape is null and {\"Variant\": value} otherwise. network_query with program `programs` and query \"Roster\" lists the programs that run.";
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
        let mut shape = codec::shape(&interface.schema, &roots)
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
    const DESCRIPTION: &'static str = "Send a program an op, written in the op shape network_schema gives for it, as your own account. The op is yours: the program applies it where it allows your account and refuses it otherwise, with its reason. What the op returns comes back as hex.";
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
        Ok(write(args.program, payload))
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        Ok(match output {
            [] => json!({ "ok": true }),
            returned => json!({ "ok": true, "returned": abi::hex(returned) }),
        })
    }
}
