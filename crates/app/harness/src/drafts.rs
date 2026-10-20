//! A turn's live draft in chat (`Drafting`), written as the agent: opened only once a chat_post
//! call forming in the reply names where it posts (under its thread as soon as reply_to arrives),
//! grown by the post's text, and taken down by the post, the next model call or the turn's end.
use ::program::{Ctx, ProgramId, QueryCtx};

use crate::state::{DRAFTING, Drafted, THREADS};
use crate::{AccountNumber, Asking, Forming, Partial, Place, Said, ThreadRow};

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

// the chat_post call forming in a reply, once its input names where it posts: that place and the
// post's text so far
fn posting(partial: &Partial) -> Option<(Place, String)> {
    let Forming::Kept { .. } = partial.forming else {
        return None;
    };
    let Some(Said::ToolUse { name, input, .. }) = partial.said.last() else {
        return None;
    };
    let (channel, reply_to, text) = crate::tools::posting(name, input)?;
    let place = match reply_to {
        Some(root) => Place::Thread { channel, root },
        None => Place::Channel { channel },
    };
    Some((place, text))
}

// a model call of the turn begins: a draft an earlier call left, whose post never landed, goes
pub(crate) async fn asking(ctx: &Ctx, id: u64) {
    take_down(ctx, id).await;
}

// an event of the reply: a chat_post call forming in it drafts where it posts, as its text grows;
// a reply that started over with no such call takes the draft down
pub(crate) async fn wrote(ctx: &Ctx, id: u64, before: &Partial, after: &Partial, restarted: bool) {
    let Some((place, text)) = posting(after) else {
        if restarted {
            take_down(ctx, id).await;
        }
        return;
    };
    let Some(row) = row(ctx, id) else {
        return;
    };
    let held = held(ctx, id);
    let kept = held.as_ref().is_some_and(|held| held.place == place);
    let was = posting(before).filter(|(at, _)| kept && !restarted && *at == place);
    let grown = was.and_then(|(_, was)| text.strip_prefix(was.as_str()).map(str::to_owned));
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

// the conversation's loop is through for now: a turn that is over, or waits to ask its model
// again, posts nothing meanwhile and takes its draft down; one still under way keeps it
pub(crate) async fn settled(ctx: &Ctx, id: u64) {
    let Some(row) = row(ctx, id) else {
        return;
    };
    if row.closed || row.asking == Asking::Idle {
        take_down(ctx, id).await;
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
