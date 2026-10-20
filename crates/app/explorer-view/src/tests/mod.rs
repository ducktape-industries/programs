//! A node the explorer reads, played by the fake host: a short chain, three accounts, one
//! validator and `programs`, each answered as the program would. The modules split the tests by
//! what they cover.
mod access;
mod describe;
mod detail;
mod faces;
mod follow;
mod pages;
mod phone;
mod search;
mod transactions;
mod window;

pub(crate) use std::cell::RefCell;
pub(crate) use std::rc::Rc;

pub(crate) use crate::*;
pub(crate) use abi::BlobId;
pub(crate) use activity::Activity;
pub(crate) use identity::Identity;
pub(crate) use programs::Programs;
pub(crate) use valset::Valset;
pub(crate) use view::host::Error;
pub(crate) use view::methods::{
    BlobManifest, Block, BlockAsk, BlockList, BlockPage, BlockRef, Capability, CarriedView,
    ChainBlock, ChainBlocks, ChainHeads, ChainStatus, ChainTx, Changes, ClipboardWrite,
    Description, Head, HostRoute, HostSession, Located, Manifested, NodeStatus, Outcome,
    ProgramDescribe, Query, Receipt, Session, Tip, Tx, TxAsk, TxRef, Value,
};
pub(crate) use view::testing::TestAppContext;

pub(crate) const ADA: [u8; 32] = [1; 32];
pub(crate) const STRANGER: [u8; 32] = [2; 32];
pub(crate) const VALIDATOR: [u8; 32] = [9; 32];
pub(crate) const T0: u64 = 1_790_000_000_000;

pub(crate) fn post(channel: &str, text: &str) -> Vec<u8> {
    borsh::to_vec(&chat::Op::PostMessage {
        channel_id: channel.into(),
        message_id: "m1".into(),
        blocks: chat::parse_message(text),
        thread: None,
    })
    .unwrap()
}

pub(crate) fn tx(seed: u8, signer: [u8; 32], target: &str, payload: Vec<u8>) -> Tx {
    Tx {
        hash: [seed; 32],
        signer: signer.to_vec(),
        anchor: Tip {
            height: 10,
            id: [10; 32],
        },
        target: target.into(),
        size: payload.len() as u32,
        payload: Some(payload),
        receipt: None,
    }
}

pub(crate) fn receipt(
    program: &str,
    refused: Option<(&str, &str)>,
    nested: Vec<Receipt>,
) -> Receipt {
    let outcome = match refused {
        None => Outcome::Applied { output: Vec::new() },
        Some((code, message)) => Outcome::Rejected(Error {
            code: code.into(),
            message: message.into(),
        }),
    };
    Receipt {
        program: program.into(),
        outcome,
        events: Vec::new(),
        nested,
    }
}

pub(crate) fn chain(tip: u64) -> Vec<Block> {
    (0..=tip)
        .map(|height| Block {
            height,
            id: [(height as u8).wrapping_add(100); 32],
            parent: [(height as u8).wrapping_add(99); 32],
            time: T0 + height * 1000,
            epoch: height / 10,
            proposer: Some(VALIDATOR.to_vec()),
            txs: match height {
                11 => vec![
                    Tx {
                        receipt: Some(receipt(
                            "chat",
                            None,
                            vec![receipt(
                                "notify",
                                None,
                                vec![receipt(
                                    "mail",
                                    Some(("capacity", "the inbox is full")),
                                    Vec::new(),
                                )],
                            )],
                        )),
                        ..tx(0xa1, ADA, "chat", post("design", "hello there"))
                    },
                    tx(0xc3, ADA, "chat", post(&chat::dm_channel_id(7, 3), "ping")),
                ],
                12 => vec![Tx {
                    receipt: Some(receipt(
                        "mystery",
                        Some(("unauthorized", "not a member")),
                        Vec::new(),
                    )),
                    ..tx(0xb2, STRANGER, "mystery", vec![1, 2, 3, 4])
                }],
                _ => Vec::new(),
            },
        })
        .collect()
}

pub(crate) fn page(chain: &[Block], ask: &BlockPage) -> BlockList {
    let top = chain.len() as u64 - 1;
    let start = ask.before.map_or(Some(top), |before| before.checked_sub(1));
    let blocks = start.map_or_else(Vec::new, |start| {
        (0..=start.min(top))
            .rev()
            .take(ask.limit as usize)
            .map(|height| listed(&chain[height as usize], ask.most))
            .collect()
    });
    BlockList {
        blocks,
        listed_from: Some(0),
    }
}

pub(crate) fn listed(block: &Block, most: u32) -> Block {
    Block {
        txs: block.txs.iter().map(|tx| capped(tx, most)).collect(),
        ..block.clone()
    }
}

fn capped(tx: &Tx, most: u32) -> Tx {
    let within = tx.size <= most;
    Tx {
        payload: tx.payload.clone().filter(|_| within),
        ..tx.clone()
    }
}

fn located(chain: &[Block], ask: TxAsk) -> Option<Located> {
    chain.iter().find_map(|block| {
        let position = match ask.tx {
            TxRef::At { height, position } => (block.height == height).then_some(position)?,
            TxRef::Hash(hash) => block.txs.iter().position(|tx| tx.hash == hash)? as u32,
        };
        let tx = block.txs.get(position as usize)?;
        Some(Located {
            height: block.height,
            position,
            time: block.time,
            tx: capped(tx, ask.most),
        })
    })
}

pub(crate) fn serve(cx: &TestAppContext, chain: impl Fn() -> Vec<Block> + Clone + 'static) {
    let host = cx.host();
    let held = chain.clone();
    host.handle::<ChainBlocks>(move |ask| Ok(page(&held(), &ask)));
    let held = chain.clone();
    host.handle::<ChainBlock>(move |ask| {
        let held = held();
        let found = match ask.by {
            BlockRef::Height(height) => held.get(height as usize),
            BlockRef::Id(id) => held.iter().find(|block| block.id == id),
        };
        Ok(found.map(|block| listed(block, ask.most)))
    });
    host.handle::<ChainTx>(move |ask| Ok(located(&chain(), ask)));
}

pub(crate) fn name(cx: &TestAppContext, id: &str) -> Option<String> {
    match cx.find(id) {
        Some(view::wire::Node::Container(row)) => row
            .interactivity
            .as_ref()
            .unwrap()
            .aria
            .label
            .clone()
            .map(String::from),
        other => panic!("no row {id}: {other:?}"),
    }
}

pub(crate) fn status(height: u64) -> NodeStatus {
    NodeStatus {
        chain_id: "test#1".into(),
        block_time_ms: 1000,
        epoch_length: 10,
        height,
        epoch: (height + 1) / 10,
        ..NodeStatus::default()
    }
}

pub(crate) fn account(number: u64, name: &str, control: identity::Control) -> identity::Account {
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

pub(crate) fn ada() -> identity::Account {
    let keys = vec![identity::Key {
        scheme: abi::Scheme::Ed25519,
        key: ADA.to_vec(),
        label: Some("laptop".into()),
        added_at: 0,
    }];
    account(3, "Ada", identity::Control::User { keys })
}

pub(crate) fn scout() -> identity::Account {
    account(
        5,
        "Scout",
        identity::Control::Program {
            script: None,
            manager: 3,
            category: identity::Category::Agent,
            life: identity::Life::Suspended { keys: Vec::new() },
        },
    )
}

pub(crate) fn forge() -> identity::Account {
    account(
        6,
        "forge",
        identity::Control::System {
            program: "forge".into(),
        },
    )
}
pub(crate) fn holder(signer: &[u8]) -> Option<u64> {
    (signer == ADA).then_some(3)
}

fn whole<T>(items: Vec<T>) -> activity::PageResponse<T> {
    activity::PageResponse {
        height: 1,
        items,
        next: None,
    }
}

fn page_of(
    frames: impl Iterator<Item = activity::Frame>,
    asked: &activity::PageRequest,
) -> activity::PageResponse<activity::Frame> {
    let key = |frame: &activity::Frame| {
        let height = (u64::MAX - frame.height).to_be_bytes();
        let position = (u32::MAX - frame.position).to_be_bytes();
        [&height[..], &position[..]].concat()
    };
    let after = |frame: &activity::Frame| asked.after.as_ref().is_none_or(|at| key(frame) > *at);
    let mut frames = frames.filter(after);
    let items: Vec<activity::Frame> = frames.by_ref().take(asked.limit() as usize).collect();
    let next = frames.next().and(items.last()).map(key);
    activity::PageResponse {
        height: 1,
        items,
        next,
    }
}

pub(crate) fn index(cx: &TestAppContext, chain: impl Fn() -> Vec<Block> + 'static) {
    cx.host().handle::<Query<Activity>>(move |query| {
        let frames = chain().into_iter().rev().flat_map(|block| {
            let txs = block.txs.into_iter().enumerate().rev();
            txs.map(move |(position, tx)| {
                let frame = activity::Frame {
                    height: block.height,
                    position: position as u32,
                    time: block.time,
                    signer: tx.signer.clone(),
                    target: tx.target,
                    applied: tx
                        .receipt
                        .as_ref()
                        .is_none_or(|run| matches!(run.outcome, Outcome::Applied { .. })),
                };
                (holder(&tx.signer), frame)
            })
        });
        Ok(match query {
            activity::Query::Acted {
                account,
                page: asked,
            } => {
                let acted = frames
                    .filter(|(holder, _)| *holder == Some(account))
                    .map(|(_, frame)| frame);
                activity::Reply::Frames(page_of(acted, &asked))
            }
            activity::Query::Used { .. } => {
                let mut used: Vec<activity::Used> = Vec::new();
                for (holder, frame) in frames {
                    let Some(account) = holder else {
                        continue;
                    };
                    let counted = used
                        .iter_mut()
                        .find(|used| (used.account, &used.program) == (account, &frame.target));
                    match counted {
                        Some(used) => used.frames += 1,
                        None => used.push(activity::Used {
                            account,
                            program: frame.target,
                            frames: 1,
                        }),
                    }
                }
                used.sort_by(|a, b| (a.account, &a.program).cmp(&(b.account, &b.program)));
                activity::Reply::Used(whole(used))
            }
            other => panic!("unexpected activity query: {other:?}"),
        })
    });
}

pub(crate) fn node(cx: &mut TestAppContext, tip: Rc<RefCell<u64>>) {
    let host = cx.host();
    let head = tip.clone();
    host.handle::<ChainStatus>(move |()| Ok(status(*head.borrow())));
    let head = tip.clone();
    index(cx, move || chain(*head.borrow()));
    serve(cx, move || chain(*tip.borrow()));
    host.handle::<Query<Identity>>(|query| match query {
        identity::Query::List { .. } => Ok(identity::Reply::Accounts(programs::PageResponse {
            height: 1,
            items: vec![ada(), scout(), forge()],
            next: None,
        })),
        other => panic!("unexpected identity query: {other:?}"),
    });
    host.handle::<Query<Valset>>(|_| Ok(valset::Reply::Validators(vec![VALIDATOR.to_vec()])));
    respond(cx);
    describes(cx);
    manifests(cx);
}

pub(crate) fn manifests(cx: &TestAppContext) {
    cx.host().handle::<BlobManifest>(|id| {
        let manifest = |name: &str, capabilities: Vec<Capability>| view::wire::manifest::Manifest {
            wire_id: "0123abcd".into(),
            name: name.into(),
            description: format!("{name}, as the suite plays it."),
            capabilities,
            min_width: 480,
            targets: vec!["chat".into()],
            icon: "icons/chat.svg".into(),
            tint: Tint::Blue,
            width: None,
        };
        Ok(match id.as_str() {
            CHAT_CODE => Some(Manifested {
                size: 2_400_000,
                view: Some(CarriedView {
                    size: 1_900_000,
                    manifest: Some(manifest(
                        "Chat",
                        vec![
                            Capability::Chain,
                            Capability::Program,
                            Capability::Op,
                            Capability::Clipboard,
                            Capability::Notify,
                            Capability::Store,
                        ],
                    )),
                }),
            }),
            IDENTITY_CODE => Some(Manifested {
                size: 130_000,
                view: None,
            }),
            EXPLORER_CODE => Some(Manifested {
                size: 1_200_000,
                view: Some(CarriedView {
                    size: 1_200_000,
                    manifest: Some(manifest(
                        "Explorer",
                        vec![Capability::Chain, Capability::Clock],
                    )),
                }),
            }),
            _ => None,
        })
    });
}

pub(crate) const CHAT_CODE: &str =
    "sha256:abababababababababababababababababababababababababababababababab";
pub(crate) const IDENTITY_CODE: &str =
    "sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
pub(crate) const EXPLORER_CODE: &str =
    "sha256:efefefefefefefefefefefefefefefefefefefefefefefefefefefefefefefef";

pub(crate) const STAMP: &str = "4cd69803b2faa3996ce1ddadbbc9b1d6e7641133";

pub(crate) fn built(entries: &[programs::Entry]) -> Vec<programs::Built> {
    let source = |program: &str| match program {
        "chat" => programs::Provenance::Commit(programs::Source {
            repo: "programs".into(),
            commit: STAMP.into(),
        }),
        _ => programs::Provenance::Unstamped,
    };
    let built = entries.iter().map(|entry| programs::Built {
        program: entry.program.clone(),
        code: entry.code,
        source: source(&entry.program),
    });
    built.collect()
}

pub(crate) fn entry(program: &str, code: u8) -> programs::Entry {
    programs::Entry {
        program: program.into(),
        code: BlobId::Sha256([code; 32]),
        params: vec![1, 2, 3],
    }
}

pub(crate) fn respond(cx: &mut TestAppContext) {
    programs_with(
        cx,
        vec![entry("chat", 0xab), entry("identity", 0xcd)],
        vec![programs::View {
            name: "explorer".into(),
            view: BlobId::Sha256([0xef; 32]),
        }],
        vec![programs::Pending {
            height: 120,
            change: programs::Change::Remove("forge".into()),
            by: None,
        }],
    );
}

pub(crate) fn programs_with(
    cx: &TestAppContext,
    programs: Vec<programs::Entry>,
    views: Vec<programs::View>,
    scheduled: Vec<programs::Pending>,
) {
    programs_hooking(cx, programs, views, scheduled, Vec::new());
}

pub(crate) fn programs_hooking(
    cx: &TestAppContext,
    programs: Vec<programs::Entry>,
    views: Vec<programs::View>,
    scheduled: Vec<programs::Pending>,
    hooks: Vec<String>,
) {
    fn whole<T>(items: Vec<T>) -> programs::PageResponse<T> {
        programs::PageResponse {
            height: 1,
            items,
            next: None,
        }
    }
    cx.host().handle::<Query<Programs>>(move |query| {
        Ok(match query {
            programs::Query::Roster => programs::Reply::Roster {
                height: 1,
                programs: programs.clone(),
                views: views.clone(),
            },
            programs::Query::Scheduled { .. } => {
                programs::Reply::Scheduled(whole(scheduled.clone()))
            }
            programs::Query::Hooks(_) => programs::Reply::Hooks(hooks.clone()),
            programs::Query::Sources => programs::Reply::Sources {
                height: 1,
                programs: built(&programs),
            },
            programs::Query::Roles => programs::Reply::Roles(abi::Roles::from_iter([
                (abi::role::PROGRAMS, programs::PROGRAM),
                (abi::role::VALIDATORS, valset::PROGRAM),
                (abi::role::IDENTITY, identity::PROGRAM),
                (abi::role::BLOBS, "blobs"),
                (abi::role::AWAITS, "awaits"),
                (abi::role::EGRESS, "egress"),
                (abi::role::HALO, "halo"),
            ])),
            other => panic!("unexpected query: {other:?}"),
        })
    });
}

pub(crate) fn ready() -> (TestAppContext, Rc<RefCell<u64>>) {
    let mut cx = TestAppContext::new();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    cx.open::<Explorer>();
    cx.run_until_parked();
    (cx, tip)
}

pub(crate) fn describes(cx: &mut TestAppContext) {
    fn with<T: borsh::BorshDeserialize>(
        op: &[u8],
        describe: fn(&T) -> Description,
    ) -> Option<Description> {
        borsh::from_slice(op).ok().map(|op| describe(&op))
    }
    cx.host().handle::<ProgramDescribe>(|(program, op)| {
        Ok(match program.as_str() {
            "chat" => with(&op, chat::describe),
            "forge" => with(&op, forge::describe),
            "identity" => with(&op, identity::describe),
            "valset" => with(&op, valset::describe),
            programs::PROGRAM => with(&op, programs::describe),
            _ => None,
        })
    });
}

pub(crate) fn heavy(cx: &mut TestAppContext) {
    let tip = WINDOW as u64;
    let chain: Vec<Block> = (0..=tip)
        .map(|height| {
            let seed = (height % 250) as u8;
            let mut tx = if height == tip {
                let push = forge::Op::Push {
                    repo: "app".into(),
                    request: vec![0x50; 1 << 20],
                    pack: None,
                };
                tx(0xfe, ADA, forge::PROGRAM, borsh::to_vec(&push).unwrap())
            } else {
                let text = format!("message {height}");
                tx(seed, ADA, chat::PROGRAM, post("design", &text))
            };
            tx.hash[..8].copy_from_slice(&height.to_le_bytes());
            Block {
                height,
                id: [seed; 32],
                parent: [seed.wrapping_sub(1); 32],
                time: T0 + height * 1000,
                epoch: height / 10,
                proposer: Some(VALIDATOR.to_vec()),
                txs: vec![tx],
            }
        })
        .collect();
    let host = cx.host();
    host.handle::<ChainStatus>(move |()| Ok(status(tip)));
    let indexed = chain.clone();
    index(cx, move || indexed.clone());
    serve(cx, move || chain.clone());
    host.handle::<Query<Identity>>(|_| {
        Ok(identity::Reply::Accounts(programs::PageResponse {
            height: 1,
            items: vec![ada()],
            next: None,
        }))
    });
    host.handle::<Query<Valset>>(|_| Ok(valset::Reply::Validators(vec![VALIDATOR.to_vec()])));
    respond(cx);
    describes(cx);
    manifests(cx);
}

pub(crate) fn quiet_host(cx: &TestAppContext) {
    let host = cx.host();
    host.never::<ChainStatus>();
    host.never::<ChainBlocks>();
    host.never::<ChainTx>();
    host.never::<Query<Identity>>();
    host.never::<Query<Valset>>();
    host.never::<Query<Programs>>();
    host.never::<Query<Activity>>();
    host.never::<ProgramDescribe>();
    host.never::<BlobManifest>();
}
