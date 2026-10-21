//! The words both trees read a screen by: a state, a place, a conversation, an agent's setup,
//! a context and its usage. Each takes what the reads hold and gives a sentence or a few words;
//! the screens decide where they go.
use harness::{
    AccountBudget, Awaited, BudgetReport, StatusOutcome, StatusRead, WHOLE_WINDOW, WindowOutlook,
};
use harness::{
    AgentSetup, Asking, Body, CallRow, CallState, Configured, Flow, Frame, Partial, ServerTool,
    Spoken, Stop, Thinking, ThreadRow, Usage, Window,
};
use identity::{Ownership, Party};
use program::Writers;
use view::prelude::*;

use crate::read::{self, Card, Detail, Roster, Transcript, Wait};
use crate::timeline::{self, Work};
use crate::ui::budget::ended;
use crate::{Agents, State, agent_state, links, thread_state};

pub(crate) fn asking(asking: Asking) -> String {
    match asking {
        Asking::Idle => "Idle".into(),
        Asking::Learning => "Learning the model’s context window".into(),
        Asking::Compacting { through } => format!("Summarizing through frame {through}"),
        Asking::Asked { .. } => "Waiting for the model".into(),
        Asking::Running => "Running tool calls".into(),
    }
}

pub(crate) fn asking_lower(state: Asking) -> String {
    lower(&asking(state))
}

pub(crate) fn upper_first(words: &str) -> String {
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

pub(crate) fn lower(words: &str) -> String {
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

pub(crate) fn stop_words(stop: Stop) -> &'static str {
    match stop {
        Stop::EndTurn => "answered",
        Stop::ToolUse => "asked for tool calls",
        Stop::MaxTokens => "hit max tokens",
        Stop::StopSequence => "hit a stop sequence",
        Stop::Pause => "paused",
        Stop::Refusal => "refused",
    }
}

pub(crate) fn conversation_label(thread: &ThreadRow, roster: Option<&Roster>) -> String {
    match thread.place.is_empty() {
        true => format!("Conversation {}", thread.id),
        false => format!(
            "{} · {}",
            place_label(thread.agent, &thread.place, roster),
            thread.id
        ),
    }
}

fn conversation_place(thread: &ThreadRow, roster: &Roster) -> String {
    match thread.place.is_empty() {
        true => conversation_label(thread, Some(roster)),
        false => place_label(thread.agent, &thread.place, Some(roster)),
    }
}

pub(crate) fn place_label(agent: u64, place: &str, roster: Option<&Roster>) -> String {
    let Some(channel) = place.strip_prefix("chat/") else {
        return place.to_owned();
    };
    match chat::dm_peers(channel) {
        Some((a, b)) => {
            let other = if a == agent { b } else { a };
            let name = roster.map_or_else(|| format!("account {other}"), |r| r.name(other));
            format!("DM · {name}")
        }
        None => roster
            .and_then(|roster| roster.rooms.borrow().get(channel).cloned())
            .map_or_else(|| "a chat room".to_owned(), |name| format!("#{name}")),
    }
}

pub(crate) fn agent_sentence(card: &Card, roster: &Roster) -> String {
    let state = agent_state(card, roster);
    let busiest = busiest(card).map(|said| &said.thread);
    let model = card.setup.configured.reached();
    match (state, busiest, model) {
        (State::Unconfigured, _, _) => card
            .setup
            .configured
            .problem(&card.setup.path)
            .unwrap_or_default(),
        (State::NoNode, _, Some(model)) if card.serving == Some(0) => {
            format!("No member node serves {}", model.service)
        }
        (State::NoNode, _, _) => "No member node serves relay".to_owned(),
        (_, Some(thread), _) => format!(
            "in {} · {}",
            conversation_place(thread, roster),
            asking_lower(thread.asking)
        ),
        (_, None, _) => match card.newest.as_ref().map(|said| &said.thread) {
            Some(newest) => format!("last in {}", conversation_place(newest, roster)),
            None => "no conversation yet".to_owned(),
        },
    }
}

// the conversation an agent is busiest in: one stuck, else one working, the newest first
fn busiest(card: &Card) -> Option<&Spoken> {
    let open = card.asking.iter().filter(|said| !said.thread.closed);
    open.clone()
        .find(|said| thread_state(&said.thread) == State::Stuck)
        .or_else(|| {
            open.clone()
                .find(|said| thread_state(&said.thread) == State::Working)
        })
}

// the conversation its sentence names: the busiest, else the newest
fn spoken_of(card: &Card) -> Option<&Spoken> {
    busiest(card).or(card.newest.as_ref())
}

pub(crate) fn agent_sentence_topic(card: &Card, roster: &Roster) -> String {
    let sentence = agent_sentence(card, roster);
    let names = matches!(
        agent_state(card, roster),
        State::Idle | State::Working | State::Stuck
    );
    let topic = spoken_of(card).and_then(|said| said.topic.as_ref());
    match (topic, names) {
        (Some(topic), true) => format!("{sentence} · {}", quoted(topic)),
        _ => sentence,
    }
}

pub(crate) fn quoted(topic: &str) -> String {
    format!("“{topic}”")
}

pub(crate) fn last_moved(card: &Card) -> Option<u64> {
    card.last_time
}

pub(crate) fn ago(now: u64, then: u64) -> String {
    format!("{} ago", design::ago(now, then))
}

pub(crate) fn mentions(text: &str, roster: Option<&Roster>) -> String {
    let mut read = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("<@") {
        read.push_str(&rest[..at]);
        let tail = &rest[at + 2..];
        let number = tail
            .find('>')
            .and_then(|end| Some((tail[..end].parse::<u64>().ok()?, end)));
        match number {
            Some((number, end)) => {
                let name = roster.map_or_else(|| format!("account {number}"), |r| r.name(number));
                read.push('@');
                read.push_str(&name);
                rest = &tail[end + 1..];
            }
            None => {
                read.push_str("<@");
                rest = tail;
            }
        }
    }
    read.push_str(rest);
    read
}

pub(crate) fn context_words(context: harness::Context, window: Option<u64>) -> String {
    match (context, window) {
        (harness::Context::Measured(tokens), Some(window)) if window > 0 => format!(
            "{} / {} tokens · {}%",
            design::grouped(tokens),
            design::grouped(window),
            tokens.saturating_mul(100) / window
        ),
        (harness::Context::Measured(tokens), _) => format!("{} tokens", design::grouped(tokens)),
        (harness::Context::Overflowed, _) => "over the model’s window".to_owned(),
    }
}

pub(crate) fn short(tokens: u64) -> String {
    match tokens {
        0..1_000 => tokens.to_string(),
        1_000..1_000_000 => format!("{}K", tokens / 1_000),
        _ => format!("{}M", tokens / 1_000_000),
    }
}

pub(crate) fn full(context: harness::Context, window: Option<u64>) -> Option<u64> {
    let window = window.filter(|window| *window > 0)?;
    Some(match context {
        harness::Context::Measured(tokens) => tokens.saturating_mul(100) / window,
        harness::Context::Overflowed => 100,
    })
}

pub(crate) fn context_short(context: harness::Context, window: Option<u64>) -> String {
    match (context, window.filter(|window| *window > 0)) {
        (harness::Context::Measured(tokens), Some(window)) => format!(
            "{} of {} · {}%",
            short(tokens),
            short(window),
            tokens.saturating_mul(100) / window
        ),
        (harness::Context::Measured(tokens), None) => format!("{} tokens", short(tokens)),
        (harness::Context::Overflowed, _) => "over the model’s window".to_owned(),
    }
}

pub(crate) fn thinking_words(thinking: Option<Thinking>) -> &'static str {
    match thinking {
        None => "Model's default",
        Some(Thinking::Off) => "Off",
        Some(Thinking::Low) => "Low",
        Some(Thinking::Medium) => "Medium",
        Some(Thinking::High) => "High",
        Some(Thinking::Max) => "Max",
    }
}

pub(crate) fn thinking_short(thinking: Option<Thinking>) -> &'static str {
    match thinking {
        None => "Auto",
        Some(Thinking::Medium) => "Med",
        thinking => thinking_words(thinking),
    }
}

pub(crate) fn window_words(window: Window) -> String {
    match window {
        Window::Unasked => "not asked yet".to_owned(),
        Window::Unreported => "not reported".to_owned(),
        Window::Tokens(tokens) => format!("{} tokens", design::grouped(tokens)),
    }
}

pub(crate) fn snapshot_words(snapshot: &harness::BlobId) -> String {
    let short: String = snapshot
        .digest()
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("snapshot {short}")
}

pub(crate) fn all_tokens(usage: &Usage) -> u64 {
    usage.input + usage.output + usage.cache_read + usage.cache_write
}

pub(crate) fn usage_words(usage: &Usage) -> String {
    let all = all_tokens(usage);
    format!(
        "input {} · output {} · cache read {} · cache write {} · all {}",
        design::grouped(usage.input),
        design::grouped(usage.output),
        design::grouped(usage.cache_read),
        design::grouped(usage.cache_write),
        design::grouped(all),
    )
}

pub(crate) struct Reach {
    pub chat: Option<String>,
    pub dm: Option<String>,
}

pub(crate) fn reach(view: &Agents, detail: &Detail, agent: u64) -> Reach {
    let current = detail.setup.as_ref().and_then(|setup| setup.current);
    let place = detail
        .threads
        .iter()
        .find(|(thread, _)| Some(thread.id) == current)
        .map(|(thread, _)| thread.place.as_str())
        .filter(|place| !place.is_empty());
    let link = |place: &str| links::place_link(&view.chain, place).map(|(_, link)| link);
    let dm = view
        .me
        .filter(|me| *me != agent)
        .map(|me| format!("{}/{}", chat::PROGRAM, chat::dm_channel_id(me, agent)));
    Reach {
        chat: place.and_then(link),
        dm: dm.as_deref().and_then(link),
    }
}

pub(crate) fn about(setup: &AgentSetup, detail: &Detail, roster: &Roster) -> String {
    let mut about = format!(
        "account {} · managed by {}",
        setup.agent,
        roster.name(setup.manager)
    );
    if let Some((_, config)) = setup.configured.config() {
        about.push_str(&format!(" · {}", config.model));
    }
    about.push_str(&format!(
        " · {}",
        design::plural(detail.tools.len() as u64, "tool", "tools")
    ));
    if let Some(note) = roster.card(setup.agent).and_then(|card| card.note) {
        about.push_str(&format!(" · {note}"));
    }
    about
}

pub(crate) const PROBLEM_CAPTION: &str = "Harness says so in a thread once for each message that reaches the agent, and asks no model until the file is fixed.";

pub(crate) fn read_at(configured: &Configured) -> String {
    match configured {
        Configured::Unreadable { .. } => "read at each model call".to_owned(),
        Configured::Malformed { snapshot, .. }
        | Configured::UnknownModel { snapshot, .. }
        | Configured::Ready { snapshot, .. }
        | Configured::Routed { snapshot, .. }
        | Configured::NoRoute { snapshot, .. }
        | Configured::Unprovided { snapshot, .. } => {
            format!("read at each model call · {}", snapshot_words(snapshot))
        }
    }
}

pub(crate) fn model_more(setup: &AgentSetup, roster: &Roster) -> Option<String> {
    let reached = setup.configured.reached()?;
    let serving = roster
        .card(setup.agent)
        .and_then(|card| card.serving)
        .unwrap_or_default();
    let services: Vec<String> = reached
        .route
        .iter()
        .map(|entry| format!("{} ({})", entry.service, entry.wire.label()))
        .collect();
    let nodes = design::plural(serving as u64, "node", "nodes");
    Some(format!(
        " · {} · {}, {nodes} serving",
        reached.provider,
        services.join(", ")
    ))
}

pub(crate) fn wait_words(wait: Option<u64>) -> String {
    wait.map_or_else(
        || "egress's default".to_owned(),
        |wait| design::plural(wait, "block", "blocks"),
    )
}

pub(crate) fn configured_words(configured: &Configured) -> String {
    match configured {
        Configured::Ready { .. } | Configured::Routed { .. } => "Ready".to_owned(),
        Configured::Unprovided { .. } => {
            "No provider: it names none, so harness asks no model; pick one in its front matter"
                .to_owned()
        }
        Configured::NoRoute { provider, .. } => {
            format!("No route: it names provider {provider}, which harness keeps no route for")
        }
        Configured::UnknownModel { config, .. } => format!(
            "Unknown model: it names {}, which harness keeps no record of",
            config.model
        ),
        Configured::Malformed { malformed, .. } => format!("Malformed: {malformed}"),
        Configured::Unreadable { reason } => format!("Unreadable: {reason}"),
    }
}

pub(crate) fn server_tools_words(tools: &[ServerTool]) -> String {
    match tools {
        [] => "none".to_owned(),
        tools => {
            let words: Vec<&str> = tools.iter().map(|tool| tool.word()).collect();
            words.join(", ")
        }
    }
}

pub(crate) fn listen_words(listen: &[String], roster: &Roster) -> String {
    let rooms: Vec<String> = listen
        .iter()
        .map(|id| {
            roster
                .rooms
                .borrow()
                .get(id)
                .map_or_else(|| id.clone(), |name| format!("#{name}"))
        })
        .collect();
    match rooms.is_empty() {
        true => "none: what reaches it".to_owned(),
        false => rooms.join(", "),
    }
}

pub(crate) fn settle_words(settle: Option<u64>) -> String {
    settle.map_or_else(
        || "none".to_owned(),
        |millis| format!("{} ms", design::grouped(millis)),
    )
}

pub(crate) fn tools_words(detail: &Detail, name: &str) -> String {
    let writes = detail.tools.iter().filter(|tool| tool.writes).count();
    format!("{} · {writes} write as {name}", detail.tools.len())
}

pub(crate) fn file_link(chain: &str, path: &str) -> Option<String> {
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    ducklink::mint(chain, files::PROGRAM, &segments)
}

pub(crate) fn prompt_words(configured: &Configured) -> String {
    match prompt(configured) {
        Ok("") => "No system prompt: harness sends its ducktape resource manual alone.".to_owned(),
        Ok(prompt) => format!(
            "{} · “{}”",
            design::plural(prompt.lines().count() as u64, "line", "lines"),
            first_words(prompt)
        ),
        Err(unread) => unread.to_owned(),
    }
}

pub(crate) fn prompt(configured: &Configured) -> Result<&str, &'static str> {
    match configured {
        Configured::Ready { config, .. }
        | Configured::UnknownModel { config, .. }
        | Configured::Routed { config, .. }
        | Configured::NoRoute { config, .. }
        | Configured::Unprovided { config, .. } => Ok(&config.system),
        Configured::Unreadable { .. } | Configured::Malformed { .. } => {
            Err("No system prompt is read until the file is fixed.")
        }
    }
}

fn first_words(prompt: &str) -> String {
    let line = prompt.lines().next().unwrap_or_default();
    let sentence = line.find(['.', '!', '?']).map_or(line, |end| &line[..=end]);
    sentence.to_owned()
}

pub(crate) fn usage_total(detail: &Detail) -> String {
    let total = read::sum(detail.threads.iter().map(|(_, usage)| usage));
    format!(
        "{} tokens over {}",
        design::grouped(all_tokens(&total)),
        design::plural(detail.threads.len() as u64, "conversation", "conversations")
    )
}

pub(crate) fn answers(ownership: &Ownership, roster: &Roster) -> String {
    match ownership {
        Ownership::Owner => "Its manager only".into(),
        Ownership::Anyone => "Anyone".into(),
        Ownership::Listed(parties) => format!("Its manager, {}", named_parties(parties, roster)),
    }
}

pub(crate) fn named_parties(parties: &[Party], roster: &Roster) -> String {
    let names: Vec<String> = parties
        .iter()
        .map(|party| party_name(party, roster))
        .collect();
    names.join(", ")
}

fn party_name(party: &Party, roster: &Roster) -> String {
    match party {
        Party::Account(number) => roster.name(*number),
        Party::Program(program) => format!("the {program} program"),
    }
}

pub(crate) fn may_change(writers: &Writers, config: &str, roster: &Roster) -> String {
    let named = |owner: &Option<Party>| match owner {
        Some(owner) => party_name(owner, roster),
        None => "no account".into(),
    };
    match writers {
        Writers::Listed { at, owner, list } => {
            let who = match list {
                Ownership::Owner => format!("only {}", named(owner)),
                Ownership::Listed(parties) => {
                    format!("{}, {}", named(owner), named_parties(parties, roster))
                }
                Ownership::Anyone => "anyone".into(),
            };
            match at == config {
                true => format!("{who} may change it"),
                false => format!("{who} may change it, by the list on {at}"),
            }
        }
        Writers::Anyone => "anyone may change it".into(),
    }
}

pub(crate) fn removes(name: &str) -> String {
    format!(
        "Take {name} back from harness? Its script is cleared, so harness stops answering for it. Its conversations and its configuration file stay."
    )
}

pub(crate) fn thread_words(thread: &harness::ThreadRow) -> Option<String> {
    match (thread.closed, thread_state(thread)) {
        (true, _) => Some(format!(
            "closed at block {}",
            design::grouped(thread.last_height)
        )),
        (false, State::Stuck) => Some(format!(
            "{} · {} waiting",
            asking_lower(thread.asking),
            thread.held
        )),
        (false, State::Working) => Some(asking_lower(thread.asking)),
        (false, _) => None,
    }
}

pub(crate) fn starts(view: &Agents, detail: &Detail) -> bool {
    let before = match (&detail.conversation_writers, view.me) {
        (Some(writers), Some(me)) => writers.allows(&[Party::Account(me)]),
        _ => false,
    };
    match &detail.setup {
        Some(setup) => view.may_start(setup.agent, before),
        None => before,
    }
}

pub(crate) struct Status {
    pub state: Option<State>,
    pub words: String,
    pub caption: String,
}

pub(crate) fn conversation_state(shown: &Transcript, relaying: usize) -> State {
    match (shown.serving, relaying) {
        (None, _) => State::Unconfigured,
        (Some(0), _) | (_, 0) => State::NoNode,
        _ => thread_state(&shown.thread),
    }
}

pub(crate) fn status(shown: &Transcript, relaying: usize) -> Status {
    let row = &shown.thread;
    let frames = design::plural(row.frames, "frame", "frames");
    let turns = shown
        .frames
        .iter()
        .rev()
        .take_while(|frame| !matches!(frame.body, Body::Post { .. }))
        .filter(|frame| matches!(frame.body, Body::Turn { .. }))
        .count() as u64;
    if row.closed {
        return Status {
            state: None,
            words: "Closed".to_owned(),
            caption: format!(
                "{frames} · closed at block {}",
                design::grouped(row.last_height)
            ),
        };
    }
    let state = conversation_state(shown, relaying);
    let (words, caption) = match thread_state(row) {
        State::Stuck => (
            format!(
                "Still {} · {} waiting",
                asking_lower(row.asking),
                design::plural(row.held, "message", "messages")
            ),
            format!(
                "since {} · a harness upgrade or a faulting frame leaves a thread like this",
                design::clock(row.last_time)
            ),
        ),
        State::Working => (
            asking(row.asking),
            match turns {
                0 => frames,
                turns => format!(
                    "{frames} · {} since the last message from a person",
                    design::plural(turns, "turn", "turns")
                ),
            },
        ),
        State::Idle | State::NoNode | State::Unconfigured => (
            idle_words(shown),
            format!(
                "{frames} · last at {}, {}",
                design::clock(row.last_time),
                design::day(row.last_time)
            ),
        ),
    };
    Status {
        state: Some(state),
        words,
        caption,
    }
}

fn idle_words(shown: &Transcript) -> String {
    if let Some(window) = &shown.settling {
        return format!(
            "Collecting messages since block {}",
            design::grouped(window.opened)
        );
    }
    shown.thread.last_stop.map_or_else(
        || "Nothing asked yet".to_owned(),
        |stop| upper_first(stop_words(stop)),
    )
}

pub(crate) fn opened_words(shown: &Transcript, roster: Option<&Roster>) -> String {
    let agent = roster.map_or_else(
        || format!("account {}", shown.thread.agent),
        |roster| roster.name(shown.thread.agent),
    );
    let day = shown
        .first
        .map(|time| format!(" {}", design::day(time)))
        .unwrap_or_default();
    let asks = match shown.reach() {
        Some(reached) => reached.name.to_owned(),
        None => "asks no model until its configuration is fixed".to_owned(),
    };
    format!(
        "{agent} · opened{day} at block {} · {asks}",
        design::grouped(shown.thread.opened)
    )
}

pub(crate) fn author(author: &harness::Principal, roster: Option<&Roster>) -> String {
    match author {
        harness::Principal::Account(number) => {
            roster.map_or_else(|| format!("account {number}"), |r| r.name(*number))
        }
        harness::Principal::Root => "the chain".to_owned(),
    }
}

pub(crate) fn held_author(body: &Body, roster: Option<&Roster>) -> String {
    match body {
        Body::Post { author: who, .. } => author(who, roster),
        Body::Notice { .. } => "Notice".to_owned(),
        _ => "harness".to_owned(),
    }
}

pub(crate) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().to_owned()
}

pub(crate) fn frame_meta(frame: &Frame) -> String {
    let mut meta = format!("#{} · block {}", frame.seq, design::grouped(frame.height));
    if let Body::Turn { config, .. } = &frame.body {
        meta.push_str(&format!(" · {}", snapshot_words(config)));
    }
    meta
}

pub(crate) fn work_meta(work: &Work) -> String {
    let (first, last) = (work.first(), work.last());
    let mut meta = format!(
        "#{}–#{} · blocks {}–{}",
        first.seq,
        last.seq,
        design::grouped(first.height),
        design::grouped(last.height)
    );
    if let Body::Turn { config, .. } = &first.body {
        meta.push_str(&format!(" · {}", snapshot_words(config)));
    }
    meta
}

pub(crate) fn work_size(work: &Work) -> String {
    format!(
        "{} · {}",
        design::plural(work.calls().count() as u64, "call", "calls"),
        timeline::took_words(work.took())
    )
}

pub(crate) fn flow_words(partial: &Partial) -> (String, Option<String>) {
    let events = design::plural(partial.next_seq, "event", "events");
    match &partial.flow {
        Flow::Open => ("is answering".to_owned(), Some(format!("{events} so far"))),
        Flow::Ended => ("finished answering".to_owned(), Some(events)),
        Flow::Overflowed => ("ran out of context as it answered".to_owned(), None),
        Flow::Broken { reason } => (format!("the reply broke: {reason}"), None),
        Flow::Failed { text } => (text.clone(), None),
    }
}

pub(crate) fn flow_failed(partial: &Partial) -> bool {
    matches!(
        partial.flow,
        Flow::Broken { .. } | Flow::Failed { .. } | Flow::Overflowed
    )
}

pub(crate) fn call_state(state: Option<&CallState>, theme: &Theme) -> (&'static str, Hsla, Hsla) {
    match state {
        None => ("Not placed", theme.muted, theme.surface_raised),
        Some(CallState::Showing) => ("Showing", theme.warning, theme.warning_soft),
        Some(CallState::Relaying) => ("Relaying", theme.muted, theme.surface_raised),
        Some(CallState::Writing) => ("Writing", theme.warning, theme.warning_soft),
        Some(CallState::Done { failed: false, .. }) => ("Done", theme.success, theme.success_soft),
        Some(CallState::Done { failed: true, .. }) => ("Failed", theme.danger, theme.danger_soft),
        Some(CallState::Voided { .. }) => ("Taken back", theme.muted, theme.surface_raised),
    }
}

pub(crate) fn ran(call: &CallRow) -> String {
    let made = design::grouped(call.made);
    match call.state {
        CallState::Done { height, .. } => format!(
            "made at block {made} · done at block {}",
            design::grouped(height)
        ),
        CallState::Voided { height } => format!(
            "made at block {made} · taken back at block {}",
            design::grouped(height)
        ),
        CallState::Showing | CallState::Relaying | CallState::Writing => {
            format!("made at block {made}")
        }
    }
}

pub(crate) fn size_words(text: &str) -> String {
    format!(
        "{} · {}",
        design::plural(text.len() as u64, "byte", "bytes"),
        design::plural(text.lines().count() as u64, "line", "lines")
    )
}

pub(crate) fn summary_words(frame: &Frame) -> String {
    format!(
        "the model sees frames 1–{} only as this summary",
        frame.seq.saturating_sub(1)
    )
}

pub(crate) fn open_label(program: &str) -> String {
    match program == chat::PROGRAM {
        true => "Open in chat".to_owned(),
        false => format!("Open in {program}"),
    }
}

pub(crate) fn serving_words(serving: usize, theme: &Theme) -> (String, Hsla) {
    match serving {
        0 => ("no node serves it".to_owned(), theme.danger),
        serving => (
            format!(
                "{} serving",
                design::plural(serving as u64, "node", "nodes")
            ),
            theme.success,
        ),
    }
}

pub(crate) fn closes(shown: &Transcript) -> String {
    let current = shown
        .setup
        .as_ref()
        .is_some_and(|setup| setup.current == Some(shown.thread.id));
    let mut cost = match current {
        true => "Close this conversation? It is the agent's current one, so the next message to the agent opens a new conversation with fresh context.",
        false => "Close this conversation? It is not the agent's current one, so nothing more reaches it.",
    }
    .to_owned();
    if shown.thread.held > 0 {
        cost.push_str(&format!(
            " The {} waiting here do not carry over.",
            design::plural(shown.thread.held, "message", "messages")
        ));
    }
    cost
}

pub(crate) fn network(chain: &str) -> &str {
    chain.split_once('#').map_or(chain, |(label, _)| label)
}

pub(crate) fn no_node_words(serving: usize, relaying: usize) -> Option<&'static str> {
    match (serving, relaying) {
        (0, _) => Some(
            "The model is not asked; the agent stays silent in chat. An operator assigns a node to the service and turns relay on.",
        ),
        (_, 0) => Some(
            "No member node serves relay, so no tool call can run, chat_post included. An operator turns relay on in a member node’s egress.toml.",
        ),
        _ => None,
    }
}

fn request_words(wait: &Wait) -> String {
    let node: String = wait.node.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "egress request {} since block {} at node {node}",
        wait.id,
        design::grouped(wait.made)
    )
}

pub(crate) fn waiting_words(shown: &Transcript) -> Option<(String, bool)> {
    let row = &shown.thread;
    let waiting = matches!(
        row.asking,
        Asking::Asked { .. } | Asking::Learning | Asking::Compacting { .. }
    );
    if row.closed || !waiting {
        return None;
    }
    let name = shown.reach().map(|reached| reached.name);
    let open: Vec<&Wait> = shown
        .waits
        .iter()
        .filter(|wait| match (&wait.on, row.asking) {
            (Awaited::Reply { thread }, Asking::Asked { .. }) => *thread == row.id,
            (Awaited::Window { name: asked }, Asking::Learning) => Some(asked.as_str()) == name,
            (Awaited::Summary, Asking::Compacting { .. }) => true,
            _ => false,
        })
        .collect();
    Some(match &open[..] {
        [] => (
            format!(
                "No egress request is open for it · it last moved at block {}",
                design::grouped(row.last_height)
            ),
            true,
        ),
        [wait] => (format!("Waiting on {}", request_words(wait)), false),
        some => (
            format!("Waiting on one of {} egress requests like it", some.len()),
            false,
        ),
    })
}

pub(crate) fn armed_words(shown: &Transcript) -> Option<String> {
    if shown.thread.closed || shown.armed.is_empty() {
        return None;
    }
    let each: Vec<String> = shown
        .armed
        .iter()
        .map(|row| {
            let what = match &row.event {
                harness::WaitFor::Height { height } => {
                    format!("block {}", design::grouped(*height))
                }
                harness::WaitFor::Pod { name } => format!("pod {name} to end"),
                harness::WaitFor::Message { channel, .. } => {
                    format!("a message in chat channel {channel}")
                }
                harness::WaitFor::Change { repo, n } => format!("change {n} in {repo}"),
                harness::WaitFor::Ref { repo, reference } => format!("{reference} in {repo}"),
                harness::WaitFor::File { path } => format!("{path} in files"),
                harness::WaitFor::Settled { repo, n, .. } => format!("change {n} in {repo}"),
            };
            format!("{what} (since block {})", design::grouped(row.made))
        })
        .collect();
    Some(format!("Waits for {}", each.join(" · ")))
}

pub(crate) fn runs_words(shown: &Transcript) -> Option<String> {
    let none = shown.thread.closed || shown.runs.is_empty();
    if none {
        return None;
    }
    let each: Vec<String> = shown
        .runs
        .iter()
        .map(|run| {
            format!(
                "account {} in chat channel {} since message {} (last {})",
                run.author, run.channel, run.mentioned, run.last
            )
        })
        .collect();
    Some(format!("Hears the rest of a mention: {}", each.join(" · ")))
}

pub(crate) fn call_wait(call: &CallRow, waits: &[Wait]) -> Option<String> {
    if call.state != CallState::Relaying {
        return None;
    }
    let relay = waits.iter().find(|wait| match &wait.on {
        Awaited::Relay { call: id } => *id == call.id,
        _ => false,
    });
    Some(match relay {
        Some(wait) => format!("relaying on {}", request_words(wait)),
        None => "no egress relay is open for it".to_owned(),
    })
}

pub(crate) fn compacts_words(threshold: Option<u64>) -> String {
    match threshold {
        Some(tokens) => format!("{} tokens", design::grouped(tokens)),
        None => "when the window fills".to_owned(),
    }
}

pub(crate) fn threshold_source(threshold: Option<u64>) -> Option<String> {
    threshold.map(|_| "the agent's configuration".to_owned())
}

pub(crate) fn compacts_line(threshold: Option<u64>) -> String {
    match threshold_source(threshold) {
        Some(source) => format!("{} · {source}", compacts_words(threshold)),
        None => compacts_words(threshold),
    }
}

pub(crate) fn budget_window_words(name: &str) -> String {
    match name {
        "5h" => "5-hour window".into(),
        "7d" => "weekly window".into(),
        other => format!("{other} window"),
    }
}

pub(crate) fn budget_account(row: &AccountBudget) -> String {
    let named = match (&row.label, &row.organization) {
        (Some(label), _) => format!("Account {label}"),
        (None, Some(id)) => format!("Account {id}"),
        (None, None) => "An account the model did not name".to_owned(),
    };
    format!("{named} · {}", row.service)
}

fn spaced(word: &str) -> String {
    word.replace('_', " ")
}

pub(crate) fn budget_state(row: &AccountBudget) -> Option<String> {
    let status = row
        .status
        .as_deref()
        .map(|s| format!("Status {}", spaced(s)));
    let fallback = row
        .fallback
        .as_deref()
        .map(|f| format!("fallback {}", spaced(f)));
    match (status, fallback) {
        (Some(status), Some(fallback)) => Some(format!("{status} · {fallback}")),
        (status, fallback) => status.or(fallback),
    }
}

pub(crate) fn budget_error(row: &AccountBudget) -> Option<String> {
    let error = row.error.as_deref()?;
    Some(format!(
        "ccs could not read this account: {}",
        spaced(error)
    ))
}

pub(crate) fn budget_read(read: &StatusRead, now: u64) -> String {
    let service = &read.service;
    match &read.outcome {
        StatusOutcome::Read { accounts } => format!(
            "{service} status read at {} · {}",
            clock_from(now, read.time),
            design::plural(*accounts, "account", "accounts")
        ),
        StatusOutcome::Unavailable => format!("{service} status unavailable"),
        StatusOutcome::Failed(why) => format!("Couldn’t read {service}’s status: {why}"),
    }
}

fn clock_from(now: u64, then: u64) -> String {
    match design::day(now) == design::day(then) {
        true => design::clock(then),
        false => format!("{}, {}", design::day(then), design::clock(then)),
    }
}

pub(crate) fn budget_calls(report: &BudgetReport) -> String {
    let calls = report.services.iter().map(|pace| pace.last_minute).sum();
    format!(
        "Model budget · {} in the last minute · {} in flight",
        design::plural(calls, "call", "calls"),
        design::plural(report.turns, "turn", "turns")
    )
}

pub(crate) fn budget_used(window: &WindowOutlook, now: u64) -> String {
    let name = budget_window_words(&window.name);
    let reset = clock_from(now, window.reset.saturating_mul(1000));
    match ended(window, now) {
        true => format!("{name} · reset at {reset}, no answer since"),
        false => format!("{}% of the {name} · resets {reset}", window.used / 10_000),
    }
}

pub(crate) fn budget_outlook(window: &WindowOutlook, now: u64) -> Option<String> {
    if ended(window, now) {
        return None;
    }
    if window.used >= WHOLE_WINDOW {
        return Some("Spent until it resets".into());
    }
    let reset = window.reset.saturating_mul(1000);
    Some(match window.runs_out {
        Some(out) if out < reset => format!(
            "At this pace it runs out at {}, before it resets",
            clock_from(now, out)
        ),
        Some(_) => "At this pace it lasts until it resets".into(),
        None => "No climb seen yet".into(),
    })
}

pub(crate) fn failing_words(failing: &harness::FinalError) -> String {
    match failing.count {
        1 => format!(
            "Its last input ended in a final error at block {}: {}",
            failing.height, failing.text
        ),
        count => format!(
            "Its last {count} inputs ended in final errors since block {}, the newest at block {}: {}",
            failing.since, failing.height, failing.text
        ),
    }
}
