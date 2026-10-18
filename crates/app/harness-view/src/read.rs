//! Every read the screens make: the roster (the accounts whose script calls harness, what
//! harness resolves for each, their threads and the topic of the one its sentence names, the
//! chat rooms' names, how many member nodes serve each model's service and relay, and the
//! newest block's time), one agent in full with who may change its configuration file and whom
//! it answers, both read from access, and each conversation's topic, one thread's transcript
//! with its topic and the reply its model is streaming back, the model records, and the
//! services the models form offers. A topic is the line the agent last set with
//! `thread_topic`: its own words on what the conversation is about.
use std::collections::HashMap;

use egress::{RELAY, ServiceRow};
use harness::{
    AgentSetup, Body, CallRow, Frame, ModelRow, Partial, ThreadRow, ToolInfo, Usage, WithheldFrame,
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

async fn frames(host: &Host, thread: u64) -> Result<Vec<Frame>, Error> {
    host.query_all(|after| harness::ask::Frames {
        thread,
        page: page(after),
    })
    .await
}

/// Every turn's usage, added up.
pub(crate) fn usage<'a>(frames: impl IntoIterator<Item = &'a Frame>) -> Usage {
    sum(frames.into_iter().filter_map(|frame| match &frame.body {
        Body::Turn { usage, .. } => Some(usage),
        _ => None,
    }))
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
    let mut used = Vec::new();
    let mut topics = HashMap::new();
    // ponytail: the totals read every frame of every thread; a total kept on the thread row
    // when transcripts run long
    for thread in threads {
        let usage = usage(&frames(&host, thread.id).await?);
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
        tools,
    })
}

#[derive(PartialEq)]
pub(crate) struct Transcript {
    pub thread: ThreadRow,
    pub frames: Vec<Frame>,
    pub calls: Vec<CallRow>,
    /// the input waiting behind the loop
    pub held: Vec<Body>,
    /// the frames left out of the model's view, which it reads in parts
    pub withheld: Vec<WithheldFrame>,
    pub setup: Option<AgentSetup>,
    /// member nodes that serve the agent's model's service, once it names one
    pub serving: Option<usize>,
    pub partial: Option<Partial>,
    pub topic: Option<String>,
}

impl Transcript {
    pub fn model(&self) -> Option<&ModelRow> {
        self.setup
            .as_ref()
            .and_then(|setup| setup.configured.model())
    }
}

pub(crate) async fn transcript(host: Host, id: u64) -> Result<Option<Transcript>, Error> {
    let Some(thread) = host.query(harness::ask::Thread { thread: id }).await? else {
        return Ok(None);
    };
    // ponytail: the whole transcript again on each block that writes harness; a paged tail
    // when threads run long
    let frames = frames(&host, id).await?;
    let calls = host
        .query_all(|after| harness::ask::Calls {
            thread: id,
            page: page(after),
        })
        .await?;
    let held = host.query(harness::ask::Held { thread: id }).await?;
    let withheld = host.query(harness::ask::Withheld { thread: id }).await?;
    let partial = host.query(harness::ask::Partial { thread: id }).await?;
    let topic = topic(&host, id).await?;
    let setup = setup(&host, thread.agent).await?;
    let service = setup
        .as_ref()
        .and_then(|setup| setup.configured.model())
        .map(|model| model.service.clone());
    let serving = match service {
        Some(service) => Some(served(&host, &service).await?),
        None => None,
    };
    Ok(Some(Transcript {
        thread,
        frames,
        calls,
        held,
        withheld,
        setup,
        serving,
        partial,
        topic,
    }))
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
