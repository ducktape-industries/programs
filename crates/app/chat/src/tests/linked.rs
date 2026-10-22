//! A channel linked to its peer on another network, on a `MockChain` beside crosspond, access
//! and attribution: what an account here writes there is one packet, what arrives from the peer
//! is sent nowhere, and the peer's post reaches whoever listens here.
use ::program::{Error, Foreign, MockChain, MockHost, NetworkId, Origin, Roles, code};
use abi::role::crosspond::{self as role, Packet};
use abi::role::identity::{Kind, Profile};
use attribution::{Actor, Attribution, PageRequest as Paged, Reason};

use crate::{
    Block, Mark, MsgRow, Op, Peer, PeerMessage, PeerWrite, PostPolicy, Principal, Query, Reply,
    copy_id,
};

pub(super) const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";
const HERE: NetworkId = [0x11; 32];
pub(super) const THERE: NetworkId = [0x22; 32];
pub(super) const CHIEF: Foreign = Foreign {
    network: THERE,
    account: 18,
};
const DEV: Foreign = Foreign {
    network: THERE,
    account: 19,
};

fn roles() -> Roles {
    MockHost::roles()
        .iter()
        .map(|(role, program)| (role.to_owned(), program.clone()))
        .chain([
            (
                abi::role::CROSSPOND.to_owned(),
                crosspond::PROGRAM.to_owned(),
            ),
            (crate::ROLE.to_owned(), crate::PROGRAM.to_owned()),
        ])
        .collect()
}

pub(super) fn chain() -> MockChain {
    let mut chain = MockChain::founded(roles());
    chain.seat::<awaits::Awaits>(awaits::PROGRAM);
    chain.seat::<scheduler::Scheduler>(scheduler::PROGRAM);
    chain.seat_awaiting::<crosspond::Crosspond>(crosspond::PROGRAM);
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat::<Attribution>(attribution::PROGRAM);
    chain.register(crosspond::PROGRAM, 900);
    chain.register(crate::PROGRAM, 901);
    chain.register(attribution::PROGRAM, 902);
    for (key, number, name) in [(ADA, 1, "Ada"), (BO, 2, "Bo")] {
        chain.hold(key.to_vec(), number);
        chain.profile(Profile {
            number,
            name: name.into(),
            kind: Kind::User,
        });
    }
    chain.init(crosspond::PROGRAM, &HERE).unwrap();
    let open = role::Op::Open {
        network: THERE,
        name: "there".into(),
        epoch_length: 4,
        epoch: 0,
        kind: role::CertificateKind::Ed25519SimplexFinalize,
        validators: vec![vec![1; 32]],
    };
    let validators = MockHost::role(abi::role::VALIDATORS);
    chain
        .submit(Origin::Program(validators), crosspond::PROGRAM, &open)
        .unwrap();
    for id in ["general", "quiet"] {
        let create = Op::CreateChannel {
            channel_id: id.into(),
            name: id.into(),
            post_policy: PostPolicy::Open,
        };
        chain.submit(signed(ADA), crate::PROGRAM, &create).unwrap();
    }
    chain
        .submit(signed(ADA), crate::PROGRAM, &link("general", THERE))
        .unwrap();
    chain
}

pub(super) fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

pub(super) fn code_of(result: Result<Vec<u8>, Error>) -> String {
    result.expect_err("the op was refused").code
}

fn link(channel: &str, network: NetworkId) -> Op {
    Op::Link {
        channel_id: channel.into(),
        peer: Some(Peer {
            network,
            channel_id: "lobby".into(),
        }),
    }
}

pub(super) fn post(channel: &str, id: &str, text: &str, thread: Option<u64>) -> Op {
    Op::PostMessage {
        channel_id: channel.into(),
        message_id: id.into(),
        blocks: crate::parse_message(text),
        thread,
    }
}

fn from_peer(write: PeerWrite) -> Op {
    Op::FromPeer {
        channel_id: "general".into(),
        write,
    }
}

pub(super) fn peer_post(id: &str, text: &str, thread: Option<PeerMessage>) -> Op {
    from_peer(PeerWrite::Post {
        message_id: id.into(),
        blocks: crate::parse_message(text),
        thread,
    })
}

fn outbound(chain: &MockChain) -> Vec<Packet> {
    let asked = role::Query::Outbound {
        to: THERE,
        after: 0,
    };
    let role::Reply::Outbound(packets) = chain.query(crosspond::PROGRAM, &asked).unwrap() else {
        panic!("Outbound answers packets");
    };
    packets
}

fn sent(packet: &Packet) -> (String, PeerWrite) {
    assert_eq!(
        (packet.via.as_deref(), packet.port.as_str()),
        (Some(crate::ROLE), crate::ROLE)
    );
    let Op::FromPeer { channel_id, write } = abi::decode(&packet.op).unwrap() else {
        panic!("a linked channel sends FromPeer");
    };
    (channel_id, write)
}

pub(super) fn by_id(chain: &MockChain, id: &str) -> MsgRow {
    let asked = Query::MessageById {
        message_id: id.into(),
    };
    let Reply::Message(Some(row)) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!("chat holds no message {id}");
    };
    row
}

fn react(seq: u64) -> Op {
    Op::AddReaction {
        channel_id: "general".into(),
        seq,
        emoji: "👍".into(),
    }
}

#[test]
fn a_local_post_in_a_linked_channel_sends_one_packet_and_a_foreign_post_sends_none() {
    let chain = chain();
    let hi = post("general", "a1", "hi @bo and @18@there", None);
    chain.submit(signed(ADA), crate::PROGRAM, &hi).unwrap();
    let packets = outbound(&chain);
    let [packet] = packets.as_slice() else {
        panic!("one post sends one packet, not {}", packets.len());
    };
    assert_eq!((packet.from, packet.name.as_str()), (1, "Ada"));
    let (channel, write) = sent(packet);
    assert_eq!(channel, "lobby");
    let PeerWrite::Post {
        message_id,
        blocks,
        thread: None,
    } = write
    else {
        panic!("a root post is sent as a post");
    };
    assert_eq!(message_id, "a1");
    let [Block::Paragraph(spans)] = blocks.as_slice() else {
        panic!("one paragraph");
    };
    let mentioned: Vec<(String, Vec<Mark>)> = (spans.iter())
        .filter(|span| !span.marks.is_empty())
        .map(|span| (span.text.clone(), span.marks.clone()))
        .collect();
    let bo_there = Principal::Foreign(Foreign {
        network: HERE,
        account: 2,
    });
    assert_eq!(
        mentioned,
        [
            ("@2@net".to_owned(), vec![Mark::Mention(bo_there)]),
            (
                "<@18>".to_owned(),
                vec![Mark::Mention(Principal::Account(18))]
            ),
        ]
    );

    chain
        .submit_foreign(
            CHIEF,
            crate::PROGRAM,
            &peer_post("b1", "from the lobby", None),
        )
        .unwrap();
    let liked = from_peer(PeerWrite::React {
        message: PeerMessage::Here("a1".into()),
        emoji: "👍".into(),
        on: true,
    });
    chain.submit_foreign(CHIEF, crate::PROGRAM, &liked).unwrap();
    assert_eq!(
        outbound(&chain).len(),
        1,
        "what came from there is not sent on"
    );

    let elsewhere = post("quiet", "q1", "an unlinked room", None);
    chain
        .submit(signed(BO), crate::PROGRAM, &elsewhere)
        .unwrap();
    assert_eq!(outbound(&chain).len(), 1);
}

#[test]
fn edits_deletes_reactions_and_replies_name_each_message_where_it_began() {
    let chain = chain();
    chain
        .submit(
            signed(ADA),
            crate::PROGRAM,
            &post("general", "a1", "mine", None),
        )
        .unwrap();
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &peer_post("b1", "theirs", None))
        .unwrap();
    let theirs = by_id(&chain, &copy_id(&THERE, "b1")).seq;
    let edit = Op::EditMessage {
        channel_id: "general".into(),
        seq: 1,
        blocks: crate::parse_message("mine, edited"),
        base_rev: Some(0),
    };
    chain.submit(signed(ADA), crate::PROGRAM, &edit).unwrap();
    chain
        .submit(signed(BO), crate::PROGRAM, &react(theirs))
        .unwrap();
    let reply = post("general", "a2", "a reply", Some(theirs));
    chain.submit(signed(BO), crate::PROGRAM, &reply).unwrap();
    let delete = Op::DeleteMessage {
        channel_id: "general".into(),
        seq: 1,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &delete).unwrap();
    let writes: Vec<PeerWrite> = (outbound(&chain).iter())
        .map(|packet| sent(packet).1)
        .collect();
    let there = |id: &str| PeerMessage::There(id.into());
    let here = |id: &str| PeerMessage::Here(id.into());
    assert_eq!(
        writes,
        [
            PeerWrite::Post {
                message_id: "a1".into(),
                blocks: crate::parse_message("mine"),
                thread: None,
            },
            PeerWrite::Edit {
                message: there("a1"),
                blocks: crate::parse_message("mine, edited"),
                base_rev: Some(0),
            },
            PeerWrite::React {
                message: here("b1"),
                emoji: "👍".into(),
                on: true,
            },
            PeerWrite::Post {
                message_id: "a2".into(),
                blocks: crate::parse_message("a reply"),
                thread: Some(here("b1")),
            },
            PeerWrite::Delete {
                message: there("a1")
            },
        ]
    );
}

#[test]
fn a_write_from_the_peer_runs_as_its_author_on_the_message_it_names() {
    let chain = chain();
    chain
        .submit(
            signed(ADA),
            crate::PROGRAM,
            &post("general", "a1", "here first", None),
        )
        .unwrap();
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &peer_post("b1", "there first", None))
        .unwrap();
    let copy = by_id(&chain, &copy_id(&THERE, "b1"));
    assert_eq!(copy.author, Principal::Foreign(CHIEF));
    let reply = peer_post("b2", "a reply there", Some(PeerMessage::There("b1".into())));
    chain.submit_foreign(DEV, crate::PROGRAM, &reply).unwrap();
    assert_eq!(by_id(&chain, &copy_id(&THERE, "b2")).thread, Some(copy.seq));
    let on_ours = from_peer(PeerWrite::React {
        message: PeerMessage::Here("a1".into()),
        emoji: "🎉".into(),
        on: true,
    });
    chain.submit_foreign(DEV, crate::PROGRAM, &on_ours).unwrap();
    assert_eq!(by_id(&chain, "a1").reactions[0].emoji, "🎉");
    let edit = |text: &str| {
        from_peer(PeerWrite::Edit {
            message: PeerMessage::There("b1".into()),
            blocks: crate::parse_message(text),
            base_rev: None,
        })
    };
    let not_theirs = chain.submit_foreign(DEV, crate::PROGRAM, &edit("taken"));
    assert_eq!(code_of(not_theirs), code::UNAUTHORIZED);
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &edit("there first, edited"))
        .unwrap();
    assert_eq!(by_id(&chain, &copy.message_id).text, "there first, edited");
    let missing = peer_post("b3", "under nothing", Some(PeerMessage::There("b9".into())));
    let missing = chain.submit_foreign(CHIEF, crate::PROGRAM, &missing);
    assert_eq!(code_of(missing), code::NOT_FOUND);
    let local = chain.submit(signed(BO), crate::PROGRAM, &peer_post("b4", "forged", None));
    assert_eq!(code_of(local), code::UNAUTHORIZED);
    let named_by_hand = post("general", &copy_id(&THERE, "b5"), "squatting", None);
    let named_by_hand = chain.submit(signed(BO), crate::PROGRAM, &named_by_hand);
    assert_eq!(code_of(named_by_hand), code::UNAUTHORIZED);
    let delete = from_peer(PeerWrite::Delete {
        message: PeerMessage::There("b1".into()),
    });
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &delete)
        .unwrap();
    assert!(by_id(&chain, &copy.message_id).deleted);
}

#[test]
fn a_post_from_the_peer_reaches_whoever_listens_to_the_channel_here() {
    let chain = chain();
    let listen = Op::Listen {
        channel_id: "general".into(),
        listening: true,
    };
    chain.submit(signed(BO), crate::PROGRAM, &listen).unwrap();
    chain
        .submit_foreign(
            CHIEF,
            crate::PROGRAM,
            &peer_post("b1", "anyone here?", None),
        )
        .unwrap();
    let asked = attribution::Query::ChangesTo {
        recipient: 2,
        page: Paged::default(),
    };
    let attribution::Reply::Changes(page) = chain.query(attribution::PROGRAM, &asked).unwrap()
    else {
        panic!("ChangesTo answers changes");
    };
    let heard: Vec<(String, Reason, Actor)> = (page.items.into_iter())
        .map(|change| (change.source.object.id, change.reason, change.actor))
        .collect();
    assert_eq!(
        heard,
        [(
            copy_id(&THERE, "b1"),
            Reason::Defined(crate::attributed::LISTEN.into()),
            Actor::Foreign(CHIEF.into()),
        )]
    );
}

#[test]
fn only_whoever_may_rename_a_channel_links_it_and_only_to_an_open_link() {
    let chain = chain();
    let by_bo = chain.submit(signed(BO), crate::PROGRAM, &link("quiet", THERE));
    assert_eq!(code_of(by_bo), code::UNAUTHORIZED);
    let nowhere = chain.submit(signed(ADA), crate::PROGRAM, &link("quiet", [0x33; 32]));
    assert_eq!(code_of(nowhere), code::NOT_FOUND);
    let dm = Op::CreateDmChannel {
        counterpart: 2,
        name: "ada and bo".into(),
    };
    chain.submit(signed(ADA), crate::PROGRAM, &dm).unwrap();
    let direct = chain.submit(signed(ADA), crate::PROGRAM, &link("dm-1-2", THERE));
    assert_eq!(code_of(direct), code::UNAUTHORIZED);
    let peer = |channel: &str| {
        let asked = Query::Peer {
            channel_id: channel.into(),
        };
        let Reply::Peer(peer) = chain.query(crate::PROGRAM, &asked).unwrap() else {
            panic!("Peer answers a peer");
        };
        peer
    };
    assert_eq!(peer("quiet"), None);
    assert_eq!(peer("general").map(|peer| peer.network), Some(THERE));
    let validators = MockHost::role(abi::role::VALIDATORS);
    let close = role::Op::Close { network: THERE };
    chain
        .submit(Origin::Program(validators), crosspond::PROGRAM, &close)
        .unwrap();
    let closed = chain.submit(signed(ADA), crate::PROGRAM, &link("quiet", THERE));
    assert_eq!(code_of(closed), code::WRONG_STATE);
    let refused = chain.submit(
        signed(ADA),
        crate::PROGRAM,
        &post("general", "a1", "hi", None),
    );
    assert_eq!(code_of(refused), code::WRONG_STATE);
    let unlink = Op::Link {
        channel_id: "general".into(),
        peer: None,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &unlink).unwrap();
    assert_eq!(peer("general"), None);
    chain
        .submit(
            signed(ADA),
            crate::PROGRAM,
            &post("general", "a1", "hi", None),
        )
        .unwrap();
}

#[test]
fn a_write_from_another_network_lands_only_in_a_channel_linked_to_that_network() {
    let chain = chain();
    let into = |channel: &str, write: PeerWrite| Op::FromPeer {
        channel_id: channel.into(),
        write,
    };
    let post = |id: &str| PeerWrite::Post {
        message_id: id.into(),
        blocks: crate::parse_message("hello from there"),
        thread: None,
    };
    let notify = PeerWrite::Notify {
        message_id: "n1".into(),
        thread: None,
        notice: crate::Notice {
            kind: "pod".into(),
            group: "builds-1".into(),
            subject: "pod builds-1".into(),
            link: None,
            status: crate::message::Status::Failed,
            detail: "exit code 1".into(),
            concerns: Vec::new(),
        },
    };
    let unlinked = chain.submit_foreign(CHIEF, crate::PROGRAM, &into("quiet", post("q1")));
    assert_eq!(code_of(unlinked), code::UNAUTHORIZED);
    let notice = chain.submit_foreign(CHIEF, crate::PROGRAM, &into("quiet", notify.clone()));
    assert_eq!(code_of(notice), code::UNAUTHORIZED);
    let elsewhere = Foreign {
        network: [0x33; 32],
        account: 18,
    };
    let other = chain.submit_foreign(elsewhere, crate::PROGRAM, &into("general", post("g1")));
    assert_eq!(code_of(other), code::UNAUTHORIZED);
    let asked = |id: String| Query::MessageById { message_id: id };
    for id in [
        copy_id(&THERE, "q1"),
        copy_id(&THERE, "n1"),
        copy_id(&[0x33; 32], "g1"),
    ] {
        let held: Reply = chain.query(crate::PROGRAM, &asked(id.clone())).unwrap();
        assert_eq!(held, Reply::Message(None), "{id} was stored");
    }
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &into("general", post("g2")))
        .unwrap();
    let unlink = Op::Link {
        channel_id: "general".into(),
        peer: None,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &unlink).unwrap();
    let after = chain.submit_foreign(CHIEF, crate::PROGRAM, &into("general", post("g3")));
    assert_eq!(code_of(after), code::UNAUTHORIZED);
    let edit = PeerWrite::Edit {
        message: PeerMessage::There("g2".into()),
        blocks: crate::parse_message("changed after the unlink"),
        base_rev: None,
    };
    let edited = chain.submit_foreign(CHIEF, crate::PROGRAM, &into("general", edit));
    assert_eq!(code_of(edited), code::UNAUTHORIZED);
    assert_eq!(
        by_id(&chain, &copy_id(&THERE, "g2")).text,
        "hello from there"
    );
}
