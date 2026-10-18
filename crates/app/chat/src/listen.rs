//! Who listens to each channel: `Listen` starts or stops an account hearing every new message
//! there, and each message relates to the accounts that listened since before it was posted.
use ::program::{Error, ExecCtx, QueryCtx};

use crate::state::{LISTENERS, LISTENING, channel};
use crate::{AccountNumber, MsgRow, PageRequest, PageResponse};

pub(crate) fn listen(ctx: &ExecCtx, channel_id: &str, listening: bool) -> Result<(), Error> {
    let account = ctx.sender_account()?;
    channel(ctx, channel_id)?;
    let by_channel = (channel_id.to_owned(), account);
    let by_account = (account, channel_id.to_owned());
    if !listening {
        LISTENERS.remove(ctx, &by_channel);
        LISTENING.remove(ctx, &by_account);
        return Ok(());
    }
    if LISTENERS.has(ctx, &by_channel) {
        return Ok(());
    }
    LISTENERS.put(ctx, &by_channel, &ctx.env().height);
    LISTENING.insert(ctx, &by_account);
    Ok(())
}

pub(crate) fn listened(
    ctx: &QueryCtx,
    account: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<String>, Error> {
    Ok(LISTENING
        .range_of(ctx, &account, page)?
        .map(|(_, channel_id)| channel_id))
}

pub(crate) fn listeners(ctx: &QueryCtx, row: &MsgRow) -> Result<Vec<AccountNumber>, Error> {
    let listening = LISTENERS.scan(ctx, LISTENERS.prefix_of(&row.channel_id))?;
    Ok(listening
        .into_iter()
        .filter(|(_, since)| *since <= row.height)
        .map(|((_, account), _)| account)
        .collect())
}
