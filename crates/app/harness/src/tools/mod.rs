//! The tools a thread's model may call, each over one program of the network: its schema, the
//! check a call's input passes, the run once its input is whole, and for a streamable tool what
//! it shows while its input arrives and how it takes that back.
mod chat;
mod files;
mod forge;
mod interface;
mod network;
pub(crate) mod prefix;
mod thread;

pub(crate) use chat::shown_by;

use ::program::{BlobId, Error, ExecCtx, Principal, ProgramId, invalid, unexpected_reply};
use abi::role::identity::AccountNumber;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use store::PageRequest;

use crate::ToolInfo;
use prefix::Prefix;

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

pub(crate) enum Run {
    Done(Value),
    Write(Vec<Write>),
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

pub(crate) trait Tool {
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    const WRITES: bool = false;
    const STREAMS: bool = false;
    type Args: DeserializeOwned;
    fn schema() -> Value;
    fn run(ctx: &ExecCtx, seat: &Seat, args: Self::Args) -> Result<Run, Error>;
    fn render(_output: &[u8]) -> Value {
        json!({ "ok": true })
    }
    fn show(_ctx: &ExecCtx, _seat: &Seat, _input: &Prefix) -> Result<Show, Error> {
        Ok(Show::Wait)
    }
    fn void(_ctx: &ExecCtx, _seat: &Seat) -> Result<Vec<Write>, Error> {
        Ok(Vec::new())
    }
}

pub(crate) struct Entry {
    pub name: &'static str,
    pub description: &'static str,
    pub writes: bool,
    pub streams: bool,
    pub schema: fn() -> Value,
    pub check: fn(&Value) -> Result<(), String>,
    pub run: fn(&ExecCtx, &Seat, Value) -> Result<Run, Error>,
    pub render: fn(&[u8]) -> Value,
    pub show: fn(&ExecCtx, &Seat, &Prefix) -> Result<Show, Error>,
    pub void: fn(&ExecCtx, &Seat) -> Result<Vec<Write>, Error>,
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
        streams: T::STREAMS,
        schema: T::schema,
        check: check::<T>,
        run: run::<T>,
        render: T::render,
        show: T::show,
        void: T::void,
    }
}

pub(crate) const CATALOG: &[Entry] = &[
    entry::<files::List>(),
    entry::<files::Read>(),
    entry::<files::Find>(),
    entry::<files::Grep>(),
    entry::<files::History>(),
    entry::<files::Write>(),
    entry::<files::SetList>(),
    entry::<files::Remove>(),
    entry::<files::Move>(),
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
    entry::<forge::Changes>(),
    entry::<forge::Change>(),
    entry::<forge::Commit>(),
    entry::<network::Accounts>(),
    entry::<network::Account>(),
    entry::<network::Activity>(),
    entry::<interface::Schema>(),
    entry::<interface::Query>(),
    entry::<interface::Call>(),
    entry::<thread::Search>(),
    entry::<thread::Read>(),
];

pub(crate) fn find(name: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|entry| entry.name == name)
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
            let after = abi::unhex(cursor)
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
