//! Tools over `forge`, the network's git server: repositories, history, trees, file
//! contents and changes under review, read, and the commits the agent pushes (`commit.rs`).
mod commit;

pub(crate) use commit::Commit;

use ::program::{Error, ExecCtx, invalid};
use forge::{ChangeFilter, ChangeState, Query, Reply, Revision};
use serde::Deserialize;
use serde_json::{Value, json};
use store::PageRequest;

use super::{Run, Seat, Tool, cursor_schema, next, object, page, principal, unexpected};

fn range(offset: Option<u64>, length: Option<u64>) -> Result<Option<forge::ByteRange>, Error> {
    match (offset, length) {
        (None, None) => Ok(None),
        (offset, Some(len)) => Ok(Some(forge::ByteRange {
            offset: offset.unwrap_or(0),
            len,
        })),
        (Some(_), None) => Err(invalid("an offset is read with a length")),
    }
}

fn ask(ctx: &ExecCtx, query: Query) -> Result<Reply, Error> {
    ctx.query(forge::PROGRAM, &query)
}

fn repo_schema() -> Value {
    json!({ "type": "string", "description": "a repository name, as forge_repos lists them" })
}

fn rev_schema() -> Value {
    json!({ "type": "string", "description": "a branch (main), a ref (refs/heads/main) or a commit id; the repository's default branch if omitted" })
}

fn is_oid(rev: &str) -> bool {
    let hex = rev.bytes().all(|byte| byte.is_ascii_hexdigit());
    hex && (rev.len() == 40 || rev.len() == 64)
}

fn revision(rev: &str) -> Revision {
    if is_oid(rev) {
        return Revision::Oid(rev.to_ascii_lowercase());
    }
    match rev.starts_with("refs/") {
        true => Revision::Ref(rev.as_bytes().to_vec()),
        false => Revision::Ref(format!("refs/heads/{rev}").into_bytes()),
    }
}

fn spelled(revision: &Revision) -> String {
    match revision {
        Revision::Ref(name) => String::from_utf8_lossy(name).into_owned(),
        Revision::Oid(oid) => oid.clone(),
    }
}

fn head_of(ctx: &ExecCtx, repo: &str) -> Result<Revision, Error> {
    let asked = Query::Repo { repo: repo.into() };
    match ask(ctx, asked)? {
        Reply::Repo { repo, .. } => Ok(Revision::Ref(repo.repo.settings.head)),
        other => Err(unexpected(forge::PROGRAM, "Repo", &other)),
    }
}

fn chosen(ctx: &ExecCtx, repo: &str, rev: &Option<String>) -> Result<Revision, Error> {
    match rev {
        Some(rev) => Ok(revision(rev)),
        None => head_of(ctx, repo),
    }
}

fn commit_of(ctx: &ExecCtx, repo: &str, rev: Revision) -> Result<String, Error> {
    if let Revision::Oid(oid) = rev {
        return Ok(oid);
    }
    let asked = Query::Log {
        repo: repo.into(),
        from: rev,
        exclude: None,
        page: PageRequest::first(1),
    };
    match ask(ctx, asked)? {
        Reply::Log { tip, .. } => Ok(tip),
        other => Err(unexpected(forge::PROGRAM, "Log", &other)),
    }
}

fn state_name(state: ChangeState) -> &'static str {
    match state {
        ChangeState::Open => "open",
        ChangeState::Closed => "closed",
        ChangeState::Merged => "merged",
    }
}

#[derive(Deserialize)]
pub(crate) struct ReposArgs {
    cursor: Option<String>,
}

pub(crate) struct Repos;

impl Tool for Repos {
    const NAME: &'static str = "forge_repos";
    const DESCRIPTION: &'static str =
        "List the network's git repositories: name, owner and default branch.";
    type Args = ReposArgs;

    fn schema() -> Value {
        object(json!({ "cursor": cursor_schema() }), &[])
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ReposArgs) -> Result<Run, Error> {
        match ask(
            ctx,
            Query::Repos {
                page: page(&args.cursor)?,
            },
        )? {
            Reply::Repos { page, .. } => {
                let repos: Vec<Value> = page
                    .items
                    .iter()
                    .map(|info| {
                        json!({
                            "name": info.name,
                            "owner": principal(&info.repo.owner),
                            "default_branch": String::from_utf8_lossy(&info.repo.settings.head),
                            "refs": info.repo.refs_count,
                        })
                    })
                    .collect();
                Ok(Run::Done(
                    json!({ "repos": repos, "next": next(&page.next) }),
                ))
            }
            other => Err(unexpected(forge::PROGRAM, "Repos", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct LogArgs {
    repo: String,
    rev: Option<String>,
    cursor: Option<String>,
}

pub(crate) struct Log;

impl Tool for Log {
    const NAME: &'static str = "forge_log";
    const DESCRIPTION: &'static str = "A repository's commit history from a branch or commit, newest first: id, author, time and summary line.";
    type Args = LogArgs;

    fn schema() -> Value {
        object(
            json!({ "repo": repo_schema(), "rev": rev_schema(), "cursor": cursor_schema() }),
            &["repo"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: LogArgs) -> Result<Run, Error> {
        let asked = Query::Log {
            from: chosen(ctx, &args.repo, &args.rev)?,
            repo: args.repo,
            exclude: None,
            page: page(&args.cursor)?,
        };
        match ask(ctx, asked)? {
            Reply::Log { tip, page, .. } => {
                let commits: Vec<Value> = page
                    .items
                    .iter()
                    .map(|commit| {
                        let message = String::from_utf8_lossy(&commit.message);
                        json!({
                            "id": commit.oid,
                            "author": String::from_utf8_lossy(&commit.author.name),
                            "time": commit.author.time,
                            "summary": message.lines().next().unwrap_or_default(),
                        })
                    })
                    .collect();
                Ok(Run::Done(
                    json!({ "tip": tip, "commits": commits, "next": next(&page.next) }),
                ))
            }
            other => Err(unexpected(forge::PROGRAM, "Log", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct TreeArgs {
    repo: String,
    rev: Option<String>,
    path: Option<String>,
    cursor: Option<String>,
}

pub(crate) struct Tree;

impl Tool for Tree {
    const NAME: &'static str = "forge_tree";
    const DESCRIPTION: &'static str = "List a directory of a repository at a branch or commit: each entry's name, object id and kind. forge_read reads a file by its id.";
    type Args = TreeArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "rev": rev_schema(),
                "path": { "type": "string", "description": "a directory inside the repository, like src/; its root if omitted" },
                "cursor": cursor_schema(),
            }),
            &["repo"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: TreeArgs) -> Result<Run, Error> {
        let rev = chosen(ctx, &args.repo, &args.rev)?;
        let at = commit_of(ctx, &args.repo, rev)?;
        let path = args.path.unwrap_or_default();
        let asked = Query::Tree {
            repo: args.repo,
            at: at.clone(),
            path: path.trim_matches('/').as_bytes().to_vec(),
            page: page(&args.cursor)?,
        };
        match ask(ctx, asked)? {
            Reply::Tree { page, .. } => {
                let entries: Vec<Value> = page
                    .items
                    .iter()
                    .map(|entry| {
                        json!({
                            "name": String::from_utf8_lossy(&entry.name),
                            "id": entry.oid,
                            "kind": format!("{:?}", entry.kind).to_lowercase(),
                        })
                    })
                    .collect();
                Ok(Run::Done(
                    json!({ "commit": at, "entries": entries, "next": next(&page.next) }),
                ))
            }
            other => Err(unexpected(forge::PROGRAM, "Tree", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ReadArgs {
    repo: String,
    id: String,
    offset: Option<u64>,
    length: Option<u64>,
}

pub(crate) struct Read;

impl Tool for Read {
    const NAME: &'static str = "forge_read";
    const DESCRIPTION: &'static str = "Read a file of a repository by its object id (from forge_tree) as text: the whole file, or a byte range of it. Forge refuses a range longer than it serves at once, and answers a file larger than that as oversize.";
    type Args = ReadArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "id": { "type": "string", "description": "the file's object id" },
                "offset": { "type": "integer", "minimum": 0, "description": "the first byte to read; give a length with it" },
                "length": { "type": "integer", "minimum": 1, "description": "how many bytes; the whole file if both are omitted" },
            }),
            &["repo", "id"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ReadArgs) -> Result<Run, Error> {
        let asked = Query::Blob {
            repo: args.repo,
            oid: args.id,
            range: range(args.offset, args.length)?,
        };
        match ask(ctx, asked)? {
            Reply::Blob { blob, .. } => {
                let mut value = json!({
                    "id": blob.oid,
                    "size": blob.size,
                    "offset": blob.range.offset,
                    "content": format!("{:?}", blob.content).to_lowercase(),
                });
                if let Ok(text) = String::from_utf8(blob.bytes) {
                    value["text"] = json!(text);
                }
                Ok(Run::Done(value))
            }
            other => Err(unexpected(forge::PROGRAM, "Blob", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ChangesArgs {
    repo: String,
    state: Option<String>,
    cursor: Option<String>,
}

pub(crate) struct Changes;

impl Tool for Changes {
    const NAME: &'static str = "forge_changes";
    const DESCRIPTION: &'static str = "List a repository's changes (proposals to merge one branch into another): number, title, state, author and review counts.";
    type Args = ChangesArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "state": { "type": "string", "enum": ["open", "closed", "merged"] },
                "cursor": cursor_schema(),
            }),
            &["repo"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ChangesArgs) -> Result<Run, Error> {
        let state = match args.state.as_deref() {
            None => None,
            Some("open") => Some(ChangeState::Open),
            Some("closed") => Some(ChangeState::Closed),
            Some("merged") => Some(ChangeState::Merged),
            Some(other) => return Err(::program::invalid(format!("no change state {other}"))),
        };
        let asked = Query::Changes {
            repo: args.repo,
            filter: ChangeFilter {
                state,
                author: None,
                involves: None,
            },
            page: page(&args.cursor)?,
        };
        match ask(ctx, asked)? {
            Reply::Changes { page, .. } => {
                let changes: Vec<Value> = page
                    .items
                    .iter()
                    .map(|change| {
                        json!({
                            "number": change.n,
                            "title": change.title,
                            "state": state_name(change.state),
                            "author": principal(&change.author),
                            "from": spelled(&change.from),
                            "into": String::from_utf8_lossy(&change.into),
                            "reviews": change.review_count,
                            "approvals": change.verdicts.approve,
                        })
                    })
                    .collect();
                Ok(Run::Done(
                    json!({ "changes": changes, "next": next(&page.next) }),
                ))
            }
            other => Err(unexpected(forge::PROGRAM, "Changes", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ChangeArgs {
    repo: String,
    number: u64,
}

pub(crate) struct Change;

impl Tool for Change {
    const NAME: &'static str = "forge_change";
    const DESCRIPTION: &'static str =
        "Read one change of a repository: its title, description, branches, heads and reviews.";
    type Args = ChangeArgs;

    fn schema() -> Value {
        object(
            json!({ "repo": repo_schema(), "number": { "type": "integer" } }),
            &["repo", "number"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ChangeArgs) -> Result<Run, Error> {
        let asked = Query::Change {
            repo: args.repo,
            n: args.number,
            page: PageRequest::first(20),
        };
        match ask(ctx, asked)? {
            Reply::Change {
                change,
                source_head,
                target_head,
                reviews,
                ..
            } => {
                let reviews: Vec<Value> = reviews
                    .items
                    .iter()
                    .map(|review| {
                        json!({
                            "author": principal(&review.author),
                            "verdict": format!("{:?}", review.draft.verdict),
                            "body": review.draft.body,
                        })
                    })
                    .collect();
                Ok(Run::Done(json!({
                    "number": change.n,
                    "title": change.title,
                    "body": change.body,
                    "state": state_name(change.state),
                    "author": principal(&change.author),
                    "from": spelled(&change.from),
                    "into": String::from_utf8_lossy(&change.into),
                    "source_head": source_head,
                    "target_head": target_head,
                    "reviews": reviews,
                })))
            }
            other => Err(unexpected(forge::PROGRAM, "Change", &other)),
        }
    }
}
