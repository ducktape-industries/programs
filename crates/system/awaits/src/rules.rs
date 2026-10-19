//! The rules: a ticket opens when the kernel parks a frame on a call, and closes when the called
//! program resolves it or when its frame finishes.

use abi::role::awaits::{Answer, Call, FrameId, Ticket, TicketRow};
use program::kernel::error_from;
use program::{Error, ExecCtx, ProgramId, QueryCtx, Range, unauthorized, wrong_state};
use store::{Map, Set};

const TICKETS: Map<Ticket, TicketRow> = Map::new("t/");
const BY_FRAME: Set<(FrameId, Ticket)> = Set::new("f/");
const OWED: Set<(ProgramId, Ticket)> = Set::new("o/");

fn open(ctx: &ExecCtx, row: &TicketRow) {
    TICKETS.put(ctx, &row.ticket, row);
    BY_FRAME.insert(ctx, &(row.frame, row.ticket));
    OWED.insert(ctx, &(row.target.clone(), row.ticket));
}

fn shut(ctx: &ExecCtx, row: &TicketRow) {
    TICKETS.remove(ctx, &row.ticket);
    BY_FRAME.remove(ctx, &(row.frame, row.ticket));
    OWED.remove(ctx, &(row.target.clone(), row.ticket));
}

fn hand(ctx: &ExecCtx, row: TicketRow, answer: Answer) -> Result<(), Error> {
    shut(ctx, &row);
    ctx.resume(row.frame, row.ticket, answer.map_err(error_from))
}

pub(crate) fn park(
    ctx: &ExecCtx,
    frame: FrameId,
    program: ProgramId,
    calls: Vec<Call>,
) -> Result<(), Error> {
    ctx.env().kernel()?;
    let opened = ctx.env().height;
    for call in calls {
        let row = TicketRow {
            ticket: call.ticket,
            frame,
            caller: program.clone(),
            target: call.target,
            opened,
        };
        open(ctx, &row);
    }
    let mut waiting = BY_FRAME.prefix_of(&frame);
    waiting.limit = Some(1);
    let waits = !BY_FRAME.scan(ctx, waiting)?.is_empty();
    match waits {
        true => Ok(()),
        false => Err(wrong_state(format!("frame {frame} awaits nothing"))),
    }
}

pub(crate) fn resolve(ctx: &ExecCtx, ticket: Ticket, answer: Answer) -> Result<(), Error> {
    let Some(row) = TICKETS.get(ctx, &ticket)? else {
        return Ok(());
    };
    let by_target = ctx.env().sent_by(&row.target).is_ok();
    if !by_target {
        return Err(unauthorized(format!(
            "only {} answers call {ticket}",
            row.target
        )));
    }
    hand(ctx, row, answer)
}

pub(crate) fn close(ctx: &ExecCtx, frame: FrameId) -> Result<(), Error> {
    ctx.env().kernel()?;
    for (_, ticket) in BY_FRAME.scan(ctx, BY_FRAME.prefix_of(&frame))? {
        if let Some(row) = TICKETS.get(ctx, &ticket)? {
            shut(ctx, &row);
        }
    }
    Ok(())
}

pub(crate) fn ticket(ctx: &QueryCtx, ticket: Ticket) -> Result<Option<TicketRow>, Error> {
    TICKETS.get(ctx, &ticket)
}

fn rows(
    ctx: &QueryCtx,
    tickets: impl IntoIterator<Item = Ticket>,
) -> Result<Vec<TicketRow>, Error> {
    let mut rows = Vec::new();
    for ticket in tickets {
        if let Some(row) = TICKETS.get(ctx, &ticket)? {
            rows.push(row);
        }
    }
    Ok(rows)
}

pub(crate) fn owed(
    ctx: &QueryCtx,
    target: ProgramId,
    after: Option<Ticket>,
    limit: u32,
) -> Result<Vec<TicketRow>, Error> {
    let mut range: Range = OWED.prefix_of(&target);
    if let Some(after) = after {
        range = range.after(OWED.key(&(target.clone(), after)));
    }
    range.limit = Some(u64::from(limit));
    let tickets = OWED.scan(ctx, range)?.into_iter().map(|(_, ticket)| ticket);
    rows(ctx, tickets)
}
