//! Tools over `chat`: channels, messages, threads and search read as the agent reads them, and
//! posts and reactions the agent makes as itself. A post lands whole once its call runs (its draft
//! shows the text as it arrives), a reaction once its input is whole.
use ::program::{Error, ExecCtx, Principal, QueryCtx, stale};
use chat::{ChannelInfo, MsgRow, Query, Reply};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use store::PageRequest;

use super::prefix::prefix;
use super::{
    LISTED, Run, Seat, Show, Tool, Undo, Write, cursor_schema, next, object, page, principal,
    unexpected,
};
use crate::Place;
use crate::state::{HEARD_AT, HELD_AT, POST_READ_THROUGH};

fn ask(ctx: &ExecCtx, seat: &Seat, query: Query) -> Result<Reply, Error> {
    crate::reading::chat_for(ctx, seat.agent, &query)
}

fn agent(seat: &Seat) -> Principal {
    Principal::Account(seat.agent)
}

fn channel(info: &ChannelInfo) -> Value {
    json!({
        "id": info.channel.id,
        "name": info.channel.name,
        "members_only": info.channel.members_only(),
        "archived": info.channel.archived,
        "newest_seq": info.head_seq,
    })
}

fn message(ctx: &QueryCtx, row: &MsgRow) -> Value {
    let reactions: Vec<Value> = row
        .reactions
        .iter()
        .map(|reaction| json!({ "emoji": reaction.emoji, "count": reaction.count }))
        .collect();
    json!({
        "seq": row.seq,
        "author": principal(ctx, &row.author),
        "time": row.time,
        "text": row.text,
        "reply_to": row.thread,
        "replies": row.reply_count,
        "reactions": reactions,
        "edited": row.edited,
        "deleted": row.deleted,
    })
}

fn messages(ctx: &QueryCtx, rows: &chat::PageResponse<MsgRow>) -> Value {
    json!({
        "messages": rows.items.iter().map(|row| message(ctx, row)).collect::<Vec<_>>(),
        "next": next(&rows.next),
    })
}

fn channel_schema() -> Value {
    json!({ "type": "string", "description": "a channel id, as chat_channels lists them" })
}

#[derive(Deserialize)]
pub(crate) struct ChannelsArgs {
    name: Option<String>,
    cursor: Option<String>,
}

fn channels(
    ctx: &ExecCtx,
    seat: &Seat,
    page: PageRequest,
) -> Result<chat::PageResponse<ChannelInfo>, Error> {
    match ask(ctx, seat, Query::Channels { page })? {
        Reply::Channels(rows) => Ok(rows),
        other => Err(unexpected(chat::PROGRAM, "Channels", &other)),
    }
}

fn named(info: &ChannelInfo, wanted: &str) -> bool {
    info.channel.name.to_lowercase().contains(wanted)
        || info.channel.id.to_lowercase().contains(wanted)
}

pub(crate) struct Channels;

impl Tool for Channels {
    const NAME: &'static str = "chat_channels";
    const DESCRIPTION: &'static str = "List the network's chat channels: id, name, whether only members post, and the newest message's seq. With name, only the channels whose name or id holds it, from every page at once.";
    type Args = ChannelsArgs;

    fn schema() -> Value {
        object(
            json!({
                "name": { "type": "string", "description": "only the channels whose name or id holds this text, in any case, like j-071 or engineering" },
                "cursor": cursor_schema(),
            }),
            &[],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ChannelsArgs) -> Result<Run, Error> {
        let mut asked = page(&args.cursor)?;
        let Some(name) = args.name else {
            let rows = channels(ctx, seat, asked)?;
            return Ok(Run::Done(json!({
                "channels": rows.items.iter().map(channel).collect::<Vec<_>>(),
                "next": next(&rows.next),
            })));
        };
        let wanted = name.to_lowercase();
        let mut kept = Vec::new();
        loop {
            let rows = channels(ctx, seat, asked)?;
            let matching = rows.items.iter().filter(|info| named(info, &wanted));
            kept.extend(matching.map(channel));
            let Some(after) = rows.next else { break };
            asked = PageRequest {
                after: Some(after),
                ..PageRequest::default()
            };
        }
        Ok(Run::Done(json!({ "channels": kept, "next": null })))
    }
}

#[derive(Deserialize)]
pub(crate) struct MessagesArgs {
    channel: String,
    limit: Option<u64>,
    cursor: Option<String>,
}

pub(crate) struct Messages;

impl Tool for Messages {
    const NAME: &'static str = "chat_messages";
    const DESCRIPTION: &'static str = "Read a chat channel's messages, newest first, without their thread replies (chat_thread reads those): the newest limit of them (20 if omitted), and `next` reads the older ones.";
    type Args = MessagesArgs;

    fn schema() -> Value {
        object(
            json!({
                "channel": channel_schema(),
                "limit": { "type": "integer", "minimum": 1, "description": "at most this many messages, the newest; 20 if omitted" },
                "cursor": cursor_schema(),
            }),
            &["channel"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: MessagesArgs) -> Result<Run, Error> {
        let limit = args.limit.unwrap_or(LISTED);
        if limit == 0 {
            return Err(::program::invalid("a limit is a number of messages from 1"));
        }
        let mut page = page(&args.cursor)?;
        page.limit = Some(limit);
        let asked = Query::Roots {
            channel_id: args.channel,
            viewer: vec![agent(seat)],
            page,
        };
        match ask(ctx, seat, asked)? {
            Reply::Roots(rows) => Ok(Run::Done(messages(ctx, &rows))),
            other => Err(unexpected(chat::PROGRAM, "Roots", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct ThreadArgs {
    channel: String,
    root: u64,
    cursor: Option<String>,
}

pub(crate) struct Thread;

impl Tool for Thread {
    const NAME: &'static str = "chat_thread";
    const DESCRIPTION: &'static str = "Read a chat message and the replies in its thread.";
    type Args = ThreadArgs;

    fn schema() -> Value {
        object(
            json!({
                "channel": channel_schema(),
                "root": { "type": "integer", "description": "the seq of the message that opened the thread" },
                "cursor": cursor_schema(),
            }),
            &["channel", "root"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ThreadArgs) -> Result<Run, Error> {
        let asked = Query::Thread {
            channel_id: args.channel,
            root_seq: args.root,
            viewer: vec![agent(seat)],
            page: page(&args.cursor)?,
        };
        match ask(ctx, seat, asked)? {
            Reply::Thread { root, replies } => Ok(Run::Done(json!({
                "root": root.as_ref().map(|row| message(ctx, row)),
                "replies": replies.items.iter().map(|row| message(ctx, row)).collect::<Vec<_>>(),
                "next": next(&replies.next),
            }))),
            other => Err(unexpected(chat::PROGRAM, "Thread", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct SearchArgs {
    text: String,
    channel: Option<String>,
    around: Option<usize>,
    cursor: Option<String>,
}

fn hit(ctx: &QueryCtx, row: &MsgRow, terms: &[String], around: Option<usize>) -> Value {
    let snippets = match around {
        Some(bytes) => chat::around_matches(&row.text, terms, bytes),
        None => chat::sentences_holding(&row.text, terms),
    };
    json!({
        "channel": row.channel_id,
        "seq": row.seq,
        "author": principal(ctx, &row.author),
        "time": row.time,
        "reply_to": row.thread,
        "snippets": snippets,
    })
}

pub(crate) struct Search;

impl Tool for Search {
    const NAME: &'static str = "chat_search";
    const DESCRIPTION: &'static str = "Search chat messages for words, across every channel or in one: each hit's channel, seq, author and time, and the sentences of it that hold a word (with around, the bytes around each match instead). chat_thread or chat_messages reads a message whole.";
    type Args = SearchArgs;

    fn schema() -> Value {
        object(
            json!({
                "text": { "type": "string", "description": "the words to find" },
                "channel": channel_schema(),
                "around": { "type": "integer", "minimum": 0, "description": "instead of the sentences: this many bytes on each side of each match" },
                "cursor": cursor_schema(),
            }),
            &["text"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: SearchArgs) -> Result<Run, Error> {
        let terms = chat::search_terms(&args.text);
        let asked = Query::TextSearch {
            text: args.text,
            viewer: vec![agent(seat)],
            channel_id: args.channel,
            page: page(&args.cursor)?,
        };
        match ask(ctx, seat, asked)? {
            Reply::TextHits(rows) => Ok(Run::Done(json!({
                "hits": rows.items.iter().map(|row| hit(ctx, row, &terms, args.around)).collect::<Vec<_>>(),
                "next": next(&rows.next),
            }))),
            other => Err(unexpected(chat::PROGRAM, "TextSearch", &other)),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct PostArgs {
    channel: String,
    text: String,
    reply_to: Option<u64>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Posted {
    channel: String,
    reply_to: Option<u64>,
    text: String,
}

impl Posted {
    fn place(&self) -> Place {
        let channel = self.channel.clone();
        match self.reply_to {
            Some(root) => Place::Thread { channel, root },
            None => Place::Channel { channel },
        }
    }
}

fn read_since(ctx: &ExecCtx, seat: &Seat, posted: &Posted) -> Result<(), Error> {
    let place = posted.place();
    let key = place.key();
    let read_through = POST_READ_THROUGH.get(ctx, &(seat.thread, seat.call.to_owned()))?;
    let arrived_after_read = match read_through {
        Some(read_through) => HEARD_AT
            .get(ctx, &(seat.thread, key.clone()))?
            .is_some_and(|arrived| arrived > read_through),
        None => HELD_AT
            .scan(ctx, HELD_AT.prefix_of(&seat.thread))?
            .iter()
            .any(|(_, at)| *at == key),
    };
    if !arrived_after_read {
        return Ok(());
    }
    Err(stale(format!(
        "not posted: a message in {} reached you after your reply read it; you read it before your next model call, so post again if it still needs saying",
        place.words()
    )))
}

pub(crate) fn message_id(thread: u64, call: &str) -> String {
    format!("harness-{thread}-{call}")
}

fn record<T: serde::de::DeserializeOwned>(seat: &Seat) -> Option<T> {
    seat.shown
        .and_then(|shown| serde_json::from_value(shown.clone()).ok())
}

fn standing(ctx: &ExecCtx, seat: &Seat, message_id: String) -> Result<Option<MsgRow>, Error> {
    match ask(ctx, seat, Query::MessageById { message_id })? {
        Reply::Message(row) => Ok(row.filter(|row| !row.deleted)),
        other => Err(unexpected(chat::PROGRAM, "MessageById", &other)),
    }
}

fn moved_id(own: &str) -> String {
    format!("{own}-moved")
}

fn post(message_id: String, posted: &Posted) -> Write {
    let op = chat::Op::PostMessage {
        channel_id: posted.channel.clone(),
        message_id,
        blocks: chat::parse_message(&posted.text),
        thread: posted.reply_to,
    };
    Write {
        target: chat::PROGRAM.into(),
        payload: abi::encode(&op),
    }
}

fn edit(row: &MsgRow, text: &str) -> Write {
    let op = chat::Op::EditMessage {
        channel_id: row.channel_id.clone(),
        seq: row.seq,
        blocks: chat::parse_message(text),
        base_rev: None,
    };
    Write {
        target: chat::PROGRAM.into(),
        payload: abi::encode(&op),
    }
}

fn delete(row: &MsgRow) -> Write {
    let op = chat::Op::DeleteMessage {
        channel_id: row.channel_id.clone(),
        seq: row.seq,
    };
    Write {
        target: chat::PROGRAM.into(),
        payload: abi::encode(&op),
    }
}

// where a chat_post call's input says it posts, once it says so, and its text so far: under its
// reply_to's thread as soon as reply_to is whole, at the room's foot once the text begins without
// one
pub(crate) fn posting_at(input: &str) -> Option<(String, Option<u64>, String)> {
    let input = prefix(input);
    let channel = input.settled("channel")?.as_str()?.to_owned();
    let reply_to = input.settled("reply_to").and_then(Value::as_u64);
    let text = input.arriving("text").and_then(Value::as_str);
    if reply_to.is_none() && text.is_none() {
        return None;
    }
    Some((channel, reply_to, text.unwrap_or_default().to_owned()))
}

pub(crate) struct Post;

impl Tool for Post {
    const NAME: &'static str = "chat_post";
    const DESCRIPTION: &'static str = "Post a message to a chat channel, or a reply in a message's thread. Markdown works; `<@N>` mentions account N, and `@name` mentions the one account named so. The message is yours, and chat allows it where it allows your account. It lands whole once you finish it, and your draft shows where it goes as soon as channel and reply_to are given, then its text as you write it, so give channel and reply_to before text. If a message in that same place (the channel itself, or that one thread), other than a notice, reaches you after your reply read it, the post is refused unsent: you read the message before your next model call, and post again if it still needs saying.";
    const WRITES: bool = true;
    type Args = PostArgs;

    fn schema() -> Value {
        object(
            json!({
                "channel": channel_schema(),
                "reply_to": { "type": "integer", "description": "the seq of the message whose thread this replies in" },
                "text": { "type": "string" },
            }),
            &["channel", "text"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: PostArgs) -> Result<Run, Error> {
        let posted = Posted {
            channel: args.channel,
            reply_to: args.reply_to,
            text: args.text,
        };
        let own = message_id(seat.thread, seat.call);
        let Some(row) = standing(ctx, seat, own.clone())? else {
            read_since(ctx, seat, &posted)?;
            return Ok(Run::Write(vec![post(own, &posted)]));
        };
        let same_place = row.channel_id == posted.channel && row.thread == posted.reply_to;
        let same_text = row.blocks == chat::parse_message(&posted.text);
        match (same_place, same_text) {
            (true, true) => Ok(Run::Done(json!({ "ok": true }))),
            (true, false) => Ok(Run::Write(vec![edit(&row, &posted.text)])),
            (false, _) => {
                read_since(ctx, seat, &posted)?;
                Ok(Run::Write(vec![
                    delete(&row),
                    post(moved_id(&own), &posted),
                ]))
            }
        }
    }

    fn void(ctx: &ExecCtx, seat: &Seat) -> Result<Undo, Error> {
        let own = message_id(seat.thread, seat.call);
        let mut writes = Vec::new();
        for id in [moved_id(&own), own] {
            if let Some(row) = standing(ctx, seat, id)? {
                writes.push(delete(&row));
            }
        }
        Ok(Undo::Writes(writes))
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ReactArgs {
    channel: String,
    seq: u64,
    emoji: String,
}

fn reaction(args: &ReactArgs, on: bool) -> Write {
    let (channel_id, seq, emoji) = (args.channel.clone(), args.seq, args.emoji.clone());
    let op = match on {
        true => chat::Op::AddReaction {
            channel_id,
            seq,
            emoji,
        },
        false => chat::Op::RemoveReaction {
            channel_id,
            seq,
            emoji,
        },
    };
    Write {
        target: chat::PROGRAM.into(),
        payload: abi::encode(&op),
    }
}

pub(crate) struct React;

impl Tool for React {
    const NAME: &'static str = "chat_react";
    const DESCRIPTION: &'static str = "React to a chat message with an emoji, as yourself.";
    const WRITES: bool = true;
    type Args = ReactArgs;

    fn schema() -> Value {
        object(
            json!({
                "channel": channel_schema(),
                "seq": { "type": "integer" },
                "emoji": { "type": "string" },
            }),
            &["channel", "seq", "emoji"],
        )
    }

    fn run(ctx: &ExecCtx, seat: &Seat, args: ReactArgs) -> Result<Run, Error> {
        if let Err(refused) = crate::unanswered::reacted(ctx, seat.agent, &args.channel, args.seq) {
            let why = crate::calls::refusal(&refused);
            let (seq, channel) = (args.seq, &args.channel);
            let text = format!(
                "reacting to message {seq} in {channel} let go of nothing said there, as reading it failed: {why}"
            );
            crate::calls::notice(ctx, seat.thread, text)?;
        }
        match record::<ReactArgs>(seat) {
            Some(shown) if shown == args => Ok(Run::Done(json!({ "ok": true }))),
            Some(shown) => Ok(Run::Write(vec![
                reaction(&shown, false),
                reaction(&args, true),
            ])),
            None => Ok(Run::Write(vec![reaction(&args, true)])),
        }
    }

    fn show(_: &ExecCtx, seat: &Seat, input: &str) -> Result<Show, Error> {
        let input = prefix(input);
        let settled = |key| input.settled(key).cloned();
        let (Some(channel), Some(seq), Some(emoji)) =
            (settled("channel"), settled("seq"), settled("emoji"))
        else {
            return Ok(Show::Wait);
        };
        let whole = json!({ "channel": channel, "seq": seq, "emoji": emoji });
        let Ok(args) = serde_json::from_value::<ReactArgs>(whole) else {
            return Ok(Show::Wait);
        };
        if seat.shown.is_some() {
            return Ok(Show::Wait);
        }
        Ok(Show::Write {
            writes: vec![reaction(&args, true)],
            shown: json!(args),
        })
    }

    fn void(_: &ExecCtx, seat: &Seat) -> Result<Undo, Error> {
        let writes = record::<ReactArgs>(seat)
            .map(|shown| reaction(&shown, false))
            .into_iter()
            .collect();
        Ok(Undo::Writes(writes))
    }
}
