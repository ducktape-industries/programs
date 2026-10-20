//! Tools over the pods service, the egress service `pods`: a command run in a pod, answered
//! whole or streamed until it ends, a file read straight out of a pod, and a commit a pod made
//! packed there and pushed to forge as the agent.
use ::program::{BlobId, Error, ExecCtx, HashKind, code, invalid, not_found};
use abi::role::egress::{self, http};
use forge::{Query, Reply, Revision};
use serde::Deserialize;
use serde_json::{Value, json};
use store::PageRequest;

use super::forge::{branch_ref, found, moved, oid, push_request};
use super::{Run, Seat, Tool, Write, object, unexpected};
use crate::PART;

const SERVICE: &str = "pods";

// what pod_read runs: the file's size on a line of its own, then its bytes from the offset, as
// many as the length asks or all of them
const READ: &str = r#"size=$(wc -c < "$1") || exit
printf '%s\n' "$size"
if [ -n "$3" ]; then tail -c +$(($2 + 1)) -- "$1" | head -c "$3"; else tail -c +$(($2 + 1)) -- "$1"; fi"#;

fn pod_schema() -> Value {
    json!({ "type": "string", "description": "the pod's name, as POST /v1/pods answered it, like duckpond-3041" })
}

fn container_schema() -> Value {
    json!({ "type": "string", "description": "the container: run (the default), or out, which still answers after run ended and sees /out and the pod's inputs" })
}

fn named(pod: &str) -> Result<&str, Error> {
    let fits = !pod.is_empty()
        && pod
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');
    if !fits {
        return Err(invalid(format!(
            "{pod:?} is not a pod's name: letters, digits, - and ."
        )));
    }
    Ok(pod)
}

fn absolute(path: &str) -> Result<(), Error> {
    if !path.starts_with('/') {
        return Err(invalid(format!("{path:?} is not an absolute path")));
    }
    Ok(())
}

// a POST to the pods service, its JSON body put as one part
pub(crate) fn posted(
    ctx: &ExecCtx,
    target: String,
    body: &Value,
) -> Result<egress::Request, Error> {
    let part = ctx.blob_put(HashKind::Sha256, PART, body.to_string().into_bytes())?;
    let head = http::Head {
        method: http::Method::Post,
        target,
        headers: vec![("content-type".into(), "application/json".into())],
    };
    Ok(egress::Request::Http {
        service: SERVICE.into(),
        head,
        body: vec![part],
    })
}

pub(crate) fn whole(ctx: &ExecCtx, request: egress::Request) -> Result<Vec<Write>, Error> {
    let target = ctx.env().role(abi::role::EGRESS)?.clone();
    let op = egress::Op::Request {
        request,
        delivery: egress::Delivery::Whole,
        wait: None,
    };
    let payload = abi::encode(&op);
    Ok(vec![Write { target, payload }])
}

fn refused_as(status: u16) -> &'static str {
    match status {
        400 | 422 => code::INVALID_INPUT,
        403 => code::UNAUTHORIZED,
        404 => code::NOT_FOUND,
        503 => code::UNSUPPORTED,
        _ => code::WRONG_STATE,
    }
}

// the service's JSON answer, or its refusal in its own words
fn answered(output: &[u8]) -> Result<Value, Error> {
    let response: http::Response = abi::decode(output).map_err(|fault| {
        let why = format!("the answer is no HTTP response: {}", fault.sentence);
        Error::new(code::UNEXPECTED_REPLY, why)
    })?;
    let text = String::from_utf8_lossy(&response.body);
    let body = serde_json::from_str(&text).unwrap_or_else(|_| json!(text));
    if (200..300).contains(&response.status) {
        return Ok(body);
    }
    let why = match body.get("error").and_then(Value::as_str) {
        Some(error) => error.to_owned(),
        None => body.to_string(),
    };
    let status = response.status;
    Err(Error::new(
        refused_as(status),
        format!("pods answered {status}: {why}"),
    ))
}

// one event of a text/event-stream answer as the node hands it: its name and its data
fn event(data: &[u8]) -> (String, String) {
    let text = String::from_utf8_lossy(data);
    let mut name = "message".to_owned();
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            name = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("data:") {
            lines.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    (name, lines.join("\n"))
}

#[derive(Deserialize)]
pub(crate) struct ExecArgs {
    pod: String,
    command: Option<Vec<String>>,
    script: Option<String>,
    container: Option<String>,
    every: Option<u64>,
    stream: Option<bool>,
}

fn command(command: Option<Vec<String>>, script: Option<String>) -> Result<Vec<String>, Error> {
    match (command, script) {
        (Some(command), None) if !command.is_empty() => Ok(command),
        (None, Some(script)) => Ok(vec!["sh".into(), "-c".into(), script]),
        (Some(_), Some(_)) => Err(invalid("give command or script, not both")),
        _ => Err(invalid(
            "give command (an array, run as it is) or script (shell text, run with sh -c)",
        )),
    }
}

pub(crate) struct Exec;

impl Tool for Exec {
    const NAME: &'static str = "pod_exec";
    const DESCRIPTION: &'static str = "Run a command in a pod of the pods service and read its exit code, stdout and stderr (stdout_hex or stderr_hex where they are not UTF-8). Give command (an array, run as it is) or script (shell text, run with sh -c). Without every or stream the call answers when the command ends, which must be within egress's wait for one request (DEFAULT_WAIT, 64 blocks): stream a longer command. With stream true, harness picks the heartbeat itself the block after: 64 blocks of the block time it measures, less what a read of the pod takes to land, so you give no number. With every (seconds), the call answers at once that it streams, and when the command ends this conversation gets one message with its exit code, stdout and stderr, so nothing needs polling; meanwhile the pod sends a heartbeat each every seconds, and egress gives the stream up if 64 blocks pass with none, so pick every well under that (a block is about a second). An answer comes back whole only while it fits one block (BLOCK_BYTES, 16 MiB, the JSON around the output included): a larger one fails with \"the answer is larger than a block carries\", never a cut body, so write large output to a file and read it with pod_read in parts. With every, an ended event larger than a block ends the stream as cut (event_too_large), and the message says so. A refusal by the pods service (no such pod, a container that ended) fails the call, or is the message.";
    const WRITES: bool = true;
    type Args = ExecArgs;

    fn schema() -> Value {
        object(
            json!({
                "pod": pod_schema(),
                "command": { "type": "array", "items": { "type": "string" }, "description": "the command and its arguments, run as they are" },
                "script": { "type": "string", "description": "instead of command: shell text, run with sh -c" },
                "container": container_schema(),
                "every": { "type": "integer", "minimum": 1, "description": "stream it: the seconds between heartbeats while it runs; the conversation is told when it ends" },
                "stream": { "type": "boolean", "description": "stream it at a heartbeat harness measures, as every does with one you give; the conversation is told when it ends" },
            }),
            &["pod"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ExecArgs) -> Result<Run, Error> {
        let pod = named(&args.pod)?;
        let command = command(args.command, args.script)?;
        let first = command.join(" ");
        let about = format!("pod {pod}: {}", first.lines().next().unwrap_or_default());
        let mut body = json!({ "command": command });
        if let Some(container) = args.container {
            body["container"] = json!(container);
        }
        let target = format!("/v1/pods/{pod}/exec");
        let Some(every) = args.every else {
            if args.stream == Some(true) {
                let pod = pod.to_owned();
                return Ok(Run::Measured {
                    pod,
                    target,
                    body,
                    about,
                });
            }
            let writes = whole(ctx, posted(ctx, target, &body)?)?;
            return Ok(Run::Read {
                writes,
                read: Box::new(answered),
            });
        };
        if every == 0 {
            return Err(invalid("every is a whole number of seconds from 1"));
        }
        body["every"] = json!(every);
        let request = posted(ctx, target, &body)?;
        Ok(Run::Stream { request, about })
    }

    fn heard(data: &[u8]) -> Result<Option<Value>, Error> {
        let (name, data) = event(data);
        let said = serde_json::from_str(&data).unwrap_or_else(|_| json!(data));
        Ok(match name.as_str() {
            "ended" => Some(json!({ "ended": said })),
            "refused" => Some(json!({ "refused": said })),
            _ => None,
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct ReadArgs {
    pod: String,
    path: String,
    offset: Option<u64>,
    length: Option<u64>,
    container: Option<String>,
}

pub(crate) struct Read;

fn stdout(answer: &Value) -> Result<Vec<u8>, Error> {
    if let Some(text) = answer["stdout"].as_str() {
        return Ok(text.as_bytes().to_vec());
    }
    let hex = answer["stdout_hex"].as_str().unwrap_or_default();
    design::unhex(hex).ok_or_else(|| unexpected(SERVICE, "exec", answer))
}

fn contents(path: &str, offset: u64, answer: &Value) -> Result<Value, Error> {
    let exit = answer["exit_code"].as_i64();
    if exit != Some(0) {
        let stderr = answer["stderr"].as_str().unwrap_or_default().trim();
        let missing = stderr.contains("No such file");
        let class = match missing {
            true => code::NOT_FOUND,
            false => code::INVALID_INPUT,
        };
        return Err(Error::new(class, format!("{path} was not read: {stderr}")));
    }
    let out = stdout(answer)?;
    let Some(line) = out.iter().position(|byte| *byte == b'\n') else {
        return Err(unexpected(SERVICE, "exec", answer));
    };
    let size: u64 = String::from_utf8_lossy(&out[..line])
        .trim()
        .parse()
        .map_err(|_| unexpected(SERVICE, "exec", answer))?;
    let bytes = &out[line + 1..];
    let read = bytes.len() as u64;
    let mut value = json!({
        "path": path,
        "size": size,
        "offset": offset,
        "length": read,
        "eof": offset.saturating_add(read) >= size,
    });
    match std::str::from_utf8(bytes) {
        Ok(text) => value["text"] = json!(text),
        Err(_) => value["hex"] = json!(abi::hex(bytes)),
    }
    Ok(value)
}

impl Tool for Read {
    const NAME: &'static str = "pod_read";
    const DESCRIPTION: &'static str = "Read a file straight out of a pod: its size, and its bytes from offset (0 if omitted), length of them or all the rest, as text, or as hex where they are not UTF-8; eof says whether the file goes on. The answer comes back whole only while it fits one block (BLOCK_BYTES, 16 MiB, the JSON around it included, and hex doubles bytes that are not UTF-8): a larger read fails with \"the answer is larger than a block carries\", so read a large file in parts with offset and length. A file in /out or in one of the pod's inputs reads from container out too, after run ended.";
    const WRITES: bool = true;
    type Args = ReadArgs;

    fn schema() -> Value {
        object(
            json!({
                "pod": pod_schema(),
                "path": { "type": "string", "description": "an absolute path in the pod" },
                "offset": { "type": "integer", "minimum": 0, "description": "the first byte to read; 0 if omitted" },
                "length": { "type": "integer", "minimum": 1, "description": "how many bytes; the rest of the file if omitted" },
                "container": container_schema(),
            }),
            &["pod", "path"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ReadArgs) -> Result<Run, Error> {
        let pod = named(&args.pod)?;
        absolute(&args.path)?;
        let offset = args.offset.unwrap_or(0);
        let length = args.length.map(|length| length.to_string());
        let command = [
            "sh",
            "-c",
            READ,
            "pod_read",
            &args.path,
            &offset.to_string(),
            length.as_deref().unwrap_or_default(),
        ];
        let mut body = json!({ "command": command });
        if let Some(container) = args.container {
            body["container"] = json!(container);
        }
        let writes = whole(ctx, posted(ctx, format!("/v1/pods/{pod}/exec"), &body)?)?;
        let path = args.path;
        let read = move |output: &[u8]| contents(&path, offset, &answered(output)?);
        Ok(Run::Read {
            writes,
            read: Box::new(read),
        })
    }
}

#[derive(Deserialize)]
pub(crate) struct PushArgs {
    pod: String,
    path: String,
    repo: String,
    branch: String,
    #[serde(rename = "ref")]
    reference: Option<String>,
    container: Option<String>,
    #[serde(default)]
    allow_stack: bool,
}

pub(crate) struct Push;

// the commit a branch of a forge repository is at, or `None` for a branch it does not have
fn branch_head(ctx: &ExecCtx, repo: &str, branch: &[u8]) -> Result<Option<String>, Error> {
    let asked = Query::Log {
        repo: repo.into(),
        from: Revision::Ref(branch.to_vec()),
        exclude: None,
        page: PageRequest::first(1),
    };
    match ctx.query(forge::PROGRAM, &asked) {
        Ok(Reply::Log { tip, .. }) => Ok(Some(tip)),
        Ok(other) => Err(unexpected(forge::PROGRAM, "Log", &other)),
        Err(missing) if missing.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

fn blob_of(id: &str) -> Result<BlobId, Error> {
    let digest = id.strip_prefix("sha256:").and_then(design::unhex);
    let digest = digest.and_then(|digest| <[u8; 32]>::try_from(digest).ok());
    let digest = digest.ok_or_else(|| unexpected(SERVICE, "push", &id))?;
    Ok(BlobId::Sha256(digest))
}

struct Pushing {
    repo: String,
    branch: Vec<u8>,
    hash: gitcore::Hash,
    old: Option<String>,
    allow_stack: bool,
}

impl Pushing {
    // a commit lands alone: on the branch's head, or for a new branch on a commit forge holds
    fn alone(&self, commit: &str, packed: &Value) -> Result<(), Error> {
        let parents = packed["parents"].as_array();
        let (Some(parents), Some(commits)) = (parents, packed["commits"].as_u64()) else {
            return Err(unexpected(SERVICE, "push", packed));
        };
        let parent = parents
            .first()
            .and_then(Value::as_str)
            .unwrap_or("no parent");
        let branch = String::from_utf8_lossy(&self.branch);
        let asked = "allow_stack: true is for a push like that made on purpose, such as carrying someone's commits";
        if let Some(head) = self.old.as_deref()
            && parent != head
        {
            return Err(Error::new(
                code::WRONG_STATE,
                format!(
                    "{commit} is not a child of {branch}'s head {head}: its parent is {parent}. Rebase it onto {head}, or push {parent} first if it belongs there; {asked}"
                ),
            ));
        }
        if commits > 1 {
            return Err(Error::new(
                code::WRONG_STATE,
                format!(
                    "{commit} stacks on {parent}, which forge does not hold yet: the push would land {commits} commits on the new branch {branch}. Push {parent} first, or rebase {commit} onto a commit forge holds, like dev's head; {asked}"
                ),
            ));
        }
        Ok(())
    }

    // the pack the pods service answered, pushed to forge onto the branch as the agent
    fn push(self, output: &[u8]) -> Result<Run, Error> {
        let packed = answered(output)?;
        let Some(commit) = packed["commit"].as_str() else {
            return Err(unexpected(SERVICE, "push", &packed));
        };
        let shown = json!({
            "pushed": String::from_utf8_lossy(&self.branch),
            "head": commit,
            "old": self.old,
            "objects": packed["objects"],
        });
        if self.old.as_deref() == Some(commit) {
            let mut same = json!({ "ok": true, "unchanged": true });
            merged(&mut same, &shown);
            return Ok(Run::Done(same));
        }
        if !self.allow_stack {
            self.alone(commit, &packed)?;
        }
        let new = oid(self.hash, commit)?;
        let old = match &self.old {
            Some(old) => oid(self.hash, old)?,
            None => self.hash.zero(),
        };
        let pack = packed["pack"].as_str().map(blob_of).transpose()?;
        let op = forge::Op::Push {
            repo: self.repo,
            request: push_request(old, new, &self.branch),
            pack,
        };
        let writes = vec![Write {
            target: forge::PROGRAM.into(),
            payload: abi::encode(&op),
        }];
        // forge's report of the push, read as forge_commit reads it: a branch forge did not move
        // (a push that does not fast-forward) fails the call
        let read = move |output: &[u8]| {
            moved(output)?;
            let mut pushed = json!({ "ok": true });
            merged(&mut pushed, &shown);
            Ok(pushed)
        };
        Ok(Run::Read {
            writes,
            read: Box::new(read),
        })
    }
}

fn merged(into: &mut Value, from: &Value) {
    if let (Some(into), Some(from)) = (into.as_object_mut(), from.as_object()) {
        into.extend(from.clone());
    }
}

impl Tool for Push {
    const NAME: &'static str = "pod_push";
    const DESCRIPTION: &'static str = "Push a commit a pod made to a branch of a forge repository, as your account, in one call: the pods service packs the commit ref names (HEAD if omitted) in the git work tree at path, leaving out what the branch's head on forge and the work tree's remote refs already reach, and puts the pack as a blob; then forge takes the push from you, and its write list decides as for any push. A branch that does not exist yet is created; one that has moved on is not moved (forge refuses a push that does not fast-forward). Unless allow_stack is true, it refuses a commit whose parent is not the branch's head, or, for a new branch, one that would bring commits forge does not hold yet along under it. It answers the branch, the new head and the old one. A clone from a pod's forge input has forge's refs as its remote refs, so a new branch packs only its own commits.";
    const WRITES: bool = true;
    type Args = PushArgs;

    fn schema() -> Value {
        object(
            json!({
                "pod": pod_schema(),
                "path": { "type": "string", "description": "the git work tree in the pod, an absolute path like /work/programs" },
                "repo": { "type": "string", "description": "the forge repository, as forge_repos lists them" },
                "branch": { "type": "string", "description": "the branch to push to, like j-057/pod-tools" },
                "ref": { "type": "string", "description": "the commit to push, as git in the pod names it: a branch, a commit id or HEAD (the default)" },
                "container": container_schema(),
                "allow_stack": { "type": "boolean", "description": "push it even when its parent is not the branch's head, or when commits forge does not hold yet come along under it, such as carrying someone's commits; false if omitted" },
            }),
            &["pod", "path", "repo", "branch"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: PushArgs) -> Result<Run, Error> {
        let pod = named(&args.pod)?;
        absolute(&args.path)?;
        let Some((repo, _)) = found(ctx, &args.repo)? else {
            return Err(not_found(format!(
                "forge has no repository named {}",
                args.repo
            )));
        };
        let branch = branch_ref(Some(&args.branch), &repo.settings.head)?;
        let old = branch_head(ctx, &args.repo, &branch)?;
        let mut body = json!({ "path": args.path, "have": old.iter().collect::<Vec<_>>() });
        if let Some(reference) = args.reference {
            body["ref"] = json!(reference);
        }
        if let Some(container) = args.container {
            body["container"] = json!(container);
        }
        let writes = whole(ctx, posted(ctx, format!("/v1/pods/{pod}/push"), &body)?)?;
        let pushing = Pushing {
            repo: args.repo,
            branch,
            hash: forge::hash_of(repo.hash),
            old,
            allow_stack: args.allow_stack,
        };
        Ok(Run::Then {
            writes,
            then: Box::new(move |_: &ExecCtx, output: &[u8]| pushing.push(output)),
        })
    }
}
