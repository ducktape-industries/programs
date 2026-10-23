//! A turn's live draft in chat (`Drafting`), written as the agent: opened only once a chat_post
//! call forming in the reply names where it posts (under its thread as soon as reply_to arrives),
//! grown by the post's text, and kept until that call has run or the turn goes on without it.
use ::program::{Ctx, ProgramId, QueryCtx};

use crate::state::{CALLS, DRAFTING, Drafted, THREADS};
use crate::{AccountNumber, Asking, CallState, Forming, Partial, Place, Said, ThreadRow};

pub(crate) const WRITING: &str = "writing an answer";

fn row(ctx: &QueryCtx, id: u64) -> Option<ThreadRow> {
    THREADS.get(ctx, &id).ok().flatten()
}

fn held(ctx: &QueryCtx, id: u64) -> Option<Drafted> {
    DRAFTING.get(ctx, &id).ok().flatten()
}

// a refused write leaves the draft as chat holds it and the turn as it is
async fn write(ctx: &Ctx, agent: AccountNumber, op: &chat::Op) {
    let target: ProgramId = chat::PROGRAM.into();
    let _ = ctx.call_raw_as(agent, target, abi::encode(op)).await;
}

fn posting(partial: &Partial) -> Option<(Place, String, String)> {
    let Forming::Kept { .. } = partial.forming else {
        return None;
    };
    let Some(Said::ToolUse { id, name, input }) = partial.said.last() else {
        return None;
    };
    let (channel, reply_to, text) = crate::tools::posting(name, input)?;
    let place = match reply_to {
        Some(root) => Place::Thread { channel, root },
        None => Place::Channel { channel },
    };
    Some((place, text, id.clone()))
}

pub(crate) async fn asking(ctx: &Ctx, id: u64) {
    through(ctx, id).await;
}

pub(crate) async fn ran(ctx: &Ctx, id: u64, call: &str) {
    if held(ctx, id).is_some_and(|held| held.call == call) {
        through(ctx, id).await;
    }
}

pub(crate) async fn wrote(ctx: &Ctx, id: u64, before: &Partial, after: &Partial, restarted: bool) {
    let Some((place, text, call)) = posting(after) else {
        if restarted {
            through(ctx, id).await;
        }
        return;
    };
    let Some(row) = row(ctx, id) else {
        return;
    };
    let held = held(ctx, id);
    let kept = held.as_ref().is_some_and(|held| held.place == place);
    let was = posting(before).filter(|(at, _, _)| kept && !restarted && *at == place);
    let grown = was.and_then(|(_, was, _)| text.strip_prefix(was.as_str()).map(str::to_owned));
    let (text, anew) = match grown {
        Some(tail) if tail.is_empty() => return,
        Some(tail) => (tail, false),
        None => (text, true),
    };
    if let Some(left) = held.filter(|held| held.place.channel() != place.channel()) {
        let op = chat::Op::Undraft {
            channel_id: left.place.channel().to_owned(),
        };
        write(ctx, row.agent, &op).await;
    }
    let drafted = Drafted {
        place: place.clone(),
        doing: WRITING.to_owned(),
        call,
    };
    DRAFTING.put(ctx, &id, &drafted);
    let thread = match &place {
        Place::Thread { root, .. } => Some(*root),
        Place::Channel { .. } => None,
    };
    let op = chat::Op::Drafting {
        channel_id: place.channel().to_owned(),
        thread,
        text,
        anew,
        doing: anew.then(|| WRITING.to_owned()),
    };
    write(ctx, row.agent, &op).await;
}

pub(crate) async fn settled(ctx: &Ctx, id: u64) {
    let Some(row) = row(ctx, id) else {
        return;
    };
    if row.closed {
        take_down(ctx, id).await;
    } else if row.asking == Asking::Idle {
        through(ctx, id).await;
    }
}

async fn through(ctx: &Ctx, id: u64) {
    let Some(held) = held(ctx, id) else {
        return;
    };
    let key = (id, held.call);
    let state = CALLS.get(ctx, &key).ok().flatten().map(|row| row.state);
    match state {
        Some(CallState::Relaying | CallState::Writing | CallState::Showing) => {}
        Some(CallState::Done { failed: false, .. }) => DRAFTING.remove(ctx, &id),
        Some(CallState::Done { failed: true, .. } | CallState::Voided { .. }) | None => {
            take_down(ctx, id).await;
        }
    }
}

async fn take_down(ctx: &Ctx, id: u64) {
    let Some(held) = held(ctx, id) else {
        return;
    };
    DRAFTING.remove(ctx, &id);
    let Some(row) = row(ctx, id) else {
        return;
    };
    let op = chat::Op::Undraft {
        channel_id: held.place.channel().to_owned(),
    };
    write(ctx, row.agent, &op).await;
}
