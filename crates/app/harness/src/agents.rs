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
use crate::conversations::{binding, current, route};
use crate::state::{BOUND, LAST, MODELS, PLACES, THREADS};
use crate::turns::{converse, say};
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
    let (snapshot, bytes) = match read(ctx, path, None) {
        Ok(read) => read,
        Err(error) => {
            let reason = format!("{}: {}", error.code, error.message);
            return Ok(Configured::Unreadable { reason });
        }
    };
    let config = match Config::parse(&bytes) {
        Ok(config) => config,
        Err(malformed) => {
            return Ok(Configured::Malformed {
                snapshot,
                malformed,
            });
        }
    };
    Ok(match MODELS.get(ctx, &config.model)? {
        Some(model) => Configured::Ready {
            snapshot,
            config,
            model,
        },
        None => Configured::UnknownModel { snapshot, config },
    })
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

fn answers(ctx: &QueryCtx, agent: &Agent, actor: AccountNumber) -> Result<bool, Error> {
    let by_manager = actor == agent.manager;
    if by_manager {
        return Ok(true);
    }
    let list = listed(ctx, agent.number)?.unwrap_or_default();
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
         - Forge: forge_commit commits files to a git repository and pushes them as account \
         {number}, creating the repository if it does not exist.\n\
         - Programs: network_schema gives any program's op, query and reply shapes; \
         network_query asks it a query and network_call sends it an op as account {number}.",
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
}

fn name_of(ctx: &QueryCtx, number: AccountNumber) -> String {
    match profile(ctx, number) {
        Ok(Some(profile)) => format!("{} (account {number})", profile.name),
        _ => format!("account {number}"),
    }
}

enum Heard {
    Mention,
    Direct,
    Thread,
}

fn heard_as(reason: &Reason) -> Option<Heard> {
    match reason {
        Reason::Mention => Some(Heard::Mention),
        Reason::Defined(name) => match name.as_str() {
            chat::attributed::DIRECT => Some(Heard::Direct),
            chat::attributed::THREAD => Some(Heard::Thread),
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
        });
    };
    let verb = match heard {
        Heard::Mention => "mentioned you",
        Heard::Direct => "wrote to you directly",
        Heard::Thread => "continued a thread with you",
    };
    let in_thread = match row.thread {
        Some(root) => format!(", in the thread of message {root} (reply_to {root})"),
        None => format!(" (reply_to {} to answer in its thread)", row.seq),
    };
    let text = format!(
        "{who} {verb} in chat channel {channel}, message {seq}{in_thread}:\n{body}",
        channel = row.channel_id,
        seq = row.seq,
        body = row.text,
    );
    Ok(Designation {
        place: format!("{}/{}", chat::PROGRAM, row.channel_id),
        places: chat_places(&row.channel_id, row.thread),
        text,
    })
}

pub(crate) async fn attributed(ctx: Ctx, change: Change) -> Result<(), Error> {
    let Some(thread) = heard(&ctx, change)? else {
        return Ok(());
    };
    converse(&ctx, thread).await;
    Ok(())
}

pub(crate) fn hear_again(ctx: &ExecCtx, changes: Vec<Change>) {
    for change in changes {
        let resent = Op::Attributed(Box::new(change));
        ctx.emit(PROGRAM, &resent, ::program::Reply::None);
    }
}

fn heard(ctx: &ExecCtx, change: Change) -> Result<Option<u64>, Error> {
    let from_attribution = ctx.env().sent_by(attribution::PROGRAM).is_ok();
    let held_back_by_harness = ctx.env().sent_by(PROGRAM).is_ok();
    if !(from_attribution || held_back_by_harness) {
        return Err(unauthorized(
            "only attribution delivers a change, and harness one it held back",
        ));
    }
    let added = change.kind == Changed::Added;
    let Actor::Account(actor) = change.actor else {
        return Ok(None);
    };
    let Some(why) = heard_as(&change.reason) else {
        return Ok(None);
    };
    let by_another = actor != change.recipient;
    let told_already =
        LAST.get(ctx, &change.recipient)? == Some((change.source.clone(), change.revision));
    let heeded = added && by_another && !told_already;
    if !heeded {
        return Ok(None);
    }
    if deferred_while_shown(ctx, &change)? {
        return Ok(None);
    }
    let Some(agent) = running(ctx, change.recipient)? else {
        return Ok(None);
    };
    if !answers(ctx, &agent, actor)? {
        return Ok(None);
    }
    LAST.put(
        ctx,
        &agent.number,
        &(change.source.clone(), change.revision),
    );
    let Designation {
        place,
        places,
        text,
    } = designation(ctx, &change, actor, why)?;
    let (mut thread, reached) = route(ctx, &agent, &places, place)?;
    PLACES.put(ctx, &(agent.number, reached), &thread.id);
    let author = Principal::Account(actor);
    say(ctx, &mut thread, Body::Post { author, text });
    THREADS.put(ctx, &thread.id, &thread);
    Ok(Some(thread.id))
}
