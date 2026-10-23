//! What will wake a conversation again (`Query::Wakes`): each thing harness keeps that will, so
//! an empty answer says nothing will, and the conversation sits until someone writes to it.
use ::program::{Error, QueryCtx};

use crate::agents::thread;
use crate::state::{ARMED, BOOKED, BOOKINGS, FLEET_RUNS, FLIGHTS, RETRIES, SETTLING, STREAMING};
use crate::{Asking, Wake};

pub(crate) fn wakes(ctx: &QueryCtx, id: u64) -> Result<Vec<Wake>, Error> {
    let row = thread(ctx, id)?;
    let mut wakes = Vec::new();
    if row.due() {
        wakes.push(Wake::Unseen);
    }
    if row.asking != Asking::Idle {
        wakes.push(Wake::Turn);
    }
    let open = row.open_calls;
    if open > 0 {
        wakes.push(Wake::Calls { open });
    }
    if FLIGHTS.get(ctx, &id)?.is_some() {
        wakes.push(Wake::Flight);
    }
    if let Some(retry) = RETRIES.get(ctx, &id)? {
        wakes.push(match retry.until {
            Some(until) => Wake::RetryAfter {
                at: retry.at,
                until,
            },
            None => Wake::Retry { at: retry.at },
        });
    }
    if let Some(window) = SETTLING.get(ctx, &id)? {
        wakes.push(Wake::Settle {
            until: window.until,
        });
    }
    for (_, wait) in ARMED.scan(ctx, ARMED.prefix_of(&id))? {
        wakes.push(Wake::Wait { wait });
    }
    for ((_, watch), _) in STREAMING.scan(ctx, STREAMING.prefix_of(&id))? {
        wakes.push(Wake::Stream { watch });
    }
    for (_, run) in FLEET_RUNS.scan(ctx, FLEET_RUNS.prefix_of(&id))? {
        wakes.push(Wake::Run { run });
    }
    for (_, entry) in BOOKED.scan(ctx, BOOKED.prefix_of(&row.agent))? {
        let pending = BOOKINGS
            .get(ctx, &entry)?
            .is_some_and(|booking| booking.thread == Some(id) && booking.ran.is_none());
        if pending {
            wakes.push(Wake::Booking { entry });
        }
    }
    Ok(wakes)
}
