//! `forge_commit`, the agent's one write to forge: files written whole or edited by span and paths
//! removed, as one git commit (objects built with `gitcore` over the parent's trees, read through
//! forge) pushed as one pack blob as the agent's account; a repository not there yet is created.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{BlobId, Error, ExecCtx, HashKind, capacity, code, invalid, not_found, stale};
use abi::role::identity::AccountNumber;
use files::{EntryInfo, Node};
use forge::Content;
use forge::{Bounds, EntryKind, Query, Reply, Repo, Settings};
use gitcore::wire::pktline::{self, Pkt};
use gitcore::{Hash, Kind, Mode, Object, Oid, Signature, Tree, TreeEntry, pack};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ask, commit_of, every, revision};
use crate::agents::profile;
use crate::tools::edits::{EditArg, applied, edit_schema, steps};
use crate::tools::files::{beneath, read_all, resolved, snapshot, stat};
use crate::tools::{Run, Seat, Tool, Write, object, unexpected};

type Objects = BTreeMap<Oid, Object>;

fn kept(objects: &mut Objects, hash: Hash, kind: Kind, body: Vec<u8>) -> Result<Oid, Error> {
    let object = Object::new(kind, body);
    let id = object
        .id(hash)
        .map_err(|error| invalid(error.to_string()))?;
    objects.insert(id, object);
    Ok(id)
}

pub(crate) fn oid(hash: Hash, hex: &str) -> Result<Oid, Error> {
    Oid::from_hex(hash, hex)
        .map_err(|_| invalid(format!("{hex} is not an object id of this repository")))
}

pub(crate) fn found(ctx: &ExecCtx, name: &str) -> Result<Option<(Repo, Bounds)>, Error> {
    match ask(ctx, Query::Repo { repo: name.into() }) {
        Ok(Reply::Repo { repo, bounds, .. }) => Ok(Some((repo.repo, bounds))),
        Ok(other) => Err(unexpected(forge::PROGRAM, "Repo", &other)),
        Err(missing) if missing.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

fn mode(kind: EntryKind) -> Mode {
    match kind {
        EntryKind::File => Mode::Regular,
        EntryKind::Executable => Mode::Executable,
        EntryKind::Symlink => Mode::Symlink,
        EntryKind::Directory => Mode::Directory,
        EntryKind::Gitlink => Mode::Gitlink,
    }
}

type Entries = BTreeMap<Vec<u8>, (Mode, Oid)>;

struct Reading<'a> {
    ctx: &'a ExecCtx,
    repo: &'a str,
    hash: Hash,
}

impl Reading<'_> {
    fn refs(&self) -> Result<BTreeMap<Vec<u8>, Oid>, Error> {
        let refs = every(|page| {
            let asked = Query::Refs {
                repo: self.repo.into(),
                page,
            };
            match ask(self.ctx, asked)? {
                Reply::Refs { page, .. } => Ok(page),
                other => Err(unexpected(forge::PROGRAM, "Refs", &other)),
            }
        })?;
        refs.into_iter()
            .map(|listed| Ok((listed.name, oid(self.hash, &listed.target)?)))
            .collect()
    }

    fn entries(&self, at: Oid) -> Result<Entries, Error> {
        let entries = every(|page| {
            let asked = Query::Tree {
                repo: self.repo.into(),
                at: at.to_hex(),
                path: Vec::new(),
                page,
            };
            match ask(self.ctx, asked)? {
                Reply::Tree { page, .. } => Ok(page),
                other => Err(unexpected(forge::PROGRAM, "Tree", &other)),
            }
        })?;
        entries
            .into_iter()
            .map(|entry| Ok((entry.name, (mode(entry.kind), oid(self.hash, &entry.oid)?))))
            .collect()
    }

    fn file(&self, at: Oid, path: &str) -> Result<(Mode, Oid), Error> {
        let (dirs, name) = split(path)?;
        let missing = || not_found(format!("there is no file at {path} to edit"));
        let mut entries = self.entries(at)?;
        for dir in dirs {
            entries = match entries.get(dir.as_bytes()) {
                Some((Mode::Directory, tree)) => self.entries(*tree)?,
                _ => return Err(missing()),
            };
        }
        match entries.get(name.as_bytes()) {
            Some((mode @ (Mode::Regular | Mode::Executable), blob)) => Ok((*mode, *blob)),
            Some(_) => Err(invalid(format!("{path} is not a file"))),
            None => Err(missing()),
        }
    }

    fn text(&self, id: Oid, path: &str) -> Result<String, Error> {
        let asked = Query::Blob {
            repo: self.repo.into(),
            oid: id.to_hex(),
            range: None,
        };
        let blob = match ask(self.ctx, asked)? {
            Reply::Blob { blob, .. } => blob,
            other => return Err(unexpected(forge::PROGRAM, "Blob", &other)),
        };
        match blob.content {
            Content::Text if blob.bytes.len() as u64 == blob.size => {
                String::from_utf8(blob.bytes).map_err(|_| invalid(format!("{path} is not text")))
            }
            Content::Text | Content::Oversize => Err(capacity(format!(
                "{path} is {} bytes, more than forge serves in one read, so it can't be edited by span; write it whole",
                blob.size
            ))),
            Content::Binary | Content::Gitlink => Err(invalid(format!("{path} is not text"))),
        }
    }
}

#[derive(Default)]
struct Edits {
    put: BTreeMap<Vec<u8>, (Mode, Oid)>,
    removed: BTreeSet<Vec<u8>>,
    below: BTreeMap<Vec<u8>, Edits>,
}

impl Edits {
    fn at(&mut self, dirs: &[&str]) -> &mut Edits {
        dirs.iter().fold(self, |edits, dir| {
            edits.below.entry(dir.as_bytes().to_vec()).or_default()
        })
    }
}

fn split(path: &str) -> Result<(Vec<&str>, &str), Error> {
    let parts: Vec<&str> = path.strip_prefix('/').unwrap_or(path).split('/').collect();
    let bad = |part: &&str| matches!(*part, "" | "." | ".." | ".git") || part.contains('\0');
    if parts.iter().any(bad) {
        return Err(invalid(format!(
            "{path:?} is not a file path inside a repository"
        )));
    }
    let (name, dirs) = parts.split_last().expect("a split yields a part");
    Ok((dirs.to_vec(), *name))
}

fn joined(dir: &str, name: &[u8]) -> String {
    let name = String::from_utf8_lossy(name);
    match dir {
        "" => name.into_owned(),
        dir => format!("{dir}/{name}"),
    }
}

fn rebuilt(
    reading: &Reading,
    dir: &str,
    mut entries: Entries,
    edits: Edits,
    objects: &mut Objects,
) -> Result<Option<Oid>, Error> {
    let Edits {
        put,
        removed,
        below,
    } = edits;
    let twice = |name: &[u8]| invalid(format!("{} is changed twice", joined(dir, name)));
    for name in removed {
        if put.contains_key(&name) || below.contains_key(&name) {
            return Err(twice(&name));
        }
        if entries.remove(&name).is_none() {
            return Err(not_found(format!(
                "there is no {} to remove",
                joined(dir, &name)
            )));
        }
    }
    for (name, edits) in below {
        if put.contains_key(&name) {
            return Err(twice(&name));
        }
        let path = joined(dir, &name);
        let inside = match entries.get(&name) {
            None => Entries::new(),
            Some((Mode::Directory, tree)) => reading.entries(*tree)?,
            Some(_) => return Err(invalid(format!("{path} is a file, not a directory"))),
        };
        match rebuilt(reading, &path, inside, edits, objects)? {
            Some(tree) => entries.insert(name, (Mode::Directory, tree)),
            None => entries.remove(&name),
        };
    }
    for (name, file) in put {
        if let Some((Mode::Directory, _)) = entries.get(&name) {
            return Err(invalid(format!("{} is a directory", joined(dir, &name))));
        }
        entries.insert(name, file);
    }
    if entries.is_empty() {
        return Ok(None);
    }
    let tree = Tree {
        entries: entries
            .into_iter()
            .map(|(name, (mode, id))| TreeEntry { mode, name, id })
            .collect(),
    };
    kept(objects, reading.hash, Kind::Tree, tree.serialize()).map(Some)
}

fn signature(ctx: &ExecCtx, agent: AccountNumber) -> Result<Signature, Error> {
    let name: String = profile(ctx, agent)?
        .map(|profile| profile.name)
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '<' | '>') && !c.is_control())
        .collect();
    let name = match name.trim() {
        "" => format!("account {agent}"),
        name => name.to_owned(),
    };
    let env = ctx.env();
    Ok(Signature {
        name: name.into_bytes(),
        email: format!("{agent}@{}", String::from_utf8_lossy(&env.chain_id)).into_bytes(),
        time: (env.time / 1000) as i64,
        offset_minutes: 0,
    })
}

pub(crate) fn request(old: Oid, new: Oid, branch: &[u8]) -> Vec<u8> {
    let mut line = format!("{old} {new} ").into_bytes();
    line.extend_from_slice(branch);
    line.extend_from_slice(b"\0report-status");
    let mut request = Vec::new();
    pktline::push(&mut request, &line);
    request.extend_from_slice(pktline::flush());
    request
}

pub(crate) fn branch_ref(branch: Option<&str>, default: &[u8]) -> Result<Vec<u8>, Error> {
    let Some(branch) = branch else {
        return Ok(default.to_vec());
    };
    let name = branch.strip_prefix("refs/heads/").unwrap_or(branch);
    let reference = format!("refs/heads/{name}").into_bytes();
    if !gitcore::server::valid_ref_name(&reference) {
        return Err(invalid(format!("{branch:?} is not a branch name")));
    }
    Ok(reference)
}

struct Put {
    path: String,
    mode: Mode,
    bytes: Vec<u8>,
}

fn file_mode(executable: bool) -> Mode {
    match executable {
        true => Mode::Executable,
        false => Mode::Regular,
    }
}

fn under(dir: &str, inside: &str) -> String {
    match dir.trim_matches('/') {
        "" => inside.to_owned(),
        dir => format!("{dir}/{inside}"),
    }
}

fn put_of(
    ctx: &ExecCtx,
    path: String,
    entry: &EntryInfo,
    at: Option<&BlobId>,
) -> Result<Option<Put>, Error> {
    let (mode, bytes) = match &entry.node {
        Node::File { exec, .. } => (file_mode(*exec), read_all(ctx, &entry.path, at)?),
        Node::Symlink { target } => (Mode::Symlink, target.clone().into_bytes()),
        Node::Dir { .. } => return Ok(None),
    };
    Ok(Some(Put { path, mode, bytes }))
}

fn sourced(
    ctx: &ExecCtx,
    path: &str,
    source: &str,
    at: Option<&BlobId>,
) -> Result<Vec<Put>, Error> {
    let source = match source.trim_end_matches('/') {
        "" => "/",
        trimmed => trimmed,
    };
    let Some(entry) = stat(ctx, source, at)? else {
        let read = at.map_or("files holds nothing yet".to_owned(), |id| {
            format!("at snapshot {}", abi::hex(id.digest()))
        });
        return Err(not_found(format!(
            "there is no file or directory at {source} in files ({read})"
        )));
    };
    if !matches!(entry.node, Node::Dir { .. }) {
        return Ok(put_of(ctx, path.to_owned(), &entry, at)?
            .into_iter()
            .collect());
    }
    let prefix = match source {
        "/" => "/".to_owned(),
        dir => format!("{dir}/"),
    };
    let mut puts = Vec::new();
    for entry in beneath(ctx, &prefix, at)? {
        let Some(inside) = entry.path.strip_prefix(&prefix) else {
            return Err(unexpected(files::PROGRAM, "Find", &entry.path));
        };
        if let Some(put) = put_of(ctx, under(path, inside), &entry, at)? {
            puts.push(put);
        }
    }
    Ok(puts)
}

struct Edited {
    path: String,
    steps: Vec<crate::tools::edits::Step>,
}

fn gathered(
    ctx: &ExecCtx,
    files: Vec<FileArg>,
    snapshot_arg: &Option<String>,
) -> Result<(Vec<Put>, Vec<Edited>), Error> {
    let named = snapshot(snapshot_arg)?;
    let at = match files.iter().any(|file| file.source.is_some()) {
        true => resolved(ctx, named)?,
        false => named,
    };
    let mut puts = Vec::new();
    let mut edited = Vec::new();
    for file in files {
        if let Some(edits) = file.edits {
            if file.content.is_some() || file.source.is_some() || file.executable.is_some() {
                return Err(invalid(format!(
                    "{}: edits go alone; the file keeps its mode",
                    file.path
                )));
            }
            if edits.is_empty() {
                return Err(invalid(format!("{}: edits names no edit", file.path)));
            }
            edited.push(Edited {
                path: file.path,
                steps: steps(edits)?,
            });
            continue;
        }
        match (file.content, file.source) {
            (Some(content), None) => puts.push(Put {
                path: file.path,
                mode: file_mode(file.executable.unwrap_or(false)),
                bytes: content.into_bytes(),
            }),
            (None, Some(source)) => {
                if file.executable.is_some() {
                    return Err(invalid(format!(
                        "{}: executable goes with content; a source keeps its mode in files",
                        file.path
                    )));
                }
                puts.extend(sourced(ctx, &file.path, &source, at.as_ref())?);
            }
            (Some(_), Some(_)) => {
                return Err(invalid(format!(
                    "{}: give content or source, not both",
                    file.path
                )));
            }
            (None, None) => {
                return Err(invalid(format!(
                    "{}: give content, source or edits",
                    file.path
                )));
            }
        }
    }
    Ok((puts, edited))
}

fn sized(bounds: &Bounds, puts: &[Put]) -> Result<(), Error> {
    match puts
        .iter()
        .find(|put| put.bytes.len() as u64 > bounds.max_object_size)
    {
        Some(put) => Err(capacity(format!(
            "{} is {} bytes; forge takes files of at most {} bytes",
            put.path,
            put.bytes.len(),
            bounds.max_object_size
        ))),
        None => Ok(()),
    }
}

fn counted(bounds: &Bounds, objects: usize) -> Result<(), Error> {
    if objects as u64 > bounds.max_objects {
        return Err(capacity(format!(
            "the commit makes {objects} git objects (a file or a changed directory is one each); forge takes at most {} in one push: commit fewer files at a time",
            bounds.max_objects
        )));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct FileArg {
    path: String,
    content: Option<String>,
    source: Option<String>,
    executable: Option<bool>,
    edits: Option<Vec<EditArg>>,
}

#[derive(Deserialize)]
pub(crate) struct CommitArgs {
    repo: String,
    branch: Option<String>,
    from: Option<String>,
    files: Vec<FileArg>,
    #[serde(default)]
    remove: Vec<String>,
    message: String,
    base: Option<String>,
    snapshot: Option<String>,
}

pub(crate) struct Commit;

impl Tool for Commit {
    const NAME: &'static str = "forge_commit";
    const DESCRIPTION: &'static str = "Commit files to a forge repository as one git commit on a branch, as your account, and push it. Each item of files is written whole, from inline content or from a files source, or edited by span: a source file lands at its path, and a source directory lands under its path with every file beneath it, each in its files mode; an item with edits changes part of a text file the branch holds (each span must occur exactly once in it, as in files_edit), keeps its mode, and is refused naming where a span occurs when it is not there once. Every source is read at one files snapshot, the newest if omitted, so a workspace kept in files commits without re-sending it. A source directory adds and replaces files and never deletes; remove deletes paths (a directory with everything in it). Paths are inside the repository, like src/main.rs. A repository that does not exist yet is created, owned by you, and stays even if forge refuses the commit. An existing one takes the push only if its write list in the access program allows your account or harness: its owner alone unless the owner lists others, and a refusal says unauthorized. The commit goes on top of the branch's newest commit, and a new branch starts from from (a branch, ref or commit id), or from the default branch; with base, only while the branch is still at base, and a branch that moved since the edits read it is refused, never overwritten. Forge takes a bounded number of objects (each file and each changed directory is one) and of bytes per file in one push; a larger commit is refused, so split it. The answer names the commit, which is the branch's head after the push, and its parent.";
    const WRITES: bool = true;
    type Args = CommitArgs;

    fn schema() -> Value {
        let file = object(
            json!({
                "path": { "type": "string", "description": "where it lands in the repository, like src/main.rs; for a source directory, the directory its files land under, \"\" for the root" },
                "content": { "type": "string", "description": "the whole file, inline" },
                "source": { "type": "string", "description": "instead of content: an absolute files path, a file or a directory, read at snapshot" },
                "executable": { "type": "boolean", "description": "with content only: whether the file is executable; a source keeps its files mode" },
                "edits": { "type": "array", "items": edit_schema(), "description": "instead of content or source: edits of the text file the branch holds at path, applied in order" },
            }),
            &["path"],
        );
        object(
            json!({
                "repo": { "type": "string", "description": "a repository name, as forge_repos lists them; a new name creates it" },
                "branch": { "type": "string", "description": "the branch to commit on, like main; the repository's default branch if omitted" },
                "from": { "type": "string", "description": "for a branch that does not exist yet: the branch (dev), ref or commit id it starts at; the default branch if omitted" },
                "files": { "type": "array", "items": file, "description": "the files to write or edit, each with content, a source or edits" },
                "remove": { "type": "array", "items": { "type": "string" }, "description": "paths to delete" },
                "message": { "type": "string", "description": "the commit message" },
                "base": { "type": "string", "description": "the commit id the branch must still be at, from forge_log; the commit is refused if the branch moved since. Omitted, it goes on top of the newest" },
                "snapshot": { "type": "string", "description": "the files snapshot (64 hex digits) every source is read at, from a files read; omitted, the newest" },
            }),
            &["repo", "files", "message"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: CommitArgs) -> Result<Run, Error> {
        if args.files.is_empty() && args.remove.is_empty() {
            return Err(invalid("a commit writes or removes at least one file"));
        }
        let (mut puts, edited) = gathered(ctx, args.files, &args.snapshot)?;
        if puts.is_empty() && edited.is_empty() && args.remove.is_empty() {
            return Err(invalid(
                "the sources hold no files and nothing is removed, so the commit changes nothing",
            ));
        }
        let held = found(ctx, &args.repo)?;
        let (kind, default) = match &held {
            Some((repo, _)) => (repo.hash, repo.settings.head.clone()),
            None => (HashKind::Sha1, Settings::default().head),
        };
        let hash = forge::hash_of(kind);
        let reading = Reading {
            ctx,
            repo: &args.repo,
            hash,
        };
        let branch = branch_ref(args.branch.as_deref(), &default)?;
        let refs = match held {
            Some(_) => reading.refs()?,
            None => BTreeMap::new(),
        };
        let head = refs.get(&branch).copied();
        if let Some(base) = &args.base
            && Some(oid(hash, base)?) != head
        {
            let at = head.map_or("no commit".to_owned(), |head| head.to_hex());
            return Err(stale(format!(
                "{} is at {at}, not at base {base}: read it again and commit on top",
                String::from_utf8_lossy(&branch)
            )));
        }
        let parent = match (&args.from, head) {
            (None, _) => head.or_else(|| refs.get(&default).copied()),
            (Some(_), Some(head)) => {
                return Err(invalid(format!(
                    "{} exists, at {head}; from only starts a new branch",
                    String::from_utf8_lossy(&branch)
                )));
            }
            (Some(from), None) if held.is_none() => {
                return Err(not_found(format!(
                    "there is no repository {} for {from} to be in",
                    args.repo
                )));
            }
            (Some(from), None) => {
                let at = commit_of(ctx, &args.repo, revision(from))?;
                Some(oid(hash, &at)?)
            }
        };
        for Edited { path, steps } in edited {
            let Some(parent) = parent else {
                return Err(not_found(format!("there is no file at {path} to edit")));
            };
            let (mode, blob) = reading.file(parent, &path)?;
            let text = applied(reading.text(blob, &path)?, steps, &path)?;
            puts.push(Put {
                path,
                mode,
                bytes: text.into_bytes(),
            });
        }
        if let Some((_, bounds)) = &held {
            sized(bounds, &puts)?;
        }

        let mut objects = Objects::new();
        let mut edits = Edits::default();
        for Put { path, mode, bytes } in puts {
            let (dirs, name) = split(&path)?;
            let blob = kept(&mut objects, hash, Kind::Blob, bytes)?;
            if edits
                .at(&dirs)
                .put
                .insert(name.into(), (mode, blob))
                .is_some()
            {
                return Err(invalid(format!("{path} is written twice")));
            }
        }
        for path in &args.remove {
            let (dirs, name) = split(path)?;
            edits.at(&dirs).removed.insert(name.into());
        }
        let root = match parent {
            Some(parent) => reading.entries(parent)?,
            None => Entries::new(),
        };
        let tree = match rebuilt(&reading, "", root, edits, &mut objects)? {
            Some(tree) => tree,
            None => kept(&mut objects, hash, Kind::Tree, Vec::new())?,
        };
        let author = signature(ctx, seat.agent)?;
        let mut message = args.message.into_bytes();
        if !message.ends_with(b"\n") {
            message.push(b'\n');
        }
        let commit = gitcore::Commit {
            tree,
            parents: parent.into_iter().collect(),
            author: author.clone(),
            committer: author,
            extra: Vec::new(),
            message,
        };
        let commit = kept(&mut objects, hash, Kind::Commit, commit.serialize())?;
        if let Some((_, bounds)) = &held {
            counted(bounds, objects.len())?;
        }

        let pack = pack::write(objects.into_values(), hash).map_err(|e| invalid(e.to_string()))?;
        let pack = ctx.blob_put(HashKind::Sha256, forge::PACK_KIND, pack)?;
        let old = head.unwrap_or_else(|| hash.zero());
        let push = forge::Op::Push {
            repo: args.repo.clone(),
            request: request(old, commit, &branch),
            pack: Some(pack),
        };
        let create = held.is_none().then_some(forge::Op::Create {
            repo: args.repo,
            hash: kind,
        });
        let writes = create
            .into_iter()
            .chain([push])
            .map(|op| Write {
                target: forge::PROGRAM.into(),
                payload: abi::encode(&op),
            })
            .collect();
        let (commit, parent) = (commit.to_hex(), parent.map(|parent| parent.to_hex()));
        let read = Box::new(move |output: &[u8]| reported(output, &commit, parent.as_deref()));
        Ok(Run::Read { writes, read })
    }
}

fn reported(output: &[u8], commit: &str, parent: Option<&str>) -> Result<Value, Error> {
    let branch = moved(output)?;
    Ok(json!({ "ok": true, "pushed": branch, "commit": commit, "parent": parent }))
}

pub(crate) fn moved(output: &[u8]) -> Result<String, Error> {
    let lines = pktline::Reader::new(output).filter_map(|pkt| match pkt {
        Ok(Pkt::Data(data)) => Some(String::from_utf8_lossy(pktline::strip_newline(data))),
        _ => None,
    });
    for line in lines {
        if let Some(branch) = line.strip_prefix("ok ") {
            return Ok(branch.to_owned());
        }
        match line.strip_prefix("unpack ") {
            Some("ok") => {}
            Some(refused) => {
                return Err(capacity(format!(
                    "forge took none of the commit: {refused}; commit fewer or smaller files at a time"
                )));
            }
            None => return Err(stale(format!("forge did not move the branch: {line}"))),
        }
    }
    Err(unexpected(
        forge::PROGRAM,
        "Push",
        &String::from_utf8_lossy(output),
    ))
}
