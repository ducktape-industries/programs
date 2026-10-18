//! `forge_commit`, the agent's one write to forge: files written whole and paths removed, made
//! one git commit on a branch and pushed as the agent's account. Git's objects are built here
//! with `gitcore`: the trees on the paths the commit touches are rebuilt over its parent's, read
//! through forge's queries, and every other tree is the parent's as is. The new objects go up as
//! one pack blob the push names, so no file's bytes ride in the transaction. A repository that
//! does not exist yet is created in the same write, the agent's.
use std::collections::{BTreeMap, BTreeSet};

use ::program::{Error, ExecCtx, HashKind, capacity, code, invalid, not_found, stale};
use abi::role::identity::AccountNumber;
use forge::{Bounds, EntryKind, Query, Reply, Repo, Settings};
use gitcore::wire::pktline::{self, Pkt};
use gitcore::{Hash, Kind, Mode, Object, Oid, Signature, Tree, TreeEntry, pack};
use serde::Deserialize;
use serde_json::{Value, json};
use store::{PageRequest, PageResponse};

use super::ask;
use crate::agents::profile;
use crate::tools::{Run, Seat, Tool, Write, object, unexpected};

/// The objects a commit adds, by id: two files of the same bytes are one blob.
type Objects = BTreeMap<Oid, Object>;

fn kept(objects: &mut Objects, hash: Hash, kind: Kind, body: Vec<u8>) -> Result<Oid, Error> {
    let object = Object::new(kind, body);
    let id = object
        .id(hash)
        .map_err(|error| invalid(error.to_string()))?;
    objects.insert(id, object);
    Ok(id)
}

fn oid(hash: Hash, hex: &str) -> Result<Oid, Error> {
    Oid::from_hex(hash, hex)
        .map_err(|_| invalid(format!("{hex} is not an object id of this repository")))
}

/// The repository named `name` and forge's bounds, or `None` while no repository is named so.
fn found(ctx: &ExecCtx, name: &str) -> Result<Option<(Repo, Bounds)>, Error> {
    match ask(ctx, Query::Repo { repo: name.into() }) {
        Ok(Reply::Repo { repo, bounds, .. }) => Ok(Some((repo.repo, bounds))),
        Ok(other) => Err(unexpected(forge::PROGRAM, "Repo", &other)),
        Err(missing) if missing.code == code::NOT_FOUND => Ok(None),
        Err(refused) => Err(refused),
    }
}

/// Every item of a listing forge answers a page at a time.
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

fn mode(kind: EntryKind) -> Mode {
    match kind {
        EntryKind::File => Mode::Regular,
        EntryKind::Executable => Mode::Executable,
        EntryKind::Symlink => Mode::Symlink,
        EntryKind::Directory => Mode::Directory,
        EntryKind::Gitlink => Mode::Gitlink,
    }
}

/// One directory's entries, by name.
type Entries = BTreeMap<Vec<u8>, (Mode, Oid)>;

/// A repository as forge answers it.
struct Reading<'a> {
    ctx: &'a ExecCtx,
    repo: &'a str,
    hash: Hash,
}

impl Reading<'_> {
    /// Every ref of the repository and the object it names.
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

    /// The entries of the tree `at` names, or of a commit's root tree.
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
}

/// What a commit changes in one directory: files put in it, names removed from it, and the
/// directories beneath it that change.
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

/// A path inside a repository as its directories and its name: no empty, `.`, `..` or `.git`
/// part. A leading `/` is the repository's root.
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

/// The tree of the directory `dir` once `edits` apply over `entries`, its entries in the parent
/// commit; `None` once it holds nothing, as git keeps no empty directory.
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

/// Who the commit names as its author and committer: the agent by its name as identity knows
/// it (git's `<` and `>` and control characters left out), at an address no mail reaches (its
/// number at the network), at the block's time.
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

/// Git's receive-pack request moving `branch` from `old` to `new`, asking for git's report.
fn request(old: Oid, new: Oid, branch: &[u8]) -> Vec<u8> {
    let mut line = format!("{old} {new} ").into_bytes();
    line.extend_from_slice(branch);
    line.extend_from_slice(b"\0report-status");
    let mut request = Vec::new();
    pktline::push(&mut request, &line);
    request.extend_from_slice(pktline::flush());
    request
}

/// `branch` as the ref it names: `main` is `refs/heads/main`; omitted, the default branch.
fn branch_ref(branch: Option<&str>, default: &[u8]) -> Result<Vec<u8>, Error> {
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

/// The bounds forge checks a push's pack against, checked before the pack is made, so a commit
/// over them is refused in the tool's words.
fn within(bounds: &Bounds, files: &[FileArg], objects: usize) -> Result<(), Error> {
    if let Some(file) = files
        .iter()
        .find(|file| file.content.len() as u64 > bounds.max_object_size)
    {
        return Err(capacity(format!(
            "{} is {} bytes; forge takes files of at most {} bytes",
            file.path,
            file.content.len(),
            bounds.max_object_size
        )));
    }
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
    content: String,
    #[serde(default)]
    executable: bool,
}

#[derive(Deserialize)]
pub(crate) struct CommitArgs {
    repo: String,
    branch: Option<String>,
    files: Vec<FileArg>,
    #[serde(default)]
    remove: Vec<String>,
    message: String,
    base: Option<String>,
}

pub(crate) struct Commit;

impl Tool for Commit {
    const NAME: &'static str = "forge_commit";
    const DESCRIPTION: &'static str = "Commit files to a forge repository as one git commit on a branch, as your account, and push it. Each file is written whole; remove deletes paths (a directory with everything in it). Paths are inside the repository, like src/main.rs. A repository that does not exist yet is created, owned by you, and stays even if forge refuses the commit. An existing one takes the push only if its write list in the access program allows your account or harness: its owner alone unless the owner lists others, and a refusal says unauthorized. The commit goes on top of the branch's newest commit, and a new branch starts from the default branch; with base, only while the branch is still at base. Forge takes a bounded number of objects (each file and each changed directory is one) and of bytes per file in one push; a larger commit is refused, so split it. forge_log shows the commit.";
    const WRITES: bool = true;
    type Args = CommitArgs;

    fn schema() -> Value {
        let file = object(
            json!({
                "path": { "type": "string", "description": "a path inside the repository, like src/main.rs" },
                "content": { "type": "string", "description": "the whole file" },
                "executable": { "type": "boolean" },
            }),
            &["path", "content"],
        );
        object(
            json!({
                "repo": { "type": "string", "description": "a repository name, as forge_repos lists them; a new name creates it" },
                "branch": { "type": "string", "description": "the branch to commit on, like main; the repository's default branch if omitted" },
                "files": { "type": "array", "items": file, "description": "the files to write" },
                "remove": { "type": "array", "items": { "type": "string" }, "description": "paths to delete" },
                "message": { "type": "string", "description": "the commit message" },
                "base": { "type": "string", "description": "the commit id the branch must still be at, from forge_log; the commit is refused if the branch moved since. Omitted, it goes on top of the newest" },
            }),
            &["repo", "files", "message"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: CommitArgs) -> Result<Run, Error> {
        if args.files.is_empty() && args.remove.is_empty() {
            return Err(invalid("a commit writes or removes at least one file"));
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
        let parent = head.or_else(|| refs.get(&default).copied());

        let mut objects = Objects::new();
        let mut edits = Edits::default();
        for file in &args.files {
            let (dirs, name) = split(&file.path)?;
            let mode = match file.executable {
                true => Mode::Executable,
                false => Mode::Regular,
            };
            let content = file.content.as_bytes().to_vec();
            let blob = kept(&mut objects, hash, Kind::Blob, content)?;
            if edits
                .at(&dirs)
                .put
                .insert(name.into(), (mode, blob))
                .is_some()
            {
                return Err(invalid(format!("{} is written twice", file.path)));
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
            within(bounds, &args.files, objects.len())?;
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
        Ok(Run::Write(writes))
    }

    /// Git's report of the push: the branch it moved. A report that refuses it fails the call
    /// in forge's words: objects forge did not take (a commit over its bounds, which a new
    /// repository's first commit cannot be checked against before the repository exists) or
    /// a branch it did not move.
    fn render(output: &[u8]) -> Result<Value, Error> {
        let lines = pktline::Reader::new(output).filter_map(|pkt| match pkt {
            Ok(Pkt::Data(data)) => Some(String::from_utf8_lossy(pktline::strip_newline(data))),
            _ => None,
        });
        for line in lines {
            if let Some(branch) = line.strip_prefix("ok ") {
                return Ok(json!({ "ok": true, "pushed": branch }));
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
}
