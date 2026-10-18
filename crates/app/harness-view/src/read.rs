//! Every read the screens make: the roster (agents, their threads and topics, room names, nodes
//! serving each service and relay, the time), one agent in full (access lists, topics, tallies,
//! bound conversations), one transcript as far as its list shows it, the models and services.
use std::collections::{BTreeSet, HashMap};

use egress::{RELAY, ServiceRow};
use harness::{
    AgentSetup, Asking, Awaited, Body, CallRow, Frame, ModelRow, Partial, SettleWindow, Tally,
    ThreadRow, ToolInfo, Usage, WaitRow, WithheldFrame,
};
use identity::{Category, Kind, Ownership, PageRequest};
use program::Writers;
use view::methods::{BlockPage, ChainBlocks};
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
    /// each chat room's name by its channel id, which a thread's place holds
    pub rooms: HashMap<String, String>,
    /// member nodes that serve relay: no tool call runs without one
    pub relaying: usize,
    /// the newest block's time, in milliseconds: now, as the chain tells it
    pub now: u64,
}

/// One account whose script calls harness, as the list draws it.
#[derive(PartialEq)]
pub(crate) struct Card {
    pub name: String,
    pub setup: AgentSetup,
    /// newest first
    pub threads: Vec<ThreadRow>,
    /// member nodes that serve its model's service, once its configuration names a model
    pub serving: Option<usize>,
    /// "suspended" or "revoked", for an agent that is
    pub note: Option<&'static str>,
    /// the topic of the conversation its sentence names (`words::spoken_of`)
    pub topic: Option<String>,
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

fn calls_harness(kind: &Kind) -> bool {
    match kind {
        Kind::Program {
            category: Category::Agent,
            script: Some(script),
            ..
        } => script.calls == harness::PROGRAM,
        Kind::User | Kind::Program { .. } | Kind::System(_) => false,
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

pub(crate) async fn roster(host: Host, me: Option<u64>) -> Result<Roster, Error> {
    let tip = host
        .ask::<ChainBlocks>(BlockPage {
            before: None,
            limit: 1,
            most: 0,
        })
        .await?;
    let accounts = host
        .query_all(|after| identity::ask::List { page: page(after) })
        .await?;
    let mut serving: HashMap<String, usize> = HashMap::new();
    serving.insert(RELAY.to_owned(), served(&host, RELAY).await?);
    let mut agents = Vec::new();
    // ponytail: one Agent and one Threads read per agent; a harness query of every agent's live
    // state when networks run many agents
    for account in accounts
        .iter()
        .filter(|account| calls_harness(&account.kind()))
    {
        let agent = account.number;
        let Some(setup) = setup(&host, agent).await? else {
            continue;
        };
        let service = setup.configured.model().map(|model| model.service.clone());
        let mut count = None;
        if let Some(service) = service {
            if !serving.contains_key(&service) {
                let now = served(&host, &service).await?;
                serving.insert(service.clone(), now);
            }
            count = serving.get(&service).copied();
        }
        let threads = host
            .query_all(|after| harness::ask::Threads {
                agent,
                page: page(after),
            })
            .await?;
        let topic = match crate::words::spoken_of(&threads) {
            Some(thread) => topic(&host, thread.id).await?,
            None => None,
        };
        agents.push(Card {
            name: account.card.name.clone(),
            note: account.kind().note(),
            serving: count,
            threads,
            setup,
            topic,
        });
    }
    let unscripted = accounts
        .iter()
        .filter(|account| unscripted_of(&account.kind(), me))
        .map(|account| Unscripted {
            number: account.number,
            name: account.card.name.clone(),
        })
        .collect();
    // ponytail: every room, direct messages too, on each roster read, and a rename waits for
    // the next one since chat's writes start none; one Channel read per room a thread names
    // when networks hold many rooms
    let rooms = host
        .query_all(|after| chat::ask::Channels { page: page(after) })
        .await?
        .into_iter()
        .map(|info| (info.channel.id, info.channel.name))
        .collect();
    Ok(Roster {
        agents,
        unscripted,
        names: accounts
            .into_iter()
            .map(|account| (account.number, account.card.name))
            .collect(),
        rooms,
        relaying: serving[RELAY],
        now: tip.blocks.first().map_or(0, |block| block.time),
    })
}

// the model budget, as harness keeps it
pub(crate) async fn budget(host: &Host) -> Result<harness::BudgetReport, Error> {
    host.query(harness::ask::Budget).await
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
}

pub(crate) async fn detail(host: Host, agent: u64) -> Result<Detail, Error> {
    let setup = setup(&host, agent).await?;
    let ownership = host
        .query(access::ask::List {
            program: harness::PROGRAM.to_owned(),
            key: harness::agent_key(agent),
        })
        .await?
        .unwrap_or(Ownership::Owner);
    let config_writers = match &setup {
        Some(setup) => Some(
            host.query(access::ask::Writers {
                program: files::PROGRAM.to_owned(),
                key: setup.path.clone(),
            })
            .await?,
        ),
        None => None,
    };
    let conversation_writers = match &setup {
        Some(_) => Some(
            host.query(access::ask::Writers {
                program: harness::PROGRAM.to_owned(),
                key: harness::conversation_key(agent),
            })
            .await?,
        ),
        None => None,
    };
    let threads = host
        .query_all(|after| harness::ask::Threads {
            agent,
            page: page(after),
        })
        .await?;
    let bound = host
        .query_all(|after| harness::ask::Bound {
            agent,
            page: page(after),
        })
        .await?
        .into_iter()
        .map(|thread| thread.id)
        .collect();
    let mut used = Vec::new();
    let mut topics = HashMap::new();
    for thread in threads {
        let usage = tally(&host, thread.id).await?.usage;
        if let Some(topic) = topic(&host, thread.id).await? {
            topics.insert(thread.id, topic);
        }
        used.push((thread, usage));
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
    })
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
}

impl Transcript {
    pub fn model(&self) -> Option<&ModelRow> {
        self.setup
            .as_ref()
            .and_then(|setup| setup.configured.model())
    }

    pub fn threshold(&self) -> Option<u64> {
        let setup = self.setup.as_ref()?;
        setup.configured.threshold().map(harness::Threshold::tokens)
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
    let setup = setup(host, agent).await?;
    let service = setup
        .as_ref()
        .and_then(|setup| setup.configured.model())
        .map(|model| model.service.clone());
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
) -> Result<Option<Transcript>, Error> {
    let Some(thread) = host.query(harness::ask::Thread { thread: id }).await? else {
        return Ok(None);
    };
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
pub(crate) struct Model {
    pub row: ModelRow,
    pub serving: usize,
}

pub(crate) async fn models(host: Host) -> Result<Vec<Model>, Error> {
    let rows = host
        .query_all(|after| harness::ask::Models { page: page(after) })
        .await?;
    let mut models = Vec::new();
    for row in rows {
        let serving = served(&host, &row.service).await?;
        models.push(Model { row, serving });
    }
    Ok(models)
}

/// A service the models form offers, with how many member nodes serve it now.
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
