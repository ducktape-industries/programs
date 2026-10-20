//! Waits: one event an agent names (a block's height, a pod's end, a chat message, a forge ref or
//! change, a files path) wakes the conversation that waits for it once, and the turn that armed it
//! ends. harness hears it from the scheduler, the pods wait stream, chat, or forge's or files' watchers.
use ::program::{
    BlobId, Cause, Ctx, Error, ExecCtx, Principal, QueryCtx, corrupt, decoded, invalid, not_found,
    unauthorized, unexpected_reply, wrong_state,
};
use abi::role::egress::{DEFAULT_WAIT, Delivery, Event, Request, http};
use abi::role::identity::AccountNumber;
use abi::role::scheduler;
use serde_json::Value;
use store::{PageRequest, PageResponse};

use crate::agents::{configured, harnessed_agent, thread};
use crate::calls::refusal;
use crate::conversations::current;
use crate::pictures::Picture;
use crate::state::{
    ARMED, CHANGE_WAITS, CLOCK, Clock, ENDING, FILE_WAIT_COUNT, FILE_WAITS, FORGE_WAITS, FileWatch,
    MESSAGE_WAITS, POD_WAITS, PodWatch, REF_WAITS, SCHEDULED, THREADS, Tick, WAIT_COUNT,
    WAIT_ENTRIES, WAIT_REFS, WAITING, WAITS, WATCHED,
};
use crate::turns::{bound, converse, request, say_from};
use crate::{
    Asking, Body, Ended, Op, PROGRAM, ThreadRow, Until, WaitFor, WaitRow, filed_as, forged_as,
    listening, pod_event_as,
};

pub(crate) const PODS: &str = "pods";

pub(crate) fn what(event: &WaitFor) -> String {
    match event {
        WaitFor::Height { height } => format!("block {height}"),
        WaitFor::Pod { name } => format!("pod {name} to end"),
        WaitFor::Message {
            channel,
            from,
            mentions_me,
        } => {
            let who = from.map_or("anyone".to_owned(), |from| format!("account {from}"));
            let to_you = if *mentions_me {
                " addressed to you"
            } else {
                ""
            };
            format!("a message{to_you} from {who} in chat channel {channel}")
        }
        WaitFor::Change { repo, n } => format!("change {n} in {repo} to move, close or merge"),
        WaitFor::Ref { repo, reference } => format!("{reference} in {repo} to move"),
        WaitFor::File { path } => format!("{path} to change in files"),
        WaitFor::Settled { repo, n, until } => {
            let how = match until {
                Until::Merged => "be merged",
                Until::Closed => "be closed",
                Until::Either => "be merged or closed",
            };
            format!("change {n} in {repo} to {how}")
        }
    }
}

fn kept(ctx: &QueryCtx, wait: u64) -> Result<WaitRow, Error> {
    WAITS
        .get(ctx, &wait)?
        .ok_or_else(|| not_found(format!("no wait {wait}")))
}

fn open(ctx: &QueryCtx, wait: u64) -> Result<Option<WaitRow>, Error> {
    Ok(WAITS.get(ctx, &wait)?.filter(|row| row.ended.is_none()))
}

fn by_harness(ctx: &QueryCtx) -> bool {
    ctx.env()
        .sending_program()
        .is_ok_and(|program| program == PROGRAM)
}

fn scheduler_of(ctx: &QueryCtx) -> Result<String, Error> {
    Ok(ctx.env().role(abi::role::SCHEDULER)?.clone())
}

async fn schedule(ctx: &Ctx, height: u64, op: &Op) -> Result<u64, Error> {
    let schedule = scheduler::Op::Schedule {
        height,
        target: PROGRAM.into(),
        payload: abi::encode(op),
    };
    let bound = scheduler_of(ctx)?;
    let answer = ctx.call_raw(bound.clone(), abi::encode(&schedule)).await?;
    abi::decode(&answer).map_err(|fault| unexpected_reply(&bound, "Schedule", &fault.sentence))
}

async fn unschedule(ctx: &Ctx, entry: u64) -> Result<(), Error> {
    let cancel = scheduler::Op::Cancel { entry };
    ctx.call_raw(scheduler_of(ctx)?, abi::encode(&cancel))
        .await?;
    Ok(())
}

fn normalized(event: WaitFor) -> WaitFor {
    match event {
        WaitFor::Ref { repo, reference } if !reference.starts_with("refs/") => WaitFor::Ref {
            repo,
            reference: format!("refs/heads/{reference}"),
        },
        other => other,
    }
}

pub(crate) async fn wait(ctx: &Ctx, thread_id: u64, event: WaitFor) -> Result<(), Error> {
    let agent = ctx.sender_account()?;
    let row = thread(ctx, thread_id)?;
    if row.agent != agent {
        return Err(invalid(format!(
            "conversation {thread_id} is not account {agent}'s: choose a conversation owned by the acting account"
        )));
    }
    if row.closed {
        return Err(wrong_state(format!(
            "conversation {thread_id} is closed: open a new conversation before waiting"
        )));
    }
    let event = normalized(event);
    let id = WAIT_COUNT.get(ctx)?.unwrap_or(0) + 1;
    WAIT_COUNT.put(ctx, &id);
    match &event {
        WaitFor::Height { height } => {
            let entry = schedule(ctx, *height, &Op::WaitDue).await?;
            WAIT_ENTRIES.put(ctx, &entry, &id);
            SCHEDULED.put(ctx, &id, &entry);
        }
        WaitFor::Pod { name } => {
            named_pod(name)?;
            watch_from_next_block(ctx, id).await?;
        }
        WaitFor::Message { channel, .. } => {
            if !listening::exists(ctx, channel)? {
                return Err(not_found(format!("chat holds no channel {channel}")));
            }
            MESSAGE_WAITS.insert(ctx, &(agent, (channel.clone(), id)));
            WATCHED.insert(ctx, &id);
            listening::watch(ctx, agent, channel, true);
        }
        WaitFor::Change { repo, n } => {
            if let Some(source) = open_change(ctx, repo, *n)? {
                watch_ref(ctx, id, repo, source);
            }
            CHANGE_WAITS.insert(ctx, &((repo.clone(), *n), id));
            forge_waits(ctx, true)?;
        }
        WaitFor::Settled { repo, n, .. } => {
            open_change(ctx, repo, *n)?;
            CHANGE_WAITS.insert(ctx, &((repo.clone(), *n), id));
            forge_waits(ctx, true)?;
        }
        WaitFor::Ref { repo, reference } => {
            known_repo(ctx, repo)?;
            watch_ref(ctx, id, repo, reference.clone());
            forge_waits(ctx, true)?;
        }
        WaitFor::File { path } => {
            let node = node_at(ctx, path, None)?;
            let watch = FileWatch {
                path: path.clone(),
                node,
            };
            FILE_WAITS.put(ctx, &id, &watch);
            files_waits(ctx, true)?;
        }
    }
    let waiting = WaitRow {
        id,
        agent,
        thread: thread_id,
        made: ctx.env().height,
        event,
        ended: None,
    };
    WAITS.put(ctx, &id, &waiting);
    WAITING.insert(ctx, &(agent, id));
    ARMED.insert(ctx, &(thread_id, id));
    let in_turn = thread(ctx, thread_id)?.asking != Asking::Idle;
    if in_turn {
        ENDING.insert(ctx, &thread_id);
    }
    ctx.set_return_data(abi::encode(&id));
    Ok(())
}

pub(crate) async fn unwait(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let row = kept(ctx, wait)?;
    let theirs = ctx.sender_account()? == row.agent;
    if !theirs {
        return Err(unauthorized(format!(
            "wait {wait} is account {}'s to cancel",
            row.agent
        )));
    }
    if let Some(ended) = &row.ended {
        return Err(wrong_state(format!(
            "wait {wait} ended at block {}: create a new wait instead of cancelling the ended one",
            ended.height
        )));
    }
    if let Some(entry) = SCHEDULED.get(ctx, &wait)? {
        unschedule(ctx, entry).await?;
    }
    if let Some(PodWatch {
        entry: Some(entry), ..
    }) = POD_WAITS.get(ctx, &wait)?
    {
        unschedule(ctx, entry).await?;
    }
    disarm(ctx, &row)?;
    WAITS.remove(ctx, &wait);
    WAITING.remove(ctx, &(row.agent, wait));
    Ok(())
}

fn disarm(ctx: &ExecCtx, row: &WaitRow) -> Result<(), Error> {
    ARMED.remove(ctx, &(row.thread, row.id));
    if let Some(entry) = SCHEDULED.get(ctx, &row.id)? {
        WAIT_ENTRIES.remove(ctx, &entry);
        SCHEDULED.remove(ctx, &row.id);
    }
    POD_WAITS.remove(ctx, &row.id);
    if let Some(key) = WAIT_REFS.get(ctx, &row.id)? {
        REF_WAITS.remove(ctx, &(key, row.id));
        WAIT_REFS.remove(ctx, &row.id);
    }
    match &row.event {
        WaitFor::Height { .. } | WaitFor::Pod { .. } => {}
        WaitFor::Message { channel, .. } => {
            MESSAGE_WAITS.remove(ctx, &(row.agent, (channel.clone(), row.id)));
            let watched = WATCHED.has(ctx, &row.id);
            WATCHED.remove(ctx, &row.id);
            unwatch(ctx, row.agent, channel, watched)?;
        }
        WaitFor::Change { repo, n } | WaitFor::Settled { repo, n, .. } => {
            CHANGE_WAITS.remove(ctx, &((repo.clone(), *n), row.id));
            forge_waits(ctx, false)?;
        }
        WaitFor::Ref { .. } => forge_waits(ctx, false)?,
        WaitFor::File { .. } => {
            FILE_WAITS.remove(ctx, &row.id);
            files_waits(ctx, false)?;
        }
    }
    Ok(())
}

pub(crate) fn end(ctx: &ExecCtx, wait: u64, what: String) -> Result<Option<u64>, Error> {
    ended(ctx, wait, what, Vec::new(), None, None)
}

pub(crate) fn end_seen(
    ctx: &ExecCtx,
    wait: u64,
    what: String,
    pictures: Vec<Picture>,
    room: &str,
    place: Option<String>,
) -> Result<Option<u64>, Error> {
    ended(ctx, wait, what, pictures, Some(room), place)
}

fn ended(
    ctx: &ExecCtx,
    wait: u64,
    what: String,
    pictures: Vec<Picture>,
    room: Option<&str>,
    place: Option<String>,
) -> Result<Option<u64>, Error> {
    let Some(mut row) = open(ctx, wait)? else {
        return Ok(None);
    };
    row.ended = Some(Ended {
        height: ctx.env().height,
        text: what.clone(),
    });
    WAITS.put(ctx, &wait, &row);
    disarm(ctx, &row)?;
    if let Some(mut closed) = THREADS.get(ctx, &row.thread)?.filter(|told| told.closed) {
        let text = format!("Wait {wait} is over: {what}");
        crate::bookings::undelivered(ctx, &mut closed, text);
        return Ok(None);
    }
    let Some(mut told) = told(ctx, &row)? else {
        return Ok(None);
    };
    let still = still_armed(ctx, told.id)?;
    let stop = if what.contains('\n') { "" } else { "." };
    let text = format!("Wait {wait} is over: {what}{stop}{still}");
    if let Some(room) = room {
        crate::room_rules::tell(ctx, &mut told, room)?;
    }
    let author = Principal::Root;
    say_from(ctx, &mut told, Body::Post { author, text }, pictures, place);
    THREADS.put(ctx, &told.id, &told);
    Ok(Some(told.id))
}

async fn fire(ctx: &Ctx, wait: u64, what: String) -> Result<(), Error> {
    if let Some(thread) = end(ctx, wait, what)? {
        converse(ctx, thread).await;
    }
    Ok(())
}

fn told(ctx: &QueryCtx, row: &WaitRow) -> Result<Option<ThreadRow>, Error> {
    match THREADS.get(ctx, &row.thread)?.filter(|told| !told.closed) {
        Some(told) => Ok(Some(told)),
        None => current(ctx, row.agent),
    }
}

pub(crate) async fn wait_due(ctx: &Ctx) -> Result<(), Error> {
    let (Cause::Scheduled { entry }, true) = (ctx.env().cause.clone(), by_harness(ctx)) else {
        return Err(unauthorized(
            "a wait for a height is due only as harness's own scheduled entry, at its block",
        ));
    };
    let Some(wait) = WAIT_ENTRIES.get(ctx, &entry)? else {
        return Ok(());
    };
    let height = ctx.env().height;
    fire(ctx, wait, format!("block {height} has come")).await
}

pub(crate) fn message_waits(
    ctx: &QueryCtx,
    agent: AccountNumber,
) -> Result<Vec<(String, u64)>, Error> {
    let waits = MESSAGE_WAITS.scan(ctx, MESSAGE_WAITS.prefix_of(&agent))?;
    Ok(waits.into_iter().map(|(_, waited)| waited).collect())
}

fn unwatch(ctx: &ExecCtx, agent: AccountNumber, channel: &str, watched: bool) -> Result<(), Error> {
    let needed = message_waits(ctx, agent)?
        .iter()
        .any(|(waited, _)| waited == channel);
    if needed {
        return Ok(());
    }
    if watched {
        listening::watch(ctx, agent, channel, false);
        return Ok(());
    }
    let Some(harnessed) = harnessed_agent(ctx, agent)? else {
        return Ok(());
    };
    let configured = configured(ctx, &harnessed.path)?;
    let named = configured.config().is_some_and(|(_, config)| {
        config
            .listen
            .iter()
            .any(|entry| listening::names(entry, channel))
    });
    if !named {
        listening::tell(ctx, agent, channel, false);
    }
    Ok(())
}

fn open_change(ctx: &QueryCtx, repo: &str, n: u64) -> Result<Option<String>, Error> {
    let asked = forge::Query::Change {
        repo: repo.to_owned(),
        n,
        page: PageRequest::first(1),
    };
    let change = match ctx.query(forge::PROGRAM, &asked)? {
        forge::Reply::Change { change, .. } => change,
        other => return Err(unexpected_reply(forge::PROGRAM, "Change", &other)),
    };
    let ended = match change.state {
        forge::ChangeState::Open => {
            return Ok(match change.from {
                forge::Revision::Ref(name) => Some(String::from_utf8_lossy(&name).into_owned()),
                forge::Revision::Oid(_) => None,
            });
        }
        forge::ChangeState::Closed => "closed",
        forge::ChangeState::Merged => "merged",
    };
    Err(wrong_state(format!(
        "change {n} in {repo} is already {ended}: read the change's final state or wait on an open change"
    )))
}

fn known_repo(ctx: &QueryCtx, repo: &str) -> Result<(), Error> {
    let asked = forge::Query::Repo {
        repo: repo.to_owned(),
    };
    match ctx.query(forge::PROGRAM, &asked)? {
        forge::Reply::Repo { .. } => Ok(()),
        other => Err(unexpected_reply(forge::PROGRAM, "Repo", &other)),
    }
}

fn watch_ref(ctx: &ExecCtx, wait: u64, repo: &str, reference: String) {
    let key = (repo.to_owned(), reference);
    REF_WAITS.insert(ctx, &(key.clone(), wait));
    WAIT_REFS.put(ctx, &wait, &key);
}

fn forge_waits(ctx: &ExecCtx, opened: bool) -> Result<(), Error> {
    let before = FORGE_WAITS.get(ctx)?.unwrap_or(0);
    let after = match opened {
        true => before + 1,
        false => before.saturating_sub(1),
    };
    FORGE_WAITS.put(ctx, &after);
    let deliver = match (before, after) {
        (0, 1) => forged_as(),
        (1, 0) => Vec::new(),
        _ => return Ok(()),
    };
    let watch = forge::Op::Watch { deliver };
    ctx.emit(forge::PROGRAM, &watch, ::program::Reply::None);
    Ok(())
}

fn who(by: Option<&Principal>) -> String {
    match by {
        Some(Principal::Account(number)) => format!("account {number}"),
        Some(Principal::Root) => "the network".to_owned(),
        None => "someone".to_owned(),
    }
}

fn moved_words((old, new): (&Option<String>, &Option<String>), by: Option<&Principal>) -> String {
    let by = who(by);
    match (old, new) {
        (Some(old), Some(new)) => format!("moved from {old} to {new} by {by}"),
        (None, Some(new)) => format!("was made at {new} by {by}"),
        (_, None) => format!("was deleted by {by}"),
    }
}

fn change_ended(
    ctx: &QueryCtx,
    repo: &str,
    n: u64,
    how: &str,
    ending: Until,
) -> Result<Vec<(u64, String)>, Error> {
    let key = (repo.to_owned(), n);
    let mut ended = Vec::new();
    for (_, wait) in CHANGE_WAITS.scan(ctx, CHANGE_WAITS.prefix_of(&key))? {
        let Some(row) = open(ctx, wait)? else {
            continue;
        };
        let words = format!("change {n} in {repo} {how}");
        let what = match row.event {
            WaitFor::Settled { until, .. } => settled_words(until, ending, words),
            _ => words,
        };
        ended.push((wait, what));
    }
    Ok(ended)
}

// a change ended as `ending`: the words for a wait until `until`, saying where it can no longer
// end as asked
fn settled_words(until: Until, ending: Until, words: String) -> String {
    match (until, ending) {
        (Until::Merged, Until::Closed) => format!("{words}, so it will not be merged"),
        (Until::Closed, Until::Merged) => format!("{words}, so it will not be closed"),
        _ => words,
    }
}

pub(crate) async fn forged(ctx: &Ctx, event: forge::Watched) -> Result<(), Error> {
    ctx.env().sent_by(forge::PROGRAM)?;
    let ended = match &event {
        forge::Watched::Moved {
            repo,
            reference,
            old,
            new,
            by,
        } => {
            let reference = String::from_utf8_lossy(reference).into_owned();
            let moved = moved_words((old, new), by.as_ref());
            let key = (repo.clone(), reference.clone());
            let mut ended = Vec::new();
            for (_, wait) in REF_WAITS.scan(ctx, REF_WAITS.prefix_of(&key))? {
                let Some(row) = open(ctx, wait)? else {
                    continue;
                };
                let what = match row.event {
                    WaitFor::Change { n, .. } => {
                        format!("change {n} in {repo}: its source {reference} {moved}")
                    }
                    _ => format!("{reference} in {repo} {moved}"),
                };
                ended.push((wait, what));
            }
            ended
        }
        forge::Watched::Opened { .. }
        | forge::Watched::Previewed { .. }
        | forge::Watched::Reviewed { .. }
        | forge::Watched::LeftQueue { .. } => Vec::new(),
        forge::Watched::Closed { repo, n, by } => {
            let how = format!("was closed by {}", who(Some(by)));
            change_ended(ctx, repo, *n, &how, Until::Closed)?
        }
        forge::Watched::Merged { repo, n, oid, by } => {
            let how = format!("was merged by {} as {oid}", who(Some(by)));
            change_ended(ctx, repo, *n, &how, Until::Merged)?
        }
    };
    let mut woken = Vec::new();
    for (wait, what) in ended {
        if let Some(thread) = end(ctx, wait, what)?
            && !woken.contains(&thread)
        {
            woken.push(thread);
        }
    }
    for thread in woken {
        converse(ctx, thread).await;
    }
    Ok(())
}

fn node_at(ctx: &QueryCtx, path: &str, at: Option<BlobId>) -> Result<Option<files::Node>, Error> {
    let asked = files::Query::Stat {
        path: path.to_owned(),
        at,
    };
    match ctx.query(files::PROGRAM, &asked)? {
        files::Reply::Stat { entry, .. } => Ok(entry.map(|entry| entry.node)),
        other => Err(unexpected_reply(files::PROGRAM, "Stat", &other)),
    }
}

fn files_waits(ctx: &ExecCtx, opened: bool) -> Result<(), Error> {
    let before = FILE_WAIT_COUNT.get(ctx)?.unwrap_or(0);
    let after = match opened {
        true => before + 1,
        false => before.saturating_sub(1),
    };
    FILE_WAIT_COUNT.put(ctx, &after);
    let deliver = match (before, after) {
        (0, 1) => filed_as(),
        (1, 0) => Vec::new(),
        _ => return Ok(()),
    };
    let watch = files::Op::Watch { deliver };
    ctx.emit(files::PROGRAM, &watch, ::program::Reply::None);
    Ok(())
}

pub(crate) async fn filed(ctx: &Ctx, row: files::SnapshotRow) -> Result<(), Error> {
    ctx.env().sent_by(files::PROGRAM)?;
    let mut woken = Vec::new();
    for (wait, watch) in FILE_WAITS.all(ctx)? {
        let now = node_at(ctx, &watch.path, Some(row.id))?;
        if now == watch.node {
            continue;
        }
        let what = filed_words(&watch, now.is_some(), &row);
        if let Some(thread) = end(ctx, wait, what)?
            && !woken.contains(&thread)
        {
            woken.push(thread);
        }
    }
    for thread in woken {
        converse(ctx, thread).await;
    }
    Ok(())
}

fn filed_words(watch: &FileWatch, there: bool, row: &files::SnapshotRow) -> String {
    let how = match (watch.node.is_some(), there) {
        (false, _) => "was made",
        (true, false) => "was removed",
        (true, true) => "changed",
    };
    let snapshot = abi::hex(row.id.digest());
    let by = who(Some(&row.author));
    let said = match row.message.trim() {
        "" => String::new(),
        message => format!(" (\"{message}\")"),
    };
    format!(
        "{} {how} in files snapshot {snapshot} by {by}{said}",
        watch.path
    )
}

fn named_pod(name: &str) -> Result<(), Error> {
    let fits = !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-.".contains(&byte)
        });
    match fits {
        true => Ok(()),
        false => Err(invalid(format!(
            "{name:?} is no pod's name: lowercase letters, digits, - and ."
        ))),
    }
}

async fn ask_pods(ctx: &Ctx, target: String, delivery: Delivery) -> Result<http::Response, Error> {
    let head = http::Head {
        method: http::Method::Get,
        target,
        headers: Vec::new(),
    };
    let asked = Request::Http {
        service: PODS.to_owned(),
        head,
        body: Vec::new(),
    };
    let answer = request(ctx, asked, delivery, None).await?;
    decoded(PROGRAM, "http::Response", &answer)
}

fn refused_by_pods(what: &str, answer: &http::Response) -> String {
    let error = serde_json::from_slice::<Value>(&answer.body)
        .ok()
        .and_then(|body| body["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(&answer.body).into_owned());
    format!("{what} answered {}: {error}", answer.status)
}

pub(crate) fn alive_every(ctx: &ExecCtx, opened: Tick) -> Result<Option<u64>, String> {
    let clock = clock(ctx, opened).map_err(|error| refusal(&error))?;
    let spanned = opened.height.saturating_sub(clock.older.height);
    if spanned == 0 {
        return Ok(None);
    }
    let steady = opened.time.saturating_sub(clock.older.time) / spanned;
    let env = ctx.env();
    let blocks = env.height.saturating_sub(opened.height).max(1);
    let block_ms = (env.time.saturating_sub(opened.time) / blocks).min(steady);
    let lasts = DEFAULT_WAIT.saturating_mul(block_ms);
    if lasts < 2000 {
        return Err(format!(
            "a request lasts {lasts} ms ({DEFAULT_WAIT} blocks of {block_ms} ms), under the 2 s two whole-second beats need"
        ));
    }
    Ok(Some(lasts / 2 / 1000))
}

fn clock(ctx: &ExecCtx, opened: Tick) -> Result<Clock, Error> {
    let env = ctx.env();
    let now = Tick {
        height: env.height,
        time: env.time,
    };
    let clock = match CLOCK.get(ctx)? {
        None => Clock {
            older: opened,
            newer: now,
        },
        Some(clock) if now.height.saturating_sub(clock.newer.height) >= DEFAULT_WAIT => Clock {
            older: clock.newer,
            newer: now,
        },
        Some(clock) => return Ok(clock),
    };
    CLOCK.put(ctx, &clock);
    Ok(clock)
}

async fn watch_from_next_block(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let height = ctx.env().height;
    let entry = schedule(ctx, height + 1, &Op::PodWait { wait }).await?;
    let watch = PodWatch {
        height,
        time: ctx.env().time,
        entry: Some(entry),
    };
    POD_WAITS.put(ctx, &wait, &watch);
    Ok(())
}

pub(crate) async fn pod_wait(ctx: &Ctx, wait: u64) -> Result<(), Error> {
    let Some(row) = open(ctx, wait)? else {
        return Err(wrong_state(format!("wait {wait} is not open")));
    };
    let WaitFor::Pod { name } = &row.event else {
        return Err(invalid(format!("wait {wait} waits for no pod")));
    };
    let Some(watch) = POD_WAITS.get(ctx, &wait)? else {
        let key = POD_WAITS.key(&wait);
        return Err(corrupt("pod wait", &key, "an open pod wait has no watch"));
    };
    let scheduled = match ctx.env().cause {
        Cause::Scheduled { entry } => Some(entry),
        _ => None,
    };
    match (by_harness(ctx), scheduled) {
        (true, Some(entry)) if watch.entry == Some(entry) => {}
        (true, Some(_)) => return Ok(()),
        _ => {
            if let Some(entry) = watch.entry {
                unschedule(ctx, entry).await?;
            }
            return watch_from_next_block(ctx, wait).await;
        }
    }
    let opened = Tick {
        height: watch.height,
        time: watch.time,
    };
    let paced = match alive_every(ctx, opened) {
        Ok(None) => return watch_from_next_block(ctx, wait).await,
        Ok(Some(every)) => Ok(every),
        Err(why) => Err(why),
    };
    let streaming = PodWatch {
        entry: None,
        ..watch
    };
    POD_WAITS.put(ctx, &wait, &streaming);
    let read = ask_pods(ctx, format!("/v1/pods/{name}"), Delivery::Whole).await;
    if let Some(over) = unwatchable(name, read) {
        return fire(ctx, wait, over).await;
    }
    let every = match paced {
        Ok(every) => every,
        Err(why) => {
            let over = format!("harness can't wait on pod {name}: {why}");
            return fire(ctx, wait, over).await;
        }
    };
    let target = format!("/v1/pods/{name}/wait?every={every}");
    let events = Delivery::Events {
        deliver: pod_event_as(wait),
    };
    let streamed = ask_pods(ctx, target, events).await;
    let opened_again = POD_WAITS
        .get(ctx, &wait)?
        .is_some_and(|watch| watch.entry.is_some());
    if open(ctx, wait)?.is_none() || opened_again {
        return Ok(());
    }
    match streamed {
        Ok(answer) if answer.status != 200 => {
            let over = unwatchable(name, Ok(answer)).unwrap_or_default();
            fire(ctx, wait, over).await
        }
        Ok(_) | Err(_) => watch_from_next_block(ctx, wait).await,
    }
}

fn unwatchable(name: &str, answer: Result<http::Response, Error>) -> Option<String> {
    match answer {
        Ok(answer) if answer.status == 200 => None,
        Ok(answer) if answer.status == 404 => Some(format!("the pods service knows no pod {name}")),
        Ok(answer) => Some(format!(
            "harness can't wait on pod {name}: {}",
            refused_by_pods("the pods service", &answer)
        )),
        Err(error) => Some(format!(
            "harness couldn't reach the pods service to wait on pod {name}: {}",
            refusal(&error)
        )),
    }
}

pub(crate) async fn pod_event(ctx: &Ctx, wait: u64, event: Event) -> Result<(), Error> {
    let egress = bound(ctx)?;
    let from_egress = ctx
        .env()
        .sending_program()
        .is_ok_and(|program| program == egress);
    if !from_egress {
        return Err(unauthorized(
            "only the program bound to the egress role hands harness a pod's wait events",
        ));
    }
    let Some(row) = open(ctx, wait)? else {
        return Ok(());
    };
    let WaitFor::Pod { name } = &row.event else {
        return Ok(());
    };
    let Some(line) = ended_line(&event.data, name) else {
        return Ok(());
    };
    fire(ctx, wait, line).await
}

fn ended_line(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let is_ended = text.lines().any(|line| line.trim_end() == "event: ended");
    if !is_ended {
        return None;
    }
    let data = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .and_then(|data| serde_json::from_str::<Value>(data.trim()).ok());
    let line = data.as_ref().and_then(|data| data["line"].as_str());
    Some(match line {
        Some(line) => line.to_owned(),
        None => format!("pod {name} ended"),
    })
}

pub(crate) fn still_armed(ctx: &QueryCtx, thread: u64) -> Result<String, Error> {
    let mut armed = Vec::new();
    for (_, id) in ARMED.scan(ctx, ARMED.prefix_of(&thread))? {
        if let Some(row) = open(ctx, id)? {
            armed.push(format!("wait {id} for {}", what(&row.event)));
        }
    }
    Ok(match armed.is_empty() {
        true => String::new(),
        false => format!("\n(This conversation still waits: {}.)", armed.join("; ")),
    })
}

pub(crate) fn waits(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<WaitRow>, Error> {
    WAITING.range_of(ctx, &agent, page)?.try_map(|(_, id)| {
        WAITS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("waiting", &id.to_be_bytes(), "names a wait that is gone"))
    })
}

pub(crate) fn armed(
    ctx: &QueryCtx,
    thread: u64,
    page: &PageRequest,
) -> Result<PageResponse<WaitRow>, Error> {
    ARMED.range_of(ctx, &thread, page)?.try_map(|(_, id)| {
        WAITS
            .get(ctx, &id)?
            .ok_or_else(|| corrupt("armed", &id.to_be_bytes(), "names a wait that is gone"))
    })
}

pub(crate) fn wait_row(ctx: &QueryCtx, wait: u64) -> Result<Option<WaitRow>, Error> {
    WAITS.get(ctx, &wait)
}
