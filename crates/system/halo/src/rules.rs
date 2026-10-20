//! The rules: a lane is listed under a valid name at the next id, changed or unlisted only by
//! name, and its id is never handed out again; the 256 ids a lane byte holds are all there are.
//! A lane the founding pins is served for the network's life: no vote pauses or closes it.

use ::program::{Error, ExecCtx, QueryCtx, already_exists, capacity, invalid, not_found};
use store::{Item, Map};

use crate::{Class, Founded, Genesis, Lane, LaneId, MAX_NAME};

const LANES: Map<String, Lane> = Map::new("l/");
const NEXT: Item<u16> = Item::new("next");

pub(crate) fn init(ctx: &ExecCtx, genesis: Genesis) -> Result<(), Error> {
    for Founded { name, cap, pinned } in genesis.lanes {
        list(ctx, name, cap, pinned, 0)?;
    }
    Ok(())
}

pub(crate) fn open(ctx: &ExecCtx, name: String, cap: Class) -> Result<(), Error> {
    list(ctx, name, cap, false, ctx.env().height)
}

pub(crate) fn set(ctx: &ExecCtx, name: String, cap: Class, enabled: bool) -> Result<(), Error> {
    let held = listed(ctx, &name)?;
    let pauses = !enabled;
    if pauses {
        refuse_pinned_lane_removal(&held, "paused")?;
    }
    let set = Lane {
        cap,
        enabled,
        ..held
    };
    LANES.put(ctx, &name, &set);
    Ok(())
}

pub(crate) fn close(ctx: &ExecCtx, name: String) -> Result<(), Error> {
    refuse_pinned_lane_removal(&listed(ctx, &name)?, "closed")?;
    LANES.remove(ctx, &name);
    Ok(())
}

pub(crate) fn lanes(ctx: &QueryCtx) -> Result<Vec<Lane>, Error> {
    let mut lanes: Vec<Lane> = LANES.all(ctx)?.into_iter().map(|(_, lane)| lane).collect();
    lanes.sort_by_key(|lane| lane.id);
    Ok(lanes)
}

pub(crate) fn lane(ctx: &QueryCtx, name: &str) -> Result<Option<Lane>, Error> {
    LANES.get(ctx, &name.to_owned())
}

fn list(ctx: &ExecCtx, name: String, cap: Class, pinned: bool, since: u64) -> Result<(), Error> {
    printable_lane_name(&name)?;
    if LANES.has(ctx, &name) {
        return Err(already_listed(&name));
    }
    let next = NEXT.get(ctx)?.unwrap_or(0);
    let Ok(id) = LaneId::try_from(next) else {
        return Err(capacity("every lane id was handed out"));
    };
    let lane = Lane {
        name,
        id,
        cap,
        enabled: true,
        pinned,
        since,
    };
    LANES.put(ctx, &lane.name, &lane);
    NEXT.put(ctx, &(next + 1));
    Ok(())
}

fn listed(ctx: &QueryCtx, name: &str) -> Result<Lane, Error> {
    lane(ctx, name)?.ok_or_else(|| unlisted(name))
}

fn printable_lane_name(name: &str) -> Result<(), Error> {
    let sized = (1..=MAX_NAME).contains(&name.len());
    let printable = name.bytes().all(|byte| byte.is_ascii_graphic());
    if !(sized && printable) {
        return Err(invalid(format!(
            "a lane's name is 1 to {MAX_NAME} printable ASCII bytes, not {name:?}"
        )));
    }
    Ok(())
}

fn refuse_pinned_lane_removal(lane: &Lane, done: &str) -> Result<(), Error> {
    if lane.pinned {
        return Err(invalid(format!(
            "the lane {} is pinned at founding: it is never {done}",
            lane.name
        )));
    }
    Ok(())
}

fn already_listed(name: &str) -> Error {
    already_exists(format!(
        "the lane {name} is listed: choose a new lane name or update the existing lane"
    ))
}

fn unlisted(name: &str) -> Error {
    not_found(format!("no lane {name}"))
}
