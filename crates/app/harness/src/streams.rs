//! A turn's reply as the model streams it: each event the egress role hands harness
//! (`Op::Streamed`) is checked against the thread's model call and folded, in its wire, into
//! the thread's [`Partial`]; a tool call's tool sees its input as it arrives, the call starts
//! once its block is whole, and a reply that starts over takes back what the old one did.
use ::program::{Ctx, Error, unauthorized, wrong_state};

use crate::calls::{Settled, dispatch, show, void_reply};
use crate::state::{PASSES, THREADS};
use crate::turns::{bound, converse};
use crate::{Asking, Event, Flow, Forming, Partial, Said, wire};

pub(crate) async fn streamed(ctx: &Ctx, thread: u64, event: Event) -> Result<(), Error> {
    let from_egress = ctx.env().sending_program()? == bound(ctx)?;
    if !from_egress {
        return Err(unauthorized(
            "only the program bound to the egress role streams a model's reply",
        ));
    }
    let asked = THREADS
        .get(ctx, &thread)?
        .is_some_and(|row| matches!(row.asking, Asking::Asked { .. }));
    let Some(partial) = crate::replies::read(ctx, thread)?.filter(|_| asked) else {
        return Err(wrong_state(format!(
            "thread {thread} is waiting on no model reply"
        )));
    };
    let restarted = matches!(arrival(partial.next_seq, event.seq), Arrival::Restart);
    let folded = fold(partial.clone(), &event);
    if partial.stop.is_none()
        && folded.stop.is_some()
        && let Some(tokens) = wire::last_pass(folded.wire, &event.data)
    {
        PASSES.put(ctx, &thread, &tokens);
    }
    crate::replies::keep(ctx, thread, event.seq, &partial, &folded)?;
    crate::resume::moved(ctx, thread)?;
    crate::drafts::wrote(ctx, thread, &partial, &folded, restarted).await;
    let settled = match step(&partial, &folded, restarted) {
        Step::Nothing => Settled::default(),
        Step::Restarted => void_reply(ctx, thread).await?,
        Step::Showing { index } => show(ctx, thread, index).await?,
        Step::Formed { index } => dispatch(ctx, thread, index).await?,
    };
    let ended = !crate::resume::streamed(&partial) && crate::resume::streamed(&folded);
    if settled.carry_on || ended {
        converse(ctx, thread).await;
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Nothing,
    Restarted,
    Showing { index: u64 },
    Formed { index: u64 },
}

fn forming_call(partial: &Partial) -> Option<(u64, &str)> {
    let Forming::Kept { .. } = partial.forming else {
        return None;
    };
    let Some(Said::ToolUse { input, .. }) = partial.said.last() else {
        return None;
    };
    let index = u64::try_from(partial.said.len()).ok()?.checked_sub(1)?;
    Some((index, input))
}

fn last_call(partial: &Partial) -> Option<u64> {
    let Some(Said::ToolUse { .. }) = partial.said.last() else {
        return None;
    };
    u64::try_from(partial.said.len()).ok()?.checked_sub(1)
}

pub(crate) fn step(before: &Partial, after: &Partial, restarted: bool) -> Step {
    if restarted {
        return Step::Restarted;
    }
    if after.flow != Flow::Open {
        return Step::Nothing;
    }
    let was_forming = forming_call(before);
    let now_forming = forming_call(after);
    match (was_forming, now_forming) {
        (_, Some((index, input))) => {
            let grew = was_forming != Some((index, input));
            match grew {
                true => Step::Showing { index },
                false => Step::Nothing,
            }
        }
        (Some((index, _)), None) => {
            let stopped = last_call(after) == Some(index) && after.said.len() == before.said.len();
            match stopped {
                true => Step::Formed { index },
                false => Step::Nothing,
            }
        }
        (None, None) => Step::Nothing,
    }
}

enum Arrival {
    Repeat,
    Restart,
    Next,
    Gap,
}

fn arrival(next: u64, seq: u64) -> Arrival {
    let restarted = seq == 0 && next > 0;
    if restarted {
        return Arrival::Restart;
    }
    match seq.cmp(&next) {
        std::cmp::Ordering::Less => Arrival::Repeat,
        std::cmp::Ordering::Equal => Arrival::Next,
        std::cmp::Ordering::Greater => Arrival::Gap,
    }
}

pub(crate) fn fold(mut partial: Partial, event: &Event) -> Partial {
    match arrival(partial.next_seq, event.seq) {
        Arrival::Repeat => partial,
        Arrival::Restart => {
            let mut restarted = Partial::new(partial.wire);
            grow(&mut restarted, event);
            restarted
        }
        Arrival::Next => {
            grow(&mut partial, event);
            partial
        }
        Arrival::Gap => {
            gapped(&mut partial, event.seq);
            partial
        }
    }
}

fn gapped(partial: &mut Partial, seq: u64) {
    let reason = format!(
        "event {seq} arrived where event {} was due",
        partial.next_seq
    );
    match partial.flow {
        Flow::Open => partial.flow = Flow::Broken { reason },
        Flow::Ended | Flow::Overflowed | Flow::Broken { .. } | Flow::Failed { .. } => {}
    }
}

fn grow(partial: &mut Partial, event: &Event) {
    partial.next_seq = event.seq + 1;
    match partial.flow {
        Flow::Open => wire::fold(partial, &event.data),
        Flow::Ended | Flow::Overflowed | Flow::Broken { .. } | Flow::Failed { .. } => {}
    }
}

#[cfg(test)]
mod tests;
