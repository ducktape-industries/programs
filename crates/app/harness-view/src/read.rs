//! Every read the screens make: the roster (harness's cards, the accounts, the relay nodes, the
//! rooms its conversations sit in), one agent in full (who writes, topics, tallies, bound
//! conversations), one transcript as far as its list shows it, the models and services.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::rc::Rc;

use egress::{RELAY, ServiceRow};
use harness::{
    AgentSetup, Asking, Awaited, BlobId, Body, CallRow, FinalError, Frame, Partial,
    ProviderRouteRow, RunRow, SettleWindow, Spoken, Tally, ThreadRow, ToolInfo, Usage, WaitRow,
    WithheldFrame,
};
use identity::{Category, Kind, Ownership, PageRequest};
use policy_view::link;
use program::Writers;
use view::prelude::*;

fn page(after: Option<Vec<u8>>) -> PageRequest {
    PageRequest { after, limit: None }
}

#[derive(PartialEq)]
pub(crate) struct Roster {
    pub agents: Vec<Card>,
    /// the reader's agent accounts that no script hands to harness yet
    pub unscripted: Vec<Unscripted>,
    pub names: HashMap<u64, String>,
    // each account's picture by its number, as its card holds it
    pub avatars: HashMap<u64, BlobId>,
    // the rooms the session has named, by channel id, which a thread's place holds
    pub rooms: Rooms,
    /// member nodes that serve relay: no tool call runs without one
    pub relaying: usize,
    // the newest head's time, in milliseconds: now, as the chain tells it
    pub now: u64,
}

// One account whose script calls harness, as the list draws it: harness's card of it
// (`AgentCard`), with its name and its note from identity.
#[derive(PartialEq)]
pub(crate) struct Card {
    pub name: String,
    pub setup: AgentSetup,
    pub open: u64,
    pub closed: u64,
    // when one of its conversations last moved, once it has one
    pub last_time: Option<u64>,
    pub newest: Option<Spoken>,
    // each open conversation asking, newest first
    pub asking: Vec<Spoken>,
    // member nodes that serve its model's service, once its configuration names a model
    pub serving: Option<usize>,
    // "suspended" or "revoked", for an agent that is
    pub note: Option<&'static str>,
}

#[derive(PartialEq)]
pub(crate) struct Unscripted {
    pub number: u64,
    pub name: String,
}

impl Roster {
    pub fn name(&self, number: u64) -> String {
        self.names
            .get(&number)
            .cloned()
            .unwrap_or_else(|| format!("account {number}"))
    }

    pub fn card(&self, agent: u64) -> Option<&Card> {
        self.agents.iter().find(|card| card.setup.agent == agent)
    }
}

fn unscripted_of(kind: &Kind, me: Option<u64>) -> bool {
    match kind {
        Kind::Program {
            manager,
            category: Category::Agent,
            script: None,
            ..
        } => Some(*manager) == me,
        Kind::User | Kind::Program { .. } | Kind::System(_) => false,
    }
}

pub(crate) async fn setup(host: &Host, agent: u64) -> Result<Option<AgentSetup>, Error> {
    host.query(harness::ask::Agent { agent }).await
}

// what a list or a conversation shows of an agent: its setup without its system prompt, which
// only the agent's own screen reads (`setup`)
async fn card(host: &Host, agent: u64) -> Result<Option<AgentSetup>, Error> {
    host.query(harness::ask::Card { agent }).await
}

// Each chat room's name the session has read, by channel id: a read names the rooms it meets that
// the session has not named (`name_rooms`), and a rename waits for the next session.
pub(crate) type Rooms = Rc<RefCell<HashMap<String, String>>>;

// the channel of a place in chat
fn room_of(place: &str) -> Option<&str> {
    place.strip_prefix("chat/")
}

// each room of `channels` the session has not named yet, named in one read; a DM reads as the
// person it is with, so it needs no name
async fn name_rooms(host: &Host, rooms: &Rooms, channels: Vec<&str>) -> Result<(), Error> {
    let ids: BTreeSet<String> = {
        let named = rooms.borrow();
        let unnamed = channels
            .into_iter()
            .filter(|id| chat::dm_peers(id).is_none() && !named.contains_key(*id));
        unnamed.map(str::to_owned).collect()
    };
    if ids.is_empty() {
        return Ok(());
    }
    let asked = chat::ask::Names {
        ids: ids.into_iter().collect(),
    };
    let names = host.query(asked).await?;
    rooms.borrow_mut().extend(names);
    Ok(())
}

// every room's name, for the setup's room finder, which searches them all
pub(crate) async fn every_room(host: Host, rooms: Rooms) -> Result<(), Error> {
    let every = host
        .query_all(|after| chat::ask::Channels { page: page(after) })
        .await?;
    let named = every
        .into_iter()
        .map(|info| (info.channel.id, info.channel.name));
    rooms.borrow_mut().extend(named);
    Ok(())
}

// the list: harness's card of each agent it runs, the accounts (names, pictures, the reader's
// agents no script hands to harness yet), the nodes serving relay and the rooms the cards name;
// its time is the newest head's
pub(crate) async fn roster(
    host: Host,
    me: Option<u64>,
    rooms: Rooms,
    now: Rc<Cell<u64>>,
) -> Result<Roster, Error> {
    let accounts = host
        .query_all(|after| identity::ask::List { page: page(after) })
        .await?;
    let cards = host
        .query_all(|after| harness::ask::Cards { page: page(after) })
        .await?;
    let relaying = served(&host, RELAY).await?;
    let spoken = cards
        .iter()
        .flat_map(|card| card.newest.iter().chain(&card.asking));
    let channels = spoken.filter_map(|said| room_of(&said.thread.place));
    name_rooms(&host, &rooms, channels.collect()).await?;
    let agents = cards
        .into_iter()
        .map(|card| {
            let number = card.setup.agent;
            let account = accounts.iter().find(|account| account.number == number);
            Card {
                name: account.map_or_else(
                    || format!("account {number}"),
                    |account| account.card.name.clone(),
                ),
                note: account.and_then(|account| view::design::note(&account.kind())),
                open: card.open,
                closed: card.closed,
                last_time: (card.open + card.closed > 0).then_some(card.last_time),
                newest: card.newest,
                asking: card.asking,
                serving: card.serving.map(|serving| serving as usize),
                setup: card.setup,
            }
        })
        .collect();
    let unscripted = accounts
        .iter()
        .filter(|account| unscripted_of(&account.kind(), me))
        .map(|account| Unscripted {
            number: account.number,
            name: account.card.name.clone(),
        })
        .collect();
    Ok(Roster {
        agents,
        unscripted,
        avatars: accounts
            .iter()
            .filter_map(|account| Some((account.number, account.card.avatar?)))
            .collect(),
        names: accounts
            .into_iter()
            .map(|account| (account.number, account.card.name))
            .collect(),
        rooms,
        relaying,
        now: now.get(),
    })
}

// the model budget, as harness keeps it
pub(crate) async fn budget(host: &Host) -> Result<harness::BudgetFactsReport, Error> {
    host.query(harness::ask::BudgetFacts).await
}

/// How many member nodes the chain would elect for `service` now.
async fn served(host: &Host, service: &str) -> Result<usize, Error> {
    let asked = egress::ask::Serving {
        service: service.to_owned(),
    };
    Ok(host.query(asked).await?.len())
}

/// The line `thread`'s agent last set with `thread_topic`, if it set one.
async fn topic(host: &Host, thread: u64) -> Result<Option<String>, Error> {
    host.query(harness::ask::Topic { thread }).await
}

// what `thread`'s turns used and how often it was summarized, as harness keeps it
async fn tally(host: &Host, thread: u64) -> Result<Tally, Error> {
    host.query(harness::ask::Tally { thread }).await
}

pub(crate) fn sum<'a>(usages: impl IntoIterator<Item = &'a Usage>) -> Usage {
    let mut total = Usage::default();
    for usage in usages {
        total.input += usage.input;
        total.output += usage.output;
        total.cache_read += usage.cache_read;
        total.cache_write += usage.cache_write;
    }
    total
}

async fn owned_writers(
    host: &Host,
    program: &str,
    key: Vec<u8>,
    at: String,
) -> Result<Writers, Error> {
    let asked = ownership::ask::Effective {
        program: program.to_owned(),
        key,
    };
    Ok(match host.query(asked).await? {
        Some(policy) => Writers::Listed {
            at,
            owner: Some(link::listed(&policy.owner)),
            list: link::list(&policy.write),
        },
        None => Writers::Anyone,
    })
}

async fn file_writers(host: &Host, path: &str) -> Result<Writers, Error> {
    let guarded = host
        .query(files::ask::Guarded {
            path: path.to_owned(),
        })
        .await?;
    match guarded {
        Some(at) => owned_writers(host, files::PROGRAM, files::guard_key(&at), at).await,
        None => Ok(Writers::Anyone),
    }
}

#[derive(PartialEq)]
pub(crate) struct Detail {
    /// `None` once no script hands the account to harness
    pub setup: Option<AgentSetup>,
    pub ownership: Ownership,
    pub config_writers: Option<Writers>,
    pub conversation_writers: Option<Writers>,
    /// newest first, each with what its turns used
    pub threads: Vec<(ThreadRow, Usage)>,
    /// each conversation's topic, by its id, for those whose agent set one
    pub topics: HashMap<u64, String>,
    pub bound: Vec<u64>,
    pub tools: Vec<ToolInfo>,
    pub failing: HashMap<u64, FinalError>,
}

pub(crate) async fn detail(host: Host, agent: u64, rooms: Rooms) -> Result<Detail, Error> {
    let setup = setup(&host, agent).await?;
    let ownership = host
        .query(identity::ask::Answers { account: agent })
        .await?
        .unwrap_or(Ownership::Anyone);
    let config_writers = match &setup {
        Some(setup) => Some(file_writers(&host, &setup.path).await?),
        None => None,
    };
    let conversation_writers = match &setup {
        Some(_) => {
            let key = harness::tables::CONVERSATIONS.key(&agent);
            let at = harness::conversation_key(agent);
            Some(owned_writers(&host, harness::PROGRAM, key, at).await?)
        }
        None => None,
    };
    let conversations = host
        .query_all(|after| harness::ask::Conversations {
            agent,
            page: page(after),
        })
        .await?;
    let listen = setup.as_ref().and_then(|setup| setup.configured.config());
    let listen = listen.map_or(&[][..], |(_, config)| &config.listen[..]);
    let places = conversations
        .iter()
        .filter_map(|conversation| room_of(&conversation.row.place));
    let channels = places.chain(listen.iter().map(String::as_str));
    name_rooms(&host, &rooms, channels.collect()).await?;
    let bound = host
        .query_all(|after| harness::ask::Bound {
            agent,
            page: page(after),
        })
        .await?
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    let failing = final_errors(&host, agent)
        .await?
        .into_iter()
        .map(|error| (error.thread, error))
        .collect();
    let mut used = Vec::new();
    let mut topics = HashMap::new();
    for conversation in conversations {
        if let Some(topic) = conversation.topic {
            topics.insert(conversation.row.id, topic);
        }
        let usage = conversation.tally.map(|tally| tally.usage);
        used.push((conversation.row, usage.unwrap_or_default()));
    }
    let tools = host.query(harness::ask::Tools).await?;
    Ok(Detail {
        setup,
        ownership,
        config_writers,
        conversation_writers,
        threads: used,
        topics,
        bound,
        tools,
        failing,
    })
}

async fn final_errors(host: &Host, agent: u64) -> Result<Vec<FinalError>, Error> {
    host.query_all(|after| harness::ask::FinalErrors {
        agent,
        page: page(after),
    })
    .await
}

async fn failing_of(host: &Host, thread: &ThreadRow) -> Result<Option<FinalError>, Error> {
    let errors = final_errors(host, thread.agent).await?;
    Ok(errors.into_iter().find(|error| error.thread == thread.id))
}

#[derive(PartialEq)]
pub(crate) struct Transcript {
    pub thread: ThreadRow,
    // the frames read, one run from frame `from` on: those the list showed, and every one since
    pub frames: Vec<Frame>,
    pub from: u64,
    // when the conversation's first frame was written: when it opened
    pub first: Option<u64>,
    // the calls the list drew, as each was read, and every call asked for, read or not placed
    pub calls: Vec<CallRow>,
    pub asked: BTreeSet<String>,
    /// the input waiting behind the loop
    pub held: Vec<Body>,
    /// the frames left out of the model's view, which it reads in parts
    pub withheld: Vec<WithheldFrame>,
    pub setup: Option<AgentSetup>,
    /// member nodes that serve the agent's model's service, once it names one
    pub serving: Option<usize>,
    pub partial: Option<Partial>,
    pub topic: Option<String>,
    pub tally: Tally,
    // harness's egress requests still open that the conversation waits on
    pub waits: Vec<Wait>,
    // the window its general input waits out before a turn, while one is open
    pub settling: Option<SettleWindow>,
    // what its agent waits for (`wait_for`), each until it is over
    pub armed: Vec<WaitRow>,
    pub failing: Option<FinalError>,
    pub runs: Vec<RunRow>,
}

impl Transcript {
    pub fn reach(&self) -> Option<harness::Reached<'_>> {
        self.setup
            .as_ref()
            .and_then(|setup| setup.configured.reached())
    }

    pub fn threshold(&self) -> Option<u64> {
        self.setup.as_ref()?.configured.compact_at()
    }

    // the frame after those read
    pub fn next(&self) -> u64 {
        self.from + self.frames.len() as u64
    }

    pub fn called(&mut self, calls: Vec<(String, Option<CallRow>)>) {
        for (id, row) in calls {
            self.calls.retain(|call| call.id != id);
            self.calls.extend(row);
            self.asked.insert(id);
        }
    }

    // the frames just above those read, read as the list reached them
    pub fn reached(&mut self, mut earlier: Vec<Frame>) {
        earlier.retain(|frame| frame.seq < self.from);
        let Some(top) = earlier.first() else {
            return;
        };
        self.from = top.seq;
        earlier.append(&mut self.frames);
        self.frames = earlier;
        self.opened();
    }

    pub fn moved(&mut self, delta: Delta) {
        if let Some(thread) = delta.thread {
            self.thread = thread;
        }
        let next = self.next();
        let newer = delta.frames.into_iter().filter(|frame| frame.seq >= next);
        self.frames.extend(newer);
        self.opened();
        self.called(delta.calls);
        if let Some(held) = delta.held {
            self.held = held;
        }
        if let Some(withheld) = delta.withheld {
            self.withheld = withheld;
        }
        if let Some(partial) = delta.partial {
            self.partial = partial;
        }
        if let Some(topic) = delta.topic {
            self.topic = topic;
        }
        if let Some(tally) = delta.tally {
            self.tally = tally;
        }
        if let Some(waits) = delta.waits {
            self.waits = waits;
        }
        if let Some(settling) = delta.settling {
            self.settling = settling;
        }
        if let Some(armed) = delta.armed {
            self.armed = armed;
        }
        if let Some(runs) = delta.runs {
            self.runs = runs;
        }
        if let Some(failing) = delta.failing {
            self.failing = failing;
        }
        if let Some((setup, serving)) = delta.setup {
            self.setup = setup;
            self.serving = serving;
        }
    }

    fn opened(&mut self) {
        if self.first.is_none() {
            let first = self.frames.first().filter(|frame| frame.seq == 1);
            self.first = first.map(|frame| frame.time);
        }
    }
}

// the frames of `thread` from frame `from` on, every one
async fn frames_from(host: &Host, thread: u64, from: u64) -> Result<Vec<Frame>, Error> {
    host.query_all(|after| harness::ask::FramesFrom {
        thread,
        from,
        page: page(after),
    })
    .await
}

// when `thread`'s first frame was written
async fn opened_at(host: &Host, thread: u64) -> Result<Option<u64>, Error> {
    let first = PageRequest {
        after: None,
        limit: Some(1),
    };
    let asked = harness::ask::FramesFrom {
        thread,
        from: 1,
        page: first,
    };
    let opened = host.query(asked).await?;
    Ok(opened.items.first().map(|frame| frame.time))
}

async fn served_setup(
    host: &Host,
    agent: u64,
) -> Result<(Option<AgentSetup>, Option<usize>), Error> {
    let setup = card(host, agent).await?;
    let service = setup
        .as_ref()
        .and_then(|setup| setup.configured.reached())
        .map(|reached| reached.service.to_owned());
    let serving = match service {
        Some(service) => Some(served(host, &service).await?),
        None => None,
    };
    Ok((setup, serving))
}

// a conversation opened: its row, its frames from `from` on (from the frame after its last,
// none, unless a read before held more), and all else its screen shows besides its calls
pub(crate) async fn transcript(
    host: Host,
    id: u64,
    from: Option<u64>,
    rooms: Rooms,
) -> Result<Option<Transcript>, Error> {
    let Some(thread) = host.query(harness::ask::Thread { thread: id }).await? else {
        return Ok(None);
    };
    name_rooms(&host, &rooms, room_of(&thread.place).into_iter().collect()).await?;
    let from = from.unwrap_or(thread.frames + 1);
    let frames = frames_from(&host, id, from).await?;
    let first = match frames.first().filter(|frame| frame.seq == 1) {
        Some(frame) => Some(frame.time),
        None => opened_at(&host, id).await?,
    };
    let held = host.query(harness::ask::Held { thread: id }).await?;
    let withheld = host.query(harness::ask::Withheld { thread: id }).await?;
    let partial = host.query(harness::ask::Partial { thread: id }).await?;
    let topic = topic(&host, id).await?;
    let tally = tally(&host, id).await?;
    let settling = host.query(harness::ask::Settling { thread: id }).await?;
    let (setup, serving) = served_setup(&host, thread.agent).await?;
    let waits = waits(&host, &thread).await?;
    let armed = armed(&host, id).await?;
    let failing = failing_of(&host, &thread).await?;
    let runs = host.query(harness::ask::Runs { thread: id }).await?;
    Ok(Some(Transcript {
        thread,
        frames,
        from,
        first,
        calls: Vec::new(),
        asked: BTreeSet::new(),
        held,
        withheld,
        setup,
        serving,
        partial,
        topic,
        tally,
        waits,
        settling,
        armed,
        failing,
        runs,
    }))
}

// the waits `thread`'s agent armed there that are not over
async fn armed(host: &Host, thread: u64) -> Result<Vec<WaitRow>, Error> {
    host.query_all(|after| harness::ask::Armed {
        thread,
        page: page(after),
    })
    .await
}

// what blocks wrote to the open conversation, gathered until it is read
#[derive(Default)]
pub(crate) struct Touched {
    pub row: bool,
    pub frames: bool,
    pub calls: BTreeSet<String>,
    pub held: bool,
    pub withheld: bool,
    pub partial: bool,
    pub topic: bool,
    pub tally: bool,
    pub settling: bool,
    pub setup: bool,
    pub armed: bool,
    pub failing: bool,
    pub runs: bool,
}

impl Touched {
    pub fn any(&self) -> bool {
        self.row
            || self.frames
            || !self.calls.is_empty()
            || self.held
            || self.withheld
            || self.partial
            || self.topic
            || self.tally
            || self.settling
            || self.setup
            || self.armed
            || self.failing
            || self.runs
    }

    pub fn add(&mut self, more: Touched) {
        self.row |= more.row;
        self.frames |= more.frames;
        self.calls.extend(more.calls);
        self.held |= more.held;
        self.withheld |= more.withheld;
        self.partial |= more.partial;
        self.topic |= more.topic;
        self.tally |= more.tally;
        self.settling |= more.settling;
        self.setup |= more.setup;
        self.armed |= more.armed;
        self.failing |= more.failing;
        self.runs |= more.runs;
    }
}

// what a read of the touched parts brings: the row always, each other part when touched
pub(crate) struct Delta {
    pub thread: Option<ThreadRow>,
    pub frames: Vec<Frame>,
    pub calls: Vec<(String, Option<CallRow>)>,
    pub held: Option<Vec<Body>>,
    pub withheld: Option<Vec<WithheldFrame>>,
    pub partial: Option<Option<Partial>>,
    pub topic: Option<Option<String>>,
    pub tally: Option<Tally>,
    pub settling: Option<Option<SettleWindow>>,
    pub setup: Option<(Option<AgentSetup>, Option<usize>)>,
    pub waits: Option<Vec<Wait>>,
    pub armed: Option<Vec<WaitRow>>,
    pub failing: Option<Option<FinalError>>,
    pub runs: Option<Vec<RunRow>>,
}

pub(crate) async fn delta(
    host: Host,
    id: u64,
    agent: u64,
    next: u64,
    touched: Touched,
) -> Result<Delta, Error> {
    let thread = host.query(harness::ask::Thread { thread: id }).await?;
    // a wait starts or ends only with a write to the row or to a call: egress's blocks alone
    // move none
    let moved = touched.row || !touched.calls.is_empty();
    let waits = match (&thread, moved) {
        (Some(row), true) => Some(waits(&host, row).await?),
        _ => None,
    };
    let frames = match touched.frames {
        true => frames_from(&host, id, next).await?,
        false => Vec::new(),
    };
    let calls = calls(host.clone(), id, touched.calls.into_iter().collect()).await?;
    let held = match touched.held {
        true => Some(host.query(harness::ask::Held { thread: id }).await?),
        false => None,
    };
    let withheld = match touched.withheld {
        true => Some(host.query(harness::ask::Withheld { thread: id }).await?),
        false => None,
    };
    let partial = match touched.partial {
        true => Some(host.query(harness::ask::Partial { thread: id }).await?),
        false => None,
    };
    let topic = match touched.topic {
        true => Some(topic(&host, id).await?),
        false => None,
    };
    let tally = match touched.tally {
        true => Some(tally(&host, id).await?),
        false => None,
    };
    let settling = match touched.settling {
        true => Some(host.query(harness::ask::Settling { thread: id }).await?),
        false => None,
    };
    let setup = match touched.setup {
        true => Some(served_setup(&host, agent).await?),
        false => None,
    };
    let armed = match touched.armed {
        true => Some(armed(&host, id).await?),
        false => None,
    };
    let failing = match (&thread, touched.failing) {
        (Some(row), true) => Some(failing_of(&host, row).await?),
        _ => None,
    };
    let runs = match touched.runs {
        true => Some(host.query(harness::ask::Runs { thread: id }).await?),
        false => None,
    };
    Ok(Delta {
        thread,
        frames,
        calls,
        held,
        withheld,
        partial,
        topic,
        tally,
        settling,
        setup,
        waits,
        armed,
        failing,
        runs,
    })
}

// the frames of `thread` from `from` up to the first one read, `before`
pub(crate) async fn earlier(
    host: Host,
    thread: u64,
    from: u64,
    before: u64,
) -> Result<Vec<Frame>, Error> {
    let span = PageRequest {
        after: None,
        limit: Some(before - from),
    };
    let asked = harness::ask::FramesFrom {
        thread,
        from,
        page: span,
    };
    Ok(host.query(asked).await?.items)
}

// each call the list drew, as harness keeps it, or none while it is not placed
pub(crate) async fn calls(
    host: Host,
    thread: u64,
    calls: Vec<String>,
) -> Result<Vec<(String, Option<CallRow>)>, Error> {
    let mut read = Vec::new();
    for call in calls {
        let asked = harness::ask::Call {
            thread,
            call: call.clone(),
        };
        let row = host.query(asked).await?;
        read.push((call, row));
    }
    Ok(read)
}

#[derive(PartialEq)]
pub(crate) struct Route {
    pub row: ProviderRouteRow,
    // the member nodes serving each of its services, in its order
    pub nodes: Vec<usize>,
    // whose it is and its policy, as ownership says, for the reader
    pub standing: policy_view::Standing,
}

impl Route {
    pub fn serving(&self) -> usize {
        self.nodes.first().copied().unwrap_or_default()
    }
}

pub(crate) async fn routes(host: Host, me: Option<u64>) -> Result<Vec<Route>, Error> {
    let reader = me.map(ownership::Party::Account);
    let rows = host
        .query_all(|after| harness::ask::ProviderRoutes { page: page(after) })
        .await?;
    let mut routes = Vec::new();
    for row in rows {
        let mut nodes = Vec::new();
        for entry in &row.services {
            nodes.push(served(&host, &entry.service).await?);
        }
        let held = crate::policies::route_item(&row.provider);
        let standing = policy_view::item::standing(host.clone(), held, reader.clone()).await?;
        routes.push(Route {
            row,
            nodes,
            standing,
        });
    }
    Ok(routes)
}

// a service the routes form offers, with how many member nodes serve it now.
#[derive(PartialEq)]
pub(crate) struct Offer {
    pub row: ServiceRow,
    pub serving: usize,
}

pub(crate) async fn offers(host: Host) -> Result<Vec<Offer>, Error> {
    let rows = host
        .query_all(|after| egress::ask::Services { page: page(after) })
        .await?;
    let mut offers = Vec::new();
    for row in rows {
        let serving = served(&host, &row.name).await?;
        offers.push(Offer { row, serving });
    }
    Ok(offers)
}

pub(crate) async fn file(host: &Host, path: &str, at: Option<BlobId>) -> Result<Vec<u8>, String> {
    let mut at = at;
    let mut bytes = Vec::new();
    loop {
        let asked = files::ask::Read {
            path: path.to_owned(),
            at,
            offset: bytes.len() as u64,
            len: files::CHUNK_SIZE,
        };
        let answered = host.query(asked).await;
        let not_found = answered
            .as_ref()
            .is_err_and(|refusal| refusal.code == files::code::NOT_FOUND);
        let read_at_a_pruned_snapshot = at.is_some() && not_found;
        if read_at_a_pruned_snapshot {
            at = None;
            bytes.clear();
            continue;
        }
        let contents = answered.map_err(|refusal| design::sentence(&refusal))?;
        // a read short of the size it states would never end
        if contents.bytes.is_empty() && !contents.eof {
            return Err(format!("{path} ended before the size files gives it."));
        }
        at = Some(contents.snapshot);
        bytes.extend(contents.bytes);
        if contents.eof {
            return Ok(bytes);
        }
    }
}

/// The thread at `place`: the open one, else the last one there.
pub(crate) async fn thread_at(host: Host, agent: u64, place: String) -> Result<Option<u64>, Error> {
    let asked = harness::ask::ThreadAt { agent, place };
    Ok(host.query(asked).await?.map(|thread| thread.id))
}

// one of harness's egress requests still open: what it waits for, at which node, since when
#[derive(PartialEq)]
pub(crate) struct Wait {
    pub id: u64,
    pub node: Vec<u8>,
    pub made: u64,
    pub on: Awaited,
}

// what `thread` and its calls wait on at egress, read only while it waits: every node's
// assigned requests, harness's kept, another thread's reply left out
pub(crate) async fn waits(host: &Host, thread: &ThreadRow) -> Result<Vec<Wait>, Error> {
    let idle = thread.asking == Asking::Idle && thread.open_calls == 0;
    if thread.closed || idle {
        return Ok(Vec::new());
    }
    let nodes = host
        .query_all(|after| egress::ask::Nodes { page: page(after) })
        .await?;
    let mut waits = Vec::new();
    for node in nodes {
        let assigned = host.query(egress::ask::Assigned { key: node.key }).await?;
        for row in assigned {
            if row.requester != harness::PROGRAM {
                continue;
            }
            let Some(on) = harness::awaited(&row.request, &row.delivery) else {
                continue;
            };
            let theirs = match &on {
                Awaited::Reply { thread: id } => *id == thread.id,
                Awaited::Window { .. } | Awaited::Summary | Awaited::Relay { .. } => true,
            };
            if theirs {
                waits.push(Wait {
                    id: row.id,
                    node: row.node,
                    made: row.made,
                    on,
                });
            }
        }
    }
    Ok(waits)
}
