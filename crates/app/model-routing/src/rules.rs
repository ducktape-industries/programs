//! What each op writes and each query answers: a pool, a full episode or a pin is written only
//! when it changes, and a pin moves only when its account goes full or leaves its pool.
use std::collections::BTreeSet;

use ::program::{Error, ExecCtx, QueryCtx, invalid};

use crate::state::{Account, FULL, Gateway, PINS, POOLS, Pinning};
use crate::{
    AccountNumber, Choice, Episode, FullRow, GatewayAccount, PageRequest, PageResponse, PinRow,
    PoolRow, Until,
};

enum Candidate {
    Unread(String),
    Account(GatewayAccount),
}

fn seconds_now(ctx: &QueryCtx) -> u64 {
    ctx.env().time / 1000
}

fn in_force(until: Until, now: u64) -> bool {
    match until {
        Until::Reset(reset) => reset > now,
        Until::Unknown => true,
    }
}

pub(crate) fn pool(ctx: &ExecCtx, gateway: Gateway, accounts: Vec<String>) -> Result<(), Error> {
    let mut named = BTreeSet::new();
    for account in &accounts {
        if account.is_empty() {
            return Err(invalid("a pool names no blank account"));
        }
        if !named.insert(account.as_str()) {
            return Err(invalid(format!(
                "the pool names {account} twice, and a label names one account"
            )));
        }
    }
    if POOLS.get(ctx, &gateway)?.as_ref() == Some(&accounts) {
        return Ok(());
    }
    POOLS.put(ctx, &gateway, &accounts);
    let now = seconds_now(ctx);
    for (account, episode) in FULL.scan(ctx, FULL.prefix_of(&gateway))? {
        let left_and_over = !named.contains(account.3.as_str()) && !in_force(episode.until, now);
        if left_and_over {
            FULL.remove(ctx, &account);
        }
    }
    Ok(())
}

pub(crate) fn full(ctx: &ExecCtx, account: Account, until: Until) -> Result<(), Error> {
    let now = seconds_now(ctx);
    let kept = FULL
        .get(ctx, &account)?
        .map(|episode| episode.until)
        .filter(|kept| in_force(*kept, now));
    let fuller = match (kept, until) {
        (None, until) => in_force(until, now),
        (Some(Until::Unknown), Until::Reset(_)) => true,
        (Some(Until::Reset(kept)), Until::Reset(reset)) => reset > kept,
        (Some(_), Until::Unknown) => false,
    };
    if fuller {
        FULL.put(ctx, &account, &Episode { until, since: now });
    }
    Ok(())
}

pub(crate) fn available(ctx: &ExecCtx, account: Account, read_at: u64) -> Result<(), Error> {
    let Some(episode) = FULL.get(ctx, &account)? else {
        return Ok(());
    };
    let read_after_it_began = episode.since < read_at;
    if read_after_it_began {
        FULL.remove(ctx, &account);
    }
    Ok(())
}

pub(crate) fn pin(ctx: &ExecCtx, pinning: Pinning, services: &[String]) -> Result<(), Error> {
    let choice = choose(ctx, &pinning, services)?;
    if let Choice::Chosen(account) = &choice {
        PINS.put(ctx, &pinning, account);
    }
    ctx.set_return_data(abi::encode(&choice));
    Ok(())
}

pub(crate) fn forget(
    ctx: &ExecCtx,
    consumer: AccountNumber,
    conversation: u64,
) -> Result<(), Error> {
    for (pinning, _) in PINS.scan(ctx, PINS.prefix_of(&(consumer, conversation)))? {
        PINS.remove(ctx, &pinning);
    }
    Ok(())
}

pub(crate) fn choose(
    ctx: &QueryCtx,
    pinning: &Pinning,
    services: &[String],
) -> Result<Choice, Error> {
    let (consumer, _, provider) = pinning;
    let mut listed = BTreeSet::new();
    let mut candidates = Vec::new();
    for service in services {
        if !listed.insert(service.as_str()) {
            return Err(invalid(format!("the route names {service} twice")));
        }
        let gateway = (*consumer, provider.clone(), service.clone());
        match POOLS.get(ctx, &gateway)? {
            None => candidates.push(Candidate::Unread(service.clone())),
            Some(accounts) => candidates.extend(accounts.into_iter().map(|account| {
                Candidate::Account(GatewayAccount {
                    service: service.clone(),
                    account,
                })
            })),
        }
    }
    let now = seconds_now(ctx);
    let full_until = |at: &GatewayAccount| -> Result<Option<Until>, Error> {
        let account = (
            *consumer,
            provider.clone(),
            at.service.clone(),
            at.account.clone(),
        );
        Ok(FULL
            .get(ctx, &account)?
            .map(|episode| episode.until)
            .filter(|until| in_force(*until, now)))
    };
    let pinned = PINS.get(ctx, pinning)?;
    let held = pinned.as_ref().and_then(|pinned| {
        candidates
            .iter()
            .position(|candidate| matches!(candidate, Candidate::Account(at) if at == pinned))
    });
    let start = match (pinned, held) {
        (Some(pinned), Some(at)) => {
            let has_room = full_until(&pinned)?.is_none();
            if has_room {
                return Ok(Choice::Pinned(pinned));
            }
            at + 1
        }
        _ => 0,
    };
    let mut earliest: Option<u64> = None;
    for step in 0..candidates.len() {
        match &candidates[(start + step) % candidates.len()] {
            Candidate::Unread(service) => return Ok(Choice::Discover(service.clone())),
            Candidate::Account(at) => match full_until(at)? {
                None => return Ok(Choice::Chosen(at.clone())),
                Some(Until::Reset(reset)) => {
                    earliest = Some(earliest.map_or(reset, |earliest| earliest.min(reset)));
                }
                Some(Until::Unknown) => {}
            },
        }
    }
    Ok(Choice::Wait(earliest))
}

pub(crate) fn pools(
    ctx: &QueryCtx,
    consumer: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<PoolRow>, Error> {
    Ok(POOLS
        .range_of(ctx, &consumer, page)?
        .map(|((_, provider, service), accounts)| PoolRow {
            provider,
            service,
            accounts,
        }))
}

pub(crate) fn fulls(
    ctx: &QueryCtx,
    consumer: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<FullRow>, Error> {
    Ok(FULL
        .range_of(ctx, &consumer, page)?
        .map(|((_, provider, service, account), episode)| FullRow {
            provider,
            service,
            account,
            until: episode.until,
            since: episode.since,
        }))
}

pub(crate) fn pins(
    ctx: &QueryCtx,
    consumer: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<PinRow>, Error> {
    Ok(PINS
        .range_of(ctx, &consumer, page)?
        .map(|((_, conversation, provider), pinned)| PinRow {
            conversation,
            provider,
            pinned,
        }))
}
