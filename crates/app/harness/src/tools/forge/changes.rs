//! The change tools: open a change from one branch into another, comment on one, close one.
//! Each writes one forge op as the agent and reads forge's answer back as the change's number.
use ::program::{Error, ExecCtx, invalid, wrong_state};
use forge::{OpReply, Query, Reply, ReviewDraft, Verdict};
use serde::Deserialize;
use serde_json::{Value, json};
use store::PageRequest;

use super::{ask, reference, repo_schema, revision};
use crate::tools::{Run, Seat, Tool, Write, object, unexpected};

fn number_schema() -> Value {
    json!({ "type": "integer", "minimum": 1, "description": "the change's number, as forge_changes lists them" })
}

// The op as the agent's one write, and its output read back as forge's `OpReply`.
fn sent(op: forge::Op, read: fn(OpReply) -> Result<Value, Error>) -> Run {
    let writes = vec![Write {
        target: forge::PROGRAM.into(),
        payload: abi::encode(&op),
    }];
    let read = Box::new(move |output: &[u8]| {
        let reply = abi::decode::<OpReply>(output)
            .map_err(|_| unexpected(forge::PROGRAM, "OpReply", &abi::hex(output)))?;
        read(reply)
    });
    Run::Read { writes, read }
}

#[derive(Deserialize)]
pub(crate) struct OpenArgs {
    repo: String,
    from: String,
    into: String,
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    reviewers: Vec<u64>,
}

pub(crate) struct ChangeOpen;

impl Tool for ChangeOpen {
    const NAME: &'static str = "forge_change_open";
    const DESCRIPTION: &'static str = "Open a change (a proposal to merge one branch into another) in a forge repository, as your account, and answer its number. Its conversation is a chat channel forge opens beside it.";
    const WRITES: bool = true;
    type Args = OpenArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "from": { "type": "string", "description": "what is to be merged: a branch (j-049/tools), a ref (refs/heads/j-049/tools) or a commit id" },
                "into": { "type": "string", "description": "the branch it merges into, like dev, or its ref" },
                "title": { "type": "string" },
                "body": { "type": "string", "description": "what changes and why, in Markdown" },
                "reviewers": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": "account numbers asked to review it" },
            }),
            &["repo", "from", "into", "title"],
        )
    }

    fn run(_: &ExecCtx, _: &Seat, args: OpenArgs) -> Result<Run, Error> {
        let op = forge::Op::ChangeOpen {
            repo: args.repo,
            from: revision(&args.from),
            into: reference(&args.into),
            title: args.title,
            body: args.body,
            reviewers: args
                .reviewers
                .into_iter()
                .map(::program::Principal::Account)
                .collect(),
        };
        Ok(sent(op, |reply| match reply {
            OpReply::Change { n, .. } => Ok(json!({ "ok": true, "number": n })),
            other => Err(unexpected(forge::PROGRAM, "Change", &other)),
        }))
    }
}

#[derive(Deserialize)]
pub(crate) struct CommentArgs {
    repo: String,
    number: u64,
    body: String,
}

pub(crate) struct ChangeComment;

impl Tool for ChangeComment {
    const NAME: &'static str = "forge_change_comment";
    const DESCRIPTION: &'static str = "Comment on a change of a forge repository, as your account: a review with the verdict Comment, pinned to the change's current head. A closed or merged change still takes one.";
    const WRITES: bool = true;
    type Args = CommentArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "number": number_schema(),
                "body": { "type": "string", "description": "the comment, in Markdown" },
            }),
            &["repo", "number", "body"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: CommentArgs) -> Result<Run, Error> {
        if args.body.trim().is_empty() {
            return Err(invalid("a comment says something"));
        }
        let asked = Query::Change {
            repo: args.repo.clone(),
            n: args.number,
            page: PageRequest::first(1),
        };
        let (change, source_head, target_head) = match ask(ctx, asked)? {
            Reply::Change {
                change,
                source_head,
                target_head,
                ..
            } => (change, source_head, target_head),
            other => return Err(unexpected(forge::PROGRAM, "Change", &other)),
        };
        let heads = change
            .merged_heads
            .map(|heads| (heads.source, Some(heads.target)));
        let Some((head, base)) = heads.or(source_head.map(|head| (head, target_head))) else {
            return Err(wrong_state(format!(
                "change {} has no head to pin a comment to: its branch is gone",
                args.number
            )));
        };
        let op = forge::Op::ReviewSubmit {
            repo: args.repo,
            n: args.number,
            review: ReviewDraft {
                commit_oid: head,
                base_oid: base,
                verdict: Verdict::Comment,
                body: args.body,
                comments: Vec::new(),
            },
        };
        Ok(sent(op, |reply| match reply {
            OpReply::Review { n, id, .. } => Ok(json!({ "ok": true, "number": n, "review": id })),
            other => Err(unexpected(forge::PROGRAM, "Review", &other)),
        }))
    }
}

#[derive(Deserialize)]
pub(crate) struct CloseArgs {
    repo: String,
    number: u64,
}

pub(crate) struct ChangeClose;

impl Tool for ChangeClose {
    const NAME: &'static str = "forge_change_close";
    const DESCRIPTION: &'static str = "Close an open change of a forge repository without merging it, as your account; closing is final. Its author closes it, or anyone the repository's write list allows.";
    const WRITES: bool = true;
    type Args = CloseArgs;

    fn schema() -> Value {
        object(
            json!({ "repo": repo_schema(), "number": number_schema() }),
            &["repo", "number"],
        )
    }

    fn run(_: &ExecCtx, _: &Seat, args: CloseArgs) -> Result<Run, Error> {
        let op = forge::Op::ChangeClose {
            repo: args.repo,
            n: args.number,
        };
        Ok(sent(op, |reply| match reply {
            OpReply::Change { n, .. } => Ok(json!({ "ok": true, "number": n, "state": "closed" })),
            other => Err(unexpected(forge::PROGRAM, "Change", &other)),
        }))
    }
}
