//! Tools written as files: a TOML file in an agent's `/home/<name>/tools/` or in `/shared/tools/`
//! names its input and the queries, ops and egress service calls it sends, `$` naming the input
//! and earlier steps' results; checked against the programs' live schemas each time it's read.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{
    BlobId, Error, ExecCtx, HashKind, QueryCtx, code, invalid, not_found, unexpected_reply,
};
use abi::role::egress::{self, http};
use abi::role::identity::AccountNumber;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use store::PageRequest;

use super::{Run, Seat, Write, object};
use crate::PART;
use crate::agents::read;
use crate::calls::refusal;
use crate::pictures::{self, Format, Picture};
use crate::schema::{Interface, schema_of};

pub(crate) const SHARED: &str = "/shared/tools";

pub(crate) fn home(agent: &str) -> String {
    format!("/home/{agent}/tools")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Written {
    name: String,
    description: String,
    #[serde(default)]
    input: BTreeMap<String, Field>,
    #[serde(default)]
    step: Vec<Step>,
    script: Option<String>,
    returns: Option<Returns>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Field {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    optional: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    query: Option<String>,
    op: Option<String>,
    service: Option<String>,
    method: Option<String>,
    target: Option<String>,
    stream: Option<bool>,
    body: Option<Value>,
    #[serde(rename = "as")]
    bind: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Returns {
    text: String,
}

const TYPES: [&str; 6] = ["string", "integer", "number", "boolean", "object", "array"];

impl Field {
    fn fits(&self, value: &Value) -> bool {
        match self.kind.as_str() {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "object" => value.is_object(),
            _ => value.is_array(),
        }
    }
}

#[derive(Clone)]
enum Sends {
    Query(Interface),
    Op(Interface),
    Service {
        method: http::Method,
        target: String,
        stream: bool,
    },
}

struct Checked {
    sends: Sends,
    program: String,
    body: Option<Value>,
    bind: Option<String>,
}

impl Checked {
    fn label(&self, n: usize) -> String {
        let sends = match &self.sends {
            Sends::Query(_) => "query",
            Sends::Op(_) => "op",
            Sends::Service { .. } => "service",
        };
        format!("step {n} ({sends} {})", self.program)
    }

    fn streams(&self) -> bool {
        matches!(self.sends, Sends::Service { stream: true, .. })
    }
}

pub(crate) struct FileTool {
    pub name: String,
    pub description: String,
    pub path: String,
    pub snapshot: BlobId,
    input: BTreeMap<String, Field>,
    steps: Vec<Checked>,
    returns: Option<String>,
}

impl FileTool {
    pub(crate) fn schema(&self) -> Value {
        let properties: Map<String, Value> = self
            .input
            .iter()
            .map(|(name, field)| {
                let property = json!({ "type": field.kind, "description": field.description });
                (name.clone(), property)
            })
            .collect();
        let required: Vec<&str> = self
            .input
            .iter()
            .filter(|(_, field)| !field.optional)
            .map(|(name, _)| name.as_str())
            .collect();
        object(Value::Object(properties), &required)
    }

    // the call's input against `[input]`, every field it gets wrong named at once
    pub(crate) fn check(&self, input: &Value) -> Result<(), String> {
        let name = &self.name;
        let Some(fields) = input.as_object() else {
            return Err(format!("{name} input: expected an object of its fields"));
        };
        let unknown = fields
            .keys()
            .filter(|field| !self.input.contains_key(*field))
            .map(|field| format!("there is no field {field}"));
        let wrong = self
            .input
            .iter()
            .filter_map(|(field, wanted)| match fields.get(field) {
                None if wanted.optional => None,
                None => Some(format!("field {field} is missing")),
                Some(value) if !wanted.fits(value) => {
                    Some(format!("field {field} is no {}", wanted.kind))
                }
                Some(_) => None,
            });
        let problems: Vec<String> = unknown.chain(wrong).collect();
        match problems.is_empty() {
            true => Ok(()),
            false => Err(format!("{name} input: {}", problems.join("; "))),
        }
    }
}

// what a reference is: a run of text, or a name and the keys and indexes below it
#[derive(Debug, PartialEq)]
enum Piece<'t> {
    Text(String),
    Ref(Vec<&'t str>),
}

fn ident(name: &str) -> bool {
    let mut chars = name.chars();
    let first = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    first && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn pieces(text: &str) -> Result<Vec<Piece<'_>>, String> {
    let mut pieces = Vec::new();
    let mut plain = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('$') {
        plain.push_str(&rest[..at]);
        rest = &rest[at + 1..];
        if let Some(after) = rest.strip_prefix('$') {
            plain.push('$');
            rest = after;
            continue;
        }
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
            .unwrap_or(rest.len());
        let path = rest[..end].trim_end_matches('.');
        let segments: Vec<&str> = path.split('.').collect();
        let named = ident(segments[0]) && segments.iter().all(|segment| !segment.is_empty());
        if !named {
            return Err(format!(
                "`{text}`: a $ starts no name here; write $$ for a dollar sign"
            ));
        }
        if !plain.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut plain)));
        }
        pieces.push(Piece::Ref(segments));
        rest = &rest[path.len()..];
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        pieces.push(Piece::Text(plain));
    }
    Ok(pieces)
}

// every reference in `value`'s strings names an input or an earlier step
fn references(value: &Value, known: &BTreeSet<String>) -> Result<(), String> {
    match value {
        Value::String(text) => {
            for piece in pieces(text)? {
                if let Piece::Ref(path) = piece
                    && !known.contains(path[0])
                {
                    return Err(format!(
                        "${} names no input and no earlier step's `as`",
                        path.join(".")
                    ));
                }
            }
            Ok(())
        }
        Value::Array(items) => items.iter().try_for_each(|item| references(item, known)),
        Value::Object(fields) => fields
            .values()
            .try_for_each(|field| references(field, known)),
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(()),
    }
}

// a string with a `$` in it is filled in only when the tool runs: the schema takes it for any type
fn hole(value: &Value) -> bool {
    value.as_str().is_some_and(|text| text.contains('$'))
}

fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn lookup<'b>(path: &[&str], bound: &'b Map<String, Value>) -> Result<&'b Value, String> {
    let mut value = bound
        .get(path[0])
        .ok_or_else(|| format!("${} is not set", path[0]))?;
    for (depth, segment) in path.iter().enumerate().skip(1) {
        let below = match value {
            Value::Object(fields) => fields.get(*segment),
            Value::Array(items) => segment.parse::<usize>().ok().and_then(|at| items.get(at)),
            _ => None,
        };
        value = below.ok_or_else(|| {
            format!(
                "${} has no {segment}: it is {}",
                path[..depth].join("."),
                kind(value)
            )
        })?;
    }
    Ok(value)
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

// a string of the file filled in: a reference alone is its value, kept as the type it is, and
// references inside other text are written into it
fn said(text: &str, bound: &Map<String, Value>) -> Result<Value, String> {
    let pieces = pieces(text)?;
    if let [Piece::Ref(path)] = pieces.as_slice() {
        return lookup(path, bound).cloned();
    }
    Ok(Value::String(written(&pieces, bound)?))
}

fn written(pieces: &[Piece], bound: &Map<String, Value>) -> Result<String, String> {
    let mut text = String::new();
    for piece in pieces {
        match piece {
            Piece::Text(plain) => text.push_str(plain),
            Piece::Ref(path) => text.push_str(&text_of(lookup(path, bound)?)),
        }
    }
    Ok(text)
}

fn encoded(text: &str, kept: &[u8]) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) || kept.contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn targeted(pieces: &[Piece], bound: &Map<String, Value>) -> Result<String, String> {
    let mut target = String::new();
    let mut query = false;
    for piece in pieces {
        match piece {
            Piece::Text(plain) => {
                target.push_str(plain);
                query |= plain.contains('?');
            }
            Piece::Ref(path) => {
                let kept: &[u8] = if query { b"" } else { b"/" };
                target.push_str(&encoded(&text_of(lookup(path, bound)?), kept));
            }
        }
    }
    Ok(target)
}

fn dropped(value: &Value, absent: &BTreeSet<String>) -> bool {
    let Some(text) = value.as_str() else {
        return false;
    };
    matches!(
        pieces(text).as_deref(),
        Ok([Piece::Ref(path)]) if path.len() == 1 && absent.contains(path[0])
    )
}

fn filled(
    value: &Value,
    bound: &Map<String, Value>,
    absent: &BTreeSet<String>,
) -> Result<Value, String> {
    match value {
        Value::String(text) => said(text, bound),
        Value::Array(items) => items
            .iter()
            .map(|item| filled(item, bound, absent))
            .collect::<Result<_, _>>()
            .map(Value::Array),
        Value::Object(fields) => fields
            .iter()
            .filter(|(_, field)| !dropped(field, absent))
            .map(|(key, field)| Ok((key.clone(), filled(field, bound, absent)?)))
            .collect::<Result<Map<_, _>, String>>()
            .map(Value::Object),
        Value::Null | Value::Bool(_) | Value::Number(_) => Ok(value.clone()),
    }
}

// each program's interface read once for one load
#[derive(Default)]
struct Schemas(BTreeMap<String, Result<Interface, String>>);

impl Schemas {
    fn of(&mut self, ctx: &QueryCtx, program: &str) -> Result<Interface, String> {
        self.0
            .entry(program.to_owned())
            .or_insert_with(|| schema_of(ctx, program).map_err(|error| refusal(&error)))
            .clone()
    }
}

// a tool file's name is its file's, fits the model API's rule for tool names
// (^[a-zA-Z0-9_-]{1,64}$, Anthropic's tool-use docs), and is none of harness's own
fn named(stem: &str, name: &str) -> Result<(), String> {
    if name != stem {
        return Err(format!(
            "its name is {name}, and a tool file is named as its file is: {stem}"
        ));
    }
    let fits = (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if !fits {
        return Err("a tool's name is 1 to 64 letters, digits, _ and -".to_owned());
    }
    if super::find(name).is_some() {
        return Err(format!(
            "{name} is harness's own tool, which a file does not take the place of"
        ));
    }
    Ok(())
}

fn parsed(text: &str) -> Result<Written, String> {
    toml::from_str(text).map_err(|error| {
        let line = error
            .span()
            .map(|span| text[..span.start.min(text.len())].lines().count().max(1));
        match line {
            Some(line) => format!("line {line}: {}", error.message()),
            None => error.message().to_owned(),
        }
    })
}

fn checked(
    ctx: &QueryCtx,
    stem: &str,
    path: &str,
    at: Option<BlobId>,
    schemas: &mut Schemas,
) -> Result<FileTool, String> {
    let (snapshot, bytes) = read(ctx, path, at).map_err(|error| refusal(&error))?;
    let text = String::from_utf8(bytes).map_err(|_| "the file is no UTF-8 text".to_owned())?;
    let written = parsed(&text)?;
    named(stem, &written.name)?;
    for (field, wanted) in &written.input {
        if !ident(field) {
            return Err(format!(
                "input {field}: an input's name is a letter or _, then letters, digits and _"
            ));
        }
        if !TYPES.contains(&wanted.kind.as_str()) {
            return Err(format!(
                "input {field}: type {} is none of {}",
                wanted.kind,
                TYPES.join(", ")
            ));
        }
    }
    if written.script.is_some() {
        return Err("a tool file holds steps only: there is no script body".to_owned());
    }
    if written.step.is_empty() {
        return Err("it has no [[step]]".to_owned());
    }
    let last = written.step.len();
    let mut known: BTreeSet<String> = written.input.keys().cloned().collect();
    let mut steps = Vec::new();
    for (index, step) in written.step.into_iter().enumerate() {
        let n = index + 1;
        steps.push(step_checked(ctx, n, n == last, step, &mut known, schemas)?);
    }
    if let Some(returns) = &written.returns {
        if steps.last().is_some_and(Checked::streams) {
            return Err(
                "its last step streams, and a stream's end is the call's result: there is no [returns]"
                    .to_owned(),
            );
        }
        references(&Value::String(returns.text.clone()), &known)
            .map_err(|why| format!("returns: {why}"))?;
    }
    Ok(FileTool {
        name: written.name,
        description: written.description,
        path: path.to_owned(),
        snapshot,
        input: written.input,
        steps,
        returns: written.returns.map(|returns| returns.text),
    })
}

fn service_sends(
    n: usize,
    last: bool,
    service: &str,
    method: Option<String>,
    target: Option<String>,
    stream: Option<bool>,
) -> Result<Sends, String> {
    let reserved = egress::RESERVED.contains(&service);
    if !crate::providers::lowercase_name(service) || reserved {
        return Err(format!(
            "step {n}: {service} is no egress service's name: a registered service, lowercase letters, digits, -, _ and ., and not http or relay"
        ));
    }
    let method = match method.as_deref().unwrap_or("POST") {
        "GET" => http::Method::Get,
        "POST" => http::Method::Post,
        "PUT" => http::Method::Put,
        "PATCH" => http::Method::Patch,
        "DELETE" => http::Method::Delete,
        other => {
            return Err(format!(
                "step {n}: method {other} is none of GET, POST, PUT, PATCH and DELETE"
            ));
        }
    };
    let Some(target) = target.filter(|target| target.starts_with('/')) else {
        return Err(format!(
            "step {n}: a service step names its target, a path on the service like /v1/run"
        ));
    };
    let stream = stream.unwrap_or(false);
    if stream && !last {
        return Err(format!(
            "step {n}: only the last step streams, since a stream's end is the call's result"
        ));
    }
    Ok(Sends::Service {
        method,
        target,
        stream,
    })
}

fn step_checked(
    ctx: &QueryCtx,
    n: usize,
    last: bool,
    step: Step,
    known: &mut BTreeSet<String>,
    schemas: &mut Schemas,
) -> Result<Checked, String> {
    let served = step.method.is_some() || step.target.is_some() || step.stream.is_some();
    let program_step = step.query.is_some() || step.op.is_some();
    if program_step && served {
        return Err(format!(
            "step {n}: method, target and stream belong to a service step"
        ));
    }
    if program_step && step.body.is_none() {
        return Err(format!("step {n}: a query or op step has a body"));
    }
    let interface = |program: &str, schemas: &mut Schemas| {
        schemas
            .of(ctx, program)
            .map_err(|why| format!("step {n}: {why}"))
    };
    let (sends, program) = match (step.query, step.op, step.service) {
        (Some(program), None, None) => (Sends::Query(interface(&program, schemas)?), program),
        (None, Some(program), None) => (Sends::Op(interface(&program, schemas)?), program),
        (None, None, Some(service)) => {
            let sends = service_sends(n, last, &service, step.method, step.target, step.stream)?;
            (sends, service)
        }
        _ => {
            return Err(format!(
                "step {n}: a step names one program or service, as query = \"<program>\", op = \"<program>\" or service = \"<egress service>\""
            ));
        }
    };
    let checked = Checked {
        sends,
        program,
        body: step.body,
        bind: step.bind,
    };
    let label = checked.label(n);
    if let Some(body) = &checked.body {
        references(body, known).map_err(|why| format!("{label}: {why}"))?;
    }
    if let Sends::Service { target, .. } = &checked.sends {
        references(&Value::String(target.clone()), known)
            .map_err(|why| format!("{label}: {why}"))?;
    }
    let fits = match (&checked.sends, &checked.body) {
        (Sends::Query(interface), Some(body)) => interface
            .query_fits(body, hole)
            .map_err(|fault| fault.to_string()),
        (Sends::Op(interface), Some(body)) => interface
            .op_fits(body, hole)
            .map_err(|fault| fault.to_string()),
        _ => Ok(()),
    };
    fits.map_err(|fault| format!("{label}: {fault}"))?;
    if let Some(bind) = &checked.bind {
        if checked.streams() {
            return Err(format!(
                "{label}: a streamed step's end is the call's result, so it has no `as`"
            ));
        }
        if !ident(bind) {
            return Err(format!(
                "{label}: `as` is a letter or _, then letters, digits and _"
            ));
        }
        if !known.insert(bind.clone()) {
            return Err(format!(
                "{label}: as = \"{bind}\" is already an input's or an earlier step's name"
            ));
        }
    }
    Ok(checked)
}

fn stem(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    name.strip_suffix(".toml").filter(|stem| !stem.is_empty())
}

// the `*.toml` files directly in `folder`, read at `at`, which the first listing sets
fn listed(
    ctx: &QueryCtx,
    folder: &str,
    at: &mut Option<BlobId>,
) -> Result<Vec<(String, String)>, Error> {
    let mut found = Vec::new();
    let mut after = None;
    loop {
        let asked = files::Query::List {
            path: folder.to_owned(),
            at: *at,
            page: PageRequest { after, limit: None },
        };
        let (snapshot, entries) = match ctx.query(files::PROGRAM, &asked) {
            Ok(files::Reply::Listing { snapshot, entries }) => (snapshot, entries),
            Ok(other) => return Err(unexpected_reply(files::PROGRAM, "List", &other)),
            Err(error) if error.code == code::NOT_FOUND => return Ok(found),
            Err(error) => return Err(error),
        };
        *at = at.or(snapshot);
        for entry in entries.items {
            let file = matches!(entry.node, files::Node::File { .. });
            if let Some(stem) = stem(&entry.path).filter(|_| file) {
                found.push((stem.to_owned(), entry.path.clone()));
            }
        }
        match entries.next {
            Some(next) => after = Some(next),
            None => return Ok(found),
        }
    }
}

#[derive(Default)]
pub(crate) struct Loaded {
    pub tools: Vec<FileTool>,
    pub shadowed: Vec<String>,
    pub problems: Vec<String>,
}

// every tool file `agent` gets, its own first: a file of its own takes the place of a shared one
// of the same name
pub(crate) fn load(ctx: &QueryCtx, agent: &str) -> Loaded {
    let mut loaded = Loaded::default();
    let mut schemas = Schemas::default();
    let mut at = None;
    let mut seen = BTreeSet::new();
    for folder in [home(agent), SHARED.to_owned()] {
        let found = match listed(ctx, &folder, &mut at) {
            Ok(found) => found,
            Err(error) => {
                let problem = format!("{folder}: {}", refusal(&error));
                loaded.problems.push(problem);
                continue;
            }
        };
        for (stem, path) in found {
            if !seen.insert(stem.clone()) {
                loaded.shadowed.push(path);
                continue;
            }
            match checked(ctx, &stem, &path, at, &mut schemas) {
                Ok(tool) => loaded.tools.push(tool),
                Err(problem) => loaded.problems.push(format!("tool {stem}: {problem}")),
            }
        }
    }
    loaded.tools.sort_by(|a, b| a.name.cmp(&b.name));
    loaded
}

// the system prompt's line on the tool files left out of the tool list, and why
pub(crate) fn left_out(problems: &[String]) -> Option<String> {
    (!problems.is_empty()).then(|| format!("- File tools left out: {}", problems.join("; ")))
}

// the file tool `name` is for `agent`, as its file reads now; a file that no longer loads says why
pub(crate) fn resolve(ctx: &QueryCtx, agent: &str, name: &str) -> Result<Option<FileTool>, String> {
    if named(name, name).is_err() {
        return Ok(None);
    }
    let mut schemas = Schemas::default();
    for folder in [home(agent), SHARED.to_owned()] {
        let path = format!("{folder}/{name}.toml");
        let asked = files::Query::Stat {
            path: path.clone(),
            at: None,
        };
        let (snapshot, entry) = match ctx.query(files::PROGRAM, &asked) {
            Ok(files::Reply::Stat { snapshot, entry }) => (snapshot, entry),
            Ok(other) => {
                return Err(refusal(&unexpected_reply(files::PROGRAM, "Stat", &other)));
            }
            Err(error) => return Err(refusal(&error)),
        };
        let file = entry.is_some_and(|entry| matches!(entry.node, files::Node::File { .. }));
        if file {
            return checked(ctx, name, &path, snapshot, &mut schemas)
                .map(Some)
                .map_err(|problem| format!("tool {name}: {problem}"));
        }
    }
    Ok(None)
}

struct Walk {
    tool: FileTool,
    next: usize,
    bound: Map<String, Value>,
    absent: BTreeSet<String>,
    thread: u64,
    call: String,
    agent: AccountNumber,
}

// the tool's steps from the first, as the agent: a query answers in place, an op is a write
// whose output the next steps read
pub(crate) fn run(ctx: &ExecCtx, seat: &Seat, tool: FileTool, input: Value) -> Result<Run, Error> {
    let Value::Object(bound) = input else {
        return Err(invalid(format!(
            "{} input: expected an object of its fields",
            tool.name
        )));
    };
    let absent = tool
        .input
        .iter()
        .filter(|(name, field)| field.optional && !bound.contains_key(name.as_str()))
        .map(|(name, _)| name.clone())
        .collect();
    walk(
        ctx,
        Walk {
            tool,
            next: 0,
            bound,
            absent,
            thread: seat.thread,
            call: seat.call.to_owned(),
            agent: seat.agent,
        },
    )
}

fn labelled(label: &str, error: Error) -> Error {
    Error::new(error.code, format!("{label}: {}", error.message))
}

fn headed(method: http::Method, target: String, body: bool) -> http::Head {
    let headers = match body {
        true => vec![("content-type".to_owned(), "application/json".to_owned())],
        false => Vec::new(),
    };
    http::Head {
        method,
        target,
        headers,
    }
}

fn walk(ctx: &ExecCtx, mut walk: Walk) -> Result<Run, Error> {
    while let Some(step) = walk.tool.steps.get(walk.next) {
        let index = walk.next;
        walk.next += 1;
        let label = step.label(walk.next);
        let body = match &step.body {
            Some(body) => Some(
                filled(body, &walk.bound, &walk.absent)
                    .map_err(|why| invalid(format!("{label}: {why}")))?,
            ),
            None => None,
        };
        match &step.sends {
            Sends::Query(interface) => {
                let payload = interface
                    .encode_query(&body.unwrap_or_default())
                    .map_err(|fault| invalid(format!("{label}: {fault}")))?;
                let answer = ctx
                    .query_raw(step.program.clone(), payload)
                    .map_err(|error| labelled(&label, error))?;
                let reply = interface.decode_reply(&answer).map_err(|fault| {
                    let why = format!("{label}: the reply does not read: {fault}");
                    Error::new(code::CORRUPT, why)
                })?;
                let name = step.bind.clone();
                bind(&mut walk.bound, name, reply);
            }
            Sends::Op(interface) => {
                let payload = interface
                    .encode_op(&body.unwrap_or_default())
                    .map_err(|fault| invalid(format!("{label}: {fault}")))?;
                let write = Write {
                    target: step.program.clone(),
                    payload: payload.clone(),
                };
                let interface = interface.clone();
                let then = move |ctx: &ExecCtx, output: &[u8]| {
                    let step = &walk.tool.steps[index];
                    let answer = interface.returned(&step.program, &payload, output);
                    let value = answer.get("returned").cloned().unwrap_or(Value::Null);
                    bind(&mut walk.bound, step.bind.clone(), value);
                    self::walk(ctx, walk)
                };
                return Ok(Run::Step {
                    step: label,
                    writes: vec![write],
                    then: Box::new(then),
                });
            }
            Sends::Service {
                method,
                target,
                stream,
            } => {
                let target = pieces(target)
                    .and_then(|pieces| targeted(&pieces, &walk.bound))
                    .map_err(|why| invalid(format!("{label}: {why}")))?;
                let service = step.program.clone();
                let about = format!("{service} {target}");
                let head = headed(*method, target, body.is_some());
                if *stream {
                    let request = egress::Request::Http {
                        service,
                        head,
                        body: Vec::new(),
                    };
                    let body = body.map(|body| body.to_string());
                    return Ok(Run::Paced {
                        request,
                        body,
                        about,
                    });
                }
                let parts = match &body {
                    Some(body) => {
                        let text = body.to_string().into_bytes();
                        let held = super::held_for(walk.thread, &walk.call);
                        vec![ctx.blob_put_on(HashKind::Sha256, PART, text, held)?]
                    }
                    None => Vec::new(),
                };
                let request = egress::Request::Http {
                    service: service.clone(),
                    head,
                    body: parts,
                };
                let writes = super::pods::whole(ctx, request)?;
                let step = label.clone();
                let then = move |ctx: &ExecCtx, output: &[u8]| {
                    let answer =
                        served(&service, output).map_err(|error| labelled(&label, error))?;
                    heard_back(ctx, walk, index, label, answer)
                };
                return Ok(Run::Step {
                    step,
                    writes,
                    then: Box::new(then),
                });
            }
        }
    }
    Ok(Run::Done(finished(&walk)))
}

enum Served {
    Said(Value),
    Image(Vec<u8>, Format),
}

fn served(service: &str, output: &[u8]) -> Result<Served, Error> {
    let response: http::Response = abi::decode(output).map_err(|fault| {
        let why = format!("the answer is no HTTP response: {}", fault.sentence);
        Error::new(code::UNEXPECTED_REPLY, why)
    })?;
    let media = crate::resume::header(&response, "content-type")
        .map(|value| value.split(';').next().unwrap_or_default().trim())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let json = serde_json::from_slice::<Value>(&response.body).ok();
    let status = response.status;
    if (200..300).contains(&status) {
        if !media.starts_with("image/") {
            let text = || json!(String::from_utf8_lossy(&response.body));
            return Ok(Served::Said(json.unwrap_or_else(text)));
        }
        return match pictures::format(&response.body) {
            Some(format) => Ok(Served::Image(response.body, format)),
            None => Err(invalid(format!(
                "{service} answered {media}, and its bytes are no png, jpeg, gif or webp image"
            ))),
        };
    }
    let why = match json {
        Some(body) => match body.get("error").and_then(Value::as_str) {
            Some(error) => format!(": {error}"),
            None => format!(": {body}"),
        },
        None if response.body.is_empty() => " with no body".to_owned(),
        None if media.is_empty() => " with a body of no media type, not JSON".to_owned(),
        None => format!(" with a {media} body, not JSON"),
    };
    Err(Error::new(
        super::pods::refused_as(status),
        format!("{service} answered {status}{why}"),
    ))
}

fn heard_back(
    ctx: &ExecCtx,
    mut walk: Walk,
    index: usize,
    label: String,
    answer: Served,
) -> Result<Run, Error> {
    let name = walk.tool.steps[index].bind.clone();
    let (bytes, format) = match answer {
        Served::Said(value) => {
            bind(&mut walk.bound, name, value);
            return self::walk(ctx, walk);
        }
        Served::Image(bytes, format) => (bytes, format),
    };
    let profile = crate::agents::profile(ctx, walk.agent)?
        .ok_or_else(|| not_found(format!("no account {}", walk.agent)))?;
    let file = format!(
        "{}-{}-{}.{}",
        walk.thread,
        walk.call,
        index + 1,
        format.extension()
    );
    let seen = Seen {
        path: format!("/home/{}/seen/{file}", profile.name),
        file,
        format,
        size: bytes.len() as u64,
        side: pictures::dimensions(&bytes, format),
    };
    let head = super::files::resolved(ctx, None)?;
    let held = match head.as_ref() {
        Some(at) => held_at(ctx, &seen.path, at)?,
        None => None,
    };
    if let (Some(at), Some(held)) = (head, &held)
        && *held == bytes
    {
        return shown(ctx, walk, name, seen, at);
    }
    let base = head.filter(|_| held.is_some());
    let put = files::Change::Put {
        path: seen.path.clone(),
        exec: false,
        meta: BTreeMap::new(),
        content: super::files::chunked(ctx, &bytes)?,
    };
    let message = format!(
        "harness thread {} call {}: what {} answered",
        walk.thread, walk.call, walk.tool.steps[index].program
    );
    let commit = files::Op::Commit {
        base,
        message,
        changes: vec![put],
    };
    let write = Write {
        target: files::PROGRAM.into(),
        payload: abi::encode(&commit),
    };
    let then = move |ctx: &ExecCtx, output: &[u8]| {
        let snapshot = abi::decode::<BlobId>(output).map_err(|fault| {
            let why = format!(
                "files named no snapshot for {}: {}",
                seen.path, fault.sentence
            );
            Error::new(code::UNEXPECTED_REPLY, why)
        })?;
        shown(ctx, walk, name, seen, snapshot)
    };
    Ok(Run::Step {
        step: label,
        writes: vec![write],
        then: Box::new(then),
    })
}

struct Seen {
    path: String,
    file: String,
    format: Format,
    size: u64,
    side: Option<(u32, u32)>,
}

fn held_at(ctx: &ExecCtx, path: &str, at: &BlobId) -> Result<Option<Vec<u8>>, Error> {
    let file = super::files::stat(ctx, path, Some(at))?
        .is_some_and(|entry| matches!(entry.node, files::Node::File { .. }));
    match file {
        true => super::files::read_all(ctx, path, Some(at)).map(Some),
        false => Ok(None),
    }
}

fn shown(
    ctx: &ExecCtx,
    mut walk: Walk,
    name: Option<String>,
    seen: Seen,
    snapshot: BlobId,
) -> Result<Run, Error> {
    let mut said = json!({
        "path": seen.path,
        "snapshot": super::blob(&snapshot),
        "media_type": seen.format.media(),
        "size": seen.size,
    });
    if let Some((width, height)) = seen.side {
        said["width"] = json!(width);
        said["height"] = json!(height);
    }
    let picture = Picture {
        name: seen.file,
        path: seen.path,
        snapshot,
    };
    pictures::added(ctx, walk.thread, &walk.call, picture)?;
    bind(&mut walk.bound, name, said);
    self::walk(ctx, walk)
}

pub(crate) fn heard(data: &[u8]) -> Option<Value> {
    let (name, data) = super::pods::event(data);
    let said = serde_json::from_str(&data).unwrap_or_else(|_| json!(data));
    match name.as_str() {
        "ended" => Some(said),
        "refused" => Some(json!({ "refused": said })),
        _ => None,
    }
}

fn bind(bound: &mut Map<String, Value>, name: Option<String>, value: Value) {
    if let Some(name) = name {
        bound.insert(name, value);
    }
}

// the call's result: `[returns] text` filled in, else what each step bound; a text whose
// references fail once the steps ran says why beside what they bound
fn finished(walk: &Walk) -> Value {
    let steps = walk.tool.steps.iter().filter_map(|step| step.bind.as_ref());
    let bound: Map<String, Value> = steps
        .filter_map(|name| Some((name.clone(), walk.bound.get(name)?.clone())))
        .collect();
    let Some(text) = &walk.tool.returns else {
        let mut answer = Map::new();
        answer.insert("ok".into(), json!(true));
        answer.extend(bound);
        return Value::Object(answer);
    };
    let said = pieces(text).and_then(|pieces| written(&pieces, &walk.bound));
    match said {
        Ok(text) => json!({ "ok": true, "text": text }),
        Err(why) => json!({ "ok": true, "unsaid": format!("returns: {why}"), "steps": bound }),
    }
}

#[cfg(test)]
mod tests;
