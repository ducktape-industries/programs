//! An agent's conversations of one status, a page at a time: `OPEN_BY_AGENT` keeps the open ones
//! apart, newest first, so a page of live, running, stuck or idle ones never reads closed ones.
use ::program::{Error, ExecCtx, QueryCtx, Range, corrupt};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse, Set};

use crate::state::{BY_AGENT, MODEL_KEPT, OPEN_BY_AGENT, TALLIES, THREADS, TOPICS, newest_first};
use crate::tools::LISTED;
use crate::{Asking, Session, SessionStatus, ThreadRow, Window};

impl SessionStatus {
    pub fn admits(self, row: &ThreadRow) -> bool {
        let asking = !row.closed && row.asking != Asking::Idle;
        match self {
            SessionStatus::Live => !row.closed,
            SessionStatus::Running => asking && row.held == 0,
            SessionStatus::Stuck => asking && row.held > 0,
            SessionStatus::Idle => !row.closed && !asking,
            SessionStatus::Closed => row.closed,
            SessionStatus::All => true,
        }
    }

    fn open_only(self) -> bool {
        !matches!(self, SessionStatus::Closed | SessionStatus::All)
    }
}

pub(crate) fn opened(ctx: &ExecCtx, agent: AccountNumber, id: u64) {
    OPEN_BY_AGENT.insert(ctx, &(agent, newest_first(id)));
}

pub(crate) fn closed(ctx: &ExecCtx, row: &ThreadRow) {
    OPEN_BY_AGENT.remove(ctx, &(row.agent, newest_first(row.id)));
}

pub(crate) fn switched(ctx: &ExecCtx, row: &ThreadRow) {
    let key = (row.agent, newest_first(row.id));
    let open_and_unlisted = !row.closed && !OPEN_BY_AGENT.has(ctx, &key);
    if open_and_unlisted {
        OPEN_BY_AGENT.insert(ctx, &key);
    }
}

pub(crate) fn sessions(
    ctx: &QueryCtx,
    agent: AccountNumber,
    status: SessionStatus,
    page: &PageRequest,
) -> Result<PageResponse<Session>, Error> {
    let page = PageRequest::resume(page.after.clone(), page.limit.unwrap_or(LISTED));
    let listing = page.listing(ctx, PageRequest::scope_of(&(agent, status)))?;
    let index: &Set<(AccountNumber, u64)> = match status.open_only() {
        true => &OPEN_BY_AGENT,
        false => &BY_AGENT,
    };
    let prefix = index.key(&agent);
    let chunk = listing.limit().saturating_add(1);
    let mut scan = listing.scan(&prefix).limit(chunk);
    let mut admitted = Vec::new();
    loop {
        let keys = index.scan(ctx, scan)?;
        let exhausted = (keys.len() as u64) < chunk;
        let mut last = None;
        for key in keys {
            let raw = index.key(&key);
            let row = named(ctx, u64::MAX - key.1)?;
            if status.admits(&row) {
                admitted.push((raw.clone(), row));
            }
            last = Some(raw);
        }
        let short = (admitted.len() as u64) < chunk;
        match (last, short && !exhausted) {
            (Some(last), true) => scan = Range::prefix(&prefix).after(&last).limit(chunk),
            _ => break,
        }
    }
    listing.reply(admitted).try_map(|row| session(ctx, row))
}

fn named(ctx: &QueryCtx, id: u64) -> Result<ThreadRow, Error> {
    THREADS.get(ctx, &id)?.ok_or_else(|| {
        corrupt(
            "thread",
            &id.to_be_bytes(),
            "an agent names a thread that is gone",
        )
    })
}

fn session(ctx: &QueryCtx, row: ThreadRow) -> Result<Session, Error> {
    let id = row.id;
    let kept = MODEL_KEPT.get(ctx, &id)?;
    let window = match &kept {
        Some(kept) => crate::providers::window(ctx, &kept.provider, &kept.model)?,
        None => Window::Unasked,
    };
    Ok(Session {
        topic: TOPICS.get(ctx, &id)?,
        tally: TALLIES.get(ctx, &id)?,
        kept,
        window,
        row,
    })
}
