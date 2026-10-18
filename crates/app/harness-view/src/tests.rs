mod conversations;
mod live_calls;
mod thinking;

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use harness::{
    AgentSetup, BlobId, Body, CallRow, CallState, Config, Configured, Context as Fill, Flow,
    Forming, Frame, ModelRow, Partial, Principal, Said, Standing, Stop, ToolInfo, Usage, Window,
    Wire, WithheldFrame,
};
use view::methods::{BlobBody, Change, Changes, LinkOpen, OpPut, Query, Session, Submitted};
use view::testing::TestAppContext;

const EDDY: u64 = 7;
const ADA: u64 = 8;
const SCOUT: u64 = 12;
const PAGER: u64 = 19;
const ATLAS: u64 = 24;
const ROVER: u64 = 31;

fn page<T>(items: Vec<T>) -> harness::PageResponse<T> {
    harness::PageResponse {
        height: 1,
        items,
        next: None,
    }
}

fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
    identity::Account {
        number,
        card: identity::Card {
            name: name.into(),
            avatar: None,
            bio: None,
            updated_at: 0,
        },
        control,
    }
}

fn person(number: u64, name: &str) -> identity::Account {
    account(number, name, identity::Control::User { keys: Vec::new() })
}

fn home_of(manager: u64) -> &'static str {
    match manager {
        EDDY => "Eddy",
        ADA => "Ada",
        other => panic!("no person is account {other}"),
    }
}

fn path_of(manager: u64, name: &str) -> String {
    format!(
        "/home/{}/agents/{}.md",
        home_of(manager),
        name.to_lowercase()
    )
}

fn agent(number: u64, name: &str, manager: u64, scripted: bool) -> identity::Account {
    account(
        number,
        name,
        identity::Control::Program {
            manager,
            category: identity::Category::Agent,
            life: identity::Life::Active { keys: Vec::new() },
            script: scripted.then(|| identity::Script {
                calls: harness::PROGRAM.into(),
                config: path_of(manager, name),
            }),
        },
    )
}

const SNAPSHOT: BlobId = BlobId::Sha256([0xab; 32]);
const SONNET: &str = "sonnet";

fn model_row(window: Window) -> ModelRow {
    ModelRow {
        id: SONNET.into(),
        owner: ADA,
        wire: Wire::AnthropicMessages,
        service: "anthropic".into(),
        name: "claude-sonnet-5-5".into(),
        window,
    }
}

fn config_of(name: &str) -> Config {
    Config {
        model: SONNET.into(),
        max_tokens: 4096,
        wait: None,
        thinking: None,
        system: format!("You are {name}."),
    }
}

fn setup(agent: u64, name: &str, manager: u64, window: Window) -> AgentSetup {
    AgentSetup {
        agent,
        manager,
        standing: Standing::Active,
        path: path_of(manager, name),
        configured: Configured::Ready {
            snapshot: SNAPSHOT,
            config: config_of(name),
            model: model_row(window),
        },
        current: None,
    }
}

fn thread(id: u64, agent: u64, manager: u64, place: &str) -> ThreadRow {
    ThreadRow {
        id,
        agent,
        manager,
        place: place.into(),
        title: format!("conversation {id}"),
        opened: 8960,
        closed: false,
        frames: 0,
        last_input: 0,
        seen: 0,
        last_stop: None,
        asking: Asking::Idle,
        open_calls: 0,
        held: 0,
        sealed: 0,
        base: 1,
        tail_from: 1,
        summary: None,
        context: Fill::Measured(72_140),
        compact_before: None,
        last_height: 9003,
        last_time: LAST_TIME,
    }
}

const LAST_TIME: u64 = 1_760_000_003_000;

fn room((id, name): &(String, String)) -> chat::ChannelInfo {
    chat::ChannelInfo {
        channel: chat::ChannelRow {
            id: id.clone(),
            name: name.clone(),
            created_at: 0,
            post_policy: chat::PostPolicy::Open,
            owner: Principal::Account(EDDY),
            archived: false,
        },
        head_seq: 0,
    }
}

fn frame(seq: u64, body: Body) -> Frame {
    Frame {
        seq,
        height: 9000 + seq,
        time: 1_760_000_000_000 + seq * 1000,
        body,
    }
}

fn post(seq: u64, text: &str) -> Frame {
    frame(
        seq,
        Body::Post {
            author: Principal::Account(ADA),
            text: text.into(),
        },
    )
}

/// The network the fake host answers from. A test changes it before it seats the view,
/// or through its handle after.
struct Net {
    accounts: Vec<identity::Account>,
    setups: Vec<AgentSetup>,
    models: Vec<ModelRow>,
    threads: Vec<ThreadRow>,
    frames: Vec<(u64, Frame)>,
    calls: Vec<CallRow>,
    held: Vec<(u64, Body)>,
    withheld: Vec<(u64, WithheldFrame)>,
    partials: Vec<(u64, Partial)>,
    serving: Vec<(String, usize)>,
    services: Vec<egress::ServiceRow>,
    answers: Vec<(u64, identity::Ownership)>,
    /// chat's rooms: (channel id, name)
    rooms: Vec<(String, String)>,
}

impl Default for Net {
    /// Eddy manages Scout (working in #general, a closed thread in #release) and Pager
    /// (no script hands it to harness yet); Ada manages Atlas (idle) and the model record
    /// sonnet. One node serves anthropic and relay.
    fn default() -> Self {
        let mut general = thread(7, SCOUT, EDDY, "chat/general");
        general.asking = Asking::Asked { upto: 3 };
        general.frames = 3;
        let mut release = thread(3, SCOUT, EDDY, "chat/release");
        release.closed = true;
        Net {
            accounts: vec![
                person(EDDY, "Eddy"),
                person(ADA, "Ada"),
                agent(SCOUT, "Scout", EDDY, true),
                agent(PAGER, "Pager", EDDY, false),
                agent(ATLAS, "Atlas", ADA, true),
            ],
            setups: vec![
                AgentSetup {
                    current: Some(7),
                    ..setup(SCOUT, "Scout", EDDY, Window::Tokens(200_000))
                },
                AgentSetup {
                    current: Some(9),
                    ..setup(ATLAS, "Atlas", ADA, Window::Tokens(200_000))
                },
            ],
            models: vec![model_row(Window::Tokens(200_000))],
            threads: vec![general, release, thread(9, ATLAS, ADA, "chat/deploys")],
            frames: vec![
                (
                    7,
                    post(
                        1,
                        "Ada mentioned you in chat channel general:\nWhy is payments slow?",
                    ),
                ),
                (
                    7,
                    frame(
                        2,
                        Body::Turn {
                            model: "claude-sonnet-5-5".into(),
                            config: SNAPSHOT,
                            said: vec![
                                Said::Text("Reading the deploy log first.".into()),
                                Said::ToolUse {
                                    id: "toolu_01".into(),
                                    name: "forge_log".into(),
                                    input: r#"{"repo":"payments","limit":20}"#.into(),
                                },
                            ],
                            stop: Stop::ToolUse,
                            usage: Usage {
                                input: 1204,
                                output: 312,
                                cache_read: 38_410,
                                cache_write: 0,
                            },
                        },
                    ),
                ),
                (
                    7,
                    frame(
                        3,
                        Body::Result {
                            call: "toolu_01".into(),
                            tool: "forge_log".into(),
                            text: "c41a90e2 payments: timeout 3s -> 0.8s".into(),
                            failed: false,
                        },
                    ),
                ),
            ],
            calls: vec![CallRow {
                thread: 7,
                id: "toolu_01".into(),
                tool: "forge_log".into(),
                input: r#"{"repo":"payments","limit":20}"#.into(),
                made: 8962,
                state: CallState::Done {
                    height: 8964,
                    failed: false,
                },
            }],
            held: Vec::new(),
            withheld: Vec::new(),
            partials: Vec::new(),
            serving: vec![("anthropic".into(), 1), (egress::RELAY.into(), 1)],
            services: vec![egress::ServiceRow {
                name: "anthropic".into(),
                owner: ADA,
                nodes: vec![vec![1], vec![2]],
                registered: 5,
            }],
            answers: Vec::new(),
            rooms: ["general", "release", "deploys"]
                .map(|room| (room.to_owned(), room.to_owned()))
                .into(),
        }
    }
}

type Shared = Rc<RefCell<Net>>;

/// The view open over `net`, seated as `me`.
fn seated(net: Net, me: u64) -> (TestAppContext, Shared) {
    let net = Rc::new(RefCell::new(net));
    let mut cx = TestAppContext::new();
    // every config goes up as one chunk, held as its bytes' stand-in id
    cx.host().handle::<OpPut>(|ask| {
        Ok(ask
            .blobs
            .iter()
            .map(|blob| match &blob.body {
                BlobBody::Bytes(bytes) if blob.kind == files::CHUNK => held_as(bytes),
                _ => panic!("a config goes up as chunk bytes: {blob:?}"),
            })
            .collect())
    });
    let shared = net.clone();
    cx.host().handle::<Query<harness::Harness>>(move |query| {
        let net = shared.borrow();
        Ok(match query {
            harness::Query::Agent { agent } => harness::Reply::Agent(
                net.setups
                    .iter()
                    .find(|setup| setup.agent == agent)
                    .cloned(),
            ),
            harness::Query::Models { .. } => harness::Reply::Models(page(net.models.clone())),
            harness::Query::Model { id } => {
                harness::Reply::Model(net.models.iter().find(|row| row.id == id).cloned())
            }
            harness::Query::Threads { agent, .. } => {
                let mut threads: Vec<ThreadRow> = net
                    .threads
                    .iter()
                    .filter(|thread| thread.agent == agent)
                    .cloned()
                    .collect();
                threads.sort_by_key(|thread| u64::MAX - thread.id);
                harness::Reply::Threads(page(threads))
            }
            harness::Query::Thread { thread } => {
                harness::Reply::Thread(net.threads.iter().find(|row| row.id == thread).cloned())
            }
            harness::Query::Frames { thread, .. } => harness::Reply::Frames(page(
                net.frames
                    .iter()
                    .filter(|(id, _)| *id == thread)
                    .map(|(_, frame)| frame.clone())
                    .collect(),
            )),
            harness::Query::Calls { thread, .. } => harness::Reply::Calls(page(
                net.calls
                    .iter()
                    .filter(|call| call.thread == thread)
                    .cloned()
                    .collect(),
            )),
            harness::Query::Held { thread } => harness::Reply::Held(
                net.held
                    .iter()
                    .filter(|(id, _)| *id == thread)
                    .map(|(_, body)| body.clone())
                    .collect(),
            ),
            harness::Query::ThreadAt { agent, place } => harness::Reply::Thread(
                net.threads
                    .iter()
                    .filter(|row| row.agent == agent && row.place == place)
                    .max_by_key(|row| row.id)
                    .cloned(),
            ),
            harness::Query::Withheld { thread } => harness::Reply::Withheld(
                net.withheld
                    .iter()
                    .filter(|(id, _)| *id == thread)
                    .map(|(_, withheld)| *withheld)
                    .collect(),
            ),
            harness::Query::Partial { thread } => harness::Reply::Partial(
                net.partials
                    .iter()
                    .find(|(id, _)| *id == thread)
                    .map(|(_, partial)| partial.clone()),
            ),
            harness::Query::Tools => harness::Reply::Tools(vec![
                ToolInfo {
                    name: "forge_log".into(),
                    description: String::new(),
                    schema: String::new(),
                    writes: false,
                },
                ToolInfo {
                    name: "chat_post".into(),
                    description: String::new(),
                    schema: String::new(),
                    writes: true,
                },
            ]),
        })
    });
    let shared = net.clone();
    cx.host().handle::<Query<Identity>>(move |query| {
        let net = shared.borrow();
        Ok(match query {
            identity::Query::List { .. } => identity::Reply::Accounts(page(net.accounts.clone())),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    let shared = net.clone();
    cx.host().handle::<Query<access::Access>>(move |query| {
        let net = shared.borrow();
        Ok(match query {
            access::Query::List { program, key } if program == harness::PROGRAM => {
                let answers = net
                    .answers
                    .iter()
                    .find(|(agent, _)| harness::agent_key(*agent) == key)
                    .map(|(_, list)| list.clone());
                access::Reply::List(answers)
            }
            access::Query::Writers { program, key } if program == files::PROGRAM => {
                let owner = net
                    .setups
                    .iter()
                    .find(|setup| setup.path == key)
                    .map(|setup| identity::Party::Account(setup.manager));
                access::Reply::Writers(program::Writers::Listed {
                    at: key,
                    owner,
                    list: identity::Ownership::Owner,
                })
            }
            access::Query::Writers { program, key } if program == harness::PROGRAM => {
                let owner = net
                    .setups
                    .iter()
                    .find(|setup| harness::conversation_key(setup.agent) == key)
                    .map(|setup| identity::Party::Account(setup.manager));
                access::Reply::Writers(program::Writers::Listed {
                    at: key,
                    owner,
                    list: identity::Ownership::Owner,
                })
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.host()
        .handle::<Submit<access::Access>>(|_| Ok(applied(Vec::new())));
    let shared = net.clone();
    cx.host().handle::<Query<egress::Egress>>(move |query| {
        let net = shared.borrow();
        Ok(match query {
            egress::Query::Serving { service } => {
                let count = net
                    .serving
                    .iter()
                    .find(|(name, _)| *name == service)
                    .map_or(0, |(_, count)| *count);
                egress::Reply::Serving(vec![vec![1]; count])
            }
            egress::Query::Services { .. } => egress::Reply::Services(page(net.services.clone())),
            other => panic!("unexpected query: {other:?}"),
        })
    });
    let shared = net.clone();
    cx.host().handle::<Query<chat::Chat>>(move |query| {
        let net = shared.borrow();
        Ok(match query {
            chat::Query::Channels { .. } => {
                chat::Reply::Channels(page(net.rooms.iter().map(room).collect()))
            }
            other => panic!("unexpected query: {other:?}"),
        })
    });
    cx.open::<Agents>();
    cx.host().stream::<HostSession>().send(Session {
        account: Some(me),
        chain_id: "garage#0a1b2c3d".into(),
        ..Session::default()
    });
    cx.run_until_parked();
    (cx, net)
}

#[test]
fn the_list_shows_each_agent_with_its_state() {
    let (cx, _) = seated(Net::default(), EDDY);
    for text in [
        "My agents",
        "Scout",
        "Eddy (you)",
        "Working",
        "Conversation 7 · Waiting for the model",
        "1 open · 1 closed",
        "Pager",
        "Not handed to harness",
        "Other agents",
        "Atlas",
        "Idle",
        "1 relay node",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn no_node_shows_on_the_agent_and_in_the_thread() {
    let net = Net {
        serving: vec![(egress::RELAY.into(), 1)],
        ..Net::default()
    };
    let (mut cx, _) = seated(net, EDDY);
    assert!(cx.has_text("No node"), "{:?}", cx.texts());
    assert!(
        cx.has_text("No member node serves anthropic"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    let missing = "No member node serves anthropic. The model is not asked, and the agent stays silent in chat.";
    assert!(cx.has_text(missing), "{:?}", cx.texts());
    assert!(
        cx.has_text("1. The service owner registers anthropic and assigns node keys to it."),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text(missing), "{:?}", cx.texts());
    assert!(cx.has_text("No node"), "{:?}", cx.texts());
}

#[test]
fn no_relay_shows_what_an_operator_turns_on() {
    let net = Net {
        serving: vec![("anthropic".into(), 1)],
        ..Net::default()
    };
    let (mut cx, _) = seated(net, EDDY);
    assert!(
        cx.has_text("No member node serves relay"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(
        cx.has_text(
            "A member node’s operator writes relay = true in egress.toml, then restarts the node."
        ),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_stuck_thread_shows_its_state_and_what_waits() {
    let mut net = Net::default();
    net.threads[0].held = 2;
    net.held = vec![
        (
            7,
            Body::Post {
                author: Principal::Account(ADA),
                text: "Is the rollback done?\nmore".into(),
            },
        ),
        (
            7,
            Body::Post {
                author: Principal::Account(EDDY),
                text: "Still there?".into(),
            },
        ),
    ];
    let (mut cx, _) = seated(net, EDDY);
    assert!(cx.has_text("Stuck"), "{:?}", cx.texts());
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    for text in [
        "Stuck",
        "This thread may be stuck",
        "2 waiting · they follow when the turn ends",
        "Is the rollback done?",
        "Still there?",
        "3 frames · 2 waiting · 0 open calls",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    let last = format!(
        "Last frame at {}, {} · block 9,003",
        view::design::day(LAST_TIME),
        view::design::clock(LAST_TIME)
    );
    assert!(cx.has_text(&last), "{last}: {:?}", cx.texts());
    assert!(
        cx.has_text("It is still “Waiting for the model” with 2 messages waiting behind it. A harness upgrade or a frame that faults leaves a thread like this, and nothing recovers it today. Its manager can close it."),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_thread_shows_its_transcript_calls_and_usage() {
    let mut net = Net::default();
    net.frames.push((
        7,
        frame(
            4,
            Body::Notice {
                text: "the model answered 529, overloaded_error: Overloaded".into(),
            },
        ),
    ));
    net.frames.push((
        7,
        frame(
            5,
            Body::Compaction {
                wire: Wire::AnthropicMessages,
                block: r#"{"type":"compaction","content":"Ada asked about payments."}"#.into(),
            },
        ),
    ));
    net.withheld = vec![(
        7,
        WithheldFrame {
            seq: 3,
            size: 48_200,
        },
    )];
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    for text in [
        "Conversation 7",
        "Waiting for the model",
        "Ada mentioned you in chat channel general:\nWhy is payments slow?",
        "Reading the deploy log first.",
        "forge_log",
        "Done",
        "input 1,204 · output 312 · cache read 38,410 · cache write 0 · all 39,926",
        "This thread · input 1,204 · output 312 · cache read 38,410 · cache write 0 · all 39,926",
        "Context 72,140 / 200,000 tokens · 36% · 1 summary",
        "Read in parts · 48,200 bytes",
        "Notice",
        "the model answered 529, overloaded_error: Overloaded",
        "Summary",
        "Open in chat",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    // a call opens to its whole input and the blocks it ran at
    assert!(!cx.has_text("made at block 8,962 · done at block 8,964"));
    cx.simulate_click("call-toolu_01");
    cx.run_until_parked();
    assert!(
        cx.has_text("made at block 8,962 · done at block 8,964"),
        "{:?}",
        cx.texts()
    );
    // a result shows its size until it is opened
    assert!(!cx.has_text("c41a90e2 payments: timeout 3s -> 0.8s"));
    cx.simulate_click("result-3");
    cx.run_until_parked();
    assert!(cx.has_text("c41a90e2 payments: timeout 3s -> 0.8s"));
    cx.simulate_click("Open in chat");
    assert_eq!(
        cx.host().requests::<LinkOpen>(),
        ["duck://garage-0a1b2c3d/chat/general"]
    );
}

fn streaming() -> Partial {
    Partial {
        wire: Wire::AnthropicMessages,
        model: "claude-sonnet-5-5".into(),
        said: vec![
            Said::Thinking {
                text: "The log names the deploy".into(),
                signature: "sig".into(),
            },
            Said::Text("The slowdown began with".into()),
            Said::ToolUse {
                id: "toolu_02".into(),
                name: "forge_log".into(),
                input: r#"{"repo":"pay"#.into(),
            },
        ],
        forming: Forming::Kept { index: 2 },
        stop: None,
        usage: Usage::default(),
        next_seq: 14,
        flow: Flow::Open,
    }
}

#[test]
fn the_reply_the_model_is_streaming_grows_under_the_transcript() {
    let net = Net {
        partials: vec![(7, streaming())],
        ..Net::default()
    };
    let (mut cx, shared) = seated(net, EDDY);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    for text in [
        "is answering · 14 events so far",
        "Thinking",
        "The log names the deploy",
        "The slowdown began with",
        "forge_log",
        r#"{"repo":"pay"#,
        "Forming",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    let broken = Partial {
        flow: Flow::Broken {
            reason: "event 15 arrived where event 14 was due".into(),
        },
        ..streaming()
    };
    shared.borrow_mut().partials = vec![(7, broken)];
    changes.send(Some(Change {
        height: 10,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(
        cx.has_text("the reply broke: event 15 arrived where event 14 was due"),
        "{:?}",
        cx.texts()
    );
    shared.borrow_mut().partials.clear();
    changes.send(Some(Change {
        height: 11,
        keys: Vec::new(),
    }));
    cx.run_until_parked();
    assert!(cx.find("thread-partial").is_none());
}

#[test]
fn a_window_not_learned_yet_shows_the_tokens_alone() {
    let mut net = Net::default();
    net.setups[0] = setup(SCOUT, "Scout", EDDY, Window::Unasked);
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text("context 72,140 tokens"), "{:?}", cx.texts());
    assert!(cx.has_text("not asked yet"), "{:?}", cx.texts());
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(
        cx.has_text("Context 72,140 tokens · 0 summaries"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn close_and_take_back_are_the_managers_alone() {
    let (mut cx, _) = seated(Net::default(), ADA);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text("Scout"), "{:?}", cx.texts());
    assert!(cx.find("agent-remove").is_none());
    assert!(cx.find("agent-edit").is_none());
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text("Conversation 7"), "{:?}", cx.texts());
    assert!(cx.find("thread-close").is_none());

    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    identity_takes(&cx);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    cx.simulate_click("thread-close");
    cx.run_until_parked();
    assert!(
        cx.has_text("Close this conversation? It is the agent's current one, so the next message to the agent opens a new conversation with fresh context."),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("thread-close-confirm");
    cx.run_until_parked();
    cx.simulate_click("thread-back");
    cx.run_until_parked();
    cx.simulate_click("agent-remove");
    cx.run_until_parked();
    assert!(
        cx.has_text(
            "Take Scout back from harness? Its script is cleared, so harness stops answering for it. Its threads and its configuration file stay."
        ),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("agent-remove-confirm");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::Close { thread: 7 }]
    );
    assert_eq!(
        cx.host().requests::<Submit<Identity>>(),
        [identity::Op::SetScript {
            account: SCOUT,
            script: None
        }]
    );
    assert!(cx.has_text("My agents"), "{:?}", cx.texts());
}

/// The node's answer to a write it predicts applies, carrying the program's `output`.
fn applied(output: Vec<u8>) -> Submitted {
    Submitted::Applied {
        frame: [0; 32],
        until: 0,
        output,
    }
}

/// Answers identity's ops as identity would: `CreateAgent` with the new account's number.
fn identity_takes(cx: &TestAppContext) {
    cx.host().handle::<Submit<Identity>>(|op| match op {
        identity::Op::CreateAgent { .. } => Ok(applied(view::methods::encode(&ROVER))),
        _ => Ok(applied(Vec::new())),
    });
}

fn files_take(cx: &TestAppContext) {
    cx.host()
        .handle::<Submit<files::Files>>(|_| Ok(applied(Vec::new())));
}

/// The id the fake host answers a put chunk with: its bytes folded into
/// 32, so a commit names the bytes that were put, not just any chunk.
fn held_as(bytes: &[u8]) -> [u8; 32] {
    let mut id = [0; 32];
    for (at, byte) in bytes.iter().enumerate() {
        id[at % 32] ^= byte;
    }
    id
}

fn edit_of(path: &str, config: &Config, base: Option<BlobId>, message: String) -> files::Op {
    files::Op::Commit {
        base,
        message,
        changes: vec![files::Change::Put {
            path: path.into(),
            exec: false,
            meta: Default::default(),
            content: files::Content::Chunks {
                size: config.write().len() as u64,
                chunks: vec![BlobId::Sha256(held_as(config.write().as_bytes()))],
            },
        }],
    }
}

fn kept(path: &str) -> access::Op {
    access::Op::Set {
        program: files::PROGRAM.into(),
        key: path.into(),
        list: Some(identity::Ownership::Owner),
    }
}

fn answering(agent: u64, list: identity::Ownership) -> access::Op {
    access::Op::Set {
        program: harness::PROGRAM.into(),
        key: harness::agent_key(agent),
        list: Some(list),
    }
}

fn fill(cx: &mut TestAppContext) {
    cx.simulate_input("create-system", "Answer about deploys.");
    cx.simulate_click("model-sonnet");
    cx.simulate_input("create-max-tokens", "8192");
}

fn rover() -> Config {
    Config {
        model: SONNET.into(),
        max_tokens: 8192,
        wait: None,
        thinking: None,
        system: "Answer about deploys.".into(),
    }
}

#[test]
fn the_create_flow_sends_its_ops_in_order() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    assert!(cx.has_text("1 serving now"), "{:?}", cx.texts());
    assert!(
        cx.has_text("anthropic · window 200,000 tokens"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    cx.simulate_click("create-answers-1");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(
            "/home/Eddy/agents/rover.md",
            &rover(),
            None,
            "Configure agent Rover (account 31)".into()
        )]
    );
    assert_eq!(
        cx.host().requests::<Submit<Identity>>(),
        [
            identity::Op::CreateAgent {
                name: "Rover".into()
            },
            identity::Op::SetScript {
                account: ROVER,
                script: Some(identity::Script {
                    calls: harness::PROGRAM.into(),
                    config: "/home/Eddy/agents/rover.md".into(),
                })
            },
        ]
    );
    assert_eq!(
        cx.host().requests::<Submit<access::Access>>(),
        [
            kept("/home/Eddy/agents/rover.md"),
            answering(ROVER, identity::Ownership::Anyone),
        ]
    );
    assert!(cx.host().requests::<Submit<harness::Harness>>().is_empty());
}

#[test]
fn the_create_flow_lists_accounts_and_programs_it_answers() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    cx.simulate_click("create-answers-2");
    cx.simulate_input("create-accounts", "7, forge");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let answered = cx.host().requests::<Submit<access::Access>>().pop();
    assert_eq!(
        answered,
        Some(answering(
            ROVER,
            identity::Ownership::Listed(vec![
                identity::Party::Account(7),
                identity::Party::Program("forge".into()),
            ])
        ))
    );
}

#[test]
fn a_path_typed_in_the_form_is_the_one_committed_and_scripted() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    cx.simulate_input("create-name", "Rover");
    cx.simulate_input("create-path", "/home/Eddy/rover.md");
    fill(&mut cx);
    cx.simulate_input("create-wait", "600");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let waits = Config {
        wait: Some(600),
        ..rover()
    };
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(
            "/home/Eddy/rover.md",
            &waits,
            None,
            "Configure agent Rover (account 31)".into()
        )]
    );
    assert_eq!(
        cx.host().requests::<Submit<Identity>>().last(),
        Some(&identity::Op::SetScript {
            account: ROVER,
            script: Some(identity::Script {
                calls: harness::PROGRAM.into(),
                config: "/home/Eddy/rover.md".into(),
            })
        })
    );
    assert_eq!(
        cx.host().requests::<Submit<access::Access>>(),
        [kept("/home/Eddy/rover.md")]
    );
}

#[test]
fn a_refused_commit_resumes_without_a_second_account() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    cx.host()
        .refuse::<Submit<files::Files>>("stale", "the file changed since the base snapshot");
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert!(
        cx.has_text("That didn’t go through: the file changed since the base snapshot"),
        "{:?}",
        cx.texts()
    );
    files_take(&cx);
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let created = cx
        .host()
        .requests::<Submit<Identity>>()
        .into_iter()
        .filter(|op| matches!(op, identity::Op::CreateAgent { .. }))
        .count();
    assert_eq!(created, 1);
    assert_eq!(cx.host().requests::<Submit<files::Files>>().len(), 2);
    assert!(
        cx.host()
            .requests::<Submit<Identity>>()
            .iter()
            .any(|op| matches!(op, identity::Op::SetScript { .. }))
    );
}

#[test]
fn a_create_with_no_answer_says_the_account_may_exist() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.host().handle::<Submit<Identity>>(|_| {
        Ok(Submitted::InDoubt {
            frame: [0; 32],
            until: 0,
        })
    });
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert!(
        cx.has_text(
            "This may have gone through: no answer came back. What you see is read again from the node. Look for the account before you create another."
        ),
        "{:?}",
        cx.texts()
    );
    assert_eq!(cx.host().requests::<Submit<Identity>>().len(), 1);
}

#[test]
fn an_unscripted_agent_continues_at_its_configuration() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click(&format!("unscripted-{PAGER}"));
    cx.run_until_parked();
    assert!(cx.has_text("Account 19 created"), "{:?}", cx.texts());
    fill(&mut cx);
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<Submit<files::Files>>().len(), 1);
    assert_eq!(
        cx.host().requests::<Submit<Identity>>(),
        [identity::Op::SetScript {
            account: PAGER,
            script: Some(identity::Script {
                calls: harness::PROGRAM.into(),
                config: "/home/Eddy/agents/pager.md".into(),
            })
        }]
    );
}

#[test]
fn edit_commits_a_new_version_of_the_file_and_scripts_only_a_moved_path() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    cx.simulate_input("create-max-tokens", "1024");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let smaller = Config {
        max_tokens: 1024,
        ..config_of("Scout")
    };
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(
            "/home/Eddy/agents/scout.md",
            &smaller,
            Some(SNAPSHOT),
            "Configure agent Scout (account 12)".into()
        )]
    );
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());
    assert!(cx.host().requests::<Submit<access::Access>>().is_empty());
    assert!(
        cx.has_text("/home/Eddy/agents/scout.md"),
        "{:?}",
        cx.texts()
    );

    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    cx.simulate_input("create-path", "/home/Eddy/scout.md");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>().last(),
        Some(&edit_of(
            "/home/Eddy/scout.md",
            &config_of("Scout"),
            Some(SNAPSHOT),
            "Configure agent Scout (account 12)".into()
        )),
    );
    assert_eq!(
        cx.host().requests::<Submit<access::Access>>(),
        [kept("/home/Eddy/scout.md")],
        "a moved file gets its own list"
    );
    assert_eq!(
        cx.host().requests::<Submit<Identity>>(),
        [identity::Op::SetScript {
            account: SCOUT,
            script: Some(identity::Script {
                calls: harness::PROGRAM.into(),
                config: "/home/Eddy/scout.md".into(),
            })
        }]
    );
}

#[test]
fn editing_a_configuration_in_another_accounts_home_shows_the_refusal_files_returns() {
    let mut net = Net::default();
    net.setups[0].path = path_of(ADA, "Scout");
    let (mut cx, _) = seated(net, EDDY);
    let refusal = "/home/Ada/agents/scout.md is in /home/Ada/, the home of the account named Ada: account 7 is named Eddy and writes under /home/Eddy/ and /shared/";
    cx.host()
        .refuse::<Submit<files::Files>>("unauthorized", refusal);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    cx.simulate_input("create-max-tokens", "1024");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let smaller = Config {
        max_tokens: 1024,
        ..config_of("Scout")
    };
    assert_eq!(
        cx.host().requests::<Submit<files::Files>>(),
        [edit_of(
            "/home/Ada/agents/scout.md",
            &smaller,
            Some(SNAPSHOT),
            "Configure agent Scout (account 12)".into()
        )]
    );
    assert!(
        cx.has_text(&format!("That didn’t go through: {refusal}")),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());
}

#[test]
fn with_no_name_read_for_the_reader_no_home_is_proposed_and_a_typed_path_is_asked_for() {
    let mut net = Net::default();
    net.accounts.retain(|account| account.number != EDDY);
    let (mut cx, _) = seated(net, EDDY);
    identity_takes(&cx);
    cx.simulate_click("agents-create");
    cx.run_until_parked();
    cx.simulate_input("create-name", "Rover");
    fill(&mut cx);
    cx.simulate_click("create-next");
    cx.run_until_parked();
    assert!(
        cx.has_text("Your account's name is not read yet, so no home is proposed: type the configuration file's path."),
        "{:?}",
        cx.texts()
    );
    assert!(cx.host().requests::<Submit<Identity>>().is_empty());
}

#[test]
fn the_agent_shows_its_configuration_model_threads_and_usage() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    for text in [
        "account 12 · agent · managed by Eddy",
        "/home/Eddy/agents/scout.md",
        "snapshot abababababab",
        "sonnet",
        "Anthropic Messages",
        "anthropic · 1 serving",
        "claude-sonnet-5-5",
        "200,000 tokens",
        "4,096 tokens",
        "egress's default",
        "Its manager only",
        "Who may change it",
        "Only Eddy",
        "2 · 1 write as Scout",
        "You are Scout.",
        "began in #general",
        "began in #release",
        "Closed",
        "input 1,204 · output 312 · cache read 38,410 · cache write 0 · all 39,926",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn an_agent_that_answers_more_than_its_manager_names_whom() {
    let net = Net {
        answers: vec![(
            SCOUT,
            identity::Ownership::Listed(vec![
                identity::Party::Account(ADA),
                identity::Party::Program("forge".into()),
            ]),
        )],
        ..Net::default()
    };
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(
        cx.has_text("Its manager, Ada, the forge program"),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn an_agent_whose_configuration_harness_cannot_run_says_what_is_wrong() {
    let mut net = Net::default();
    net.setups[0].configured = Configured::Malformed {
        snapshot: SNAPSHOT,
        malformed: harness::Malformed::NoClosingFence,
    };
    let (mut cx, _) = seated(net, EDDY);
    let problem = "the agent's configuration /home/Eddy/agents/scout.md is malformed: its front matter has no closing +++ line";
    assert!(cx.has_text("Not configured"), "{:?}", cx.texts());
    assert!(cx.has_text(problem), "{:?}", cx.texts());
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text(problem), "{:?}", cx.texts());
    assert!(
        cx.has_text("No system prompt is read until the file is fixed."),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("thread-7");
    cx.run_until_parked();
    assert!(cx.has_text(problem), "{:?}", cx.texts());
    assert!(
        cx.has_text(
            "thread 7 · opened at block 8,960 · began in #general · asks no model until its configuration is fixed"
        ),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn the_models_page_lists_the_records_and_sets_one_after_a_yes() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    cx.simulate_click("agents-models");
    cx.run_until_parked();
    for text in [
        "Models",
        "sonnet",
        "Anthropic Messages",
        "anthropic · 1 serving now",
        "window 200,000 tokens",
        "owner Ada",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    cx.simulate_input("model-id", "opus");
    cx.simulate_click("service-anthropic");
    cx.simulate_input("model-name", "claude-opus-5-5");
    cx.simulate_click("model-next");
    cx.run_until_parked();
    assert!(cx.has_text("Set model · opus"), "{:?}", cx.texts());
    assert!(cx.host().requests::<Submit<harness::Harness>>().is_empty());
    cx.simulate_click("model-set");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::SetModel {
            id: "opus".into(),
            wire: Wire::AnthropicMessages,
            service: "anthropic".into(),
            name: "claude-opus-5-5".into(),
        }]
    );
}

#[test]
fn a_work_link_opens_its_thread() {
    let mut cx = seated(Net::default(), EDDY).0;
    cx.host()
        .stream::<HostRoute>()
        .send("12/chat/general".into());
    cx.run_until_parked();
    assert!(cx.has_text("Conversation 7"), "{:?}", cx.texts());
    assert!(
        cx.has_text("Reading the deploy log first."),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.host()
            .requests::<Query<harness::Harness>>()
            .contains(&harness::Query::ThreadAt {
                agent: SCOUT,
                place: "chat/general".into()
            })
    );
    cx.host()
        .stream::<HostRoute>()
        .send("12/chat/nowhere".into());
    cx.run_until_parked();
    assert!(
        cx.has_text("Agent 12 has no thread at chat/nowhere yet."),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("You are Scout."), "{:?}", cx.texts());
}

/// Each screen of the boards, on the fake network: the stuck and no-node cases change it.
fn screens() -> Vec<(&'static str, TestAppContext)> {
    let open = |net: Net, clicks: &[&str]| {
        let (mut cx, _) = seated(net, EDDY);
        identity_takes(&cx);
        for click in clicks {
            cx.simulate_click(click);
            cx.run_until_parked();
        }
        cx
    };
    let mut stuck = Net::default();
    stuck.threads[0].held = 1;
    stuck.held = vec![(
        7,
        Body::Post {
            author: Principal::Account(ADA),
            text: "Is the rollback done?".into(),
        },
    )];
    let unserved = Net {
        serving: vec![(egress::RELAY.into(), 1)],
        ..Net::default()
    };
    let mut create = open(Net::default(), &["agents-create"]);
    create.simulate_input("create-name", "Rover");
    fill(&mut create);
    create.run_until_parked();
    vec![
        ("list", open(Net::default(), &[])),
        ("agent", open(Net::default(), &["agent-12"])),
        (
            "thread",
            open(
                Net {
                    withheld: vec![(
                        7,
                        WithheldFrame {
                            seq: 3,
                            size: 48_200,
                        },
                    )],
                    ..Net::default()
                },
                &["agent-12", "thread-7", "call-toolu_01", "result-3"],
            ),
        ),
        ("stuck", open(stuck, &["agent-12", "thread-7"])),
        ("no-node", open(unserved, &["agent-12"])),
        ("create", create),
        ("models", open(Net::default(), &["agents-models"])),
        (
            "streaming",
            open(
                Net {
                    partials: vec![(7, streaming())],
                    ..Net::default()
                },
                &["agent-12", "thread-7"],
            ),
        ),
    ]
}

/// `HARNESS_SCREEN_EXPORT=1` writes each screen's frame for the app's node-less renderer
/// (`ducktape-app --render-tree <json>`), light and dark.
#[test]
fn export_harness_screens() {
    if std::env::var_os("HARNESS_SCREEN_EXPORT").is_none() {
        return;
    }
    let out =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/harness-screens");
    std::fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for (index, (screen, mut cx)) in screens().into_iter().enumerate() {
        for dark in [false, true] {
            if dark {
                cx.set_global(view::Theme::dark());
            }
            let theme = if dark { "dark" } else { "light" };
            let name = format!("{:02}-{screen}-{theme}", index + 1);
            std::fs::write(
                out.join(format!("{name}.json")),
                serde_json::to_vec(&cx.whole_frame()).unwrap(),
            )
            .unwrap();
            manifest.push(serde_json::json!({
                "name": name,
                "theme": theme,
                "width": 1180,
                "height": 760,
            }));
        }
    }
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
}
