use ::program::{Held, MockChain, Origin, Ownership, Program as _, Table, code};

use crate::state::CHANNELS;
use crate::{Op, PostPolicy, Principal};

const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    chain.register(crate::PROGRAM, 900);
    for (key, number) in [(ADA, 1), (BO, 2)] {
        chain.hold(key.to_vec(), number);
    }
    let create = Op::CreateChannel {
        channel_id: "general".into(),
        name: "general".into(),
        post_policy: PostPolicy::Open,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &create).unwrap();
    chain
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn run(chain: &MockChain, key: &[u8], op: &Op) -> Result<(), String> {
    chain
        .submit(signed(key), crate::PROGRAM, op)
        .map(drop)
        .map_err(|refusal| refusal.code)
}

fn record(chain: &MockChain) -> Option<ownership::Record> {
    let query = ownership::Query::Record {
        program: crate::PROGRAM.into(),
        key: CHANNELS.key(&"general".to_owned()),
    };
    let ownership::Reply::Record(record) = chain.query(ownership::PROGRAM, &query).unwrap() else {
        panic!("Record answers a record");
    };
    record
}

// every op that reshapes `general`, by whoever sends it
fn reshapes(n: u64) -> Vec<Op> {
    let channel_id = || "general".to_string();
    vec![
        Op::RenameChannel {
            channel_id: channel_id(),
            name: format!("general-{n}"),
        },
        Op::SetChannelMeta {
            channel_id: channel_id(),
            key: "category".into(),
            value: format!("c{n}"),
        },
        Op::RemoveChannelMeta {
            channel_id: channel_id(),
            key: "category".into(),
        },
        Op::SetMembership {
            channel_id: channel_id(),
            principal: Principal::Account(3),
            member: n.is_multiple_of(2),
        },
        Op::SetChannelArchived {
            channel_id: channel_id(),
            archived: true,
        },
        Op::SetChannelArchived {
            channel_id: channel_id(),
            archived: false,
        },
    ]
}

#[test]
fn chat_declares_its_channels_owned_and_every_other_table_shared() {
    let owned: Vec<&Table> = crate::Chat::TABLES
        .iter()
        .filter(|table| table.held != Held::Shared)
        .collect();
    assert_eq!(owned, [&CHANNELS.table()]);
    assert_eq!(owned[0].held, Held::Owned(Some("chat.channel")));
}

#[test]
fn a_channels_policy_decides_every_op_that_reshapes_it_and_posting_stays_open() {
    let chain = chain();
    let owned = record(&chain).expect("the guard recorded the channel");
    assert_eq!(owned.owner, ownership::Party::Account(1));
    assert_eq!(owned.kind.as_deref(), Some("chat.channel"));
    // the access list lets anyone reshape it; the channel's own policy is what narrows it now
    let anyone = access::Op::Set {
        program: crate::PROGRAM.into(),
        key: "general".into(),
        list: Some(Ownership::Anyone),
    };
    chain.submit(signed(ADA), "access", &anyone).unwrap();
    for op in reshapes(1) {
        assert_eq!(run(&chain, BO, &op), Ok(()), "{op:?}");
    }
    let strict = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(signed(ADA), ownership::PROGRAM, &strict)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let relink = ownership::Op::Relink {
        program: crate::PROGRAM.into(),
        key: CHANNELS.key(&"general".to_owned()),
        policy,
    };
    chain
        .submit(signed(ADA), ownership::PROGRAM, &relink)
        .unwrap();
    // the meta to remove, put back by its owner
    let meta = Op::SetChannelMeta {
        channel_id: "general".into(),
        key: "category".into(),
        value: "c".into(),
    };
    for op in reshapes(2) {
        run(&chain, ADA, &meta).unwrap();
        assert_eq!(
            run(&chain, BO, &op),
            Err(code::UNAUTHORIZED.to_string()),
            "{op:?}"
        );
    }
    for op in reshapes(3) {
        assert_eq!(run(&chain, ADA, &op), Ok(()), "{op:?}");
    }
    let post = Op::PostMessage {
        channel_id: "general".into(),
        message_id: "m1".into(),
        blocks: crate::parse_message("still open"),
        thread: None,
    };
    assert_eq!(run(&chain, BO, &post), Ok(()));
}
