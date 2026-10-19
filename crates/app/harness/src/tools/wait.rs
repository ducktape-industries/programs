//! Tools over waits: the agent names one event (a block's height, a pod's end, a chat message, a
//! forge ref or change, a files path), and this turn ends once its calls are back; the event wakes the
//! conversation once, as a message. The agent lists its waits and cancels one before it is over.
use ::program::{Error, ExecCtx, unexpected_reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Run, Seat, Tool, cursor_schema, next, object, page, write};
use crate::waits::{waits, what};
use crate::{Op, PROGRAM, WaitFor, WaitRow};

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum EventArgs {
    Height(u64),
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
    },
    Ref {
        repo: String,
        #[serde(rename = "ref")]
        reference: String,
    },
    File {
        path: String,
    },
}

impl From<EventArgs> for WaitFor {
    fn from(event: EventArgs) -> WaitFor {
        match event {
            EventArgs::Height(height) => WaitFor::Height { height },
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
            EventArgs::Change { repo, n } => WaitFor::Change { repo, n },
            EventArgs::Ref { repo, reference } => WaitFor::Ref { repo, reference },
            EventArgs::File { path } => WaitFor::File { path },
        }
    }
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
    const DESCRIPTION: &'static str = "Wake this conversation once when an event happens, and end this turn meanwhile: once this reply's tool calls are back, you are not asked again until the event, or any other message, arrives. The event comes as one message saying what happened. Events: a `message` in a chat channel (only from one account with `from`, only one that mentions you or writes to you directly with `mentions_me`; whoever writes it, even someone you don't otherwise hear); a forge `change` whose source branch moves, or that is closed or merged (one already closed or merged is refused); a forge `ref` that moves (a branch's name alone is its refs/heads/ ref); a block's `height` (later than the current one); a `pod` of the pods service ending (by its name; a pod that has ended already wakes you at once); or a `file` of files that a commit changes (a file's bytes, mode or meta; anything below a directory; or the path being made or removed). A message that arrives first wakes you as usual, and the wait stays. Answers the wait's number, which wait_list shows and wait_cancel takes.";
    const WRITES: bool = true;
    type Args = ForArgs;

    fn schema() -> Value {
        let pod = object(
            json!({ "name": { "type": "string", "description": "the pod's name, as the pods service answered it" } }),
            &["name"],
        );
        let message = object(
            json!({
                "channel": { "type": "string", "description": "the channel's id, as chat_channels lists them" },
                "from": { "type": "integer", "description": "only a message this account writes; anyone's if omitted" },
                "mentions_me": { "type": "boolean", "description": "only a message that mentions you or writes to you directly" },
            }),
            &["channel"],
        );
        let change = object(
            json!({
                "repo": repo_schema(),
                "n": { "type": "integer", "description": "the change's number" },
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
        let event = json!({
            "type": "object",
            "description": "exactly one of these",
            "properties": {
                "message": message,
                "change": change,
                "ref": reference,
                "height": { "type": "integer", "description": "the block to wake at, later than the current one" },
                "pod": pod,
                "file": file,
            },
            "minProperties": 1,
            "maxProperties": 1,
        });
        object(json!({ "event": event }), &["event"])
    }

    fn run(_: &ExecCtx, seat: &Seat, args: ForArgs) -> Result<Run, Error> {
        let wait = Op::Wait {
            thread: seat.thread,
            event: args.event.into(),
        };
        Ok(write(PROGRAM, abi::encode(&wait)))
    }

    fn render(output: &[u8]) -> Result<Value, Error> {
        let wait: u64 = abi::decode(output)
            .map_err(|fault| unexpected_reply(PROGRAM, "Wait", &fault.sentence))?;
        Ok(json!({
            "wait": wait,
            "turn": "this turn ends once this reply's calls are back; the event wakes this conversation",
        }))
    }
}

#[derive(Deserialize)]
pub(crate) struct ListArgs {
    cursor: Option<String>,
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
    const DESCRIPTION: &'static str = "Your waits, in the order you made them: each one's number, the conversation it wakes, the block it was made at, what it waits for, and once it is over, when and what happened (`ended`), or null while it waits.";
    type Args = ListArgs;

    fn schema() -> Value {
        object(json!({ "cursor": cursor_schema() }), &[])
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ListArgs) -> Result<Run, Error> {
        let found = waits(ctx, seat.agent, &page(&args.cursor)?)?;
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

    fn run(_: &ExecCtx, _: &Seat, args: CancelArgs) -> Result<Run, Error> {
        let unwait = Op::Unwait { wait: args.wait };
        Ok(write(PROGRAM, abi::encode(&unwait)))
    }
}
