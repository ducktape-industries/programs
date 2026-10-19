//! The tools a thread's model may call, each over one program of the network: its schema, the
//! check a call's input passes, the run once its input is whole, what it shows while its input
//! arrives, how what it showed or did is taken back, and what its stream says, for one that streams.
mod blob;
mod chat;
mod edits;
pub(crate) mod file;
pub(crate) mod files;
mod forge;
mod image;
pub(crate) mod interface;
mod network;
pub(crate) mod pods;
pub(crate) mod prefix;
mod schedule;
mod thread;
mod wait;

pub(crate) use chat::shown_by;

// a call of chat_post: the post shows the answer itself
pub(crate) fn posts(tool: &str) -> bool {
    tool == <chat::Post as Tool>::NAME
}

use ::program::{
    BlobId, Error, ExecCtx, Principal, ProgramId, QueryCtx, invalid, unexpected_reply,
};
use abi::role::egress;
use abi::role::identity::AccountNumber;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use store::PageRequest;

use crate::ToolInfo;
use crate::codec::unquoted;

pub(crate) struct Seat<'a> {
    pub thread: u64,
    pub agent: AccountNumber,
    pub call: &'a str,
    pub shown: Option<&'a Value>,
}

pub(crate) struct Write {
    pub target: ProgramId,
    pub payload: Vec<u8>,
}

pub(crate) type Reading = Box<dyn FnOnce(&[u8]) -> Result<Value, Error>>;

pub(crate) type Following = Box<dyn FnOnce(&ExecCtx, &[u8]) -> Result<Run, Error>>;

// what a call does: answer at once, write and read the last write's output, write and choose the
// next run from it (as a step a refusal names), or answer that it streams and wake its
// conversation when the stream ends, at a heartbeat it gives or one harness measures
pub(crate) enum Run {
    Done(Value),
    Write(Vec<Write>),
    Read {
        writes: Vec<Write>,
        read: Reading,
    },
    Then {
        writes: Vec<Write>,
        then: Following,
    },
    Step {
        step: String,
        writes: Vec<Write>,
        then: Following,
    },
    Stream {
        request: egress::Request,
        about: String,
    },
    Measured {
        pod: String,
        target: String,
        body: Value,
        about: String,
    },
}

pub(crate) fn write(target: impl Into<ProgramId>, payload: Vec<u8>) -> Run {
    Run::Write(vec![Write {
        target: target.into(),
        payload,
    }])
}

pub(crate) enum Show {
    Wait,
    Write { writes: Vec<Write>, shown: Value },
}

pub(crate) enum Undo {
    Stays,
    Writes(Vec<Write>),
}

pub(crate) trait Tool {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    const WRITES: bool = false;
    type Args: DeserializeOwned;
    fn schema() -> Value;
    fn run(ctx: &ExecCtx, seat: &Seat, args: Self::Args) -> Result<Run, Error>;
    /// The call's result from its last write's output; an error fails the call, as a refused
    /// write does.
    fn render(output: &[u8]) -> Result<Value, Error> {
        acknowledged(output)
    }
    fn show(_ctx: &ExecCtx, _seat: &Seat, _input: &str) -> Result<Show, Error> {
        Ok(Show::Wait)
    }
    fn void(_ctx: &ExecCtx, _seat: &Seat) -> Result<Undo, Error> {
        Ok(Undo::Stays)
    }
    // what one event of the call's stream says once it says the stream's end; a heartbeat says
    // nothing
    fn heard(_data: &[u8]) -> Result<Option<Value>, Error> {
        Ok(None)
    }
}

pub(crate) struct Entry {
    pub name: &'static str,
    pub description: &'static str,
    pub writes: bool,
    pub schema: fn() -> Value,
    pub check: fn(&Value) -> Result<(), String>,
    pub run: fn(&ExecCtx, &Seat, Value) -> Result<Run, Error>,
    pub render: fn(&[u8]) -> Result<Value, Error>,
    pub show: fn(&ExecCtx, &Seat, &str) -> Result<Show, Error>,
    pub void: fn(&ExecCtx, &Seat) -> Result<Undo, Error>,
    pub heard: fn(&[u8]) -> Result<Option<Value>, Error>,
}

fn check<T: Tool>(input: &Value) -> Result<(), String> {
    serde_json::from_value::<T::Args>(input.clone())
        .map(drop)
        .map_err(|error| format!("{} input: {error}", T::NAME))
}

fn run<T: Tool>(ctx: &ExecCtx, seat: &Seat, input: Value) -> Result<Run, Error> {
    let args = serde_json::from_value(input).map_err(|error| invalid(error.to_string()))?;
    T::run(ctx, seat, args)
}

const fn entry<T: Tool>() -> Entry {
    Entry {
        name: T::NAME,
        description: T::DESCRIPTION,
        writes: T::WRITES,
        schema: T::schema,
        check: check::<T>,
        run: run::<T>,
        render: T::render,
        show: T::show,
        void: T::void,
        heard: T::heard,
    }
}

pub(crate) const CATALOG: &[Entry] = &[
    entry::<files::List>(),
    entry::<files::Read>(),
    entry::<files::Find>(),
    entry::<files::Grep>(),
    entry::<files::History>(),
    entry::<files::Write>(),
    entry::<files::Edit>(),
    entry::<files::SetList>(),
    entry::<files::Remove>(),
    entry::<files::Move>(),
    entry::<image::View>(),
    entry::<chat::Channels>(),
    entry::<chat::Messages>(),
    entry::<chat::Thread>(),
    entry::<chat::Search>(),
    entry::<chat::Post>(),
    entry::<chat::React>(),
    entry::<forge::Repos>(),
    entry::<forge::Log>(),
    entry::<forge::Tree>(),
    entry::<forge::Read>(),
    entry::<forge::Find>(),
    entry::<forge::Grep>(),
    entry::<forge::Changes>(),
    entry::<forge::Change>(),
    entry::<forge::Commit>(),
    entry::<forge::ChangeOpen>(),
    entry::<forge::ChangeComment>(),
    entry::<forge::ChangeClose>(),
    entry::<network::Accounts>(),
    entry::<network::Account>(),
    entry::<network::Activity>(),
    entry::<interface::Schema>(),
    entry::<interface::Query>(),
    entry::<interface::Call>(),
    entry::<blob::Put>(),
    entry::<pods::Exec>(),
    entry::<pods::Read>(),
    entry::<pods::Push>(),
    entry::<thread::Search>(),
    entry::<thread::Read>(),
    entry::<thread::Topic>(),
    entry::<schedule::Book>(),
    entry::<schedule::List>(),
    entry::<schedule::Cancel>(),
    entry::<wait::For>(),
    entry::<wait::List>(),
    entry::<wait::Cancel>(),
];

pub(crate) fn find(name: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|entry| entry.name == name)
}

// every tool `agent` gets and where it is read from, the shared tool files its own take the place
// of, and why each tool file left out does not load
pub(crate) fn of_agent(ctx: &QueryCtx, agent: AccountNumber) -> Result<crate::AgentTools, Error> {
    let profile = crate::agents::profile(ctx, agent)?
        .ok_or_else(|| ::program::not_found(format!("no account {agent}")))?;
    let loaded = file::load(ctx, &profile.name);
    let native = CATALOG.iter().map(|entry| crate::AgentTool {
        name: entry.name.into(),
        source: crate::ToolSource::Native,
    });
    let written = loaded.tools.iter().map(|tool| crate::AgentTool {
        name: tool.name.clone(),
        source: crate::ToolSource::File {
            path: tool.path.clone(),
            snapshot: tool.snapshot,
        },
    });
    Ok(crate::AgentTools {
        tools: native.chain(written).collect(),
        shadowed: loaded.shadowed,
        problems: loaded.problems,
    })
}

pub(crate) enum Resolved {
    Native(&'static Entry),
    File(Box<file::FileTool>),
}

// the tool `name` is for `agent`: harness's own, else the agent's tool file, else the shared one;
// a tool file that does not load says why
pub(crate) fn resolve(
    ctx: &QueryCtx,
    agent: AccountNumber,
    name: &str,
) -> Result<Option<Resolved>, String> {
    if let Some(entry) = find(name) {
        return Ok(Some(Resolved::Native(entry)));
    }
    let profile =
        crate::agents::profile(ctx, agent).map_err(|error| crate::calls::refusal(&error))?;
    let Some(profile) = profile else {
        return Ok(None);
    };
    let tool = file::resolve(ctx, &profile.name, name)?;
    Ok(tool.map(|tool| Resolved::File(Box::new(tool))))
}

// a call's input with each field its tool's schema gives no type read as the JSON it holds
pub(crate) fn untyped_read(tool: &str, input: &mut Value) {
    let Some(entry) = find(tool) else {
        return;
    };
    let schema = (entry.schema)();
    let (Some(properties), Some(fields)) =
        (schema["properties"].as_object(), input.as_object_mut())
    else {
        return;
    };
    let typed = ["type", "enum", "const", "anyOf", "oneOf", "allOf", "$ref"];
    for (name, field) in properties {
        if typed.iter().any(|key| field.get(key).is_some()) {
            continue;
        }
        if let Some(held) = fields.get(name).and_then(unquoted) {
            fields.insert(name.clone(), held);
        }
    }
}

pub(crate) fn infos() -> Vec<ToolInfo> {
    CATALOG
        .iter()
        .map(|entry| ToolInfo {
            name: entry.name.into(),
            description: entry.description.into(),
            schema: (entry.schema)().to_string(),
            writes: entry.writes,
        })
        .collect()
}

// a write's output that says nothing more than that it applied
pub(crate) fn acknowledged(_output: &[u8]) -> Result<Value, Error> {
    Ok(json!({ "ok": true }))
}

pub(crate) fn object(properties: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": properties, "required": required })
}

pub(crate) fn cursor_schema() -> Value {
    json!({ "type": "string", "description": "the `next` of the previous page, to read the page after it" })
}

pub(crate) fn page(cursor: &Option<String>) -> Result<PageRequest, Error> {
    match cursor {
        None => Ok(PageRequest::default()),
        Some(cursor) => {
            let after = design::unhex(cursor)
                .ok_or_else(|| invalid("a cursor is the hex `next` of a page"))?;
            Ok(PageRequest {
                after: Some(after),
                ..PageRequest::default()
            })
        }
    }
}

pub(crate) fn next(next: &Option<Vec<u8>>) -> Value {
    next.as_deref().map(abi::hex).into()
}

pub(crate) fn principal(principal: &Principal) -> Value {
    match principal {
        Principal::Account(number) => json!(format!("account {number}")),
        Principal::Root => json!("the network"),
    }
}

pub(crate) fn blob(id: &BlobId) -> Value {
    json!(abi::hex(id.digest()))
}

pub(crate) fn unexpected(program: &str, asked: &str, reply: &impl std::fmt::Debug) -> Error {
    unexpected_reply(program, asked, reply)
}

#[cfg(test)]
mod tests;
