//! The queries: what each one reads. They build with or without `program`, so a view reading
//! crosspond's contract links them and nothing of the light client.
use std::collections::BTreeMap;

use ::program::{Error, QueryCtx, wrong_state};

use crate::state::{HOME, LINKED, LINKS, NAMED, NAMES, OUTBOX, PORTS, RECEIPTS, STATUSES};
use crate::{AccountNumber, Foreign, Link, NetworkId, Packet, Receipt, Status};

pub(crate) fn home(ctx: &QueryCtx) -> Result<NetworkId, Error> {
    HOME.get(ctx)?
        .ok_or_else(|| wrong_state("crosspond holds no network id: its init did not run"))
}

pub(crate) fn link_row(ctx: &QueryCtx, network: &NetworkId) -> Result<Option<Link>, Error> {
    Ok(LINKS.get(ctx, network)?.map(|row| Link {
        name: row.name,
        epoch_length: row.epoch_length,
        trusted: row.trusted.epoch,
        header: row.header.map(|header| header.epoch),
        next_in: row.next_in,
        next_out: row.next_out,
        refused: row.refused,
        closed: row.closed,
    }))
}

pub(crate) fn linked(ctx: &QueryCtx, name: &str) -> Result<Vec<NetworkId>, Error> {
    Ok(LINKED
        .scan(ctx, LINKED.prefix_of(&name.to_owned()))?
        .into_iter()
        .map(|(_, network)| network)
        .collect())
}

pub(crate) fn outbound(ctx: &QueryCtx, to: &NetworkId, after: u64) -> Result<Vec<Packet>, Error> {
    let after = OUTBOX.prefix_of(to).after(OUTBOX.key(&(*to, after)));
    Ok(OUTBOX
        .scan(ctx, after)?
        .into_iter()
        .map(|(_, packet)| packet)
        .collect())
}

pub(crate) fn receipts(
    ctx: &QueryCtx,
    from: &NetworkId,
    after: u64,
) -> Result<Vec<Receipt>, Error> {
    let after = RECEIPTS
        .prefix_of(from)
        .after(RECEIPTS.key(&(*from, after)));
    Ok(RECEIPTS
        .scan(ctx, after)?
        .into_iter()
        .map(|((_, seq), result)| Receipt { seq, result })
        .collect())
}

pub(crate) fn status(ctx: &QueryCtx, to: &NetworkId, seq: u64) -> Result<Option<Status>, Error> {
    STATUSES.get(ctx, &(*to, seq))
}

pub(crate) fn ports(ctx: &QueryCtx) -> Result<BTreeMap<String, Vec<NetworkId>>, Error> {
    Ok(PORTS.all(ctx)?.into_iter().collect())
}

pub(crate) fn name(ctx: &QueryCtx, foreign: Foreign) -> Result<Option<String>, Error> {
    NAMES.get(ctx, &(foreign.network, foreign.account))
}

pub(crate) fn named(
    ctx: &QueryCtx,
    network: NetworkId,
    name: String,
) -> Result<Vec<AccountNumber>, Error> {
    Ok(NAMED
        .scan(ctx, NAMED.prefix_of(&(network, name)))?
        .into_iter()
        .map(|(_, _, account)| account)
        .collect())
}
