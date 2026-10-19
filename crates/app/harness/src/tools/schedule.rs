//! Tools over runs the agent books for a later block: harness books each with the scheduler and,
//! as its block ends, sends its op as the agent; what it did comes back to the conversation that
//! booked it. The agent lists its runs and cancels one before it runs.
use ::program::{Error, ExecCtx, invalid, unexpected_reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::interface::{encoded, program_schema};
use super::{Run, Seat, Tool, cursor_schema, object, write};
use crate::bookings::newest;
use crate::schema::schema_of;
use crate::{Booking, Op, PROGRAM, Ran};

#[derive(Deserialize)]
pub(crate) struct BookArgs {
    height: u64,
    program: String,
    op: Value,
}

pub(crate) struct Book;

impl Tool for Book {
    const NAME: &'static str = "schedule_run";
    const DESCRIPTION: &'static str = "Book an op to run as your own account at a later block: a program and an op as network_call takes them, and the block's height. As that block ends, harness sends the op as you, and what it did, or why it was refused, arrives in this conversation as a message, so a run booked for a block wakes you then. What it did reads as network_call's answer does: an HTTP request the program bound to the egress role answers whole as `status`, `headers` and `body`, any other output in `returned` in the shape network_schema's `returns` gives for the op, else as hex with `unread` saying why. A refused run is not tried again. Answers the run's entry number, which schedule_list shows and schedule_cancel takes.";
    const WRITES: bool = true;
    type Args = BookArgs;

    fn schema() -> Value {
        object(
            json!({
                "height": { "type": "integer", "description": "the block to run at, later than the current one" },
                "program": program_schema(),
                "op": { "description": "the op, in the program's op shape" },
            }),
            &["height", "program", "op"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: BookArgs) -> Result<Run, Error> {
        let payload = encoded(ctx, &args.program, &args.op)?;
        let book = Op::Book {
            height: args.height,
            target: args.program,
            payload,
            thread: Some(seat.thread),
        };
        Ok(write(PROGRAM, abi::encode(&book)))
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        let entry: u64 = abi::decode(output)
            .map_err(|fault| unexpected_reply(PROGRAM, "Book", &fault.sentence))?;
        Ok(json!({ "entry": entry }))
    }
}

#[derive(Deserialize)]
pub(crate) struct ListArgs {
    limit: Option<u64>,
    pending_only: Option<bool>,
    cursor: Option<String>,
}

// how many runs a list answers where the agent names no limit, as its requester ruled (J-066)
const LISTED: u64 = 20;

// the entry a cursor names: the hex `next` of the page before
fn below(cursor: &str) -> Result<u64, Error> {
    let bytes = design::unhex(cursor).and_then(|bytes| <[u8; 8]>::try_from(bytes).ok());
    bytes
        .map(u64::from_be_bytes)
        .ok_or_else(|| invalid("a cursor is the hex `next` of a page"))
}

pub(crate) struct List;

fn op_of(ctx: &ExecCtx, booking: &Booking) -> Value {
    let read = schema_of(ctx, &booking.target)
        .ok()
        .and_then(|interface| interface.decode_op(&booking.payload).ok());
    read.unwrap_or_else(|| json!({ "hex": abi::hex(&booking.payload) }))
}

// a run that applied keeps its answer as JSON text and lists it as that JSON; a refusal, and a
// run kept before answers were read, list as their text
fn told(ran: &Ran) -> Value {
    let answer = (!ran.failed).then(|| serde_json::from_str(&ran.text).ok());
    answer.flatten().unwrap_or_else(|| json!(ran.text))
}

fn listed(ctx: &ExecCtx, booking: &Booking) -> Value {
    let ran = booking
        .ran
        .as_ref()
        .map(|ran| json!({ "height": ran.height, "failed": ran.failed, "text": told(ran) }));
    json!({
        "entry": booking.entry,
        "booked": booking.booked,
        "height": booking.height,
        "program": booking.target,
        "op": op_of(ctx, booking),
        "ran": ran,
    })
}

impl Tool for List {
    const NAME: &'static str = "schedule_list";
    const DESCRIPTION: &'static str = "Your booked runs, newest first, limit of them (20 if omitted), with pending_only only the ones still waiting: each one's entry number, the block it was booked at and the block it runs at (`height`), its program and op, and once it ran, what it did (`ran`: its block, whether it was refused, and `text`: what the op answered, read as network_call reads it, or why it was refused), or null while it waits. `next` reads the older ones.";
    type Args = ListArgs;

    fn schema() -> Value {
        object(
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "how many runs; 20 if omitted" },
                "pending_only": { "type": "boolean", "description": "only the runs still waiting for their block" },
                "cursor": cursor_schema(),
            }),
            &[],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ListArgs) -> Result<Run, Error> {
        let limit = args.limit.unwrap_or(LISTED);
        if limit == 0 {
            return Err(invalid("a limit is a number of runs from 1"));
        }
        let from = args.cursor.as_deref().map(below).transpose()?;
        let pending = args.pending_only.unwrap_or(false);
        let (found, older) = newest(ctx, seat.agent, from, limit, pending)?;
        let runs: Vec<Value> = found.iter().map(|booking| listed(ctx, booking)).collect();
        let next = older.map(|entry| abi::hex(&entry.to_be_bytes()));
        Ok(Run::Done(json!({ "runs": runs, "next": next })))
    }
}

#[derive(Deserialize)]
pub(crate) struct CancelArgs {
    entry: u64,
}

pub(crate) struct Cancel;

impl Tool for Cancel {
    const NAME: &'static str = "schedule_cancel";
    const DESCRIPTION: &'static str =
        "Cancel a run you booked, by its entry number, before it runs.";
    const WRITES: bool = true;
    type Args = CancelArgs;

    fn schema() -> Value {
        object(
            json!({ "entry": { "type": "integer", "description": "the run's entry number, as schedule_run answered it" } }),
            &["entry"],
        )
    }

    fn run(_: &ExecCtx, _: &Seat, args: CancelArgs) -> Result<Run, Error> {
        let unbook = Op::Unbook { entry: args.entry };
        Ok(write(PROGRAM, abi::encode(&unbook)))
    }
}
