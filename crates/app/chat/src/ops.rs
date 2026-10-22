//! One function per [`Op`](crate::Op), each named by [`Chat::execute`](crate::Chat)'s
//! match. Each checks first (`rules`), then writes, so a refused op leaves
//! the store as it found it.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, already_exists, invalid, wrong_state};

use crate::attributed::report;
use crate::listen::note;
use crate::mentions::resolve_typed;
use crate::rules;
use crate::state::{
    ANSWERED, CHANNEL_META, CHANNELS, DRAFT_PIECES, DRAFTS, Drafted, HEADS, MEMBERS, MESSAGE_IDS,
    PARTICIPANTS, REACTIONS, REPLIES, ROOTS, channel, message, newest_first, put_message, rooted,
    toggle,
};
use crate::{
    AccountNumber, Block, ChannelRow, MemberRow, MsgRow, Notice, PostPolicy, Principal, Reaction,
    dm_channel_id,
};

pub(crate) fn create_channel(
    ctx: &ExecCtx,
    sender: &Principal,
    id: String,
    name: String,
    post_policy: PostPolicy,
    meta: BTreeMap<String, String>,
) -> Result<(), Error> {
    rules::channel_id(&id, &ctx.env())?;
    rules::name(&name)?;
    if CHANNELS.has(ctx, &id) {
        return Err(already_exists(format!("channel {id} exists")));
    }
    CHANNELS.put(ctx, &id, &room(ctx, sender, &id, name, post_policy));
    if !meta.is_empty() {
        CHANNEL_META.put(ctx, &id, &meta);
    }
    Ok(())
}

fn room(
    ctx: &ExecCtx,
    sender: &Principal,
    id: &str,
    name: String,
    post_policy: PostPolicy,
) -> ChannelRow {
    ChannelRow {
        id: id.to_owned(),
        name,
        created_at: ctx.env().time,
        post_policy,
        owner: sender.clone(),
        archived: false,
    }
}

pub(crate) fn open_dm(
    ctx: &ExecCtx,
    sender: &Principal,
    counterpart: AccountNumber,
    name: String,
) -> Result<(), Error> {
    let me = ctx.sender_account()?;
    if me == counterpart {
        return Err(invalid(
            "a dm needs two accounts: name a different account as the counterpart",
        ));
    }
    let id = dm_channel_id(me, counterpart);
    if CHANNELS.has(ctx, &id) {
        return Ok(());
    }
    ctx.require_account(counterpart)?;
    rules::name(&name)?;
    let channel = room(ctx, sender, &id, name, PostPolicy::MembersOnly);
    CHANNELS.put(ctx, &id, &channel);
    for peer in [me, counterpart] {
        seat(ctx, &id, Principal::Account(peer));
    }
    Ok(())
}

pub(crate) fn rename(ctx: &ExecCtx, id: &str, name: String) -> Result<(), Error> {
    rules::name(&name)?;
    let mut channel = channel(ctx, id)?;
    rules::not_dm(&channel)?;
    channel.name = name;
    CHANNELS.put(ctx, &channel.id, &channel);
    Ok(())
}

pub(crate) fn set_archived(ctx: &ExecCtx, id: &str, archived: bool) -> Result<(), Error> {
    let mut channel = channel(ctx, id)?;
    rules::not_dm(&channel)?;
    channel.archived = archived;
    CHANNELS.put(ctx, &channel.id, &channel);
    match archived {
        true => crate::leases::vacate(ctx, id, None),
        false => Ok(()),
    }
}

pub(crate) fn set_post_policy(
    ctx: &ExecCtx,
    id: &str,
    post_policy: PostPolicy,
) -> Result<(), Error> {
    let mut channel = channel(ctx, id)?;
    rules::not_dm(&channel)?;
    channel.post_policy = post_policy;
    CHANNELS.put(ctx, &channel.id, &channel);
    crate::leases::vacate_shut_out(ctx, &channel)
}

pub(crate) fn touch_channel_for_policy(ctx: &ExecCtx, channel: &ChannelRow) {
    CHANNELS.put(ctx, &channel.id, channel);
}

fn touch_message_for_author_policy(ctx: &ExecCtx, row: &MsgRow) -> Result<(), Error> {
    let at = MESSAGE_IDS.get(ctx, &row.message_id)?;
    if let Some(at) = at {
        MESSAGE_IDS.put(ctx, &row.message_id, &at);
    }
    Ok(())
}

pub(crate) fn set_meta(ctx: &ExecCtx, id: &str, key: String, value: String) -> Result<(), Error> {
    let channel = channel(ctx, id)?;
    rules::not_dm(&channel)?;
    touch_channel_for_policy(ctx, &channel);
    let mut meta = CHANNEL_META.get(ctx, &channel.id)?.unwrap_or_default();
    meta.insert(key, value);
    CHANNEL_META.put(ctx, &channel.id, &meta);
    Ok(())
}

pub(crate) fn remove_meta(ctx: &ExecCtx, id: &str, key: &str) -> Result<(), Error> {
    let channel = channel(ctx, id)?;
    rules::not_dm(&channel)?;
    let mut meta = CHANNEL_META.get(ctx, &channel.id)?.unwrap_or_default();
    if meta.remove(key).is_none() {
        return Ok(());
    }
    touch_channel_for_policy(ctx, &channel);
    if meta.is_empty() {
        CHANNEL_META.remove(ctx, &channel.id);
    } else {
        CHANNEL_META.put(ctx, &channel.id, &meta);
    }
    Ok(())
}

pub(crate) fn set_membership(
    ctx: &ExecCtx,
    id: &str,
    principal: Principal,
    member: bool,
) -> Result<(), Error> {
    let channel = channel(ctx, id)?;
    rules::not_dm(&channel)?;
    touch_channel_for_policy(ctx, &channel);
    if member {
        seat(ctx, id, principal);
        return Ok(());
    }
    MEMBERS.remove(ctx, &(id.to_owned(), principal.clone()));
    let still_posts = rules::writable(ctx, &channel, &principal).is_ok();
    match still_posts {
        true => Ok(()),
        false => crate::leases::vacate(ctx, id, Some(&principal)),
    }
}

fn seat(ctx: &ExecCtx, id: &str, principal: Principal) {
    crate::linked::heard(ctx, &principal);
    let row = MemberRow {
        principal: principal.clone(),
        height: ctx.env().height,
        time: ctx.env().time,
    };
    MEMBERS.put(ctx, &(id.to_owned(), principal), &row);
}

fn seat_involved(ctx: &ExecCtx, row: &MsgRow) -> Result<(), Error> {
    let meta = CHANNEL_META.get(ctx, &row.channel_id)?.unwrap_or_default();
    let listed = meta.get(crate::LISTED);
    let involved = listed.is_some_and(|listed| listed == crate::INVOLVED);
    let open_to_every_poster = !channel(ctx, &row.channel_id)?.members_only();
    let seats_who_takes_part = involved && open_to_every_poster;
    if !seats_who_takes_part {
        return Ok(());
    }
    let mentioned = crate::attributed::mentioned(&row.blocks).map(Principal::Account);
    for principal in std::iter::once(row.author.clone()).chain(mentioned) {
        let key = (row.channel_id.clone(), principal.clone());
        if !MEMBERS.has(ctx, &key) {
            seat(ctx, &row.channel_id, principal);
        }
    }
    Ok(())
}

pub(crate) fn post(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    message_id: String,
    blocks: Vec<Block>,
    thread: Option<u64>,
) -> Result<(), Error> {
    rules::no_notice(&blocks)?;
    write(ctx, sender, channel_id, message_id, blocks, thread)
}

pub(crate) fn notify(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    message_id: String,
    thread: Option<u64>,
    notice: Notice,
) -> Result<(), Error> {
    let blocks = noticed(ctx, notice, None)?;
    write(ctx, sender, channel_id, message_id, blocks, thread)
}

pub(crate) fn noticed(
    ctx: &ExecCtx,
    notice: Notice,
    text: Option<String>,
) -> Result<Vec<Block>, Error> {
    rules::notice(ctx, &notice)?;
    let said = text.filter(|text| !text.trim().is_empty());
    let body = said.map(|text| crate::parse_message(&text));
    let blocks = std::iter::once(Block::Notice(notice)).chain(body.into_iter().flatten());
    Ok(blocks.collect())
}

pub(crate) fn notify_with(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    message_id: String,
    thread: Option<u64>,
    blocks: Vec<Block>,
    by: Option<Principal>,
) -> Result<(), Error> {
    match &by {
        Some(Principal::Account(account)) => ctx.require_account(*account)?,
        Some(Principal::Root) => {
            return Err(invalid("by names who acted: an account, never root"));
        }
        Some(Principal::Foreign(_)) => {
            return Err(invalid(
                "by names who acted: an account of this network, never a user of another",
            ));
        }
        None => {}
    }
    write(
        ctx,
        sender,
        channel_id.clone(),
        message_id.clone(),
        blocks,
        thread,
    )?;
    let Some(by) = by else {
        return Ok(());
    };
    let Some((_, seq)) = MESSAGE_IDS.get(ctx, &message_id)? else {
        return Err(wrong_state(format!("notice {message_id} was not written")));
    };
    let root = message(ctx, &channel_id, seq)?.thread.unwrap_or(seq);
    let following = (channel_id, root, by);
    if !PARTICIPANTS.has(ctx, &following) {
        PARTICIPANTS.put(ctx, &following, &seq);
    }
    Ok(())
}

pub(crate) fn write(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    message_id: String,
    blocks: Vec<Block>,
    thread: Option<u64>,
) -> Result<(), Error> {
    rules::id("message_id", &message_id)?;
    rules::namespace(&message_id, &ctx.env())?;
    store(ctx, sender, channel_id, message_id, blocks, thread)
}

pub(crate) fn store(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: String,
    message_id: String,
    blocks: Vec<Block>,
    thread: Option<u64>,
) -> Result<(), Error> {
    rules::writable(ctx, &channel(ctx, &channel_id)?, sender)?;
    if MESSAGE_IDS.has(ctx, &message_id) {
        return Err(already_exists(format!(
            "message {message_id} exists: choose a new message id"
        )));
    }
    let thread = rooted(ctx, &channel_id, thread)?;
    let blocks = resolve_typed(ctx, blocks)?;
    crate::linked::names_foreign_where_linked(ctx, &channel_id, &blocks)?;
    let seq = HEADS.get(ctx, &channel_id)?.unwrap_or(0) + 1;
    let row = MsgRow {
        channel_id: channel_id.clone(),
        seq,
        message_id: message_id.clone(),
        height: ctx.env().height,
        time: ctx.env().time,
        blocks,
        thread,
        ..MsgRow::by(sender.clone())
    };
    match thread {
        Some(root) => answer(ctx, &channel_id, root, seq, sender)?,
        None => ROOTS.insert(ctx, &(channel_id.clone(), newest_first(seq))),
    }
    put_message(ctx, &row)?;
    if let Some(client) = ctx.client() {
        crate::state::CLIENTS.put(ctx, &(channel_id.clone(), seq), &client);
    }
    note(ctx, &row);
    seat_involved(ctx, &row)?;
    report(ctx, sender, None, &row)?;
    MESSAGE_IDS.put(ctx, &message_id, &(channel_id.clone(), seq));
    HEADS.put(ctx, &channel_id, &seq);
    undraft(ctx, sender, &channel_id)?;
    crate::leases::release(ctx, sender, &channel_id, thread)?;
    crate::linked::mentioned(ctx, &row.blocks);
    crate::linked::posted(ctx, sender, &row)
}

fn answer(
    ctx: &ExecCtx,
    channel_id: &str,
    root: u64,
    seq: u64,
    sender: &Principal,
) -> Result<(), Error> {
    let mut row = message(ctx, channel_id, root)?;
    forget_answer(ctx, &row);
    let channel_id = channel_id.to_owned();
    let answered = (channel_id.clone(), row.author.clone(), newest_first(seq));
    ANSWERED.put(ctx, &answered, &root);
    REPLIES.insert(ctx, &(channel_id.clone(), root, seq));
    let taking_part = (channel_id.clone(), root, sender.clone());
    if !PARTICIPANTS.has(ctx, &taking_part) {
        PARTICIPANTS.put(ctx, &taking_part, &seq);
    }
    row.reply_count += 1;
    row.last_reply_seq = Some(seq);
    put_message(ctx, &row)?;
    Ok(())
}

fn forget_answer(ctx: &ExecCtx, root: &MsgRow) {
    if let Some(last) = root.last_reply_seq {
        let key = (
            root.channel_id.clone(),
            root.author.clone(),
            newest_first(last),
        );
        ANSWERED.remove(ctx, &key);
    }
}

pub(crate) fn edit(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    seq: u64,
    blocks: Vec<Block>,
    base_rev: Option<u32>,
) -> Result<(), Error> {
    rules::writable(ctx, &channel(ctx, channel_id)?, sender)?;
    let old = message(ctx, channel_id, seq)?;
    rules::standing(&old)?;
    rules::not_a_notice(&old)?;
    rules::no_notice(&blocks)?;
    let blocks = resolve_typed(ctx, blocks)?;
    crate::linked::names_foreign_where_linked(ctx, channel_id, &blocks)?;
    let row = MsgRow {
        blocks,
        rev: old.rev + 1,
        edited: true,
        edited_at: Some(ctx.env().time),
        base_rev,
        ..old.clone()
    };
    put_message(ctx, &row)?;
    touch_message_for_author_policy(ctx, &old)?;
    note(ctx, &row);
    seat_involved(ctx, &row)?;
    report(ctx, sender, Some(&old), &row)?;
    crate::linked::mentioned(ctx, &row.blocks);
    crate::linked::edited(ctx, sender, &row)
}

pub(crate) fn delete(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    seq: u64,
) -> Result<(), Error> {
    let room = channel(ctx, channel_id)?;
    let old = message(ctx, channel_id, seq)?;
    if old.deleted {
        return Ok(());
    }
    match old.author == *sender {
        true => touch_message_for_author_policy(ctx, &old)?,
        false => touch_channel_for_policy(ctx, &room),
    }
    forget_answer(ctx, &old);
    let reacted = REACTIONS.scan(ctx, REACTIONS.prefix_of(&(channel_id.to_owned(), seq)))?;
    let mut withdrawn = Vec::new();
    for key in reacted {
        let (_, _, emoji, reactor) = &key;
        withdrawn.extend(crate::attributed::reaction(
            ctx, reactor, &old, emoji, false,
        )?);
        REACTIONS.remove(ctx, &key);
    }
    crate::attributed::reacted(ctx, withdrawn);
    let tombstone = MsgRow {
        blocks: Vec::new(),
        reactions: Vec::new(),
        deleted: true,
        ..old.clone()
    };
    crate::state::SEALED_BODIES.remove(ctx, &(channel_id.to_owned(), seq));
    put_message(ctx, &tombstone)?;
    report(ctx, sender, Some(&old), &tombstone)?;
    crate::linked::deleted(ctx, sender, &old)
}

pub(crate) fn react(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    seq: u64,
    emoji: &str,
    on: bool,
) -> Result<(), Error> {
    rules::emoji(emoji)?;
    rules::writable(ctx, &channel(ctx, channel_id)?, sender)?;
    let mut row = message(ctx, channel_id, seq)?;
    if row.deleted {
        return Err(wrong_state("the message is deleted"));
    }
    let key = (channel_id.to_owned(), seq, emoji.to_owned(), sender.clone());
    if REACTIONS.has(ctx, &key) == on {
        return Ok(());
    }
    if on {
        count_in(&mut row.reactions, emoji);
    } else {
        count_out(&mut row.reactions, emoji);
    }
    toggle(ctx, &REACTIONS, &key, on);
    put_message(ctx, &row)?;
    let told = crate::attributed::reaction(ctx, sender, &row, emoji, on)?;
    crate::attributed::reacted(ctx, told.into_iter().collect());
    crate::linked::reacted(ctx, sender, &row, emoji, on)
}

fn count_in(reactions: &mut Vec<Reaction>, emoji: &str) {
    if let Some(reaction) = reactions.iter_mut().find(|r| r.emoji == emoji) {
        reaction.count += 1;
        return;
    }
    reactions.push(Reaction {
        emoji: emoji.to_owned(),
        count: 1,
        reacted_by_me: false,
    });
    reactions.sort_by(|a, b| a.emoji.cmp(&b.emoji));
}

fn count_out(reactions: &mut Vec<Reaction>, emoji: &str) {
    if let Some(at) = reactions.iter().position(|r| r.emoji == emoji) {
        reactions[at].count -= 1;
        if reactions[at].count == 0 {
            reactions.remove(at);
        }
    }
}

// ── drafts ──────────────────────────────────────────────────────────────────

// `sender` answering in a room before it posts: `text` joins its draft there as one more piece
// (`anew`: in place of the pieces so far), `doing` replaces what it says it is doing, and the
// draft answers under `thread`'s root or at the room's foot; one under another place starts over
pub(crate) fn drafting(
    ctx: &ExecCtx,
    sender: &Principal,
    channel_id: &str,
    thread: Option<u64>,
    text: String,
    anew: bool,
    doing: Option<String>,
) -> Result<(), Error> {
    rules::writable(ctx, &channel(ctx, channel_id)?, sender)?;
    let thread = rooted(ctx, channel_id, thread)?;
    let key = (channel_id.to_owned(), sender.clone());
    let held = DRAFTS.get(ctx, &key)?;
    let moved = held.as_ref().is_none_or(|held| held.thread != thread);
    let left = (held.as_ref().map(|held| held.thread)).filter(|left| *left != thread);
    if let Some(left) = left {
        crate::leases::cleared(ctx, channel_id, left)?;
    }
    let mut draft = held.unwrap_or(Drafted {
        thread,
        height: ctx.env().height,
        time: ctx.env().time,
        doing: String::new(),
        pieces: 0,
    });
    if anew || moved {
        drop_pieces(ctx, &key, draft.pieces);
        draft.pieces = 0;
    }
    draft.thread = thread;
    if let Some(doing) = doing {
        draft.doing = doing;
    }
    if !text.is_empty() {
        let (channel_id, author) = key.clone();
        DRAFT_PIECES.put(ctx, &(channel_id, author, draft.pieces), &text);
        draft.pieces += 1;
    }
    DRAFTS.put(ctx, &key, &draft);
    Ok(())
}

// `sender`'s draft in the room taken down with its pieces: it posted there, or stopped answering
pub(crate) fn undraft(ctx: &ExecCtx, sender: &Principal, channel_id: &str) -> Result<(), Error> {
    let key = (channel_id.to_owned(), sender.clone());
    let Some(draft) = DRAFTS.get(ctx, &key)? else {
        return Ok(());
    };
    drop_pieces(ctx, &key, draft.pieces);
    DRAFTS.remove(ctx, &key);
    crate::leases::cleared(ctx, channel_id, draft.thread)
}

pub(crate) fn drop_pieces(ctx: &ExecCtx, (channel_id, author): &(String, Principal), pieces: u64) {
    for piece in 0..pieces {
        DRAFT_PIECES.remove(ctx, &(channel_id.clone(), author.clone(), piece));
    }
}
