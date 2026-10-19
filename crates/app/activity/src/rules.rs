//! The rules: a frame kept under the account it acted as, its count by program, and the pages read
//! back. Every frame, and every frame by signing key, is the node's to list.

use ::program::{AccountNumber, Error, ExecCtx, ProgramId, QueryCtx, code, hook};
use store::{Map, PageRequest, PageResponse};

use crate::{Frame, Used};

const ACTED: Map<(AccountNumber, u64, u32), Frame> = Map::new("a/");
const USED: Map<(AccountNumber, ProgramId), u64> = Map::new("u/");

fn newest_first(frame: &Frame) -> (u64, u32) {
    (u64::MAX - frame.height, u32::MAX - frame.position)
}

pub(crate) fn keep(ctx: &ExecCtx, record: hook::Record) -> Result<(), Error> {
    let Some(account) = record.account else {
        return Ok(());
    };
    let frame = Frame {
        height: record.height,
        position: record.position,
        time: ctx.env().time,
        signer: record.signer,
        target: record.target,
        applied: record.applied,
    };
    let (height, position) = newest_first(&frame);
    ACTED.put(ctx, &(account, height, position), &frame);
    let used = (account, frame.target);
    let frames = USED.get(ctx, &used)?.unwrap_or(0);
    USED.put(ctx, &used, &(frames + 1));
    Ok(())
}

pub(crate) fn unkept(what: &str) -> Error {
    Error::new(
        code::UNSUPPORTED,
        format!(
            "activity no longer keeps {what}: the node lists every frame in its blocks (chain.blocks, /v1/blocks) and by signing key (/v1/activity)"
        ),
    )
}

pub(crate) fn acted(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<Frame>, Error> {
    Ok(ACTED.range_of(ctx, &account, page)?.map(|(_, frame)| frame))
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
