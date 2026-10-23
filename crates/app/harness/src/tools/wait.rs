//! Tools over waits: the agent names one event (a height, a chat message, a forge ref
//! or change, a files path, a chat place clearing), and this turn ends immediately; the
//! event wakes the conversation once, as a message. The agent lists its waits and cancels one.
use ::program::{Error, ExecCtx, invalid, unexpected_reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{LISTED, Run, Seat, Tool, ahead, cursor_schema, next, object, page, write};
use crate::waits::{armed, waits, what};
use crate::{Op, PROGRAM, Until, WaitFor, WaitRow};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum EventArgs {
    Height(u64),
    In(u64),
    Pod {
        name: String,
    },
    Message {
        channel: String,
        #[serde(default)]
        from: Option<u64>,
        #[serde(default)]
        mentions_me: bool,
    },
    Change {
        repo: String,
        n: u64,
        #[serde(default)]
        until: Option<UntilArgs>,
    },
    Ref {
        repo: String,
        #[serde(rename = "ref")]
        reference: String,
    },
    File {
        path: String,
    },
    Clear {
        channel: String,
        #[serde(default)]
        thread: Option<u64>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum UntilArgs {
    Merged,
    Closed,
    Either,
}

impl From<UntilArgs> for Until {
    fn from(until: UntilArgs) -> Until {
        match until {
            UntilArgs::Merged => Until::Merged,
            UntilArgs::Closed => Until::Closed,
            UntilArgs::Either => Until::Either,
        }
    }
}

fn waited(ctx: &ExecCtx, event: EventArgs) -> Result<WaitFor, Error> {
    let event = match event {
        EventArgs::Height(height) => WaitFor::Height { height },
        EventArgs::In(blocks) => WaitFor::Height {
            height: ahead(ctx, blocks)?,
        },
        EventArgs::Pod { name } => WaitFor::Pod { name },
        EventArgs::Message {
            channel,
            from,
            mentions_me,
        } => WaitFor::Message {
            channel,
            from,
            mentions_me,
        },
        EventArgs::Change {
            repo,
            n,
            until: None,
        } => WaitFor::Change { repo, n },
        EventArgs::Change {
            repo,
            n,
            until: Some(until),
        } => WaitFor::Settled {
            repo,
            n,
            until: until.into(),
        },
        EventArgs::Ref { repo, reference } => WaitFor::Ref { repo, reference },
        EventArgs::File { path } => WaitFor::File { path },
        EventArgs::Clear { channel, thread } => WaitFor::Clear { channel, thread },
    };
    Ok(event)
}

#[derive(Deserialize)]
pub(crate) struct ForArgs {
    event: EventArgs,
}

pub(crate) struct For;

fn repo_schema() -> Value {
    json!({ "type": "string", "description": "the forge repository, as forge_repos lists them" })
}

impl Tool for For {
    const NAME: &'static str = "wait_for";
    const DESCRIPTION: &'static str = "Wake this conversation once when an event happens, and end this turn immediately. You are not asked again until the event, or any other message, arrives. The event comes as one message saying what happened. Events: a `message` in a chat channel (only from one account with `from`, only one that mentions you, writes to you directly or reacts to your message with `mentions_me`; a reaction on your message counts as a message here; whoever writes it, even someone you don't otherwise hear); a forge `change` whose source branch moves, that is reviewed, or that is closed or merged (one already closed or merged is refused), or with `until` only once it is merged, closed or either, its pushes heard by nothing (a change that ends the other way still ends the wait, saying so), and either kind ends when forge takes the change out of a merge queue, saying why; a forge `ref` that moves (a branch's name alone is its refs/heads/ ref); a block's `height` (later than the current one), or `in`, a number of blocks counted from the block this call runs in, which stays right however long the reply that makes the call took to come; a `file` of files that a commit changes (a file's bytes, mode or meta; anything below a directory; or the path being made or removed); or a chat place to `clear` (a channel's foot, or a thread by its root message's seq) of everyone else's hold and draft, not counting those who wait there too (at once when it already is). A message that arrives first wakes you as usual, and the wait stays. Answers the wait's number, which wait_list shows and wait_cancel takes.";
    const WRITES: bool = true;
    type Args = ForArgs;

    fn schema() -> Value {
        let message = object(
            json!({
                "channel": { "type": "string", "description": "the channel's id, as chat_channels lists them" },
                "from": { "type": "integer", "description": "only a message this account writes; anyone's if omitted" },
                "mentions_me": { "type": "boolean", "description": "only a message that mentions you, writes to you directly or reacts to your message" },
            }),
            &["channel"],
        );
        let change = object(
            json!({
                "repo": repo_schema(),
                "n": { "type": "integer", "description": "the change's number" },
                "until": { "type": "string", "enum": ["merged", "closed", "either"], "description": "wake only when the change ends so, not when its source moves or it is reviewed; omitted, a push or a review wakes you too" },
            }),
            &["repo", "n"],
        );
        let reference = object(
            json!({
                "repo": repo_schema(),
                "ref": { "type": "string", "description": "a branch, like dev, or a whole ref, like refs/heads/dev" },
            }),
            &["repo", "ref"],
        );
        let file = object(
            json!({ "path": { "type": "string", "description": "an absolute path, like /shared/tools/warm-pod" } }),
            &["path"],
        );
        let clear = object(
            json!({
                "channel": { "type": "string", "description": "the channel's id, as chat_channels lists them" },
                "thread": { "type": "integer", "description": "a thread, by its root message's seq; the channel's foot if omitted" },
            }),
            &["channel"],
        );
        let event = json!({
            "type": "object",
            "description": "exactly one of these",
            "properties": {
                "message": message,
                "change": change,
                "ref": reference,
                "height": { "type": "integer", "description": "the block to wake at, later than the current one" },
                "in": { "type": "integer", "minimum": 1, "description": "instead of height: how many blocks to wait, counted from the block this call runs in" },
                "file": file,
                "clear": clear,
            },
            "minProperties": 1,
            "maxProperties": 1,
        });
        object(json!({ "event": event }), &["event"])
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ForArgs) -> Result<Run, Error> {
        let wait = Op::Wait {
            thread: seat.thread,
            event: waited(ctx, args.event)?,
        };
        Ok(write(ctx.env().program.clone(), abi::encode(&wait)))
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        let wait: u64 = abi::decode(output)
            .map_err(|fault| unexpected_reply(PROGRAM, "Wait", &fault.sentence))?;
        Ok(json!({
            "wait": wait,
            "turn": "this turn ends immediately; the event wakes this conversation",
        }))
    }
}

#[derive(Deserialize)]
pub(crate) struct ListArgs {
    limit: Option<u64>,
    cursor: Option<String>,
    all: Option<bool>,
}

pub(crate) struct List;

fn listed(row: &WaitRow) -> Value {
    let ended = row
        .ended
        .as_ref()
        .map(|ended| json!({ "height": ended.height, "text": ended.text }));
    json!({
        "wait": row.id,
        "conversation": row.thread,
        "made": row.made,
        "for": what(&row.event),
        "ended": ended,
    })
}

impl Tool for List {
    const NAME: &'static str = "wait_list";
    const DESCRIPTION: &'static str = "This conversation's waits that are not over yet, oldest first, a limit of them (20 if omitted): each one's number, the conversation it wakes, the block it was made at and what it waits for. With all, your waits in any conversation instead, over or not, and once one is over, when and what happened (`ended`), or null while it waits. `next` reads the following page.";
    type Args = ListArgs;

    fn schema() -> Value {
        object(
            json!({
                "limit": { "type": "integer", "minimum": 1, "description": "how many waits; 20 if omitted" },
                "cursor": cursor_schema(),
                "all": { "type": "boolean", "description": "every wait you made, in every conversation, over ones included; this conversation's open waits if omitted" },
            }),
            &[],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ListArgs) -> Result<Run, Error> {
        let limit = args.limit.unwrap_or(LISTED);
        if limit == 0 {
            return Err(invalid("a limit is a number of waits from 1"));
        }
        let mut page = page(&args.cursor)?;
        page.limit = Some(limit);
        let found = match args.all.unwrap_or(false) {
            true => waits(ctx, seat.agent, &page)?,
            false => armed(ctx, seat.thread, &page)?,
        };
        let rows: Vec<Value> = found.items.iter().map(listed).collect();
        Ok(Run::Done(
            json!({ "waits": rows, "next": next(&found.next) }),
        ))
    }
}

#[derive(Deserialize)]
pub(crate) struct CancelArgs {
    wait: u64,
}

pub(crate) struct Cancel;

impl Tool for Cancel {
    const NAME: &'static str = "wait_cancel";
    const DESCRIPTION: &'static str = "Cancel a wait you made, by its number, before it is over.";
    const WRITES: bool = true;
    type Args = CancelArgs;

    fn schema() -> Value {
        object(
            json!({ "wait": { "type": "integer", "description": "the wait's number, as wait_for answered it" } }),
            &["wait"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: CancelArgs) -> Result<Run, Error> {
        let unwait = Op::Unwait { wait: args.wait };
        Ok(write(ctx.env().program.clone(), abi::encode(&unwait)))
    }
}
