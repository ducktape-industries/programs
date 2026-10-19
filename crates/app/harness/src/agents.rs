//! The agents harness runs: an agent's setup (identity, its configuration file, its model), and
//! what it hears from whom it answers, routed to the conversation bound to its chat thread or
//! channel, else its canonical one; anyone closes a bound one, its manager the canonical one.
use ::program::{
    BlobId, Ctx, Error, ExecCtx, Ownership, Party, Principal, QueryCtx, not_found, unauthorized,
    unexpected_reply,
};
use abi::role::identity::{AccountNumber, Category, Kind, Profile, Standing};
use attribution::{Actor, Change, Kind as Changed, Reason, Source};

use crate::calls::deferred_while_shown;
use crate::config::Pin;
use crate::conversations::{binding, conversation, current, route};
use crate::listening::chosen;
use crate::pictures::{self, Picture};
use crate::resume;
use crate::state::{BOUND, LAST, PLACES, THREADS};
use crate::turns::{converse, say, say_with};
use crate::waiting;
use crate::{AgentSetup, Body, Config, Configured, Op, PROGRAM, Place, ThreadRow};

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

fn harnessed(profile: &Profile) -> Option<(Agent, Standing)> {
    let Kind::Program {
        manager,
        category: Category::Agent,
        standing,
        script: Some(script),
    } = &profile.kind
    else {
        return None;
    };
    let calls_harness = script.calls == PROGRAM;
    let agent = Agent {
        number: profile.number,
        name: profile.name.clone(),
        manager: *manager,
        path: script.config.clone(),
    };
    calls_harness.then_some((agent, *standing))
}

fn runs_here(profile: &Profile) -> Option<Agent> {
    let (agent, standing) = harnessed(profile)?;
    let active = standing == Standing::Active;
    active.then_some(agent)
}

pub(crate) fn running(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<Agent>, Error> {
    Ok(profile(ctx, number)?.as_ref().and_then(runs_here))
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
    let configured = match crate::models::model(ctx, &config.model)? {
        Some(model) => Configured::Ready {
            snapshot,
            config,
            model,
        },
        None => Configured::UnknownModel { snapshot, config },
    };
    Ok((configured, pin))
}

pub(crate) fn setup(ctx: &QueryCtx, number: AccountNumber) -> Result<Option<AgentSetup>, Error> {
    let Some((agent, standing)) = profile(ctx, number)?.as_ref().and_then(harnessed) else {
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
        && let Configured::Ready { config, .. } | Configured::UnknownModel { config, .. } =
            &mut setup.configured
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
        .and_then(harnessed)
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

fn listed(ctx: &QueryCtx, agent: AccountNumber) -> Result<Option<Ownership>, Error> {
    let checker = &ctx.env().role(abi::role::ACCESS)?.clone();
    let asked = access::Query::List {
        program: PROGRAM.into(),
        key: crate::agent_key(agent),
    };
    match ctx.query::<access::Query, access::Reply>(checker, &asked)? {
        access::Reply::List(list) => Ok(list),
        other => Err(unexpected_reply(checker, "List", &other)),
    }
}

// an agent answers its manager always, and with no list on its key anyone: a list its manager sets
// narrows it
fn answers(ctx: &QueryCtx, agent: &Agent, actor: AccountNumber) -> Result<bool, Error> {
    let by_manager = actor == agent.manager;
    if by_manager {
        return Ok(true);
    }
    let Some(list) = listed(ctx, agent.number)? else {
        return Ok(true);
    };
    let manager = Party::Account(agent.manager);
    Ok(list.allows(Some(&manager), &caller(ctx, actor)?))
}

fn caller(ctx: &QueryCtx, actor: AccountNumber) -> Result<Vec<Party>, Error> {
    let account = Party::Account(actor);
    let program = profile(ctx, actor)?.and_then(|profile| match profile.kind {
        Kind::System(program) => Some(Party::Program(program)),
        Kind::User | Kind::Program { .. } => None,
    });
    Ok(std::iter::once(account).chain(program).collect())
}

pub(crate) fn thread(ctx: &QueryCtx, id: u64) -> Result<ThreadRow, Error> {
    THREADS
        .get(ctx, &id)?
        .ok_or_else(|| not_found(format!("no thread {id}")))
}

pub(crate) fn close(ctx: &ExecCtx, id: u64) -> Result<(), Error> {
    let mut thread = thread(ctx, id)?;
    let bound = binding(ctx, &thread)?.is_some();
    if bound {
        BOUND.remove(ctx, &(thread.agent, thread.place.clone()));
    }
    thread.closed = true;
    THREADS.put(ctx, &id, &thread);
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
         to your other conversations. Each names who wrote it. Answer there: {answer}.",
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
         always reach you, and a message wait still ends with its message.\n\
         - Rules: a chat room's rules (its rules meta) reach you ahead of the first message you \
         hear from it, and again when they change; follow them there.\n\
         - Forge: forge_commit commits files to a git repository and pushes them as account \
         {number}, creating the repository if it does not exist.\n\
         - Pods: pod_exec runs a command in a pod (with every, it answers at once and this \
         conversation hears when the command ends), pod_read reads a file out of a pod, and \
         pod_push pushes a commit a pod made to forge as account {number}.\n\
         - Programs: network_schema gives any program's op, query and reply shapes; \
         network_query asks it a query and network_call sends it an op as account {number}.\n\
         - Images: an image a message carries comes with it, and image_view shows you an \
         image file of files (copy one from a pod to files first).",
        name = agent.name,
        number = agent.number,
        manager = agent.manager,
        conversation = conversation_words(title, bound),
    )
}

struct Designation {
    place: String,
    places: Vec<String>,
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
}

impl Heard {
    // a mention or a direct message starts a turn at once; the rest waits out the settle window
    fn direct(self) -> bool {
        match self {
            Heard::Mention | Heard::Direct | Heard::Waited => true,
            Heard::Thread | Heard::Listen | Heard::Watch => false,
        }
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
    let who = name_of(ctx, actor);
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
        let Source { program, object } = &change.source;
        let place = format!("{program}/{}/{}", object.kind, object.id);
        let text = format!(
            "{who} related you to {program}'s {} {} ({:?}).",
            object.kind, object.id, change.reason
        );
        return Ok(Designation {
            place,
            places: Vec::new(),
            text,
            pictures: Vec::new(),
            room: None,
        });
    };
    let verb = match heard {
        Heard::Mention => "mentioned you",
        Heard::Direct => "wrote to you directly",
        Heard::Thread => "continued a thread with you",
        Heard::Listen | Heard::Watch | Heard::Waited => "wrote",
    };
    let listened = match heard {
        Heard::Listen => ", which you listen to",
        Heard::Mention | Heard::Direct | Heard::Thread | Heard::Watch | Heard::Waited => "",
    };
    let in_thread = match row.thread {
        Some(root) => format!(", in the thread of message {root} (reply_to {root})"),
        None => format!(" (reply_to {} to answer in its thread)", row.seq),
    };
    let text = format!(
        "{who} {verb} in chat channel {channel}{listened}, message {seq}{in_thread}:\n{body}",
        channel = row.channel_id,
        seq = row.seq,
        body = row.text,
    );
    Ok(Designation {
        place: format!("{}/{}", chat::PROGRAM, row.channel_id),
        places: chat_places(&row.channel_id, row.thread),
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
        ctx.emit(PROGRAM, &resent, ::program::Reply::None);
    }
}

fn heard(ctx: &ExecCtx, change: Change) -> Result<Hearing, Error> {
    let from_attribution = ctx.env().sent_by(attribution::PROGRAM).is_ok();
    let held_back_by_harness = ctx.env().sent_by(PROGRAM).is_ok();
    if !(from_attribution || held_back_by_harness) {
        return Err(unauthorized(
            "only attribution delivers a change, and harness one it held back",
        ));
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
    if deferred_while_shown(ctx, &change)? {
        return Ok(Hearing::default());
    }
    let Some(agent) = running(ctx, change.recipient)? else {
        return Ok(Hearing::default());
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
        text,
        pictures,
        room,
    } = designation(ctx, &change, actor, why)?;
    let (mut thread, reached) = route(ctx, &agent, &places, place)?;
    PLACES.put(ctx, &(agent.number, reached), &thread.id);
    // the wake that conversation heard already quotes the message
    if woke == Some(thread.id) {
        return Ok(Hearing { told: None, woke });
    }
    if let Some(room) = &room {
        crate::room_rules::tell(ctx, &mut thread, room)?;
    }
    let text = text + &crate::waits::still_armed(ctx, thread.id)?;
    let author = Principal::Account(actor);
    say_with(ctx, &mut thread, Body::Post { author, text }, pictures);
    THREADS.put(ctx, &thread.id, &thread);
    resume::heard(ctx, thread.id)?;
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

fn message_channel(ctx: &QueryCtx, change: &Change) -> Result<Option<String>, Error> {
    let from_chat = change.source.program == chat::PROGRAM
        && change.source.object.kind == chat::attributed::MESSAGE;
    if !from_chat {
        return Ok(None);
    }
    let asked = chat::Query::MessageById {
        message_id: change.source.object.id.clone(),
    };
    match ctx.query(chat::PROGRAM, &asked)? {
        chat::Reply::Message(row) => Ok(row.map(|row| row.channel_id)),
        other => Err(unexpected_reply(chat::PROGRAM, "MessageById", &other)),
    }
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
        (false, None) => matches!(why, Heard::Mention | Heard::Direct | Heard::Thread),
        (false, Some(chat::Notices::Mentions)) => matches!(why, Heard::Mention | Heard::Direct),
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
    let to_me = matches!(why, Heard::Mention | Heard::Direct);
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
        let Designation { text, pictures, .. } = designation(ctx, change, actor, quoted)?;
        return crate::waits::end_seen(ctx, wait, text, pictures, &channel);
    }
    Ok(None)
}

// who an agent answers is its list's to say; what it did not answer still leaves a trace
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
        // a channel it listens to is talk around the agent, not to it: what it does not answer
        // there is left without a notice, or each such message would add one
        Heard::Listen | Heard::Watch | Heard::Waited => return Ok(()),
    };
    let Source { program, object } = &change.source;
    let text = format!(
        "dropped {what} from account {actor} ({program} {} {}): it isn't on this agent's list",
        object.kind, object.id
    );
    let Designation { place, .. } = designation(ctx, change, actor, heard)?;
    let mut row = conversation(ctx, agent, &place)?;
    say(ctx, &mut row, Body::Notice { text });
    THREADS.put(ctx, &row.id, &row);
    Ok(())
}
