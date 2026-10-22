//! Tools over any program of the network by the schema and tools sections its code carries, in
//! `borsh_json::discover`'s words: the programs that run, a program's index, one op or query
//! unfolded with an example to edit, a query asked and its reply read back, and an op sent.
use ::program::{Error, ExecCtx, QueryCtx, code, invalid};
use borsh_json::discover::{self, Ask, Refusal, Side};
use borsh_json::unread;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Reading, Run, Seat, Tool, Write};

use crate::calls::refusal;
use crate::codec::{Fault, spoken};
use crate::schema::{Interface, code_of, running, schema_in, schema_of, told_in};

fn refused(fault: Fault) -> Error {
    invalid(fault.to_string())
}

fn declined(refusal: Refusal) -> Error {
    match refusal {
        Refusal::Asked(why) => invalid(why),
        Refusal::Corrupt(why) => Error::new(code::CORRUPT, why),
    }
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
    const NAME: &'static str = discover::SCHEMA;
    const DESCRIPTION: &'static str = discover::SCHEMA_DESCRIPTION;
    type Args = SchemaArgs;

    fn schema() -> Value {
        discover::schema_input()
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: SchemaArgs) -> Result<Run, Error> {
        let asked = Ask::of(args.program, args.op, args.query, args.whole).map_err(invalid)?;
        let program = match &asked {
            Ask::Roster => return Ok(Run::Done(roster(ctx)?)),
            Ask::Index(program)
            | Ask::Whole(program)
            | Ask::Op(program, _)
            | Ask::Query(program, _) => program.clone(),
        };
        let code = code_of(ctx, &program)?;
        let interface = schema_in(ctx, &program, code)?;
        let answer = match asked {
            Ask::Whole(_) => discover::whole(&program, &interface),
            Ask::Index(_) => Ok(discover::index(
                &program,
                &interface,
                &told_in(ctx, &program, code)?,
            )),
            Ask::Op(_, op) => {
                let told = told_in(ctx, &program, code)?;
                discover::unfold(&program, &interface, &told, Side::Op, &op)
            }
            Ask::Query(_, query) => {
                let told = told_in(ctx, &program, code)?;
                discover::unfold(&program, &interface, &told, Side::Query, &query)
            }
            Ask::Roster => unreachable!("the roster is answered above"),
        };
        answer.map(Run::Done).map_err(declined)
    }
}

// Every program that runs, each with what its code says it is for, or why that does not read.
fn roster(ctx: &ExecCtx) -> Result<Value, Error> {
    let told = running(ctx)?
        .into_iter()
        .map(|entry| {
            let told = told_in(ctx, &entry.program, entry.code).map_err(|error| refusal(&error));
            (entry.program, told)
        })
        .collect();
    Ok(discover::roster(told))
}

#[derive(Deserialize)]
pub(crate) struct QueryArgs {
    program: String,
    query: Value,
}

pub(crate) struct Query;

impl Tool for Query {
    const NAME: &'static str = discover::QUERY;
    const DESCRIPTION: &'static str = discover::QUERY_DESCRIPTION;
    type Args = QueryArgs;

    fn schema() -> Value {
        discover::query_input()
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: QueryArgs) -> Result<Run, Error> {
        let interface = schema_of(ctx, &args.program)?;
        let query = spoken(&args.query);
        let asked = interface.encode_query(&query).map_err(refused)?;
        let answer = crate::reading::raw_for(ctx, seat.agent, args.program, asked)?;
        Ok(Run::Done(discover::replied(&interface, &answer)))
    }
}

#[derive(Deserialize)]
pub(crate) struct CallArgs {
    program: String,
    op: Value,
}

pub(crate) struct Call;

impl Tool for Call {
    const NAME: &'static str = discover::CALL;
    const DESCRIPTION: &'static str = discover::CALL_DESCRIPTION;
    const WRITES: bool = true;
    type Args = CallArgs;

    fn schema() -> Value {
        discover::call_input()
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
    let to_egress = ctx
        .env()
        .roles
        .get(abi::role::EGRESS)
        .is_some_and(|bound| bound == program);
    if to_egress && discover::asks_http(payload) {
        return Box::new(|output| Ok(discover::http_answer(output)));
    }
    let (program, payload) = (program.to_owned(), payload.to_vec());
    Box::new(move |output| Ok(interface.returned(&program, &payload, output)))
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
