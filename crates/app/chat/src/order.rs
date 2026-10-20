//! What an account keeps of its own lists: the order it put its rooms and folders in
//! (`SetRoomOrder`), and the rooms it wrote in (`POSTED_IN`, noted with `POSTED` as it posts).
use ::program::{Error, ExecCtx, QueryCtx};

use crate::state::{ORDERS, POSTED_IN};
use crate::{AccountNumber, Entry, PageRequest, PageResponse};

// The sender's own order, written whole; an empty one takes it off.
pub(crate) fn set_order(ctx: &ExecCtx, order: Vec<Entry>) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    match order.is_empty() {
        true => ORDERS.remove(ctx, &account),
        false => ORDERS.put(ctx, &account, &order),
    }
    Ok(())
}

pub(crate) fn order(ctx: &QueryCtx, account: AccountNumber) -> Result<Vec<Entry>, Error> {
    Ok(ORDERS.get(ctx, &account)?.unwrap_or_default())
}

pub(crate) fn posted_in(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<String>, Error> {
    Ok(POSTED_IN
        .range_of(ctx, &account, page)?
        .map(|(_, channel_id)| channel_id))
}
