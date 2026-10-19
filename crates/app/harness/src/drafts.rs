//! A turn's live draft in the chat place it answers (chat's `Drafting`), written as the agent:
//! opened at each model call, grown by the reply's visible text and its posts' into that room, told
//! the calls it runs or the retry it waits for, and taken down once the turn is over.
use ::program::{Ctx, ExecCtx, ProgramId, QueryCtx};

use crate::conversations::binding;
use crate::state::{CALLS, DRAFTING, Drafted, HEARD_AT, RETRIES, THREADS};
use crate::{AccountNumber, Asking, Partial, Place, Said, ThreadRow};

pub(crate) const SUMMARIZING: &str = "summarizing its conversation";
pub(crate) const THINKING: &str = "thinking";
const RETRYING: &str = "waiting to retry";

// a chat input a conversation heard: where its next turn answers, unless it is bound to a place
pub(crate) fn heard(ctx: &ExecCtx, thread: u64, place: Place) {
    HEARD_AT.put(ctx, &thread, &place);
}

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

async fn draft(
    ctx: &Ctx,
    agent: AccountNumber,
    place: &Place,
    text: String,
    anew: bool,
    doing: Option<String>,
) {
    let thread = match place {
        Place::Thread { root, .. } => Some(*root),
        Place::Channel { .. } => None,
    };
    let op = chat::Op::Drafting {
        channel_id: place.channel().to_owned(),
        thread,
        text,
        anew,
        doing,
    };
    write(ctx, agent, &op).await;
}

// a model call of the turn begins: the draft opens where the turn answers (the place its
// conversation is bound to, else that of the input heard last, else where it already is), or
// starts its text over there, saying what the call is for; a draft in another room is taken down
pub(crate) async fn asking(ctx: &Ctx, id: u64, doing: &str) {
    let Some(row) = row(ctx, id) else {
        return;
    };
    let kept = held(ctx, id).map(|held| held.place);
    let heard = HEARD_AT.get(ctx, &id).ok().flatten();
    HEARD_AT.remove(ctx, &id);
    let bound = binding(ctx, &row).ok().flatten();
    let Some(place) = bound.or(heard).or_else(|| kept.clone()) else {
        return;
    };
    if let Some(left) = kept.filter(|left| left.channel() != place.channel()) {
        let op = chat::Op::Undraft {
            channel_id: left.channel().to_owned(),
        };
        write(ctx, row.agent, &op).await;
    }
    let drafted = Drafted {
        place: place.clone(),
        doing: doing.to_owned(),
    };
    DRAFTING.put(ctx, &id, &drafted);
    draft(
        ctx,
        row.agent,
        &place,
        String::new(),
        true,
        Some(doing.into()),
    )
    .await;
}

// an event of the reply: what it adds to the reply's visible text joins the draft, and a reply
// that started over, or whose text no longer leads with what the draft holds, starts it over
pub(crate) async fn wrote(ctx: &Ctx, id: u64, before: &Partial, after: &Partial, restarted: bool) {
    let Some(held) = held(ctx, id) else {
        return;
    };
    let channel = held.place.channel();
    let (was, now) = (visible(before, channel), visible(after, channel));
    let grown = now
        .strip_prefix(was.as_str())
        .filter(|_| !restarted)
        .map(str::to_owned);
    let (text, anew) = match grown {
        Some(tail) if tail.is_empty() => return,
        Some(tail) => (tail, false),
        None => (now, true),
    };
    let Some(row) = row(ctx, id) else {
        return;
    };
    // the reply's first words end its "thinking"
    let doing = (held.doing == THINKING).then(String::new);
    if doing.is_some() {
        let drafted = Drafted {
            place: held.place.clone(),
            doing: String::new(),
        };
        DRAFTING.put(ctx, &id, &drafted);
    }
    draft(ctx, row.agent, &held.place, text, anew, doing).await;
}

// what the reply says so far, its thinking and its other calls aside: its text blocks and each
// post into `channel` a paragraph apart, a cited run joined to the text before it
fn visible(partial: &Partial, channel: &str) -> String {
    let mut text = String::new();
    for said in &partial.said {
        let (more, apart) = match said {
            Said::Text(more) => (Some(more.clone()), true),
            Said::Cited { text: more, .. } => (Some(more.clone()), false),
            Said::ToolUse { name, input, .. } => {
                (crate::tools::posting(name, input, channel), true)
            }
            Said::Thinking { .. }
            | Said::RedactedThinking(_)
            | Said::ServerToolUse { .. }
            | Said::ServerToolResult(_) => (None, false),
        };
        let Some(more) = more else {
            continue;
        };
        if apart && !text.is_empty() && !more.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&more);
    }
    text
}

// the turn's calls run: the draft names their tools, a post into its room aside, whose text it
// shows
pub(crate) async fn running(ctx: &Ctx, id: u64, calls: &[(u64, String)]) {
    let Some(held) = held(ctx, id) else {
        return;
    };
    let mut tools: Vec<String> = Vec::new();
    for (_, call) in calls {
        let placed = CALLS.get(ctx, &(id, call.clone())).ok().flatten();
        if let Some(placed) = placed
            && crate::tools::posting(&placed.tool, &placed.input, held.place.channel()).is_none()
            && !tools.contains(&placed.tool)
        {
            tools.push(placed.tool);
        }
    }
    if tools.is_empty() {
        return;
    }
    tell(ctx, id, format!("running {}", tools.join(", "))).await;
}

// the draft's line on what the turn is doing, written when it says something new
async fn tell(ctx: &Ctx, id: u64, doing: String) {
    let Some(held) = held(ctx, id).filter(|held| held.doing != doing) else {
        return;
    };
    let Some(row) = row(ctx, id) else {
        return;
    };
    let drafted = Drafted {
        place: held.place,
        doing: doing.clone(),
    };
    DRAFTING.put(ctx, &id, &drafted);
    let place = drafted.place;
    draft(ctx, row.agent, &place, String::new(), false, Some(doing)).await;
}

// the conversation's loop is through for now: a turn still under way keeps its draft, one
// waiting to ask its model again says so, and one that is over takes it down
pub(crate) async fn settled(ctx: &Ctx, id: u64) {
    let Some(row) = row(ctx, id) else {
        return;
    };
    let idle = row.asking == Asking::Idle;
    let retrying = RETRIES.has(ctx, &id);
    match (row.closed, idle, retrying) {
        (false, false, _) => {}
        (false, true, true) => tell(ctx, id, RETRYING.to_owned()).await,
        (true, _, _) | (false, true, false) => over(ctx, &row).await,
    }
}

async fn over(ctx: &Ctx, row: &ThreadRow) {
    let Some(held) = held(ctx, row.id) else {
        return;
    };
    DRAFTING.remove(ctx, &row.id);
    HEARD_AT.remove(ctx, &row.id);
    let op = chat::Op::Undraft {
        channel_id: held.place.channel().to_owned(),
    };
    write(ctx, row.agent, &op).await;
}
