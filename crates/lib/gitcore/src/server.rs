// Server flows the host program calls: admit a pushed pack, decide ref moves, and the whole push.

use crate::error::{Error, Result};
use crate::object::{Commit, Kind, Mode, Object, Tag, Tree};
use crate::oid::{Hash, Oid};
use crate::pack::{self, Limits};
use crate::store::{Objects, WriteObjects};
use crate::walk::{Verdict, is_ancestor};
use crate::wire::receive::{RefCommand, parse_request, report_status};
use std::collections::{BTreeMap, BTreeSet};

pub fn admit_pack<S: WriteObjects + ?Sized>(
    store: &mut S,
    pack: &[u8],
    hash: Hash,
    limits: &Limits,
) -> Result<Vec<Oid>> {
    let objects = pack::read(pack, hash, limits, |id| store.get(id))?;
    let in_pack: BTreeSet<Oid> = objects.iter().map(|(id, _)| *id).collect();
    for (id, object) in &objects {
        let right_hash = id.hash() == hash;
        if !right_hash {
            return Err(Error::WrongHash {
                expected: hash,
                actual: id.hash(),
            });
        }
        for referenced in references(object, hash)? {
            let held = in_pack.contains(&referenced) || store.has(&referenced)?;
            if !held {
                return Err(Error::MissingObject(referenced));
            }
        }
    }
    let mut stored = Vec::with_capacity(objects.len());
    for (id, object) in objects {
        store.keep(id, object)?;
        stored.push(id);
    }
    Ok(stored)
}

fn references(object: &Object, hash: Hash) -> Result<Vec<Oid>> {
    match object.kind {
        Kind::Blob => Ok(Vec::new()),
        Kind::Tree => Ok(Tree::parse(&object.body, hash)?
            .entries
            .into_iter()
            .filter(|entry| entry.mode != Mode::Gitlink)
            .map(|entry| entry.id)
            .collect()),
        Kind::Commit => {
            let commit = Commit::parse(&object.body, hash)?;
            let mut ids = commit.parents;
            ids.push(commit.tree);
            Ok(ids)
        }
        Kind::Tag => Ok(vec![Tag::parse(&object.body, hash)?.object]),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Policy {
    pub allow_force: bool,
    pub allow_delete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefUpdate {
    Set(Oid),
    Delete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Invalid,
    Duplicate,
    BadName,
    StaleOld,
    NotHeld,
    NotACommit,
    NonFastForward,
    DeleteRefused,
    CapReached,
    UnpackFailed,
}

impl Refusal {
    pub fn as_bytes(self) -> &'static [u8] {
        match self {
            Refusal::Invalid => b"invalid command",
            Refusal::Duplicate => b"duplicate ref: send each ref update once",
            Refusal::BadName => b"funny refname: choose a valid full Git ref name",
            Refusal::StaleOld => b"stale info",
            Refusal::NotHeld => {
                b"missing necessary objects: upload the referenced objects before moving the ref"
            }
            Refusal::NotACommit => b"branch must point at a commit",
            Refusal::NonFastForward => b"non-fast-forward",
            Refusal::DeleteRefused => b"deletion prohibited",
            Refusal::CapReached => b"history too long to verify; push in smaller steps",
            Refusal::UnpackFailed => b"n/a (unpacker error)",
        }
    }
}

pub type Decision = std::result::Result<RefUpdate, Refusal>;

pub fn apply_commands<S: Objects + ?Sized>(
    store: &S,
    current_refs: &BTreeMap<Vec<u8>, Oid>,
    commands: &[RefCommand],
    policy: &Policy,
    cap: usize,
) -> Result<Vec<(Vec<u8>, Decision)>> {
    let mut decided = Vec::with_capacity(commands.len());
    let mut seen: BTreeSet<&[u8]> = BTreeSet::new();
    for command in commands {
        let first = seen.insert(&command.name);
        let decision = if first {
            decide(store, current_refs, command, policy, cap)?
        } else {
            Err(Refusal::Duplicate)
        };
        decided.push((command.name.clone(), decision));
    }
    Ok(decided)
}

fn decide<S: Objects + ?Sized>(
    store: &S,
    current_refs: &BTreeMap<Vec<u8>, Oid>,
    command: &RefCommand,
    policy: &Policy,
    cap: usize,
) -> Result<Decision> {
    let name_ok = valid_ref_name(&command.name);
    if !name_ok {
        return Ok(Err(Refusal::BadName));
    }
    let current = current_refs.get(&command.name);
    let old_matches = match current {
        Some(id) => *id == command.old,
        None => command.old.is_zero(),
    };
    if !old_matches {
        return Ok(Err(Refusal::StaleOld));
    }
    let is_delete = command.new.is_zero();
    if is_delete {
        let nothing_to_do = current.is_none();
        if nothing_to_do {
            return Ok(Err(Refusal::Invalid));
        }
        if !policy.allow_delete {
            return Ok(Err(Refusal::DeleteRefused));
        }
        return Ok(Ok(RefUpdate::Delete));
    }
    let Some(target) = store.get(&command.new)? else {
        return Ok(Err(Refusal::NotHeld));
    };
    let is_branch = command.name.starts_with(b"refs/heads/");
    if !is_branch {
        return Ok(Ok(RefUpdate::Set(command.new)));
    }
    let points_at_commit = target.kind == Kind::Commit;
    if !points_at_commit {
        return Ok(Err(Refusal::NotACommit));
    }
    let Some(previous) = current else {
        return Ok(Ok(RefUpdate::Set(command.new)));
    };
    if policy.allow_force {
        return Ok(Ok(RefUpdate::Set(command.new)));
    }
    let verdict = is_ancestor(store, previous, &command.new, cap)?;
    Ok(match verdict {
        Verdict::Yes => Ok(RefUpdate::Set(command.new)),
        Verdict::No => Err(Refusal::NonFastForward),
        Verdict::CapReached => Err(Refusal::CapReached),
    })
}

pub fn valid_ref_name(name: &[u8]) -> bool {
    let Some(rest) = name.strip_prefix(b"refs/") else {
        return false;
    };
    let has_component = !rest.is_empty();
    let no_bad_bytes = !name.iter().any(|byte| {
        byte.is_ascii_control()
            || matches!(byte, b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\')
    });
    let no_double_dot = !name.windows(2).any(|w| w == b".." || w == b"@{");
    let ends_well = !name.ends_with(b"/") && !name.ends_with(b".");
    let components_ok = rest.split(|byte| *byte == b'/').all(|component| {
        !component.is_empty() && !component.starts_with(b".") && !component.ends_with(b".lock")
    });
    has_component && no_bad_bytes && no_double_dot && ends_well && components_ok
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PushOutcome {
    pub report: Vec<u8>,
    pub moves: Vec<(Vec<u8>, RefUpdate)>,
    pub stored: Vec<Oid>,
    pub unpack_error: Option<Error>,
    pub refusals: Vec<(Vec<u8>, Refusal)>,
}

#[allow(clippy::too_many_arguments)]
pub fn push<S: WriteObjects + ?Sized>(
    store: &mut S,
    current_refs: &BTreeMap<Vec<u8>, Oid>,
    request: &[u8],
    pack: &[u8],
    hash: Hash,
    limits: &Limits,
    policy: &Policy,
    cap: usize,
) -> Result<PushOutcome> {
    let request = parse_request(request, hash)?;
    let carries_a_pack = !request.pack.is_empty();
    if carries_a_pack {
        return Err(Error::BadRequest);
    }
    let wants_report =
        request.has_capability(b"report-status") || request.has_capability(b"report-status-v2");
    let sideband = request.has_capability(b"side-band-64k");
    let has_pack = !pack.is_empty();
    let admitted = if has_pack {
        admit_pack(store, pack, hash, limits)
    } else {
        Ok(Vec::new())
    };
    let (stored, unpack_error) = match admitted {
        Ok(stored) => (stored, None),
        Err(Error::Storage) => return Err(Error::Storage),
        Err(error) => (Vec::new(), Some(error)),
    };
    let decisions = match unpack_error {
        Some(_) => request
            .commands
            .iter()
            .map(|command| (command.name.clone(), Err(Refusal::UnpackFailed)))
            .collect(),
        None => apply_commands(store, current_refs, &request.commands, policy, cap)?,
    };
    let mut moves = Vec::new();
    let mut refusals = Vec::new();
    let mut lines = Vec::with_capacity(decisions.len());
    for (name, decision) in decisions {
        match decision {
            Ok(update) => {
                moves.push((name.clone(), update));
                lines.push((name, Ok(())));
            }
            Err(refusal) => {
                refusals.push((name.clone(), refusal));
                lines.push((name, Err(refusal.as_bytes().to_vec())));
            }
        }
    }
    let unpack_text = unpack_error
        .as_ref()
        .map(|error| format!("{error}").into_bytes());
    let report = if wants_report {
        report_status(unpack_text.as_deref(), &lines, sideband)
    } else {
        Vec::new()
    };
    Ok(PushOutcome {
        report,
        moves,
        stored,
        unpack_error,
        refusals,
    })
}
