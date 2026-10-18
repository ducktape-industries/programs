//! The layout chat kept before [`Stored`](crate::state::Stored): each message row whole, with its
//! search and tag postings beside it. A read falls back to an old row until its channel is
//! migrated (`Op::Migrate`); this file goes once `Query::Unmigrated` lists no channel.
use std::collections::BTreeSet;

use ::program::{Error, ExecCtx, QueryCtx};
use store::{Map, PageRequest, PageResponse, Set};
use unicode_normalization::UnicodeNormalization;

use crate::MsgRow;
use crate::state::{CHANNELS, channel, newest_first, put_message, toggle};

type ChannelId = String;
type Seq = u64;

// a message as chat kept it before `Stored`
pub const OLD_MESSAGES: Map<(ChannelId, Seq), MsgRow> = Map::new("message/");
// search postings: `(word, channel, seq)`
pub const WORDS: Set<(String, ChannelId, Seq)> = Set::new("word/");
// tag postings, newest first: `(tag, newest_first(time), channel, seq)`
pub const TAGS: Set<(String, u64, ChannelId, Seq)> = Set::new("tag/");
// tag postings within a channel, newest first: `(channel, tag, newest_first(seq))`
pub const CHANNEL_TAGS: Set<(ChannelId, String, Seq)> = Set::new("channel-tag/");

// the old row at `key` and its postings go, as a new row takes its place
pub(crate) fn forget(ctx: &ExecCtx, key: &(ChannelId, Seq)) -> Result<(), Error> {
    if let Some(row) = OLD_MESSAGES.get(ctx, key)? {
        postings(ctx, &row, false);
        OLD_MESSAGES.remove(ctx, key);
    }
    Ok(())
}

// every search and tag posting chat made for `row` before `Stored`, on or off
pub(crate) fn postings(ctx: &ExecCtx, row: &MsgRow, on: bool) {
    let (channel, seq) = (row.channel_id.clone(), row.seq);
    for word in tokens(&row.text) {
        toggle(ctx, &WORDS, &(word, channel.clone(), seq), on);
    }
    for tag in &row.tags {
        let when = newest_first(row.time);
        toggle(ctx, &TAGS, &(tag.clone(), when, channel.clone(), seq), on);
        let newest = newest_first(seq);
        toggle(
            ctx,
            &CHANNEL_TAGS,
            &(channel.clone(), tag.clone(), newest),
            on,
        );
    }
}

// the words a search posting was made for: NFC lowercase alphanumeric runs of two or more chars
fn tokens(text: &str) -> BTreeSet<String> {
    text.nfc()
        .collect::<String>()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .map(str::to_string)
        .collect()
}

// every old row of `channel_id` stored as chat keeps a message now, its postings gone; no read
// answers differently, and a channel with no old row is left as it is
pub(crate) fn migrate(ctx: &ExecCtx, channel_id: &str) -> Result<(), Error> {
    channel(ctx, channel_id)?;
    let old = OLD_MESSAGES.prefix_of(&channel_id.to_owned());
    for (_, row) in OLD_MESSAGES.scan(ctx, old)? {
        put_message(ctx, &row)?;
    }
    Ok(())
}

// the channels on this page of channels that still hold an old row
pub(crate) fn unmigrated(
    ctx: &QueryCtx,
    page: &PageRequest,
) -> Result<PageResponse<String>, Error> {
    let channels = CHANNELS.range(ctx, page)?;
    let holds_old = |id: &String| !ctx.scan(OLD_MESSAGES.prefix_of(id).limit(1)).is_empty();
    Ok(PageResponse {
        height: channels.height,
        items: (channels.items.into_iter())
            .map(|(id, _)| id)
            .filter(holds_old)
            .collect(),
        next: channels.next,
    })
}
