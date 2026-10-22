//! Tools over promises and asks: the agent records what it says it will do later, or asks of
//! someone else, as it says it; whoever owes it closes it as kept or dropped. harness wakes both
//! sides with it at its due block and again while it is open. The agent lists its open ones.
use ::program::{Error, ExecCtx, invalid, unexpected_reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Run, Seat, Tool, ahead, cursor_schema, next, object, page, write};
use crate::promises::{asked, open};
use crate::{AccountNumber, Op, PROGRAM, PromiseRow};

fn due(ctx: &ExecCtx, height: Option<u64>, blocks: Option<u64>) -> Result<u64, Error> {
    match (height, blocks) {
        (Some(height), None) => Ok(height),
        (None, Some(blocks)) => ahead(ctx, blocks),
        (Some(_), Some(_)) => Err(invalid(
            "a promise is due at a height or in a number of blocks, not both: give height or in",
        )),
        (None, None) => Err(invalid(
            "a promise needs the block it is due at: a height, or in, a number of blocks from the block the call runs in",
        )),
    }
}

#[derive(Deserialize)]
pub(crate) struct MakeArgs {
    text: String,
    channel: String,
    seq: u64,
    height: Option<u64>,
    #[serde(rename = "in")]
    blocks: Option<u64>,
    owed_by: Option<AccountNumber>,
}

pub(crate) struct Make;

impl Tool for Make {
    const NAME: &'static str = "promise";
    const DESCRIPTION: &'static str = "Record a promise as you make it: whenever you say you will do something later, call this in the same turn with what you will do, the chat message it answers (`channel` and `seq`) and the block it is due at, a `height` or `in`, a number of blocks counted from the block this call runs in. To record an ask instead (something you asked another account to do), name that account in `owed_by`. At the due block harness wakes this conversation with it and everything of yours still open (your current conversation, if this one has closed), and for an ask also the current conversation of the account asked, when harness runs it; it does so again on a fixed repeat while it is open (each wake says the block it comes back at), until the account that owes it closes it with promise_keep. Answers the record's number, which promise_keep and promise_list take.";
    const WRITES: bool = true;
    type Args = MakeArgs;

    fn schema() -> Value {
        object(
            json!({
                "text": { "type": "string", "description": "what is owed, in one line" },
                "channel": { "type": "string", "description": "the chat channel of the message it answers, as chat_channels lists them" },
                "seq": { "type": "integer", "description": "that message's seq" },
                "height": { "type": "integer", "description": "the block it is due at, later than the current one; give this or in" },
                "in": { "type": "integer", "minimum": 1, "description": "instead of height: how many blocks after the block this call runs in" },
                "owed_by": { "type": "integer", "description": "the account that owes it, for an ask of someone else; you if omitted" },
            }),
            &["text", "channel", "seq"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: MakeArgs) -> Result<Run, Error> {
        let due = due(ctx, args.height, args.blocks)?;
        let promise = Op::Promise {
            thread: seat.thread,
            owed_by: args.owed_by.unwrap_or(seat.agent),
            channel: args.channel,
            seq: args.seq,
            text: args.text,
            due,
        };
        Ok(write(ctx.env().program.clone(), abi::encode(&promise)))
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        let promise: u64 = abi::decode(output)
            .map_err(|fault| unexpected_reply(PROGRAM, "Promise", &fault.sentence))?;
        Ok(json!({ "promise": promise }))
    }
}

#[derive(Deserialize)]
pub(crate) struct KeepArgs {
    promise: u64,
    kept: bool,
    how: String,
}

pub(crate) struct Keep;

impl Tool for Keep {
    const NAME: &'static str = "promise_keep";
    const DESCRIPTION: &'static str = "Close a promise you owe, or an ask of you, once it is done or will not be: `kept` true when you did what it said, false when you drop it, and `how` in one line (what you did, or why not). Only the account that owes it closes it, and harness wakes it until it does.";
    const WRITES: bool = true;
    type Args = KeepArgs;

    fn schema() -> Value {
        object(
            json!({
                "promise": { "type": "integer", "description": "the record's number, as promise answered it" },
                "kept": { "type": "boolean", "description": "true when you did what it said, false when you drop it" },
                "how": { "type": "string", "description": "one line: what you did, or why not" },
            }),
            &["promise", "kept", "how"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: KeepArgs) -> Result<Run, Error> {
        let keep = Op::Keep {
            promise: args.promise,
            kept: args.kept,
            how: args.how,
        };
        Ok(write(ctx.env().program.clone(), abi::encode(&keep)))
    }
}

#[derive(Deserialize)]
pub(crate) struct ListArgs {
    cursor: Option<String>,
    asked: Option<bool>,
}

pub(crate) struct List;

fn listed(row: &PromiseRow) -> Value {
    json!({
        "promise": row.id,
        "text": row.text,
        "owed_by": row.owed_by,
        "asked_by": row.asked_by,
        "conversation": row.thread,
        "channel": row.channel,
        "seq": row.seq,
        "made": row.made,
        "next": row.next,
    })
}

impl Tool for List {
    const NAME: &'static str = "promise_list";
    const DESCRIPTION: &'static str = "What you owe and have not closed, oldest first: your promises and what others asked of you; with asked, what you asked of others instead. Each with its number, what it says, who owes it and who asked, the asking conversation, the chat message it answers (`channel`, `seq`), the block it was made at and the block it is due next. `next` reads the page after.";
    type Args = ListArgs;

    fn schema() -> Value {
        object(
            json!({
                "cursor": cursor_schema(),
                "asked": { "type": "boolean", "description": "what you asked of others instead of what you owe" },
            }),
            &[],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ListArgs) -> Result<Run, Error> {
        let page = page(&args.cursor)?;
        let found = match args.asked.unwrap_or(false) {
            true => asked(ctx, seat.agent, &page)?,
            false => open(ctx, seat.agent, &page)?,
        };
        let rows: Vec<Value> = found.items.iter().map(listed).collect();
        Ok(Run::Done(
            json!({ "promises": rows, "next": next(&found.next) }),
        ))
    }
}
