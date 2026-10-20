//! A place's hold: `Take` grants an account a channel's foot or one thread while no one else holds
//! it or drafts there; its post there, `Release`, `Break`, or losing the right to post there ends
//! it, `Busy` names who else is there, and each end is told to the programs watching places.
use std::collections::BTreeSet;

use ::program::{Error, ExecCtx, QueryCtx, Reply, invalid, wrong_state};

use crate::ops::{drop_pieces, touch_channel_for_policy};
use crate::rules;
use crate::state::{DRAFTS, LEASES, Lease, PLACE_WATCHERS, channel, message};
use crate::{Holder, Holding, PlaceEvent, Principal};

fn place(channel_id: &str, thread: Option<u64>) -> (String, Option<u64>) {
    (channel_id.to_owned(), thread)
}

pub(crate) fn take(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    thread: Option<u64>,
) -> Result<(), Error> {
    rules::writable(ctx, &channel(ctx, channel_id)?, sender)?;
    let under_a_reply = match thread {
        Some(root) => message(ctx, channel_id, root)?.thread.is_some(),
        None => false,
    };
    if under_a_reply {
        return Err(invalid("a hold answers under a thread's root"));
    }
    let key = place(channel_id, thread);
    let held_by_sender = (LEASES.get(ctx, &key)?).is_some_and(|lease| lease.holder == *sender);
    if held_by_sender {
        return Ok(());
    }
    if let Some(holder) = busy(ctx, channel_id, thread, sender)? {
        return Err(wrong_state(refusal(&holder, channel_id, thread)));
    }
    let lease = Lease {
        holder: sender.clone(),
        height: ctx.env().height,
    };
    LEASES.put(ctx, &key, &lease);
    Ok(())
}

fn refusal(holder: &Holder, channel_id: &str, thread: Option<u64>) -> String {
    let doing = match holder.kind {
        Holding::Lease => "holds",
        Holding::Draft => "is drafting in",
    };
    let at = match thread {
        Some(root) => format!("thread {root} of #{channel_id}"),
        None => format!("#{channel_id}"),
    };
    let who = rules::handle(&holder.principal);
    format!("busy: {who} {doing} {at}")
}

pub(crate) fn release(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    thread: Option<u64>,
) -> Result<(), Error> {
    let key = place(channel_id, thread);
    let held = LEASES.get(ctx, &key)?;
    if held.is_none_or(|lease| lease.holder != *sender) {
        return Ok(());
    }
    LEASES.remove(ctx, &key);
    cleared(ctx, channel_id, thread)
}

pub(crate) fn break_place(
    ctx: &ExecCtx,
    channel_id: &str,
    thread: Option<u64>,
) -> Result<(), Error> {
    let room = channel(ctx, channel_id)?;
    touch_channel_for_policy(ctx, &room);
    let key = place(channel_id, thread);
    let held = LEASES.has(ctx, &key);
    if held {
        LEASES.remove(ctx, &key);
    }
    let drafts = DRAFTS.scan(ctx, DRAFTS.prefix_of(&room.id))?;
    let there: Vec<_> = (drafts.into_iter())
        .filter(|(_, draft)| draft.thread == thread)
        .collect();
    for (author_key, draft) in &there {
        drop_pieces(ctx, author_key, draft.pieces);
        DRAFTS.remove(ctx, author_key);
    }
    let ended = held || !there.is_empty();
    match ended {
        true => cleared(ctx, channel_id, thread),
        false => Ok(()),
    }
}

pub(crate) fn vacate(
    ctx: &ExecCtx,
    channel_id: &str,
    who: Option<&Principal>,
) -> Result<(), Error> {
    let room = channel_id.to_owned();
    let theirs = |principal: &Principal| who.is_none_or(|who| who == principal);
    let mut places = BTreeSet::new();
    for (key, lease) in LEASES.scan(ctx, LEASES.prefix_of(&room))? {
        if theirs(&lease.holder) {
            LEASES.remove(ctx, &key);
            places.insert(key.1);
        }
    }
    for (key, draft) in DRAFTS.scan(ctx, DRAFTS.prefix_of(&room))? {
        if theirs(&key.1) {
            drop_pieces(ctx, &key, draft.pieces);
            DRAFTS.remove(ctx, &key);
            places.insert(draft.thread);
        }
    }
    for thread in places {
        cleared(ctx, channel_id, thread)?;
    }
    Ok(())
}

pub(crate) fn busy(
    ctx: &QueryCtx,
    channel_id: &str,
    thread: Option<u64>,
    me: &Principal,
) -> Result<Option<Holder>, Error> {
    let lease = LEASES.get(ctx, &place(channel_id, thread))?;
    if let Some(lease) = lease.filter(|lease| lease.holder != *me) {
        return Ok(Some(Holder {
            principal: lease.holder,
            kind: Holding::Lease,
            height: lease.height,
        }));
    }
    let drafts = DRAFTS.scan(ctx, DRAFTS.prefix_of(&channel_id.to_owned()))?;
    let other =
        (drafts.into_iter()).find(|((_, author), draft)| author != me && draft.thread == thread);
    Ok(other.map(|((_, author), draft)| Holder {
        principal: author,
        kind: Holding::Draft,
        height: draft.height,
    }))
}

pub(crate) fn watch_places(ctx: &ExecCtx, deliver: Vec<u8>) -> Result<(), Error> {
    let program = ctx.env().sending_program()?;
    match deliver.is_empty() {
        true => PLACE_WATCHERS.remove(ctx, &program),
        false => PLACE_WATCHERS.put(ctx, &program, &deliver),
    }
    Ok(())
}

pub(crate) fn cleared(ctx: &ExecCtx, channel_id: &str, thread: Option<u64>) -> Result<(), Error> {
    let event = PlaceEvent::Cleared {
        channel_id: channel_id.to_owned(),
        thread,
    };
    let encoded = abi::encode(&event);
    for (program, deliver) in PLACE_WATCHERS.all(ctx)? {
        let mut payload = deliver;
        payload.extend_from_slice(&encoded);
        ctx.emit_raw(program, payload, Reply::Wanted);
    }
    Ok(())
}
