//! The rooms each account wrote in (`POSTED_IN`, noted with `POSTED` as it posts).
use ::program::{Error, QueryCtx};

use crate::state::POSTED_IN;
use crate::{AccountNumber, PageRequest, PageResponse};

pub(crate) fn posted_in(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<String>, Error> {
    Ok(POSTED_IN
        .range_of(ctx, &account, page)?
        .map(|(_, channel_id)| channel_id))
}
