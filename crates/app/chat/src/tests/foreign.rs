//! A user of another network in chat's linked rooms, on a `MockChain` beside access and a crosspond
//! that answers from a fixed table: where they post, whose posts they edit, how `@name@network`
//! resolves and how chat names them.
use ::program::{
    Error, ExecCtx, Foreign, MockChain, MockHost, NetworkId, Origin, Program, QueryCtx, Roles, code,
};
use abi::role::crosspond as role;
use abi::role::identity::{Kind, Profile};

use crate::{
    Block, ForeignName, Mark, MsgRow, Op, PageRequest, Peer, PeerMessage, PeerWrite, PostPolicy,
    Principal, Query, Reply, copy_id,
};

const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";
const THERE: NetworkId = [0x22; 32];
const TWIN_A: NetworkId = [0x44; 32];
const TWIN_B: NetworkId = [0x55; 32];
const CHIEF: Foreign = Foreign {
    network: THERE,
    account: 18,
};
const DEV: Foreign = Foreign {
    network: THERE,
    account: 19,
};

const LINKS: &[(NetworkId, &str)] = &[(THERE, "there"), (TWIN_A, "pair"), (TWIN_B, "pair")];
const HEARD: &[(NetworkId, u64, &str)] = &[
    (THERE, 18, "chief"),
    (THERE, 20, "twin"),
    (THERE, 21, "twin"),
];

struct Heard;

impl Program for Heard {
    const NAME: &'static str = "crosspond";
    type Op = role::Op;
    type Query = role::Query;
    type Reply = role::Reply;

    fn execute(_: &ExecCtx, _: role::Op) -> Result<(), Error> {
        Ok(())
    }

    fn query(_: &QueryCtx, query: role::Query) -> Result<role::Reply, Error> {
        let named = |network: &NetworkId| {
            LINKS
                .iter()
                .find(|(id, _)| id == network)
                .map(|(_, name)| role::Link {
                    name: (*name).into(),
                    epoch_length: 4,
                    trusted: 0,
                    header: None,
                    next_in: 1,
                    next_out: 1,
                    refused: None,
                    closed: false,
                })
        };
        Ok(match query {
            role::Query::Link { network } => role::Reply::Link(named(&network)),
            role::Query::Linked { name } => role::Reply::Linked(
                (LINKS.iter())
                    .filter(|(_, held)| *held == name)
                    .map(|(id, _)| *id)
                    .collect(),
            ),
            role::Query::Name(foreign) => role::Reply::Name(
                (HEARD.iter())
                    .find(|(network, account, _)| {
                        (*network, *account) == (foreign.network, foreign.account)
                    })
                    .map(|(_, _, name)| (*name).into()),
            ),
            role::Query::Named { network, name } => role::Reply::Named(
                (HEARD.iter())
                    .filter(|(held, _, heard)| (*held, *heard) == (network, name.as_str()))
                    .map(|(_, account, _)| *account)
                    .collect(),
            ),
            role::Query::Home => role::Reply::Home([0x11; 32]),
            role::Query::Outbound { .. }
            | role::Query::Receipts { .. }
            | role::Query::Status { .. }
            | role::Query::Ports => return Err(Error::new(code::UNSUPPORTED, "not here")),
        })
    }
}

fn roles() -> Roles {
    MockHost::roles()
        .iter()
        .map(|(role, program)| (role.to_owned(), program.clone()))
        .chain([
            (abi::role::CROSSPOND.to_owned(), "crosspond".to_owned()),
            (crate::ROLE.to_owned(), crate::PROGRAM.to_owned()),
        ])
        .collect()
}

fn chain() -> MockChain {
    let mut chain = MockChain::founded(roles());
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat::<Heard>("crosspond");
    chain.register(crate::PROGRAM, 900);
    for (key, number, name) in [(ADA, 1, "Ada"), (BO, 2, "Bo")] {
        chain.hold(key.to_vec(), number);
        chain.profile(Profile {
            number,
            name: name.into(),
            kind: Kind::User,
        });
    }
    for (id, policy) in [
        ("general", PostPolicy::Open),
        ("council", PostPolicy::MembersOnly),
    ] {
        let create = Op::CreateChannel {
            channel_id: id.into(),
            name: id.into(),
            post_policy: policy,
        };
        chain.submit(signed(ADA), crate::PROGRAM, &create).unwrap();
        let link = Op::Link {
            channel_id: id.into(),
            peer: Some(Peer {
                network: THERE,
                channel_id: id.into(),
            }),
        };
        chain.submit(signed(ADA), crate::PROGRAM, &link).unwrap();
    }
    chain
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn code_of(result: Result<Vec<u8>, Error>) -> String {
    result.expect_err("the op was refused").code
}

fn post(channel: &str, id: &str, text: &str) -> Op {
    Op::PostMessage {
        channel_id: channel.into(),
        message_id: id.into(),
        blocks: crate::parse_message(text),
        thread: None,
    }
}

fn from_there(channel: &str, write: PeerWrite) -> Op {
    Op::FromPeer {
        channel_id: channel.into(),
        write,
    }
}

fn peer_post(channel: &str, id: &str, text: &str) -> Op {
    let write = PeerWrite::Post {
        message_id: id.into(),
        blocks: crate::parse_message(text),
        thread: None,
    };
    from_there(channel, write)
}

fn peer_edit(message: PeerMessage, text: &str) -> Op {
    let write = PeerWrite::Edit {
        message,
        blocks: crate::parse_message(text),
        base_rev: None,
    };
    from_there("general", write)
}

fn there(id: &str) -> String {
    copy_id(&THERE, id)
}

fn edit(channel: &str, seq: u64, text: &str) -> Op {
    Op::EditMessage {
        channel_id: channel.into(),
        seq,
        blocks: crate::parse_message(text),
        base_rev: None,
    }
}

fn by_id(chain: &MockChain, id: &str) -> MsgRow {
    let asked = Query::MessageById {
        message_id: id.into(),
    };
    let Reply::Message(Some(row)) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!("chat holds no message {id}");
    };
    row
}

fn foreigners(chain: &MockChain) -> Vec<ForeignName> {
    let asked = Query::Foreigners {
        page: PageRequest::default(),
    };
    let Reply::Foreigners(page) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!("Foreigners answers a page of names");
    };
    page.items
}

fn mentions(row: &MsgRow) -> Vec<(Principal, String)> {
    let Some(Block::Paragraph(spans)) = row.blocks.first() else {
        return Vec::new();
    };
    spans
        .iter()
        .filter_map(|span| {
            span.marks.iter().find_map(|mark| match mark {
                Mark::Mention(principal) => Some((principal.clone(), span.text.clone())),
                _ => None,
            })
        })
        .collect()
}

#[test]
fn a_foreign_user_posts_in_an_open_room_and_in_a_members_only_room_once_its_owner_seats_them() {
    let chain = chain();
    let posted = chain.submit_foreign(CHIEF, crate::PROGRAM, &peer_post("general", "c1", "hello"));
    posted.unwrap();
    assert_eq!(
        by_id(&chain, &there("c1")).author,
        Principal::Foreign(CHIEF)
    );
    let council = peer_post("council", "c2", "may I?");
    let refused = chain.submit_foreign(CHIEF, crate::PROGRAM, &council);
    assert_eq!(code_of(refused), code::UNAUTHORIZED);
    let seat = Op::SetMembership {
        channel_id: "council".into(),
        principal: Principal::Foreign(CHIEF),
        member: true,
    };
    let by_bo = chain.submit(signed(BO), crate::PROGRAM, &seat);
    assert_eq!(code_of(by_bo), code::UNAUTHORIZED);
    let by_their_own = chain.submit_foreign(CHIEF, crate::PROGRAM, &seat);
    assert_eq!(code_of(by_their_own), code::UNAUTHORIZED);
    chain.submit(signed(ADA), crate::PROGRAM, &seat).unwrap();
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &council)
        .unwrap();
    assert_eq!(
        by_id(&chain, &there("c2")).author,
        Principal::Foreign(CHIEF)
    );
    let still_out =
        chain.submit_foreign(DEV, crate::PROGRAM, &peer_post("council", "d1", "me too"));
    assert_eq!(code_of(still_out), code::UNAUTHORIZED);
    let members = Query::Members {
        channel_id: "council".into(),
        page: PageRequest::default(),
    };
    let Reply::Members(page) = chain.query(crate::PROGRAM, &members).unwrap() else {
        panic!("Members answers a page of members");
    };
    let seated: Vec<Principal> = page.items.into_iter().map(|row| row.principal).collect();
    assert_eq!(seated, [Principal::Foreign(CHIEF)]);
}

#[test]
fn a_foreign_author_edits_their_own_post_and_no_one_elses_and_the_rooms_owner_deletes_it() {
    let chain = chain();
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &peer_post("general", "c1", "hello"))
        .unwrap();
    chain
        .submit(signed(BO), crate::PROGRAM, &post("general", "b1", "hi"))
        .unwrap();
    let theirs = || PeerMessage::There("c1".into());
    chain
        .submit_foreign(CHIEF, crate::PROGRAM, &peer_edit(theirs(), "hello, edited"))
        .unwrap();
    assert_eq!(by_id(&chain, &there("c1")).text, "hello, edited");
    let bos = PeerMessage::Here("b1".into());
    let others = chain.submit_foreign(CHIEF, crate::PROGRAM, &peer_edit(bos, "mine now"));
    assert_eq!(code_of(others), code::UNAUTHORIZED);
    let namesake = chain.submit_foreign(DEV, crate::PROGRAM, &peer_edit(theirs(), "mine now"));
    assert_eq!(code_of(namesake), code::UNAUTHORIZED);
    let by_bo = chain.submit(signed(BO), crate::PROGRAM, &edit("general", 1, "mine now"));
    assert_eq!(code_of(by_bo), code::UNAUTHORIZED);
    assert_eq!(by_id(&chain, &there("c1")).text, "hello, edited");
    assert_eq!(by_id(&chain, "b1").text, "hi");
    let delete = Op::DeleteMessage {
        channel_id: "general".into(),
        seq: 1,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &delete).unwrap();
    assert!(by_id(&chain, &there("c1")).deleted);
}

#[test]
fn a_number_at_a_linked_network_resolves_and_a_name_there_only_when_one_account_holds_it() {
    let chain = chain();
    let text = "@18@there @chief@there @twin@there @nobody@there @18@pair @18@nowhere";
    chain
        .submit(signed(ADA), crate::PROGRAM, &post("general", "m1", text))
        .unwrap();
    let row = by_id(&chain, "m1");
    let chief = Principal::Foreign(CHIEF);
    assert_eq!(
        mentions(&row),
        [
            (chief.clone(), "@18@there".to_owned()),
            (chief, "@18@there".to_owned())
        ]
    );
    assert_eq!(
        row.text,
        "@18@there @18@there @twin@there @nobody@there @18@pair @18@nowhere"
    );
    let here = "@bo@net and @1@net, not @bo@there";
    chain
        .submit(signed(ADA), crate::PROGRAM, &post("general", "m2", here))
        .unwrap();
    let local = mentions(&by_id(&chain, "m2"));
    assert_eq!(
        local,
        [
            (Principal::Account(2), "<@2>".to_owned()),
            (Principal::Account(1), "<@1>".to_owned())
        ]
    );
}

#[test]
fn chat_names_each_foreign_author_member_and_mention_by_what_crosspond_heard() {
    let chain = chain();
    assert!(foreigners(&chain).is_empty());
    chain
        .submit(
            signed(ADA),
            crate::PROGRAM,
            &post("general", "m1", "hi @chief@there"),
        )
        .unwrap();
    chain
        .submit_foreign(DEV, crate::PROGRAM, &peer_post("general", "d1", "hello"))
        .unwrap();
    let names = foreigners(&chain);
    let handles: Vec<String> = names.iter().map(ForeignName::handle).collect();
    assert_eq!(handles, ["chief@there", "19@there"]);
    let unlinked = ForeignName {
        foreign: Foreign {
            network: [0xab; 32],
            account: 7,
        },
        name: None,
        network: None,
    };
    assert_eq!(unlinked.handle(), format!("7@{}", crate::hex(&[0xab; 32])));
}

#[test]
fn a_typed_name_at_a_linked_network_is_refused_in_an_unlinked_room_and_lands_in_a_linked_one() {
    let chain = chain();
    let create = Op::CreateChannel {
        channel_id: "quiet".into(),
        name: "quiet".into(),
        post_policy: PostPolicy::Open,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &create).unwrap();
    for text in ["ask @chief@there", "ask @18@there"] {
        let refused = chain.submit(signed(ADA), crate::PROGRAM, &post("quiet", "q1", text));
        assert_eq!(code_of(refused), code::UNAUTHORIZED, "{text}");
    }
    assert!(
        foreigners(&chain).is_empty(),
        "a refused mention records no one"
    );
    chain
        .submit(
            signed(ADA),
            crate::PROGRAM,
            &post("general", "g1", "ask @chief@there"),
        )
        .unwrap();
    let chief = Principal::Foreign(CHIEF);
    assert_eq!(
        mentions(&by_id(&chain, "g1")),
        [(chief, "@18@there".to_owned())]
    );
}
