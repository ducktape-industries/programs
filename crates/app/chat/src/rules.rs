//! The checks an op passes before it writes. Each refuses with the reason a
//! view can act on (`program::refuse`); none writes.
use ::program::{Env, Error, Origin, QueryCtx, invalid, unauthorized, wrong_state};

use crate::state::MEMBERS;
use crate::{ChannelRow, MsgRow, Principal, dm_peers};

pub(crate) fn id(what: &str, id: &str) -> Result<(), Error> {
    if id.is_empty() || id.contains('/') {
        return Err(invalid(format!("{what} is not empty and has no '/'")));
    }
    Ok(())
}

pub(crate) fn namespace(id: &str, env: &Env) -> Result<(), Error> {
    let Some(prefix) = crate::namespace::program(id) else {
        return Ok(());
    };
    let allowed = match &env.origin {
        Origin::Program(program) => prefix == program,
        Origin::Root => true,
        Origin::Signed(_) => false,
    };
    if !allowed {
        return Err(unauthorized(
            "colon ids belong to their program namespace: use your program namespace or an id without a colon",
        ));
    }
    Ok(())
}

pub(crate) fn channel_id(channel_id: &str, env: &Env) -> Result<(), Error> {
    id("channel_id", channel_id)?;
    namespace(channel_id, env)?;
    if dm_peers(channel_id).is_some() {
        return Err(unauthorized("dm ids open only through CreateDmChannel"));
    }
    Ok(())
}

pub(crate) fn name(name: &str) -> Result<(), Error> {
    if name.trim().is_empty() {
        return Err(invalid("a name is not blank"));
    }
    Ok(())
}

pub(crate) fn emoji(emoji: &str) -> Result<(), Error> {
    if emoji.is_empty() || emoji.contains('/') {
        return Err(invalid("not an emoji"));
    }
    Ok(())
}

pub(crate) fn writable(
    ctx: &QueryCtx,
    channel: &ChannelRow,
    principal: &Principal,
) -> Result<(), Error> {
    if channel.archived {
        return Err(wrong_state(format!("{} is archived", channel.id)));
    }
    let seated = MEMBERS.has(ctx, &(channel.id.clone(), principal.clone()));
    if !channel.admits(principal, seated) {
        return Err(unauthorized(format!(
            "{} is not a member of {}: ask the channel owner to add the account as a member",
            handle(principal),
            channel.id
        )));
    }
    Ok(())
}

pub(crate) fn not_dm(channel: &ChannelRow) -> Result<(), Error> {
    if dm_peers(&channel.id).is_some() {
        return Err(unauthorized(format!(
            "{} is a dm; neither peer reshapes it: open a regular channel to change its shape",
            channel.id
        )));
    }
    Ok(())
}

pub(crate) fn standing(row: &MsgRow) -> Result<(), Error> {
    if row.deleted {
        return Err(wrong_state("the message is deleted"));
    }
    Ok(())
}

fn handle(principal: &Principal) -> String {
    match principal {
        Principal::Account(account) => format!("acct:{account}"),
        Principal::Root => "system".to_string(),
    }
}
