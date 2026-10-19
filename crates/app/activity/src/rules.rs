//! The rules: a frame kept network-wide, under its key and under its account, and the pages and counts read back.

use ::program::{AccountNumber, Error, ExecCtx, ProgramId, QueryCtx, hook};
use store::{Map, PageRequest, PageResponse};

use crate::{Frame, Used};

const FRAMES: Map<(u64, u32), Frame> = Map::new("f/");
const SIGNED: Map<(Vec<u8>, u64, u32), Frame> = Map::new("k/");
const ACTED: Map<(AccountNumber, u64, u32), Frame> = Map::new("a/");
const USED: Map<(AccountNumber, ProgramId), u64> = Map::new("u/");

fn newest_first(frame: &Frame) -> (u64, u32) {
    (u64::MAX - frame.height, u32::MAX - frame.position)
}

pub(crate) fn keep(ctx: &ExecCtx, record: hook::Record) -> Result<(), Error> {
    let frame = Frame {
        height: record.height,
        position: record.position,
        time: ctx.env().time,
        signer: record.signer,
        target: record.target,
        applied: record.applied,
    };
    let (height, position) = newest_first(&frame);
    FRAMES.put(ctx, &(height, position), &frame);
    SIGNED.put(ctx, &(frame.signer.clone(), height, position), &frame);
    let Some(account) = record.account else {
        return Ok(());
    };
    ACTED.put(ctx, &(account, height, position), &frame);
    let used = (account, frame.target);
    let frames = USED.get(ctx, &used)?.unwrap_or(0);
    USED.put(ctx, &used, &(frames + 1));
    Ok(())
}

pub(crate) fn frames(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<Frame>, Error> {
    Ok(FRAMES.range(ctx, page)?.map(|(_, frame)| frame))
}

pub(crate) fn acted(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<Frame>, Error> {
    Ok(ACTED.range_of(ctx, &account, page)?.map(|(_, frame)| frame))
}

pub(crate) fn signed(
    ctx: &QueryCtx,
    key: Vec<u8>,
    page: &PageRequest,
) -> Result<PageResponse<Frame>, Error> {
    Ok(SIGNED.range_of(ctx, &key, page)?.map(|(_, frame)| frame))
}

pub(crate) fn used(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<Used>, Error> {
    Ok(USED
        .range(ctx, page)?
        .map(|((account, program), frames)| Used {
            account,
            program,
            frames,
        }))
}
