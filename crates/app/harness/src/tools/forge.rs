//! Tools over `forge`, the network's git server: repositories, history, trees and files read,
//! found and searched, the commits the agent pushes (`commit.rs`) and the changes it opens,
//! comments on and closes (`changes.rs`).
mod changes;
mod commit;

pub(crate) use changes::{ChangeClose, ChangeComment, ChangeOpen};
pub(crate) use commit::Commit;

use ::program::{Error, ExecCtx, invalid, not_found};
use forge::{ChangeFilter, ChangeState, Content, EntryKind, Query, Reply, Revision};
use serde::Deserialize;
use serde_json::{Value, json};
use store::{PageRequest, PageResponse};

use super::{
    Run, Seat, Tool, cursor_schema, lines, next, object, page, principal, symbols, unexpected,
};

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

fn every<T>(
    mut listed: impl FnMut(PageRequest) -> Result<PageResponse<T>, Error>,
) -> Result<Vec<T>, Error> {
    let mut items = Vec::new();
    let mut page = PageRequest::default();
    loop {
        let answered = listed(page)?;
        items.extend(answered.items);
        let Some(after) = answered.next else {
            return Ok(items);
        };
        page = PageRequest {
            after: Some(after),
            ..PageRequest::default()
        };
    }
}

fn reference(name: &str) -> Vec<u8> {
    match name.starts_with("refs/") {
        true => name.as_bytes().to_vec(),
        false => format!("refs/heads/{name}").into_bytes(),
    }
}

fn listed(
    ctx: &ExecCtx,
    repo: &str,
    at: &str,
    dir: &str,
) -> Result<Vec<(String, EntryKind, String)>, Error> {
    let entries = every(|page| {
        let asked = Query::Tree {
            repo: repo.into(),
            at: at.into(),
            path: dir.as_bytes().to_vec(),
            page,
        };
        match ask(ctx, asked)? {
            Reply::Tree { page, .. } => Ok(page),
            other => Err(unexpected(forge::PROGRAM, "Tree", &other)),
        }
    })?;
    Ok(entries
        .into_iter()
        .map(|entry| {
            let name = String::from_utf8_lossy(&entry.name).into_owned();
            (name, entry.kind, entry.oid)
        })
        .collect())
}

fn entry_at(
    ctx: &ExecCtx,
    repo: &str,
    at: &str,
    path: &str,
) -> Result<Option<(EntryKind, String)>, Error> {
    let path = path.trim_matches('/');
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let found = match listed(ctx, repo, at, dir) {
        Ok(entries) => entries,
        Err(missing) if missing.code == ::program::code::NOT_FOUND => return Ok(None),
        Err(refused) => return Err(refused),
    };
    Ok(found
        .into_iter()
        .find(|(entry, _, _)| entry == name)
        .map(|(_, kind, id)| (kind, id)))
}

fn blob_of(
    ctx: &ExecCtx,
    repo: &str,
    id: &str,
    range: Option<forge::ByteRange>,
) -> Result<forge::BlobView, Error> {
    let asked = Query::Blob {
        repo: repo.into(),
        oid: id.into(),
        range,
    };
    match ask(ctx, asked)? {
        Reply::Blob { blob, .. } => Ok(blob),
        other => Err(unexpected(forge::PROGRAM, "Blob", &other)),
    }
}

fn served(ctx: &ExecCtx, repo: &str) -> Result<u64, Error> {
    match ask(ctx, Query::Repo { repo: repo.into() })? {
        Reply::Repo { bounds, .. } => Ok(bounds.blob_bytes),
        other => Err(unexpected(forge::PROGRAM, "Repo", &other)),
    }
}

fn text_of(ctx: &ExecCtx, repo: &str, id: &str, path: &str, advice: &str) -> Result<String, Error> {
    let blob = blob_of(ctx, repo, id, None)?;
    let bytes = match blob.content {
        Content::Text => blob.bytes,
        Content::Oversize => {
            let most = served(ctx, repo)?;
            let mut bytes = Vec::new();
            while (bytes.len() as u64) < blob.size {
                let at = bytes.len() as u64;
                let range = forge::ByteRange {
                    offset: at,
                    len: most,
                };
                let part = blob_of(ctx, repo, id, Some(range))?;
                if part.content != Content::Text || part.bytes.is_empty() {
                    let end = at + part.range.len;
                    return Err(invalid(format!(
                        "{path} is binary in bytes {at}..{end}: {advice}"
                    )));
                }
                bytes.extend(part.bytes);
            }
            bytes
        }
        Content::Binary | Content::Gitlink => return Err(invalid(format!("{path} is not text"))),
    };
    String::from_utf8(bytes).map_err(|_| invalid(format!("{path} is not text")))
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
    limit: Option<u64>,
    cursor: Option<String>,
}

pub(crate) struct Log;

impl Tool for Log {
    const NAME: &'static str = "forge_log";
    const DESCRIPTION: &'static str = "A repository's commit history from a branch or commit, newest first: id, author, time and summary line.";
    type Args = LogArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "rev": rev_schema(),
                "limit": { "type": "integer", "minimum": 1, "description": "at most this many commits; as many as forge answers in one page if omitted" },
                "cursor": cursor_schema(),
            }),
            &["repo"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: LogArgs) -> Result<Run, Error> {
        let mut page = page(&args.cursor)?;
        page.limit = args.limit;
        let asked = Query::Log {
            from: chosen(ctx, &args.repo, &args.rev)?,
            repo: args.repo,
            exclude: None,
            page,
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
    id: Option<String>,
    rev: Option<String>,
    path: Option<String>,
    offset: Option<u64>,
    length: Option<u64>,
    lines: Option<(u64, u64)>,
    symbol: Option<String>,
    symbols: Option<bool>,
    heading: Option<String>,
}

pub(crate) struct Read;

impl Tool for Read {
    const NAME: &'static str = "forge_read";
    const DESCRIPTION: &'static str = "Read a file of a repository as text, by its path at a branch or commit, or by its object id (from forge_tree): the whole file, some of its lines, one Rust item by name (symbol: a fn, struct, enum, union, trait, type, const, static, mod, macro_rules! or impl, at the top, in an inline mod or in an impl or trait, like main, Config::new or impl Display for Config, generics left out, from its attributes and doc comment to its end; found by a token scan, so an item a macro makes or one inside a fn body is not), the file's Rust items with their lines (symbols), the section a markdown heading opens, or a byte range of it. A name several items share is refused naming each with its lines; a name no item has is refused naming the items whose last part it is, or, when there are none, pointing to symbols. Forge refuses a range longer than it serves at once; a file larger than that reads by lines or by ranges, and as oversize whole. Such a file's range is judged on its own bytes: \"binary in this range\".";
    type Args = ReadArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "path": { "type": "string", "description": "the file's path in the repository, like src/main.rs, read at rev; give this or id" },
                "rev": rev_schema(),
                "id": { "type": "string", "description": "instead of path: the file's object id" },
                "lines": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "minItems": 2, "maxItems": 2, "description": "[from, to]: these lines only, counted from 1, both included" },
                "offset": { "type": "integer", "minimum": 0, "description": "the first byte to read; give a length with it" },
                "length": { "type": "integer", "minimum": 1, "description": "how many bytes; the whole file if both are omitted" },
                "symbol": { "type": "string", "description": "instead of lines: one Rust item by name, like main, Config::new or impl Display for Config" },
                "symbols": { "type": "boolean", "description": "instead of the text: each Rust item of the file, by the name symbol takes, with its lines" },
                "heading": { "type": "string", "description": "instead of lines: a markdown heading line of the file, like \"## Step 2\"; reads from it to the next heading of its level or a higher one, fenced code skipped" },
            }),
            &["repo"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: ReadArgs) -> Result<Run, Error> {
        let (id, path) = match (args.id, args.path) {
            (Some(id), None) => {
                if args.rev.is_some() {
                    return Err(invalid("rev goes with path; an id names one file already"));
                }
                (id, None)
            }
            (None, Some(path)) => {
                let rev = chosen(ctx, &args.repo, &args.rev)?;
                let at = commit_of(ctx, &args.repo, rev)?;
                match entry_at(ctx, &args.repo, &at, &path)? {
                    Some((EntryKind::File | EntryKind::Executable, id)) => (id, Some(path)),
                    Some(_) => return Err(invalid(format!("{path} is not a file"))),
                    None => return Err(not_found(format!("there is no {path} at {at}"))),
                }
            }
            _ => return Err(invalid("give path or id, one of them")),
        };
        let shown = path.clone().unwrap_or_else(|| id.clone());
        let outline = args.symbols.unwrap_or(false);
        let alone = [args.symbol.is_some(), args.heading.is_some(), outline];
        let picks = alone.into_iter().filter(|&on| on).count();
        let narrowed = args.lines.is_some() || args.offset.is_some() || args.length.is_some();
        let not_alone = picks > 1 || (picks == 1 && narrowed);
        if not_alone {
            return Err(invalid(
                "symbol, symbols and heading each go alone, without lines or a byte range",
            ));
        }
        let rust = path.as_deref().is_none_or(|path| path.ends_with(".rs"));
        let rust_asked = args.symbol.is_some() || outline;
        let rust_asked_of_other = rust_asked && !rust;
        if rust_asked_of_other {
            return Err(invalid(format!(
                "symbol and symbols read Rust, and {shown} is no .rs file: read it by lines, or a markdown file by heading"
            )));
        }
        let advice = "read the bytes as a blob or choose a text range";
        let text = || text_of(ctx, &args.repo, &id, &shown, advice);
        if outline {
            let text = text()?;
            let listed: Vec<Value> = symbols::items(&text)
                .into_iter()
                .map(|item| json!({ "name": item.name, "lines": [item.first, item.last] }))
                .collect();
            return Ok(Run::Done(json!({
                "id": id,
                "path": path,
                "total_lines": text.split_inclusive('\n').count(),
                "symbols": listed,
            })));
        }
        let ranged = args.offset.is_some() || args.length.is_some();
        let picked = match (args.lines, &args.symbol, &args.heading) {
            (Some(_), _, _) if ranged => {
                return Err(invalid("lines or a byte range, not both"));
            }
            (Some((from, to)), _, _) => {
                let text = text()?;
                Some((text.len(), lines::range(&text, from, to)?))
            }
            (None, Some(symbol), _) => {
                let text = text()?;
                Some((text.len(), symbols::item(&text, symbol)?))
            }
            (None, None, Some(heading)) => {
                let text = text()?;
                Some((text.len(), lines::section(&text, heading)?))
            }
            (None, None, None) => None,
        };
        if let Some((size, picked)) = picked {
            return Ok(Run::Done(json!({
                "id": id,
                "path": path,
                "size": size,
                "lines": [picked.first, picked.last],
                "total_lines": picked.total,
                "text": picked.text,
            })));
        }
        let asked = Query::Blob {
            repo: args.repo,
            oid: id,
            range: range(args.offset, args.length)?,
        };
        match ask(ctx, asked)? {
            Reply::Blob { blob, .. } => {
                let content = match blob.content {
                    Content::Binary if blob.range.len > 0 => "binary in this range".to_owned(),
                    content => format!("{content:?}").to_lowercase(),
                };
                let mut value = json!({
                    "id": blob.oid,
                    "path": path,
                    "size": blob.size,
                    "offset": blob.range.offset,
                    "content": content,
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
            Some(other) => {
                return Err(::program::invalid(format!(
                    "no change state {other}: choose open, closed or merged"
                )));
            }
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
    const DESCRIPTION: &'static str = "Read one change of a repository: its title, description, branches, heads and reviews. A merged change's target_head is the target as the merge found it; merged_as is the commit the merge made.";
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
                    "merged_as": change.merge_oid,
                    "reviews": reviews,
                })))
            }
            other => Err(unexpected(forge::PROGRAM, "Change", &other)),
        }
    }
}

fn walked(
    ctx: &ExecCtx,
    repo: &str,
    at: &str,
    dir: &str,
    prefix: &str,
    found: &mut Vec<(String, EntryKind, String)>,
) -> Result<(), Error> {
    for (name, kind, id) in listed(ctx, repo, at, dir)? {
        let path = match dir {
            "" => name,
            dir => format!("{dir}/{name}"),
        };
        let matches = path.starts_with(prefix);
        let inside =
            kind == EntryKind::Directory && (matches || prefix.starts_with(&format!("{path}/")));
        if matches {
            found.push((path.clone(), kind, id));
        }
        if inside {
            walked(ctx, repo, at, &path, prefix, found)?;
        }
    }
    Ok(())
}

fn under_prefix(
    ctx: &ExecCtx,
    repo: &str,
    at: &str,
    prefix: &str,
) -> Result<Vec<(String, EntryKind, String)>, Error> {
    let prefix = prefix.trim_start_matches('/');
    let dir = prefix.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut found = Vec::new();
    match walked(ctx, repo, at, dir, prefix, &mut found) {
        Err(missing) if missing.code == ::program::code::NOT_FOUND && found.is_empty() => Ok(found),
        walk => walk.map(|()| found),
    }
}

fn kind_name(kind: EntryKind) -> String {
    format!("{kind:?}").to_lowercase()
}

fn prefix_schema() -> Value {
    json!({ "type": "string", "description": "search only paths starting with this, like crates/app/forge/; the whole repository if omitted" })
}

#[derive(Deserialize)]
pub(crate) struct FindArgs {
    repo: String,
    rev: Option<String>,
    prefix: Option<String>,
}

pub(crate) struct Find;

impl Tool for Find {
    const NAME: &'static str = "forge_find";
    const DESCRIPTION: &'static str = "Find every file and directory of a repository at a branch or commit whose path starts with a prefix, in path order: each one's path, kind and object id. The walk reads each directory it passes through, so a narrow prefix answers a large repository where the whole of it would be too much for one call.";
    type Args = FindArgs;

    fn schema() -> Value {
        object(
            json!({ "repo": repo_schema(), "rev": rev_schema(), "prefix": prefix_schema() }),
            &["repo"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: FindArgs) -> Result<Run, Error> {
        let rev = chosen(ctx, &args.repo, &args.rev)?;
        let at = commit_of(ctx, &args.repo, rev)?;
        let prefix = args.prefix.unwrap_or_default();
        let entries: Vec<Value> = under_prefix(ctx, &args.repo, &at, &prefix)?
            .into_iter()
            .map(|(path, kind, id)| json!({ "path": path, "kind": kind_name(kind), "id": id }))
            .collect();
        Ok(Run::Done(json!({ "commit": at, "entries": entries })))
    }
}

#[derive(Deserialize)]
pub(crate) struct GrepArgs {
    repo: String,
    rev: Option<String>,
    pattern: String,
    prefix: Option<String>,
}

pub(crate) struct Grep;

impl Tool for Grep {
    const NAME: &'static str = "forge_grep";
    const DESCRIPTION: &'static str = "Search the text files of a repository at a branch or commit for a literal string: each matching line with its path and line number, in path order. A pattern holding newlines matches across lines (CRLF counts as one newline): its hit is the line it starts on, with every line it spans. A file larger than forge serves in one read is searched a range at a time. Binary files, and a big file's ranges that are binary in themselves, are listed as skipped; a hit after such a range gives its byte offset in place of a line number. It reads every file under the prefix, so narrow the prefix in a large repository.";
    type Args = GrepArgs;

    fn schema() -> Value {
        object(
            json!({
                "repo": repo_schema(),
                "rev": rev_schema(),
                "pattern": { "type": "string", "description": "the literal text to find; a newline in it matches across lines" },
                "prefix": prefix_schema(),
            }),
            &["repo", "pattern"],
        )
    }

    fn run(ctx: &ExecCtx, _: &Seat, args: GrepArgs) -> Result<Run, Error> {
        if args.pattern.is_empty() {
            return Err(invalid("forge_grep looks for a pattern that is not empty"));
        }
        let rev = chosen(ctx, &args.repo, &args.rev)?;
        let at = commit_of(ctx, &args.repo, rev)?;
        let prefix = args.prefix.unwrap_or_default();
        let mut search = Search {
            ctx,
            repo: &args.repo,
            spans: ::files::grep::Spans::new(&args.pattern),
            served: None,
            hits: Vec::new(),
            skipped: Vec::new(),
        };
        for (path, kind, id) in under_prefix(ctx, &args.repo, &at, &prefix)? {
            if matches!(kind, EntryKind::File | EntryKind::Executable) {
                search.file(&path, &id)?;
            }
        }
        Ok(Run::Done(
            json!({ "commit": at, "hits": search.hits, "skipped": search.skipped }),
        ))
    }
}

struct Search<'a> {
    ctx: &'a ExecCtx,
    repo: &'a str,
    spans: ::files::grep::Spans<'a, (Option<u64>, u64)>,
    served: Option<u64>,
    hits: Vec<Value>,
    skipped: Vec<String>,
}

impl Search<'_> {
    fn file(&mut self, path: &str, id: &str) -> Result<(), Error> {
        self.spans.gap();
        let blob = blob_of(self.ctx, self.repo, id, None)?;
        match blob.content {
            Content::Text => {
                let text = String::from_utf8_lossy(&blob.bytes);
                for (index, line) in text.lines().enumerate() {
                    self.hit(path, Some(index as u64 + 1), 0, line);
                }
                Ok(())
            }
            Content::Oversize => self.ranges(path, id, blob.size),
            Content::Binary | Content::Gitlink => {
                self.skipped.push(path.into());
                Ok(())
            }
        }
    }

    fn ranges(&mut self, path: &str, id: &str, size: u64) -> Result<(), Error> {
        let most = match self.served {
            Some(most) => most,
            None => served(self.ctx, self.repo)?,
        };
        self.served = Some(most);
        let mut carried: Vec<u8> = Vec::new();
        let (mut start, mut line, mut at) = (0u64, Some(1u64), 0u64);
        while at < size {
            let range = forge::ByteRange {
                offset: at,
                len: most,
            };
            let part = blob_of(self.ctx, self.repo, id, Some(range))?;
            if part.range.len == 0 {
                break;
            }
            at += part.range.len;
            if part.content != Content::Text {
                let from = at - part.range.len;
                let skipped = format!("{path} bytes {from}..{at}: binary in this range");
                self.skipped.push(skipped);
                carried.clear();
                self.spans.gap();
                (start, line) = (at, None);
                continue;
            }
            carried.extend(part.bytes);
            let whole = match carried.iter().rposition(|byte| *byte == b'\n') {
                _ if at >= size => carried.len(),
                Some(last) => last + 1,
                None => continue,
            };
            let rest = carried.split_off(whole);
            for piece in carried.split_inclusive(|byte| *byte == b'\n') {
                let text = String::from_utf8_lossy(piece);
                let text = text.strip_suffix('\n').unwrap_or(&text);
                self.hit(path, line, start, text.strip_suffix('\r').unwrap_or(text));
                start += piece.len() as u64;
                line = line.map(|n| n + 1);
            }
            carried = rest;
        }
        Ok(())
    }

    fn hit(&mut self, path: &str, line: Option<u64>, offset: u64, text: &str) {
        let Some(((line, offset), text)) = self.spans.line((line, offset), text) else {
            return;
        };
        let mut hit = json!({ "path": path, "line": line, "text": text });
        if line.is_none() {
            hit["offset"] = json!(offset);
        }
        self.hits.push(hit);
    }
}
