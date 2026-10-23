//! `forge_commit`, the agent's one write to forge: files written whole, edited by span or copied by
//! id from what forge holds, and paths removed, over the parent's trees or a 3-way merge with another
//! commit, as one git commit pushed as one pack blob as the agent's account; a new repo is created.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{
    BlobId, Error, ExecCtx, HashKind, capacity, code, invalid, not_found, stale, wrong_state,
};
use abi::role::identity::AccountNumber;
use files::{EntryInfo, Node};
use forge::{Bounds, EntryKind, Query, Reply, Repo, Settings};
use gitcore::merge::{Clash, Conflict};
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
    made: &'a Objects,
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
        if let Some(made) = self.made.get(&at) {
            let tree = Tree::parse(&made.body, self.hash).map_err(git_refusal)?;
            return Ok(tree
                .entries
                .into_iter()
                .map(|entry| (entry.name, (entry.mode, entry.id)))
                .collect());
        }
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

    fn entry(&self, at: Oid, path: &str) -> Result<Option<(Mode, Oid)>, Error> {
        let (dirs, name) = split(path)?;
        let mut entries = self.entries(at)?;
        for dir in dirs {
            entries = match entries.get(dir.as_bytes()) {
                Some((Mode::Directory, tree)) => self.entries(*tree)?,
                _ => return Ok(None),
            };
        }
        Ok(entries.get(name.as_bytes()).copied())
    }

    fn file(&self, at: Oid, path: &str) -> Result<(Mode, Oid), Error> {
        match self.entry(at, path)? {
            Some((mode @ (Mode::Regular | Mode::Executable), blob)) => Ok((mode, blob)),
            Some(_) => Err(invalid(format!("{path} is not a file"))),
            None => Err(not_found(format!("there is no file at {path} to edit"))),
        }
    }

    fn text(&self, id: Oid, path: &str) -> Result<String, Error> {
        let advice = "it can't be edited by span; write it whole";
        if let Some(made) = self.made.get(&id) {
            return String::from_utf8(made.body.clone())
                .map_err(|_| invalid(format!("{path} is not UTF-8 text: {advice}")));
        }
        super::text_of(self.ctx, self.repo, &id.to_hex(), path, advice)
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
    let twice = |name: &[u8]| {
        invalid(format!(
            "{} is changed twice: combine its changes into one file entry",
            joined(dir, name)
        ))
    };
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
            Some(_) => {
                return Err(invalid(format!(
                    "{path} is a file, not a directory: choose a directory or write directly to the file"
                )));
            }
        };
        match rebuilt(reading, &path, inside, edits, objects)? {
            Some(tree) => entries.insert(name, (Mode::Directory, tree)),
            None => entries.remove(&name),
        };
    }
    for (name, file) in put {
        let over_directory = matches!(entries.get(&name), Some((Mode::Directory, _)));
        if over_directory && file.0 != Mode::Directory {
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
        return Err(invalid(format!(
            "{branch:?} is not a branch name: use a valid branch name under refs/heads/"
        )));
    }
    Ok(reference)
}

struct Put {
    path: String,
    mode: Mode,
    body: Body,
}

enum Body {
    Bytes(Vec<u8>),
    Held(Oid),
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
    Ok(Some(Put {
        path,
        mode,
        body: Body::Bytes(bytes),
    }))
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

struct Copied {
    path: String,
    named: Named,
}

enum Named {
    Copy(CopyArg),
    Blob { id: String, executable: bool },
}

struct Gathered {
    puts: Vec<Put>,
    edited: Vec<Edited>,
    copied: Vec<Copied>,
}

fn gathered(
    ctx: &ExecCtx,
    files: Vec<FileArg>,
    snapshot_arg: &Option<String>,
) -> Result<Gathered, Error> {
    let named = snapshot(snapshot_arg)?;
    let at = match files.iter().any(|file| file.source.is_some()) {
        true => resolved(ctx, named)?,
        false => named,
    };
    let mut gathered = Gathered {
        puts: Vec::new(),
        edited: Vec::new(),
        copied: Vec::new(),
    };
    for file in files {
        if let Some(edits) = file.edits {
            let alone = file.content.is_none()
                && file.source.is_none()
                && file.copy.is_none()
                && file.blob.is_none()
                && file.executable.is_none();
            if !alone {
                return Err(invalid(format!(
                    "{}: edits go alone; the file keeps its mode",
                    file.path
                )));
            }
            if edits.is_empty() {
                return Err(invalid(format!("{}: edits names no edit", file.path)));
            }
            gathered.edited.push(Edited {
                path: file.path,
                steps: steps(edits)?,
            });
            continue;
        }
        if file.executable.is_some() && (file.source.is_some() || file.copy.is_some()) {
            return Err(invalid(format!(
                "{}: executable goes with content or blob; a source or a copy keeps its mode",
                file.path
            )));
        }
        let executable = file.executable.unwrap_or(false);
        let path = file.path;
        match (file.content, file.source, file.copy, file.blob) {
            (Some(content), None, None, None) => gathered.puts.push(Put {
                path,
                mode: file_mode(executable),
                body: Body::Bytes(content.into_bytes()),
            }),
            (None, Some(source), None, None) => {
                gathered
                    .puts
                    .extend(sourced(ctx, &path, &source, at.as_ref())?);
            }
            (None, None, Some(copy), None) => gathered.copied.push(Copied {
                path,
                named: Named::Copy(copy),
            }),
            (None, None, None, Some(id)) => gathered.copied.push(Copied {
                path,
                named: Named::Blob { id, executable },
            }),
            (None, None, None, None) => {
                return Err(invalid(format!(
                    "{path}: give content, source, copy, blob or edits"
                )));
            }
            _ => {
                return Err(invalid(format!(
                    "{path}: give only one of content, source, copy and blob"
                )));
            }
        }
    }
    Ok(gathered)
}

fn sized(bounds: &Bounds, puts: &[Put]) -> Result<(), Error> {
    let over = puts.iter().find_map(|put| match &put.body {
        Body::Bytes(bytes) if bytes.len() as u64 > bounds.max_object_size => {
            Some((&put.path, bytes.len()))
        }
        Body::Bytes(_) | Body::Held(_) => None,
    });
    match over {
        Some((path, len)) => Err(capacity(format!(
            "{path} is {len} bytes; forge takes files of at most {} bytes",
            bounds.max_object_size
        ))),
        None => Ok(()),
    }
}

fn held_blob(ctx: &ExecCtx, hash: Hash, hex: &str) -> Result<Oid, Error> {
    let id = oid(hash, hex)?;
    match ctx.blob_stat(forge::blob_id_of(&id)) {
        Some(header) if header.kind == "blob" => Ok(id),
        Some(header) => Err(invalid(format!(
            "{hex} is a git {}, not a blob: copy a directory by its path",
            header.kind
        ))),
        None => Err(not_found(format!("forge holds no object {hex}"))),
    }
}

fn copied_entry(
    ctx: &ExecCtx,
    target: &Reading,
    parent: Option<Oid>,
    copy: &CopyArg,
) -> Result<(Mode, Oid), Error> {
    let path = copy.path.trim_end_matches('/');
    let (reading, at) = match copy.repo.as_deref().filter(|repo| *repo != target.repo) {
        None => {
            let at = match &copy.rev {
                Some(rev) => oid(target.hash, &commit_of(ctx, target.repo, revision(rev))?)?,
                None => parent.ok_or_else(|| {
                    not_found(format!(
                        "{} has no commit to copy {path} from: name a rev or another repo",
                        target.repo
                    ))
                })?,
            };
            let reading = Reading {
                ctx,
                repo: target.repo,
                hash: target.hash,
                made: target.made,
            };
            (reading, at)
        }
        Some(repo) => {
            let Some((held, _)) = found(ctx, repo)? else {
                return Err(not_found(format!(
                    "there is no repository {repo} to copy from"
                )));
            };
            let hash = forge::hash_of(held.hash);
            if hash != target.hash {
                return Err(invalid(format!(
                    "{repo} and {} name objects by different hashes: copy between repositories of one hash",
                    target.repo
                )));
            }
            let rev = match &copy.rev {
                Some(rev) => revision(rev),
                None => forge::Revision::Ref(held.settings.head),
            };
            let at = oid(hash, &commit_of(ctx, repo, rev)?)?;
            let made = target.made;
            (
                Reading {
                    ctx,
                    repo,
                    hash,
                    made,
                },
                at,
            )
        }
    };
    reading.entry(at, path)?.ok_or_else(|| {
        not_found(format!(
            "there is nothing at {path} in {} at {}",
            reading.repo,
            at.to_hex()
        ))
    })
}

struct Merged {
    rev: String,
    commit: Oid,
    tree: Oid,
    objects: Objects,
    conflicts: Vec<Conflict>,
}

struct Stored<'a>(&'a ExecCtx);

impl gitcore::Objects for Stored<'_> {
    fn get(&self, id: &Oid) -> gitcore::Result<Option<Object>> {
        let Some(blob) = self.0.blob_get(forge::blob_id_of(id)) else {
            return Ok(None);
        };
        let kind = Kind::parse(blob.kind.as_bytes())?;
        Ok(Some(Object::new(kind, blob.body)))
    }

    fn has(&self, id: &Oid) -> gitcore::Result<bool> {
        Ok(self.0.blob_stat(forge::blob_id_of(id)).is_some())
    }
}

fn git_refusal(error: gitcore::Error) -> Error {
    match error {
        gitcore::Error::CapReached => capacity(
            "the merge reaches past forge's bounds (log_walk commits to the merge base, merge_cost per file): merge with git and push the result",
        ),
        gitcore::Error::MissingObject(id) => not_found(format!("forge holds no object {id}")),
        other => invalid(other.to_string()),
    }
}

fn bound(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn tree_of(store: &Stored, id: &Oid) -> Result<Oid, Error> {
    let Some(object) = gitcore::Objects::get(store, id).map_err(git_refusal)? else {
        return Err(not_found(format!("forge holds no object {id}")));
    };
    let commit = gitcore::Commit::parse(&object.body, id.hash()).map_err(git_refusal)?;
    Ok(commit.tree)
}

fn merging(
    ctx: &ExecCtx,
    repo: &str,
    hash: Hash,
    held: &Option<(Repo, Bounds)>,
    (into, branch): (Option<Oid>, &[u8]),
    rev: &str,
) -> Result<Merged, Error> {
    let branch = String::from_utf8_lossy(branch);
    let (Some(ours), Some((_, bounds))) = (into, held) else {
        return Err(not_found(format!(
            "{branch} has no commit for {rev} to merge into: start the branch with from"
        )));
    };
    let hex = match commit_of(ctx, repo, revision(rev)) {
        Ok(hex) => hex,
        Err(missing) if missing.code == code::NOT_FOUND => {
            return Err(not_found(format!("there is no {rev} in {repo} to merge")));
        }
        Err(refused) => return Err(refused),
    };
    let theirs = oid(hash, &hex)?;
    match ctx.blob_stat(forge::blob_id_of(&theirs)) {
        Some(header) if header.kind == "commit" => {}
        Some(header) => {
            return Err(invalid(format!(
                "{rev} names a git {} ({hex}), not a commit: merge a branch, ref or commit id",
                header.kind
            )));
        }
        None => return Err(not_found(format!("forge holds no commit {hex} to merge"))),
    }
    let store = Stored(ctx);
    let walk = bound(bounds.log_walk);
    let Some(base) =
        gitcore::walk::merge_base(&store, &ours, &theirs, walk).map_err(git_refusal)?
    else {
        return Err(wrong_state(format!(
            "{branch} and {rev} share no history: there is no merge base to merge from"
        )));
    };
    if base == theirs {
        return Err(wrong_state(format!(
            "{branch} already holds {rev} ({hex}): there is nothing to merge"
        )));
    }
    let (base, ours, theirs_tree) = (
        tree_of(&store, &base)?,
        tree_of(&store, &ours)?,
        tree_of(&store, &theirs)?,
    );
    let cost = bound(bounds.merge_cost);
    let merge = gitcore::merge::trees(&store, Some(&base), &ours, &theirs_tree, cost)
        .map_err(git_refusal)?;
    Ok(Merged {
        rev: rev.to_owned(),
        commit: theirs,
        tree: merge.tree,
        objects: merge.objects,
        conflicts: merge.conflicts,
    })
}

fn clash_words(clash: Clash) -> &'static str {
    match clash {
        Clash::Lines => {
            "both sides changed the same or adjacent lines: it holds both sides merged, each clash between a <<<<<<< ours line, the branch's lines, a ======= line, merge's lines and a >>>>>>> theirs line, so edits replacing each marked span resolve it"
        }
        Clash::Searched => {
            "edits too interleaved to merge within forge's merge_cost: it holds the branch's version whole, so the resolution carries every change merge made to it"
        }
        Clash::Binary => "a binary file both sides changed: it holds the branch's version whole",
        Clash::Mode => {
            "both sides changed its mode: it holds the branch's version whole, so the resolution carries every change merge made to it"
        }
        Clash::Deleted => {
            "deleted on one side, changed on the other: it holds the branch's side, absent where the branch deleted it; remove leaves it deleted, writing it keeps it"
        }
        Clash::Kind => {
            "changed on both sides, and not as two files: it holds the branch's side whole"
        }
    }
}

fn settled(merged: &Merged, puts: &[Put], removed: &[String]) -> Result<(), Error> {
    let given: Vec<&str> = puts
        .iter()
        .map(|put| put.path.as_str())
        .chain(removed.iter().map(String::as_str))
        .map(|path| path.trim_matches('/'))
        .collect();
    let covered = |path: &str| {
        given.iter().any(|done| {
            let under = path
                .strip_prefix(done)
                .is_some_and(|rest| rest.starts_with('/'));
            path == *done || under
        })
    };
    let left: Vec<String> = merged
        .conflicts
        .iter()
        .map(|conflict| (String::from_utf8_lossy(&conflict.path), conflict.clash))
        .filter(|(path, _)| !covered(path))
        .map(|(path, clash)| format!("{path} ({})", clash_words(clash)))
        .collect();
    if left.is_empty() {
        return Ok(());
    }
    Err(wrong_state(format!(
        "merging {} leaves conflicts the files don't resolve, so nothing was pushed: {}; write or edit each of these paths in files, or remove it, as the resolution",
        merged.rev,
        left.join("; ")
    )))
}

fn holds_a_conflict_marker(body: &[u8]) -> bool {
    body.split(|byte| *byte == b'\n')
        .any(|line| matches!(line, b"<<<<<<< ours" | b">>>>>>> theirs"))
}

fn unmarked(ctx: &ExecCtx, merged: &Merged, puts: &[Put]) -> Result<(), Error> {
    let store = Stored(ctx);
    let mut marked = Vec::new();
    for put in puts {
        let path = put.path.trim_matches('/');
        let clashed_in_lines = merged
            .conflicts
            .iter()
            .any(|conflict| conflict.clash == Clash::Lines && conflict.path == path.as_bytes());
        if !clashed_in_lines {
            continue;
        }
        let held;
        let body = match &put.body {
            Body::Bytes(bytes) => bytes.as_slice(),
            Body::Held(id) => {
                held = gitcore::Objects::get(&store, id).map_err(git_refusal)?;
                held.as_ref()
                    .map_or(&[][..], |object| object.body.as_slice())
            }
        };
        if holds_a_conflict_marker(body) {
            marked.push(path.to_owned());
        }
    }
    if marked.is_empty() {
        return Ok(());
    }
    Err(wrong_state(format!(
        "merging {} leaves conflict markers in {}, so nothing was pushed: replace each span from a <<<<<<< ours line to its >>>>>>> theirs line with the lines it should hold",
        merged.rev,
        marked.join(", ")
    )))
}

fn gone_by_the_merge(
    merged: Option<&Merged>,
    reading: &Reading,
    tree: Option<Oid>,
    path: &str,
) -> Result<bool, Error> {
    let (Some(merged), Some(tree)) = (merged, tree) else {
        return Ok(false);
    };
    let wanted = path.trim_matches('/').as_bytes();
    let clashed = merged
        .conflicts
        .iter()
        .any(|conflict| conflict.path == wanted);
    if !clashed {
        return Ok(false);
    }
    Ok(reading.entry(tree, path)?.is_none())
}

fn reached_from(
    hash: Hash,
    root: Oid,
    written: &Objects,
    mut merged: Objects,
) -> Result<Objects, Error> {
    let mut reached = Objects::new();
    let mut pending = vec![root];
    while let Some(id) = pending.pop() {
        let from_merge = merged.remove(&id);
        let Some(object) = from_merge.as_ref().or_else(|| written.get(&id)) else {
            continue;
        };
        if matches!(object.kind, Kind::Tree) {
            let tree = Tree::parse(&object.body, hash).map_err(git_refusal)?;
            pending.extend(tree.entries.into_iter().map(|entry| entry.id));
        }
        if let Some(object) = from_merge {
            reached.insert(id, object);
        }
    }
    Ok(reached)
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
    copy: Option<CopyArg>,
    blob: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CopyArg {
    repo: Option<String>,
    rev: Option<String>,
    path: String,
}

#[derive(Deserialize)]
pub(crate) struct CommitArgs {
    repo: String,
    branch: Option<String>,
    from: Option<String>,
    merge: Option<String>,
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
    const DESCRIPTION: &'static str = "Commit files to a forge repository as one git commit on a branch, as your account, and push it. Each item of files is written whole, from inline content or from a files source, or edited by span: a source file lands at its path, and a source directory lands under its path with every file beneath it, each in its files mode; an item with edits changes part of a text file the branch holds (each span must occur exactly once in it, as in files_edit), keeps its mode, and is refused naming where a span occurs when it is not there once. An item may instead land what forge already holds, with none of its bytes passing through you: copy names a path at a rev of this repository or of another of the same hash, a file landing with its mode and a directory as its whole tree (replacing any directory at path), and blob names a git blob id forge holds; a move is a copy plus a remove. Every source is read at one files snapshot, the newest if omitted, so a workspace kept in files commits without re-sending it. A source directory adds and replaces files and never deletes; remove deletes paths (a directory with everything in it). Paths are inside the repository, like src/main.rs. A repository that does not exist yet is created, owned by you, and stays even if forge refuses the commit. An existing one takes the push only if its write list in the access program allows your account or harness: its owner alone unless the owner lists others, and a refusal says unauthorized. The commit goes on top of the branch's newest commit, and a new branch starts from from (a branch, ref or commit id), or from the default branch; with base, only while the branch is still at base, and a branch that moved since the edits read it is refused, never overwritten. With merge (a branch, ref or commit id), the commit is a merge commit: the branch's head is its first parent and merge's commit, which forge must hold and the branch must not hold yet, its second; its tree is the two merged three ways from their merge base, as forge's SquashMerge merges and within its bounds, and files, edits and remove apply to that merged tree as the resolution. A path both sides changed that does not merge cleanly must be written, edited or removed, or the commit is refused naming each such path and what it holds: a text file whose lines clash holds both sides merged, each clash between a <<<<<<< ours line, the branch's lines, a ======= line, merge's lines and a >>>>>>> theirs line, so edits replacing each marked span resolve it, and a commit still holding a <<<<<<< ours or >>>>>>> theirs line in such a file is refused naming it; any other clash holds the branch's side whole. With merge, files may be empty. Forge takes a bounded number of objects (each file and each changed directory is one) and of bytes per file in one push; a larger commit is refused, so split it. The answer names the commit, which is the branch's head after the push, its parent, and with merge the merged commit.";
    const WRITES: bool = true;
    type Args = CommitArgs;

    fn schema() -> Value {
        let mut copy = object(
            json!({
                "repo": { "type": "string", "description": "the repository to copy from; this one if omitted" },
                "rev": { "type": "string", "description": "a branch, ref or commit id there; omitted, this branch's head, or the other repository's default branch" },
                "path": { "type": "string", "description": "a file or a directory there, like src/lib.rs or crates/app" },
            }),
            &["path"],
        );
        copy["description"] = json!(
            "instead of content: what forge already holds at a path, landed at path without its bytes passing through you: a file keeps its mode, and a directory lands as its whole tree, replacing any directory at path"
        );
        let file = object(
            json!({
                "path": { "type": "string", "description": "where it lands in the repository, like src/main.rs; for a source directory, the directory its files land under, \"\" for the root" },
                "content": { "type": "string", "description": "the whole file, inline" },
                "source": { "type": "string", "description": "instead of content: an absolute files path, a file or a directory, read at snapshot" },
                "executable": { "type": "boolean", "description": "with content or blob only: whether the file is executable; a source or a copy keeps its mode" },
                "edits": { "type": "array", "items": edit_schema(), "description": "instead of content or source: edits of the text file the branch holds at path, applied in order" },
                "copy": copy,
                "blob": { "type": "string", "description": "instead of content: the id of a git blob forge holds, landed at path as it is" },
            }),
            &["path"],
        );
        object(
            json!({
                "repo": { "type": "string", "description": "a repository name, as forge_repos lists them; a new name creates it" },
                "branch": { "type": "string", "description": "the branch to commit on, like main; the repository's default branch if omitted" },
                "from": { "type": "string", "description": "for a branch that does not exist yet: the branch (dev), ref or commit id it starts at; the default branch if omitted" },
                "merge": { "type": "string", "description": "a branch, ref or commit id forge holds to merge in: the commit gets it as its second parent, and its tree is the 3-way merge of the two that files, edits and remove resolve" },
                "files": { "type": "array", "items": file, "description": "the files to write or edit, each with content, a source, a copy, a blob or edits" },
                "remove": { "type": "array", "items": { "type": "string" }, "description": "paths to delete" },
                "message": { "type": "string", "description": "the commit message" },
                "base": { "type": "string", "description": "the commit id the branch must still be at, from forge_log; the commit is refused if the branch moved since. Omitted, it goes on top of the newest" },
                "snapshot": { "type": "string", "description": "the files snapshot (64 hex digits) every source is read at, from a files read; omitted, the newest" },
            }),
            &["repo", "files", "message"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: CommitArgs) -> Result<Run, Error> {
        let changes_nothing =
            args.merge.is_none() && args.files.is_empty() && args.remove.is_empty();
        if changes_nothing {
            return Err(invalid(
                "a commit writes or removes at least one file, or merges another commit",
            ));
        }
        let Gathered {
            mut puts,
            edited,
            copied,
        } = gathered(ctx, args.files, &args.snapshot)?;
        let sources_change_nothing = args.merge.is_none()
            && puts.is_empty()
            && edited.is_empty()
            && copied.is_empty()
            && args.remove.is_empty();
        if sources_change_nothing {
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
        let none = Objects::new();
        let reading = Reading {
            ctx,
            repo: &args.repo,
            hash,
            made: &none,
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
                    "{} exists, at {head}; from only starts a new branch: omit from to commit on the existing branch or choose a new branch",
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
        let merged = match &args.merge {
            Some(rev) => {
                let into = (parent, branch.as_slice());
                Some(merging(ctx, &args.repo, hash, &held, into, rev)?)
            }
            None => None,
        };
        let reading = Reading {
            made: merged.as_ref().map_or(&none, |merged| &merged.objects),
            ..reading
        };
        let base_tree = match &merged {
            Some(merged) => Some(merged.tree),
            None => parent,
        };
        for Edited { path, steps } in edited {
            let Some(at) = base_tree else {
                return Err(not_found(format!("there is no file at {path} to edit")));
            };
            let (mode, blob) = reading.file(at, &path)?;
            let text = applied(reading.text(blob, &path)?, steps, &path)?;
            puts.push(Put {
                path,
                mode,
                body: Body::Bytes(text.into_bytes()),
            });
        }
        for Copied { path, named } in copied {
            let (mode, id) = match named {
                Named::Copy(copy) => copied_entry(ctx, &reading, parent, &copy)?,
                Named::Blob { id, executable } => {
                    (file_mode(executable), held_blob(ctx, hash, &id)?)
                }
            };
            puts.push(Put {
                path,
                mode,
                body: Body::Held(id),
            });
        }
        if let Some(merged) = &merged {
            settled(merged, &puts, &args.remove)?;
            unmarked(ctx, merged, &puts)?;
        }
        if let Some((_, bounds)) = &held {
            sized(bounds, &puts)?;
        }

        let mut objects = Objects::new();
        let mut edits = Edits::default();
        for Put { path, mode, body } in puts {
            let (dirs, name) = split(&path)?;
            let blob = match body {
                Body::Bytes(bytes) => kept(&mut objects, hash, Kind::Blob, bytes)?,
                Body::Held(id) => id,
            };
            if edits
                .at(&dirs)
                .put
                .insert(name.into(), (mode, blob))
                .is_some()
            {
                return Err(invalid(format!(
                    "{path} is written twice: combine its changes into one file entry"
                )));
            }
        }
        for path in &args.remove {
            if gone_by_the_merge(merged.as_ref(), &reading, base_tree, path)? {
                continue;
            }
            let (dirs, name) = split(path)?;
            edits.at(&dirs).removed.insert(name.into());
        }
        let root = match base_tree {
            Some(tree) => reading.entries(tree)?,
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
        let second = merged.as_ref().map(|merged| merged.commit);
        if let Some(merged) = merged {
            let reached = reached_from(hash, tree, &objects, merged.objects)?;
            objects.extend(reached);
        }
        let commit = gitcore::Commit {
            tree,
            parents: parent.into_iter().chain(second).collect(),
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
        let second = second.map(|second| second.to_hex());
        let read = Box::new(move |output: &[u8]| {
            reported(output, &commit, parent.as_deref(), second.as_deref())
        });
        Ok(Run::Read { writes, read })
    }
}

fn reported(
    output: &[u8],
    commit: &str,
    parent: Option<&str>,
    merged: Option<&str>,
) -> Result<Value, Error> {
    let branch = moved(output)?;
    let mut said = json!({ "ok": true, "pushed": branch, "commit": commit, "parent": parent });
    if let Some(merged) = merged {
        said["merged"] = json!(merged);
    }
    Ok(said)
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
