//! crosspond on a MockChain beside valset, the awaits, scheduler and egress roles, a board that
//! fills the `chat` role and a parker that waits on egress, and another network built from core's
//! state, block and certificate code: the headers, packet, ack and low-water proofs it hands over.
use std::collections::BTreeMap;

use ::program::{
    AccountNumber, Awaiting, Ctx, Error, ExecCtx, MockChain, MockHost, Origin, Principal, Program,
    QueryCtx, Roles, Scheme, code,
};
use ::state::{Commitment, Slot};
use abi::Root;
use abi::role::egress::Answer;
use abi::role::identity::{Kind, Profile};
use abi::role::{self, scheduler as scheduled};
use borsh::{BorshDeserialize, BorshSerialize};
use commonware_codec::Encode as _;
use commonware_consensus::simplex::scheme::ed25519::Scheme as Simplex;
use commonware_consensus::simplex::types::{Finalization, Finalize, Proposal};
use commonware_consensus::types::{Epoch as Round_epoch, Round, View};
use commonware_cryptography::certificate::Signers;
use commonware_cryptography::ed25519::certificate::Certificate;
use commonware_cryptography::{Digestible as _, Hasher as _, Sha256, Signer as _, ed25519};
use commonware_runtime::{Runner as _, Supervisor as _, deterministic};
use commonware_storage::merkle::Location;
use commonware_utils::NZU64;
use commonware_utils::ordered::Set;
use node::{Block, Settled as Sealed};
use store::Item;
use valset::{Genesis, Member, Valset};

use crate::*;

mod acks;
mod deliver;
mod described;
mod founding;
mod header;
mod links;
mod low;
mod pump;
mod send;
mod tags;
mod who;

pub(super) const HERE: NetworkId = [0x11; 32];
pub(super) const THERE: NetworkId = [0x22; 32];
pub(super) const ELSEWHERE: NetworkId = [0x33; 32];
pub(super) const THERE_NAME: &str = "there";
pub(super) const EPOCH_LENGTH: u64 = 4;
pub(super) const PORT: &str = "chat";
pub(super) const BOARD: &str = "board";
pub(super) const PARK: &str = "park";
pub(super) const PARKER: &str = "parker";
pub(super) const VALIDATORS: &str = "valset";
pub(super) const ADA: [u8; 32] = [1; 32];
pub(super) const BO: [u8; 32] = [2; 32];
pub(super) const USER: &[u8] = b"user-key";
pub(super) const STRANGER: &[u8] = b"stranger-key";
pub(super) const ALICE: AccountNumber = 7;
pub(super) const SENDER: AccountNumber = 18;
pub(super) const CROSSPOND_ACCOUNT: AccountNumber = 900;
pub(super) const BOARD_ACCOUNT: AccountNumber = 901;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub(super) enum Note {
    Post(String),
    Refuse,
}

pub(super) type Posts = Vec<(Option<Principal>, String)>;

const POSTS: Item<Posts> = Item::new("posts");

pub(super) struct Board;

impl Program for Board {
    const NAME: &'static str = BOARD;
    type Op = Note;
    type Query = ();
    type Reply = Posts;

    fn execute(ctx: &ExecCtx, note: Note) -> Result<(), Error> {
        let text = match note {
            Note::Post(text) => text,
            Note::Refuse => return Err(::program::invalid("the board refuses this note")),
        };
        let mut posts = POSTS.get(ctx)?.unwrap_or_default();
        posts.push((ctx.env().sender.clone(), text.clone()));
        POSTS.put(ctx, &posts);
        ctx.set_return_data(text.into_bytes());
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<Posts, Error> {
        Ok(POSTS.get(ctx)?.unwrap_or_default())
    }
}

pub(super) struct Parker;

impl Program for Parker {
    const NAME: &'static str = PARKER;
    type Op = ();
    type Query = ();
    type Reply = ();

    fn query(_ctx: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

impl Awaiting for Parker {
    async fn execute(ctx: Ctx, (): ()) -> Result<(), Error> {
        let request = egress::Op::Request {
            request: egress::Request::Crosspond { link: ELSEWHERE },
            delivery: egress::Delivery::Whole,
            wait: None,
        };
        let egress = ctx.env().role(role::EGRESS)?.clone();
        ctx.call_raw(egress, abi::encode(&request)).await?;
        Ok(())
    }
}

fn verify(scheme: Scheme, key: &[u8], namespace: &[u8], message: &[u8], signature: &[u8]) -> bool {
    scheme == Scheme::Ed25519
        && keyscheme::KeyScheme::Ed25519.verify(key, namespace, message, signature)
}

pub(super) fn roles() -> Roles {
    MockHost::roles()
        .iter()
        .map(|(role, program)| (role.to_owned(), program.clone()))
        .chain([
            (role::CROSSPOND.to_owned(), PROGRAM.to_owned()),
            (PORT.to_owned(), BOARD.to_owned()),
            (PARK.to_owned(), PARKER.to_owned()),
        ])
        .collect()
}

pub(super) fn chain_with(roles: Roles) -> MockChain {
    let mut chain = MockChain::founded(roles);
    chain.seat::<Valset>(VALIDATORS);
    chain.seat::<awaits::Awaits>(awaits::PROGRAM);
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.seat_awaiting::<egress::Egress>(egress::PROGRAM);
    let crosspond = chain.seat_awaiting::<Crosspond>(PROGRAM);
    crosspond.borrow_mut().verifier = Some(Box::new(verify));
    chain.seat::<Board>(BOARD);
    chain.seat_awaiting::<Parker>(PARKER);
    let members = [ADA, BO].map(|key| Member {
        key: key.to_vec(),
        address: None,
        halo: "node.example.org:9100".into(),
    });
    let genesis = Genesis {
        validators: members.to_vec(),
        member_cap: 8,
        queries: abi::role::validators::QueryPolicy::Signed,
    };
    chain.init(VALIDATORS, &genesis).unwrap();
    chain.register(PROGRAM, CROSSPOND_ACCOUNT);
    chain.register(BOARD, BOARD_ACCOUNT);
    chain.hold(USER, ALICE);
    chain.profile(Profile {
        number: ALICE,
        name: "alice".into(),
        kind: Kind::User,
    });
    chain.init(PROGRAM, &HERE).unwrap();
    chain
}

pub(super) fn chain() -> MockChain {
    chain_with(roles())
}

pub(super) fn vote(chain: &MockChain, op: &Op) -> Result<Vec<u8>, Error> {
    chain.submit(Origin::Program(VALIDATORS.into()), PROGRAM, op)
}

pub(super) fn submit(chain: &MockChain, key: &[u8], op: &Op) -> Result<Vec<u8>, Error> {
    chain.submit(Origin::Signed(key.to_vec()), PROGRAM, op)
}

pub(super) fn code_of(refused: Result<Vec<u8>, Error>) -> String {
    refused.expect_err("the op was refused").code
}

pub(super) fn ask<R: BorshDeserialize>(
    chain: &MockChain,
    query: &Query,
    pick: impl FnOnce(Reply) -> Option<R>,
) -> R {
    let reply: Reply = chain.query(PROGRAM, query).unwrap();
    pick(reply).expect("crosspond answered another reply")
}

pub(super) fn link_of(chain: &MockChain, network: NetworkId) -> Option<Link> {
    ask(chain, &Query::Link { network }, |reply| match reply {
        Reply::Link(link) => Some(link),
        _ => None,
    })
}

pub(super) fn status_of(chain: &MockChain, to: NetworkId, seq: u64) -> Option<Status> {
    ask(chain, &Query::Status { to, seq }, |reply| match reply {
        Reply::Status(status) => Some(status),
        _ => None,
    })
}

pub(super) fn outbound(chain: &MockChain, to: NetworkId, after: u64) -> Vec<Packet> {
    ask(chain, &Query::Outbound { to, after }, |reply| match reply {
        Reply::Outbound(packets) => Some(packets),
        _ => None,
    })
}

pub(super) fn receipts(chain: &MockChain, from: NetworkId, after: u64) -> Vec<Receipt> {
    ask(
        chain,
        &Query::Receipts { from, after },
        |reply| match reply {
            Reply::Receipts(receipts) => Some(receipts),
            _ => None,
        },
    )
}

pub(super) fn posts(chain: &MockChain) -> Posts {
    chain.query(BOARD, &()).unwrap()
}

pub(super) fn seat_keys(seeds: &[u64]) -> Vec<Vec<u8>> {
    seeds
        .iter()
        .map(|seed| key(*seed).public_key().encode().to_vec())
        .collect()
}

pub(super) fn open(chain: &MockChain, trusted: &[u64]) {
    let open = Op::Open {
        network: THERE,
        name: THERE_NAME.into(),
        epoch_length: EPOCH_LENGTH,
        epoch: 0,
        kind: CertificateKind::Ed25519SimplexFinalize,
        validators: seat_keys(trusted),
    };
    vote(chain, &open).unwrap();
}

pub(super) fn open_port(chain: &MockChain, role: &str, from: Vec<NetworkId>) {
    let port = Op::Port {
        role: role.into(),
        from,
    };
    vote(chain, &port).unwrap();
}

fn key(seed: u64) -> ed25519::PrivateKey {
    ed25519::PrivateKey::from_seed(seed)
}

fn set_of(seeds: &[u64]) -> Set<ed25519::PublicKey> {
    Set::try_from(
        seeds
            .iter()
            .map(|seed| key(*seed).public_key())
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

pub(super) fn packet(seq: u64, note: &Note) -> Packet {
    Packet {
        source: THERE,
        destination: HERE,
        seq,
        from: SENDER,
        name: "chief".into(),
        via: None,
        port: PORT.into(),
        op: abi::encode(note),
        sent: 1_000,
        deadline: None,
    }
}

pub(super) fn post(seq: u64) -> Packet {
    packet(seq, &Note::Post(format!("post {seq}")))
}

pub(super) struct Epoch {
    pub(super) validators: Vec<u64>,
    pub(super) signers: Vec<u64>,
    pub(super) packets: Vec<Packet>,
    pub(super) settled: bool,
    pub(super) acks: Vec<Settled>,
    pub(super) lows: Vec<(NetworkId, u64)>,
}

impl Epoch {
    pub(super) fn seated(validators: &[u64], packets: Vec<Packet>) -> Epoch {
        Epoch {
            validators: validators.to_vec(),
            signers: validators.to_vec(),
            packets,
            settled: true,
            acks: Vec::new(),
            lows: Vec::new(),
        }
    }
}

pub(super) struct Built {
    pub(super) header: Header,
    pub(super) proven: Vec<Proven>,
    pub(super) acks: Vec<ProvenAck>,
    pub(super) lows: Vec<ProvenLow>,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::hash(&[bytes]).0
}

async fn location_of(
    commitment: &Commitment<deterministic::Context>,
    key: &[u8],
    value: &[u8],
) -> u64 {
    let db = commitment.db().unwrap();
    let wanted = state_proof::update(key, value).unwrap();
    let bounds = db.bounds();
    for location in *bounds.start..*bounds.end {
        let (_, operations) = db.proof(Location::new(location), NZU64!(1)).await.unwrap();
        if operations[0].encode().as_ref() == wanted.as_slice() {
            return location;
        }
    }
    panic!("the state crate committed no such write")
}

async fn proof_of(
    commitment: &Commitment<deterministic::Context>,
    key: &[u8],
    value: &[u8],
) -> StateProof {
    let location = location_of(commitment, key, value).await;
    let db = commitment.db().unwrap();
    let (proof, _) = db.proof(Location::new(location), NZU64!(1)).await.unwrap();
    StateProof {
        location,
        proof: proof.encode().to_vec(),
    }
}

fn certificate(epoch: u64, block: &Block, validators: &[u64], signers: &[u64]) -> Vec<u8> {
    let set = set_of(validators);
    let proposal = Proposal::new(
        Round::new(Round_epoch::new(epoch), View::new(epoch * 8 + 1)),
        View::new(epoch * 8),
        block.digest(),
    );
    let mut votes: Vec<Finalize<Simplex, node::Digest>> = signers
        .iter()
        .map(|seed| {
            let roster = consensus::Roster::new(THERE_NAME.as_bytes().to_vec(), Some(key(*seed)));
            roster.seat(epoch, set.clone());
            let scheme = roster.scheme(epoch).expect("the signer is seated");
            Finalize::sign(&scheme, proposal.clone()).expect("a seated signer signs")
        })
        .collect();
    votes.sort_by_key(|vote| vote.attestation.signer);
    let seats = u32::try_from(set.len()).unwrap();
    let signers = Signers::new(seats, votes.iter().map(|vote| vote.attestation.signer)).unwrap();
    let signatures = votes
        .into_iter()
        .map(|vote| vote.attestation.signature)
        .collect();
    let finalization: consensus::Certificate = Finalization {
        proposal,
        certificate: Certificate {
            signers,
            signatures,
        },
    };
    finalization.encode().to_vec()
}

pub(super) fn there(epochs: Vec<Epoch>) -> Vec<Built> {
    deterministic::Runner::default().start(|context| async move {
        let mut network = Commitment::open(context.child("network"), "there-network", 8)
            .await
            .unwrap();
        let mut crosspond = Commitment::open(context.child("crosspond"), "there-crosspond", 8)
            .await
            .unwrap();
        let mut built = Vec::new();
        for (number, epoch) in (1u64..).zip(epochs) {
            let first = number * EPOCH_LENGTH;
            let close = first - 1;
            let seated = abi::encode(&seat_keys(&epoch.validators));
            let validators_key = format!("validators/{number}").into_bytes();
            let network_writes: BTreeMap<Vec<u8>, Slot> =
                [(validators_key.clone(), Some(seated.clone()))].into();
            network.apply(close, &network_writes).await.unwrap();
            let packet_writes = epoch.packets.iter().map(|packet| {
                let key = commitment_key(&packet.destination, packet.seq);
                (key, Some(abi::encode(packet)))
            });
            let ack_writes = epoch.acks.iter().map(|settled| {
                let key = settled_key(&settled.source, settled.seq);
                (key, Some(abi::encode(settled)))
            });
            let low_writes = epoch
                .lows
                .iter()
                .map(|(destination, n)| (low_key(destination, *n), Some(Vec::new())));
            let crosspond_writes: BTreeMap<Vec<u8>, Slot> =
                packet_writes.chain(ack_writes).chain(low_writes).collect();
            crosspond.apply(close, &crosspond_writes).await.unwrap();
            let roots: Vec<(String, Root)> = vec![
                ("$network".into(), network.root().unwrap()),
                ("crosspond".into(), crosspond.root().unwrap()),
            ];
            let mut block = Block {
                height: first,
                parent: Sha256::hash(&[b"the block before".as_slice()]),
                time: 1_000 + first,
                frames: vec![b"a frame".to_vec()],
            };
            if epoch.settled {
                block.settle(Sealed {
                    height: close,
                    root: sha256(&abi::encode(&roots)),
                });
            }
            let header = Header {
                kind: CertificateKind::Ed25519SimplexFinalize,
                block: block.encode().to_vec(),
                certificate: certificate(number, &block, &epoch.validators, &epoch.signers),
                roots,
                validators: seat_keys(&epoch.validators),
                validators_proof: proof_of(&network, &validators_key, &seated).await,
            };
            let mut proven = Vec::new();
            for packet in epoch.packets {
                let key = commitment_key(&packet.destination, packet.seq);
                let proof = proof_of(&crosspond, &key, &abi::encode(&packet)).await;
                proven.push(Proven { packet, proof });
            }
            let mut acks = Vec::new();
            for settled in epoch.acks {
                let key = settled_key(&settled.source, settled.seq);
                let proof = proof_of(&crosspond, &key, &abi::encode(&settled)).await;
                acks.push(ProvenAck { settled, proof });
            }
            let mut lows = Vec::new();
            for (destination, n) in epoch.lows {
                let proof = proof_of(&crosspond, &low_key(&destination, n), &[]).await;
                lows.push(ProvenLow { n, proof });
            }
            built.push(Built {
                header,
                proven,
                acks,
                lows,
            });
        }
        built
    })
}

pub(super) fn header(chain: &MockChain, built: &Built) -> Result<Vec<u8>, Error> {
    let op = Op::Header {
        from: THERE,
        header: built.header.clone(),
    };
    submit(chain, STRANGER, &op)
}

pub(super) fn deliver(
    chain: &MockChain,
    epoch: u64,
    packets: Vec<Proven>,
) -> Result<Vec<u8>, Error> {
    deliver_low(chain, epoch, None, packets)
}

pub(super) fn deliver_low(
    chain: &MockChain,
    epoch: u64,
    low: Option<ProvenLow>,
    packets: Vec<Proven>,
) -> Result<Vec<u8>, Error> {
    let op = Op::DeliverProven {
        from: THERE,
        epoch,
        low,
        packets,
    };
    submit(chain, STRANGER, &op)
}

pub(super) fn settled_of(packet: &Packet, result: Result<Vec<u8>, abi::Refusal>) -> Settled {
    Settled {
        source: packet.source,
        destination: packet.destination,
        seq: packet.seq,
        packet: sha256(&abi::encode(packet)),
        result,
    }
}

pub(super) fn acked(epoch: u64, header: Option<&Header>, acks: Vec<ProvenAck>) -> Answer {
    let answer = Acked {
        epoch,
        header: header.cloned(),
        acks,
    };
    Answer::Output(abi::encode(&answer))
}

pub(super) fn raw(chain: &MockChain, key: &[u8]) -> Option<Vec<u8>> {
    chain.host(PROGRAM).borrow().state.get(key).cloned()
}

pub(super) fn settled_row(chain: &MockChain, from: NetworkId, seq: u64) -> Option<Settled> {
    raw(chain, &settled_key(&from, seq)).map(|bytes| abi::decode(&bytes).unwrap())
}

pub(super) fn serve(chain: &MockChain, node: [u8; 32]) {
    let serve = egress::Op::Serve {
        services: vec![egress::CROSSPOND.into()],
    };
    chain
        .submit(Origin::Signed(node.to_vec()), egress::PROGRAM, &serve)
        .unwrap();
}

pub(super) fn assigned(chain: &MockChain, node: [u8; 32]) -> Vec<egress::RequestRow> {
    let asked = egress::Query::Assigned { key: node.to_vec() };
    match chain.query(egress::PROGRAM, &asked).unwrap() {
        egress::Reply::Requests(rows) => rows,
        _ => panic!("egress answered another reply"),
    }
}

pub(super) fn answer(
    chain: &MockChain,
    node: [u8; 32],
    id: u64,
    answer: Answer,
) -> Result<Vec<u8>, Error> {
    let op = egress::Op::Answer { id, answer };
    chain.submit(Origin::Signed(node.to_vec()), egress::PROGRAM, &op)
}

pub(super) fn answer_next(chain: &MockChain, answer_of: Answer) -> Result<Vec<u8>, Error> {
    let rows = assigned(chain, ADA);
    let [row] = &rows[..] else {
        panic!("one request waits on egress: {rows:?}");
    };
    let height = chain.env(PROGRAM).height;
    chain.at(height + 1, 0);
    answer(chain, ADA, row.id, answer_of)
}

pub(super) fn pumps_due(chain: &MockChain, height: u64) -> usize {
    let asked = scheduled::Query::Due {
        height,
        after: None,
        limit: None,
    };
    let pump = abi::encode(&Op::Pump { to: THERE });
    match chain.query(::scheduler::PROGRAM, &asked).unwrap() {
        scheduled::Reply::Due { entries, .. } => entries
            .iter()
            .filter(|entry| entry.target == PROGRAM && entry.payload == pump)
            .count(),
        _ => panic!("the scheduler answered another reply"),
    }
}
