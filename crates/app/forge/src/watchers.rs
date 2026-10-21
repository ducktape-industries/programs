//! Watchers: programs told of every ref forge moves, every change it opens, closes or merges and
//! every dispatch a writer sends, each event a message of its own (`deliver ++ borsh(Watched)`)
//! whose refusal never fails forge's write.
use ::program::{Error, ExecCtx, Reply, invalid, not_found};
use gitcore::{Kind, Objects};

use crate::contract::{Principal, Revision, Watched};
use crate::objects::ObjectStore;
use crate::ops::refusal_of;
use crate::state::{WATCHERS, load_repo, repo_hash, resolve};

pub(crate) fn watch(ctx: &ExecCtx, deliver: Vec<u8>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    match deliver.is_empty() {
        true => WATCHERS.remove(ctx, &program),
        false => WATCHERS.put(ctx, &program, &deliver),
    }
    Ok(())
}

pub(crate) fn tell(ctx: &ExecCtx, event: &Watched) -> Result<(), Error> {
    let encoded = abi::encode(event);
    for (program, deliver) in WATCHERS.all(ctx)? {
        let mut payload = deliver;
        payload.extend_from_slice(&encoded);
        ctx.emit_raw(program, payload, Reply::Wanted);
    }
    Ok(())
}

pub(crate) fn dispatch(
    ctx: &ExecCtx,
    actor: &Principal,
    repo: &str,
    event_type: String,
    payload: Vec<u8>,
    at: &Revision,
) -> Result<(), Error> {
    let record = load_repo(ctx, repo)?;
    if event_type.is_empty() {
        return Err(invalid(
            "a dispatch names its event_type, which a workflow's on.dispatch matches",
        ));
    }
    let placed = match at {
        Revision::Ref(reference) => reference.starts_with(b"refs/heads/"),
        Revision::Oid(_) => true,
    };
    if !placed {
        return Err(invalid(
            "a dispatch's at is a branch under refs/heads/ or a full commit id forge holds",
        ));
    }
    let commit = resolve(ctx, repo, at, repo_hash(&record))?;
    let held = ObjectStore::new(ctx).get(&commit).map_err(refusal_of)?;
    if held.is_none_or(|object| object.kind != Kind::Commit) {
        return Err(not_found(format!(
            "{repo} holds no commit {commit}: dispatch at a branch or at a full commit id forge holds"
        )));
    }
    let event = Watched::Dispatched {
        repo: repo.to_owned(),
        event_type,
        payload,
        commit: commit.to_hex(),
        by: actor.clone(),
    };
    tell(ctx, &event)
}
