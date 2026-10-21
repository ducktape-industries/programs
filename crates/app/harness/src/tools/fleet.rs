//! The `fleet_run` tool: ask fleet, as the agent, to run a workload on whichever ready machine has
//! been free longest unless it is pinned; fleet tells harness when it ends, which wakes this conversation.
use std::collections::BTreeMap;

use ::program::{BlobId, Error, ExecCtx, ProgramId, invalid, unexpected_reply};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Run, Seat, Tool, Write, object};
use crate::fleet_runs::asked;
use crate::run_ended_as;

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum InputArgs {
    Forge {
        path: String,
        repo: String,
        commit: String,
    },
    Files {
        path: String,
        files: String,
        snapshot: Option<String>,
    },
    Blob {
        path: String,
        blob: String,
    },
    Scratch {
        path: String,
        memory: Option<bool>,
        size: Option<String>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AsksArgs {
    cpu: Option<String>,
    memory: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum OnArgs {
    Machine { account: u64, name: String },
    Labels(Vec<String>),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum LossArgs {
    Retry,
    Park,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunArgs {
    command: Option<Vec<String>>,
    script: Option<String>,
    image: Option<String>,
    env: Option<BTreeMap<String, String>>,
    workdir: Option<String>,
    inputs: Option<Vec<InputArgs>>,
    asks: Option<AsksArgs>,
    on: Option<OnArgs>,
    out: Option<String>,
    on_loss: Option<LossArgs>,
}

fn blob_id(text: &str, what: &str) -> Result<BlobId, Error> {
    let digest: [u8; 32] = design::unhex(text)
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| invalid(format!("{what} is 64 hex digits")))?;
    Ok(BlobId::Sha256(digest))
}

fn input(args: InputArgs) -> Result<fleet::Input, Error> {
    Ok(match args {
        InputArgs::Forge { path, repo, commit } => fleet::Input::Forge { path, repo, commit },
        InputArgs::Files {
            path,
            files,
            snapshot,
        } => fleet::Input::Files {
            path,
            files,
            snapshot: snapshot
                .map(|text| blob_id(&text, "a snapshot"))
                .transpose()?,
        },
        InputArgs::Blob { path, blob } => fleet::Input::Blob {
            path,
            blob: blob_id(&blob, "a blob id")?,
        },
        InputArgs::Scratch { path, memory, size } => fleet::Input::Scratch {
            path,
            memory: memory.unwrap_or(false),
            size,
        },
    })
}

fn command(command: Option<Vec<String>>, script: Option<String>) -> Result<Vec<String>, Error> {
    match (command, script) {
        (Some(command), None) if !command.is_empty() => Ok(command),
        (None, Some(script)) => Ok(vec!["sh".into(), "-c".into(), script]),
        _ => Err(invalid(
            "a run takes a command (an array, run as it is) or a script (shell text, run with sh -c): one of them",
        )),
    }
}

fn spec(args: RunArgs, notify: fleet::Notify) -> Result<fleet::Spec, Error> {
    let inputs = args.inputs.unwrap_or_default().into_iter().map(input);
    let on = args.on.map(|on| match on {
        OnArgs::Machine { account, name } => {
            fleet::Affinity::Machine(fleet::MachineId { account, name })
        }
        OnArgs::Labels(labels) => fleet::Affinity::Labels(labels),
    });
    let asks = args.asks.map(|asks| fleet::Asks {
        cpu: asks.cpu,
        memory: asks.memory,
    });
    let on_loss = match args.on_loss {
        Some(LossArgs::Park) => fleet::OnLoss::Park,
        Some(LossArgs::Retry) | None => fleet::OnLoss::Retry,
    };
    Ok(fleet::Spec {
        kind: fleet::Kind::Job,
        image: args.image,
        command: command(args.command, args.script)?,
        env: args.env.unwrap_or_default().into_iter().collect(),
        workdir: args.workdir,
        inputs: inputs.collect::<Result<_, _>>()?,
        asks: asks.unwrap_or_default(),
        on,
        out: args.out,
        on_loss,
        notify: Some(notify),
    })
}

fn notify(harness: ProgramId, thread: u64) -> fleet::Notify {
    fleet::Notify::Program {
        program: harness,
        deliver: run_ended_as(thread),
    }
}

fn input_schema() -> Value {
    let path = json!({ "type": "string", "description": "where it is placed in the run, an absolute path" });
    json!({
        "type": "object",
        "description": "exactly one of these",
        "properties": {
            "forge": object(json!({
                "path": path,
                "repo": { "type": "string", "description": "the forge repository" },
                "commit": { "type": "string", "description": "the full 40-hex commit" },
            }), &["path", "repo", "commit"]),
            "files": object(json!({
                "path": path,
                "files": { "type": "string", "description": "an absolute files path" },
                "snapshot": { "type": "string", "description": "a snapshot id (64 hex digits); the newest if omitted" },
            }), &["path", "files"]),
            "blob": object(json!({
                "path": path,
                "blob": { "type": "string", "description": "a blob id (64 hex digits)" },
            }), &["path", "blob"]),
            "scratch": object(json!({
                "path": path,
                "memory": { "type": "boolean", "description": "held in memory, not on disk" },
                "size": { "type": "string", "description": "a Kubernetes quantity, like 8Gi" },
            }), &["path"]),
        },
        "minProperties": 1,
        "maxProperties": 1,
    })
}

pub(crate) struct Ask;

impl Tool for Ask {
    const NAME: &'static str = "fleet_run";
    const DESCRIPTION: &'static str = "Run a workload on the network's machines through fleet, as your own account: a command (an array, run as it is) or a script (shell text, run with sh -c), in an image if you name one, with inputs placed before it starts (a forge commit, a files path, a blob, a scratch folder). It goes to whichever ready machine has been free longest, unless `on` pins it to one machine or to machines holding every label named, and a machine takes it only where your claim policy for fleet.claim admits that machine. Answers the run's id at once; when the run ends, or its machine goes silent and it is lost, this conversation gets one message with its state, its summary and the files folder its log and outputs went to.";
    const WRITES: bool = true;
    type Args = RunArgs;

    fn schema() -> Value {
        let on = json!({
            "type": "object",
            "description": "exactly one of these; any ready machine if omitted",
            "properties": {
                "machine": object(json!({
                    "account": { "type": "integer", "description": "the machine's account" },
                    "name": { "type": "string", "description": "its name among that account's machines" },
                }), &["account", "name"]),
                "labels": { "type": "array", "items": { "type": "string" }, "description": "a machine holding every one of these" },
            },
            "minProperties": 1,
            "maxProperties": 1,
        });
        let asks = object(
            json!({
                "cpu": { "type": "string", "description": "a Kubernetes quantity, like 2 or 500m" },
                "memory": { "type": "string", "description": "a Kubernetes quantity, like 4Gi" },
            }),
            &[],
        );
        object(
            json!({
                "command": { "type": "array", "items": { "type": "string" }, "description": "the command and its arguments, run as they are" },
                "script": { "type": "string", "description": "instead of command: shell text, run with sh -c" },
                "image": { "type": "string", "description": "the image to run it in, for a machine whose backend runs images" },
                "env": { "type": "object", "additionalProperties": { "type": "string" }, "description": "environment variables, by name" },
                "workdir": { "type": "string", "description": "the folder it starts in" },
                "inputs": { "type": "array", "items": input_schema(), "description": "what is placed before it starts" },
                "asks": asks,
                "on": on,
                "out": { "type": "string", "description": "the files folder its log and outputs go to; the machine's choice if omitted" },
                "on_loss": { "type": "string", "enum": ["retry", "park"], "description": "if its machine goes silent: queued again (retry, the default), or held as lost until that machine reports it or you queue it again (park), for a run that must never happen on two machines" },
            }),
            &[],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: RunArgs) -> Result<Run, Error> {
        let thread = seat.thread;
        let spec = spec(args, notify(ctx.env().program.clone(), thread))?;
        let ask = Write {
            target: fleet::PROGRAM.into(),
            payload: abi::encode(&fleet::Op::Ask { spec }),
        };
        Ok(Run::Then {
            writes: vec![ask],
            then: Box::new(move |ctx, output| {
                let run: u64 = abi::decode(output)
                    .map_err(|fault| unexpected_reply(fleet::PROGRAM, "Ask", &fault.sentence))?;
                asked(ctx, thread, run);
                Ok(Run::Done(json!({
                    "run": run,
                    "ended": "when the run ends, or is lost, this conversation gets one message saying how",
                })))
            }),
        })
    }
}
