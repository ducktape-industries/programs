//! The agents harness runs: an agent's setup (identity, its configuration file, its model), and
//! what it hears from whom it answers, routed to the conversation bound to its chat thread or
//! channel, else its canonical one; anyone closes a bound one, its manager the canonical one.
use ::program::{
    BlobId, Ctx, Error, ExecCtx, Principal, QueryCtx, not_found, unauthorized, unexpected_reply,
    wrong_state,
};
use abi::role::identity::{AccountNumber, Category, Kind, Profile, Standing};
use attribution::{Actor, Change, Kind as Changed, Reason, Source};

use crate::config::Pin;
use crate::conversations::{binding, bound_at, conversation, current, route};
use crate::listening::chosen;
use crate::pictures::{self, Picture};
use crate::resume;
use crate::state::{BOUND, LAST, PLACES, THREADS};
use crate::turns::{converse, say, say_from};
use crate::waiting;
use crate::{AgentSetup, Body, Config, Configured, Op, Place, ThreadRow};

pub(crate) struct Agent {
    pub number: AccountNumber,
    pub name: String,
    pub manager: AccountNumber,
    pub path: String,
}

pub(crate) fn profile(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<Profile>, Error> {
    use abi::role::identity::{Query, Reply};
    let identity = &ctx.env().role(abi::role::IDENTITY)?.clone();
    match ctx.query::<Query, Reply>(identity, &Query::Profile(number))? {
        Reply::Profile(profile) => Ok(profile),
        other => Err(unexpected_reply(identity, "Profile", &other)),
    }
}

pub(crate) fn harnessed(profile: &Profile, me: &str) -> Option<(Agent, Standing)> {
    let Kind::Program {
        manager,
        category: Category::Agent,
        standing,
        script: Some(script),
    } = &profile.kind
    else {
        return None;
    };
    let calls_harness = script.calls == me;
    let agent = Agent {
        number: profile.number,
        name: profile.name.clone(),
        manager: *manager,
        path: script.config.clone(),
    };
    calls_harness.then_some((agent, *standing))
}

fn runs_here(profile: &Profile, me: &str) -> Option<Agent> {
    let (agent, standing) = harnessed(profile, me)?;
    let active = standing == Standing::Active;
    active.then_some(agent)
}

pub(crate) fn running(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<Agent>, Error> {
    Ok(profile(ctx, number)?
        .as_ref()
        .and_then(|profile| runs_here(profile, &ctx.env().program)))
}

pub(crate) fn read(
    ctx: &QueryCtx,
    path: &str,
    mut at: Option<BlobId>,
) -> Result<(BlobId, Vec<u8>), Error> {
    let mut bytes = Vec::new();
    loop {
        let asked = files::Query::Read {
            path: path.to_owned(),
            at,
            offset: bytes.len() as u64,
            len: u64::MAX,
        };
        let contents = match ctx.query(files::PROGRAM, &asked)? {
            files::Reply::Contents(contents) => contents,
            other => return Err(unexpected_reply(files::PROGRAM, "Read", &other)),
        };
        let stalled = contents.bytes.is_empty() && !contents.eof;
        bytes.extend(contents.bytes);
        if contents.eof {
            return Ok((contents.snapshot, bytes));
        }
        if stalled {
            return Err(::program::corrupt(
                "file",
                path.as_bytes(),
                "files answered no bytes before the end of the file",
            ));
        }
        at = Some(contents.snapshot);
    }
}

pub(crate) fn configured(ctx: &QueryCtx, path: &str) -> Result<Configured, Error> {
    Ok(configured_pinned(ctx, path)?.0)
}

// `configured`, and the service or account its front matter pins the agent's model calls to
pub(crate) fn configured_pinned(ctx: &QueryCtx, path: &str) -> Result<(Configured, Pin), Error> {
    let (snapshot, bytes) = match read(ctx, path, None) {
        Ok(read) => read,
        Err(error) => {
            let reason = format!("{}: {}", error.code, error.message);
            return Ok((Configured::Unreadable { reason }, Pin::default()));
        }
    };
    let (config, pin) = match Config::parse_pinned(&bytes) {
        Ok(parsed) => parsed,
        Err(malformed) => {
            let configured = Configured::Malformed {
                snapshot,
                malformed,
            };
            return Ok((configured, Pin::default()));
        }
    };
    let configured = match &pin.provider {
        Some(provider) => routed(ctx, snapshot, config, provider.clone())?,
        None => Configured::Unprovided { snapshot, config },
    };
    Ok((configured, pin))
}

fn routed(
    ctx: &QueryCtx,
    snapshot: BlobId,
    config: Config,
    provider: String,
) -> Result<Configured, Error> {
    let Some(route) = crate::providers::route(ctx, &provider)? else {
        return Ok(Configured::NoRoute {
            snapshot,
            config,
            provider,
        });
    };
    let window = crate::providers::window(ctx, &provider, &config.model)?;
    Ok(Configured::Routed {
        snapshot,
        config,
        provider,
        route: route.services,
        window,
    })
}

pub(crate) fn setup(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<AgentSetup>, Error> {
    let Some((agent, standing)) = profile(ctx, number)?
        .as_ref()
        .and_then(|profile| harnessed(profile, &ctx.env().program))
    else {
        return Ok(None);
    };
    let configured = configured(ctx, &agent.path)?;
    let current = current(ctx, agent.number)?.map(|thread| thread.id);
    Ok(Some(AgentSetup {
        agent: agent.number,
        manager: agent.manager,
        standing,
        path: agent.path,
        configured,
        current,
    }))
}

// `setup` with the system prompt left out, the rest as it is
pub(crate) fn card(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<AgentSetup>, Error> {
    let mut setup = setup(ctx, number)?;
    if let Some(setup) = &mut setup
        && let Configured::Routed { config, .. }
        | Configured::Unprovided { config, .. }
        | Configured::NoRoute { config, .. } = &mut setup.configured
    {
        config.system = String::new();
    }
    Ok(setup)
}

pub(crate) fn harnessed_agent(
    ctx: &QueryCtx,
    number: AccountNumber,
) -> Result<Option<Agent>, Error> {
    Ok(profile(ctx, number)?
        .as_ref()
        .and_then(|profile| harnessed(profile, &ctx.env().program))
        .map(|(agent, _)| agent))
}

pub(crate) fn agent_named(ctx: &QueryCtx, number: AccountNumber) -> Result<Agent, Error> {
    harnessed_agent(ctx, number)?.ok_or_else(|| {
        not_found(format!(
            "account {number} is no agent whose script calls harness"
        ))
    })
}

pub(crate) fn managed(ctx: &ExecCtx, number: AccountNumber) -> Result<Agent, Error> {
    let Principal::Account(_) = ctx.sender()? else {
        return Err(unauthorized(
            "an agent's conversation is started by an account",
        ));
    };
    agent_named(ctx, number)
}

// whom an agent answers is identity's to say: its manager always, anyone else as it or its manager
// set it (`SetOwnership`), asked as every runtime that delivers to an agent asks it (`Hears`)
fn answers(ctx: &QueryCtx, agent: &Agent, actor: AccountNumber) -> Result<bool, Error> {
    let role = ctx.env().role(abi::role::IDENTITY)?.clone();
    let asked = identity::Query::Hears {
        account: agent.number,
        from: actor,
    };
    match ctx.query::<identity::Query, identity::Reply>(&role, &asked)? {
        identity::Reply::Hears(hears) => Ok(hears),
        other => Err(unexpected_reply(&role, "Hears", &other)),
    }
}

pub(crate) fn thread(ctx: &QueryCtx, id: u64) -> Result<ThreadRow, Error> {
    THREADS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no thread {id}")))
}

// a closed conversation's turn ends at once when its model call was cut off, else once the call's
// answer comes back
pub(crate) async fn close(ctx: &Ctx, id: u64) -> Result<(), Error> {
    let mut thread = thread(ctx, id)?;
    if thread.closed {
        return Err(wrong_state(format!("conversation {id} is closed already")));
    }
    let bound = binding(ctx, &thread)?.is_some();
    if bound {
        BOUND.remove(ctx, &(thread.agent, thread.place.clone()));
    }
    thread.closed = true;
    if thread.asking == crate::Asking::Idle {
        crate::turns::let_go(ctx, &mut thread)?;
    }
    THREADS.put(ctx, &id, &thread);
    converse(ctx, id).await;
    Ok(())
}

fn conversation_words(title: &str, bound: Option<&Place>) -> String {
    let Some(place) = bound else {
        return format!(
            "- Conversation: {title}. It is your canonical conversation: every message to you \
             arrives here, from every person and every place (channels, direct messages, \
             threads), except from a place one of your other conversations is bound to. Each \
             names who wrote it and where. Answer each where it came from: chat_post to its \
             channel, with reply_to for its thread."
        );
    };
    let answer = match place {
        Place::Channel { channel } => {
            format!("chat_post to channel {channel}, with reply_to for a message's thread")
        }
        Place::Thread { channel, root } => {
            format!("chat_post to channel {channel}, with reply_to {root}")
        }
    };
    format!(
        "- Conversation: {title}. It is one of several conversations you hold at once, bound \
         to {words}: only messages from there arrive here, and messages from anywhere else go \
         to your other conversations. Each names who wrote it, and a post of yours there from \
         another of your conversations arrives here too. Answer there: {answer}.",
        words = place.words(),
    )
}

pub(crate) fn manual(agent: &Agent, title: &str, bound: Option<&Place>) -> String {
    // the topic itself stays out: the system prompt opens every request, and a line in it that
    // changed would miss the prompt cache from there on
    format!(
        "ducktape resource manual\n\
         - Account: {name}, account {number}, managed by account {manager}.\n\
         {conversation}\n\
         - Topic: thread_topic sets one line on what this conversation is about and where it \
         stands, which people read beside its name; set it again when that changes.\n\
         - Writes: everything written here is written as account {number}, sent by harness, \
         and the network allows only what it allows that account or harness.\n\
         - Tools: each tool call runs in a later block, and its result arrives before the next \
         model call.\n\
         - Chat: chat_post posts to a channel; reply_to puts the post in a thread.\n\
         - Hearing: what reaches you from each chat room is what you chose there, in chat as \
         yourself: Listen (every message), SetNotices with no notices (mentions and replies in \
         your threads, the default), Mentions (mentions only) or Off (nothing). Direct messages \
         always reach you, and a message wait still ends with its message. After a post that \
         mentions you, its author's very next posts in that room reach you too, until anyone \
         else posts there, you included. A reaction on one of your messages reaches you as \
         a light input: it never starts a turn by itself and rides your next one, unless it \
         ends a message wait (from its reactor, or mentions_me).\n\
         - Rules: a chat room's rules (its rules meta) reach you ahead of the first message you \
         hear from it, and again when they change; follow them there.\n\
         - Forge: forge_commit commits files to a git repository and pushes them as account \
         {number}, creating the repository if it does not exist.\n\
         - Programs: network_schema lists the programs that run, a program's ops and queries, \
         and one of them with an example to edit; network_query asks a program a query and \
         network_call sends it an op as account {number}.\n\
         - Images: an image a message carries comes with it, and image_view shows you an \
         image file of files (copy one into files first).",
        name = agent.name,
        number = agent.number,
        manager = agent.manager,
        conversation = conversation_words(title, bound),
    )
}

struct Designation {
    place: String,
    places: Vec<String>,
    from: Option<String>,
    text: String,
    pictures: Vec<Picture>,
    // the chat room a message came from
    room: Option<String>,
}

fn name_of(ctx: &QueryCtx, number: AccountNumber) -> String {
    match profile(ctx, number) {
        Ok(Some(profile)) => format!("{} (account {number})", profile.name),
        _ => format!("account {number}"),
    }
}

#[derive(Clone, Copy)]
enum Heard {
    Mention,
    Direct,
    Thread,
    Listen,
    // a message chat relates to the agent only for its wait: it never reaches a conversation
    Watch,
    // a message a wait listened for, quoted as the wake
    Waited,
    Notice,
    Continued { mentioned: u64 },
    Own,
    Reaction,
}

impl Heard {
    // a mention or a direct message starts a turn at once; the rest waits out the settle window
    fn direct(self) -> bool {
        match self {
            Heard::Mention | Heard::Direct | Heard::Waited | Heard::Notice | Heard::Own => true,
            Heard::Thread
            | Heard::Listen
            | Heard::Watch
            | Heard::Continued { .. }
            | Heard::Reaction => false,
        }
    }

    fn light(self) -> bool {
        matches!(self, Heard::Reaction)
    }
}

fn heard_as(reason: &Reason) -> Option<Heard> {
    match reason {
        Reason::Mention => Some(Heard::Mention),
        Reason::Defined(name) => match name.as_str() {
            chat::attributed::DIRECT => Some(Heard::Direct),
            chat::attributed::THREAD => Some(Heard::Thread),
            chat::attributed::LISTEN => Some(Heard::Listen),
            chat::attributed::WATCH => Some(Heard::Watch),
            chat::attributed::NOTICE => Some(Heard::Notice),
            chat::attributed::REACTION => Some(Heard::Reaction),
            _ => None,
        },
        Reason::Authorship
        | Reason::Ownership
        | Reason::Assignment
        | Reason::Credit
        | Reason::Result
        | Reason::Report => None,
    }
}

fn chat_places(channel: &str, thread: Option<u64>) -> Vec<String> {
    let whole = Place::Channel {
        channel: channel.to_owned(),
    };
    let Some(root) = thread else {
        return vec![whole.key()];
    };
    let within = Place::Thread {
        channel: channel.to_owned(),
        root,
    };
    vec![within.key(), whole.key()]
}

fn designation(
    ctx: &QueryCtx,
    change: &Change,
    actor: AccountNumber,
    heard: Heard,
) -> Result<Designation, Error> {
    if let Some((row, emoji)) = reacted_on(ctx, change)? {
        return Ok(reacted(ctx, row, actor, &emoji));
    }
    let from_chat = change.source.program == chat::PROGRAM
        && change.source.object.kind == chat::attributed::MESSAGE;
    let message = match from_chat {
        true => {
            let asked = chat::Query::MessageById {
                message_id: change.source.object.id.clone(),
            };
            match ctx.query(chat::PROGRAM, &asked)? {
                chat::Reply::Message(row) => row,
                other => return Err(unexpected_reply(chat::PROGRAM, "MessageById", &other)),
            }
        }
        false => None,
    };
    let Some(row) = message else {
        let who = name_of(ctx, actor);
        let Source { program, object } = &change.source;
        let place = format!("{program}/{}/{}", object.kind, object.id);
        let text = format!(
            "{who} related you to {program}'s {} {} ({:?}).",
            object.kind, object.id, change.reason
        );
        return Ok(Designation {
            place,
            places: Vec::new(),
            from: None,
            text,
            pictures: Vec::new(),
            room: None,
        });
    };
    said_in(ctx, row, actor, heard)
}

fn reacted_on(ctx: &QueryCtx, change: &Change) -> Result<Option<(chat::MsgRow, String)>, Error> {
    let reaction = change.source.program == chat::PROGRAM
        && change.source.object.kind == chat::attributed::REACTION;
    if !reaction {
        return Ok(None);
    }
    let Ok(chat::attributed::Reacted { message_id, emoji }) = abi::decode(&change.detail) else {
        return Ok(None);
    };
    let asked = chat::Query::MessageById { message_id };
    match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Message(row) => Ok(row.map(|row| (row, emoji))),
        other => Err(unexpected_reply(chat::PROGRAM, "MessageById", &other)),
    }
}

fn reacted(ctx: &QueryCtx, row: chat::MsgRow, actor: AccountNumber, emoji: &str) -> Designation {
    let who = name_of(ctx, actor);
    let in_thread = match row.thread {
        Some(root) => format!(", in the thread of message {root} (reply_to {root})"),
        None => String::new(),
    };
    let quoted = row.text.lines().next().unwrap_or_default();
    let text = format!(
        "{who} reacted {emoji} to your message {seq} in chat channel {channel}{in_thread}:\n{quoted}",
        seq = row.seq,
        channel = row.channel_id,
    );
    let places = chat_places(&row.channel_id, row.thread);
    Designation {
        place: format!("{}/{}", chat::PROGRAM, row.channel_id),
        from: places.first().cloned(),
        places,
        text,
        pictures: Vec::new(),
        room: Some(row.channel_id),
    }
}

fn said_in(
    ctx: &QueryCtx,
    row: chat::MsgRow,
    actor: AccountNumber,
    heard: Heard,
) -> Result<Designation, Error> {
    let who = name_of(ctx, actor);
    let asked = chat::Query::Clients {
        channel_id: row.channel_id.clone(),
        seqs: vec![row.seq],
    };
    let client = match ctx.query(chat::PROGRAM, &asked) {
        Ok(chat::Reply::Clients(clients)) => clients.into_iter().find_map(|(_, client)| client),
        Ok(other) => return Err(unexpected_reply(chat::PROGRAM, "Clients", &other)),
        Err(_) => None,
    };
    let from = client
        .map(|client| format!(" from {client}"))
        .unwrap_or_default();
    let verb = match heard {
        Heard::Mention => "mentioned you",
        Heard::Direct => "wrote to you directly",
        Heard::Thread => "continued a thread with you",
        Heard::Notice => "sent you a notice",
        Heard::Continued { .. } => "went on",
        Heard::Listen | Heard::Watch | Heard::Waited => "wrote",
        Heard::Own => "wrote from another of your conversations",
        Heard::Reaction => "reacted to your message",
    };
    let listened = match heard {
        Heard::Listen => ", which you listen to",
        Heard::Mention
        | Heard::Direct
        | Heard::Thread
        | Heard::Watch
        | Heard::Waited
        | Heard::Notice
        | Heard::Continued { .. }
        | Heard::Own
        | Heard::Reaction => "",
    };
    let after = match heard {
        Heard::Continued { mentioned } => format!(", after mentioning you in message {mentioned}"),
        Heard::Mention
        | Heard::Direct
        | Heard::Thread
        | Heard::Listen
        | Heard::Watch
        | Heard::Waited
        | Heard::Notice
        | Heard::Own
        | Heard::Reaction => String::new(),
    };
    let in_thread = match row.thread {
        Some(root) => format!(", in the thread of message {root} (reply_to {root})"),
        None => format!(" (reply_to {} to answer in its thread)", row.seq),
    };
    let text = format!(
        "{who}{from} {verb} in chat channel {channel}{listened}, message {seq}{after}{in_thread}:\n{body}",
        channel = row.channel_id,
        seq = row.seq,
        body = row.text,
    );
    let places = chat_places(&row.channel_id, row.thread);
    let from = match row.notice() {
        Some(_) => None,
        None => places.first().cloned(),
    };
    Ok(Designation {
        place: format!("{}/{}", chat::PROGRAM, row.channel_id),
        places,
        from,
        text,
        pictures: pictures::of_message(&row.blocks),
        room: Some(row.channel_id),
    })
}

pub(crate) async fn attributed(ctx: Ctx, change: Change) -> Result<(), Error> {
    let Hearing { told, woke } = heard(&ctx, change)?;
    // a wait the message ends wakes its conversation at once, as a direct input does
    if let Some(thread) = woke {
        waiting::end(&ctx, thread).await?;
        converse(&ctx, thread).await;
    }
    let Some((thread, heard)) = told else {
        return Ok(());
    };
    if heard.light() {
        return Ok(());
    }
    match heard.direct() {
        true => waiting::end(&ctx, thread).await?,
        false => {
            if waiting::wait(&ctx, thread).await? {
                return Ok(());
            }
        }
    }
    converse(&ctx, thread).await;
    Ok(())
}

pub(crate) fn hear_again(ctx: &ExecCtx, changes: Vec<Change>) {
    for change in changes {
        let resent = Op::Attributed(Box::new(change));
        ctx.emit(&ctx.env().program, &resent, ::program::Reply::None);
    }
}

pub(crate) fn posts_in(writes: &[crate::tools::Write]) -> Vec<String> {
    let to_chat = writes.iter().filter(|write| write.target == chat::PROGRAM);
    to_chat
        .filter_map(|write| match abi::decode::<chat::Op>(&write.payload) {
            Ok(chat::Op::PostMessage { message_id, .. }) => Some(message_id),
            _ => None,
        })
        .collect()
}

pub(crate) fn own_posts(ctx: &ExecCtx, agent: AccountNumber, writer: u64, posts: &[String]) {
    for message_id in posts {
        let asked = chat::Query::MessageById {
            message_id: message_id.clone(),
        };
        let Ok(chat::Reply::Message(Some(row))) = ctx.query(chat::PROGRAM, &asked) else {
            continue;
        };
        if let Ok(Some(thread)) = own_post(ctx, agent, writer, row) {
            let resume = Op::Resume { thread };
            ctx.emit(&ctx.env().program, &resume, ::program::Reply::None);
        }
    }
}

fn own_post(
    ctx: &ExecCtx,
    agent: AccountNumber,
    writer: u64,
    row: chat::MsgRow,
) -> Result<Option<u64>, Error> {
    let places = chat_places(&row.channel_id, row.thread);
    let Some((mut thread, reached)) = bound_at(ctx, agent, &places)? else {
        return Ok(None);
    };
    if thread.id == writer {
        return Ok(None);
    }
    let Designation {
        text,
        pictures,
        from,
        room,
        ..
    } = said_in(ctx, row, agent, Heard::Own)?;
    PLACES.put(ctx, &(agent, reached), &thread.id);
    if let Some(room) = &room {
        crate::room_rules::tell(ctx, &mut thread, room)?;
    }
    let text = text + &crate::waits::still_armed(ctx, thread.id)?;
    let author = Principal::Account(agent);
    say_from(
        ctx,
        &mut thread,
        Body::Post { author, text },
        pictures,
        from,
    );
    THREADS.put(ctx, &thread.id, &thread);
    resume::heard(ctx, thread.id)?;
    Ok(Some(thread.id))
}

fn heard(ctx: &ExecCtx, change: Change) -> Result<Hearing, Error> {
    let from_attribution = ctx.env().sent_by(attribution::PROGRAM).is_ok();
    let held_back_by_harness = ctx.env().sent_by(&ctx.env().program).is_ok();
    if !(from_attribution || held_back_by_harness) {
        return Err(unauthorized(
            "only attribution delivers a change, and harness one it held back",
        ));
    }
    if crate::woken::from_wake(&change) {
        let woke = crate::woken::told(ctx, &change)?;
        return Ok(Hearing { told: None, woke });
    }
    let added = change.kind == Changed::Added;
    let Actor::Account(actor) = change.actor else {
        return Ok(Hearing::default());
    };
    let Some(why) = heard_as(&change.reason) else {
        return Ok(Hearing::default());
    };
    let by_another = actor != change.recipient;
    let told_already =
        LAST.get(ctx, &change.recipient)? == Some((change.source.clone(), change.revision));
    let heeded = added && by_another && !told_already;
    if !heeded {
        return Ok(Hearing::default());
    }
    let Some(agent) = running(ctx, change.recipient)? else {
        return Ok(Hearing::default());
    };
    let posted = posted(ctx, &change)?;
    let why = match &posted {
        Some(row) => continued(ctx, &agent, row, actor, why)?,
        None => why,
    };
    let woke = waited(ctx, &agent, &change, actor, why)?;
    let seen = (change.source.clone(), change.revision);
    // what the agent chose not to hear is never delivered: nothing of it is written, so another
    // relation of the same message may still bring it
    if !chooses(ctx, &agent, &change, why)? {
        if woke.is_some() {
            LAST.put(ctx, &agent.number, &seen);
        }
        return Ok(Hearing { told: None, woke });
    }
    if !answers(ctx, &agent, actor)? {
        match woke {
            Some(_) => LAST.put(ctx, &agent.number, &seen),
            None => dropped(ctx, &agent, actor, why, &change)?,
        }
        return Ok(Hearing { told: None, woke });
    }
    LAST.put(ctx, &agent.number, &seen);
    let Designation {
        place,
        places,
        from,
        text,
        pictures,
        room,
    } = designation(ctx, &change, actor, why)?;
    let (mut thread, reached) = route(ctx, &agent, &places, place)?;
    PLACES.put(ctx, &(agent.number, reached), &thread.id);
    if let Some(row) = &posted {
        match why {
            Heard::Mention => crate::runs::open(ctx, agent.number, row, actor, thread.id)?,
            Heard::Continued { .. } => {
                crate::runs::reached(ctx, agent.number, &row.channel_id, thread.id)?
            }
            Heard::Direct
            | Heard::Thread
            | Heard::Listen
            | Heard::Watch
            | Heard::Waited
            | Heard::Notice
            | Heard::Own
            | Heard::Reaction => {}
        }
    }
    // the wake that conversation heard already quotes the message
    if woke == Some(thread.id) {
        return Ok(Hearing { told: None, woke });
    }
    if let Some(room) = &room {
        crate::room_rules::tell(ctx, &mut thread, room)?;
    }
    let text = text + &crate::waits::still_armed(ctx, thread.id)?;
    let author = Principal::Account(actor);
    let body = match why.light() {
        true => Body::Light { author, text },
        false => Body::Post { author, text },
    };
    say_from(ctx, &mut thread, body, pictures, from);
    THREADS.put(ctx, &thread.id, &thread);
    if !why.light() {
        resume::heard(ctx, thread.id)?;
    }
    Ok(Hearing {
        told: Some((thread.id, why)),
        woke,
    })
}

// what a change brought: the conversation it reached as an input, and the one whose wait it ended
#[derive(Default)]
struct Hearing {
    told: Option<(u64, Heard)>,
    woke: Option<u64>,
}

fn posted(ctx: &QueryCtx, change: &Change) -> Result<Option<chat::MsgRow>, Error> {
    if let Some((row, _)) = reacted_on(ctx, change)? {
        return Ok(Some(row));
    }
    let from_chat = change.source.program == chat::PROGRAM
        && change.source.object.kind == chat::attributed::MESSAGE;
    if !from_chat {
        return Ok(None);
    }
    let asked = chat::Query::MessageById {
        message_id: change.source.object.id.clone(),
    };
    match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Message(row) => Ok(row),
        other => Err(unexpected_reply(chat::PROGRAM, "MessageById", &other)),
    }
}

fn message_channel(ctx: &QueryCtx, change: &Change) -> Result<Option<String>, Error> {
    Ok(posted(ctx, change)?.map(|row| row.channel_id))
}

fn continued(
    ctx: &ExecCtx,
    agent: &Agent,
    row: &chat::MsgRow,
    actor: AccountNumber,
    why: Heard,
) -> Result<Heard, Error> {
    if why.light() {
        return Ok(why);
    }
    let listened = matches!(why, Heard::Listen);
    let run = crate::runs::step(ctx, agent.number, row, actor, listened)?;
    Ok(match (run, why) {
        (Some(mentioned), Heard::Watch | Heard::Thread) => Heard::Continued { mentioned },
        _ => why,
    })
}

// whether a chat message reaches the agent, by what it chose for the message's room in chat:
// every message where it listens; else a mention or a reply in its thread by default, a mention
// alone when cut to mentions, nothing when off. A direct message always reaches it, and what chat
// relates to it only for a wait (`watch`) never does
fn chooses(ctx: &QueryCtx, agent: &Agent, change: &Change, why: Heard) -> Result<bool, Error> {
    if matches!(why, Heard::Watch | Heard::Waited) {
        return Ok(false);
    }
    let Some(channel) = message_channel(ctx, change)? else {
        return Ok(true);
    };
    if chat::dm_peers(&channel).is_some() {
        return Ok(true);
    }
    Ok(match chosen(ctx, agent.number, &channel)? {
        (true, _) => true,
        (false, None) => matches!(
            why,
            Heard::Mention
                | Heard::Direct
                | Heard::Thread
                | Heard::Notice
                | Heard::Continued { .. }
                | Heard::Reaction
        ),
        (false, Some(chat::Notices::Mentions)) => matches!(
            why,
            Heard::Mention
                | Heard::Direct
                | Heard::Notice
                | Heard::Continued { .. }
                | Heard::Reaction
        ),
        (false, Some(chat::Notices::Off)) => false,
    })
}

// the first of the agent's message waits a chat message matches (its channel, its author if the
// wait names one, a mention or a direct message if it asks for one) ends with the message,
// whoever wrote it: the conversation it wakes
fn waited(
    ctx: &ExecCtx,
    agent: &Agent,
    change: &Change,
    actor: AccountNumber,
    why: Heard,
) -> Result<Option<u64>, Error> {
    let waits = crate::waits::message_waits(ctx, agent.number)?;
    if waits.is_empty() {
        return Ok(None);
    }
    let Some(channel) = message_channel(ctx, change)? else {
        return Ok(None);
    };
    let to_me = matches!(
        why,
        Heard::Mention | Heard::Direct | Heard::Notice | Heard::Reaction
    );
    for (waited_in, wait) in waits {
        let Some(crate::WaitRow {
            event: crate::WaitFor::Message {
                from, mentions_me, ..
            },
            ..
        }) = crate::waits::wait_row(ctx, wait)?
        else {
            continue;
        };
        let theirs = from.is_none_or(|from| from == actor);
        if waited_in != channel || !theirs || (mentions_me && !to_me) {
            continue;
        }
        let quoted = match why {
            Heard::Listen | Heard::Watch => Heard::Waited,
            other => other,
        };
        let Designation {
            text,
            pictures,
            from,
            ..
        } = designation(ctx, change, actor, quoted)?;
        return crate::waits::end_seen(ctx, wait, text, pictures, &channel, from);
    }
    Ok(None)
}

// who an agent answers is identity's to say; what it did not answer still leaves a trace
fn dropped(
    ctx: &ExecCtx,
    agent: &Agent,
    actor: AccountNumber,
    heard: Heard,
    change: &Change,
) -> Result<(), Error> {
    let what = match heard {
        Heard::Mention => "a mention",
        Heard::Direct => "a direct message",
        Heard::Thread => "a thread reply",
        Heard::Notice => "a notice",
        Heard::Continued { .. } => "a mention's continuation",
        // a channel it listens to is talk around the agent, not to it: what it does not answer
        // there is left without a notice, or each such message would add one
        Heard::Listen | Heard::Watch | Heard::Waited | Heard::Own | Heard::Reaction => {
            return Ok(());
        }
    };
    let Source { program, object } = &change.source;
    let text = format!(
        "dropped {what} from account {actor} ({program} {} {}): this agent doesn't answer it (identity: Answers)",
        object.kind, object.id
    );
    let Designation { place, .. } = designation(ctx, change, actor, heard)?;
    let mut row = conversation(ctx, agent, &place)?;
    say(ctx, &mut row, Body::Notice { text });
    THREADS.put(ctx, &row.id, &row);
    Ok(())
}
