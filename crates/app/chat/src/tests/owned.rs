use ::program::{MockChain, Origin, Ownership, code};

use crate::state::{CHANNELS, MESSAGE_IDS};
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
        Op::Break {
            channel_id: channel_id(),
            thread: None,
        },
    ]
}

#[test]
fn chat_claims_its_channels_and_messages() {
    assert_eq!(CHANNELS.kind(), Some("chat.channel"));
    assert_eq!(MESSAGE_IDS.kind(), Some("chat.message"));
}

#[test]
fn a_channels_policy_decides_every_op_that_reshapes_it_and_posting_stays_open() {
    let chain = chain();
    let owned = record(&chain).expect("the guard recorded the channel");
    assert_eq!(owned.owner, ownership::Party::Account(1));
    assert_eq!(owned.kind.as_deref(), Some("chat.channel"));
    let anyone = access::Op::Set {
        program: crate::PROGRAM.into(),
        key: "general".into(),
        list: Some(Ownership::Anyone),
    };
    chain.submit(signed(ADA), "access", &anyone).unwrap();
    let rename = &reshapes(1)[0];
    assert_eq!(
        run(&chain, BO, rename),
        Err(code::UNAUTHORIZED.to_string()),
        "only its owner until they open it: {rename:?}"
    );
    let open = ownership::Op::Create {
        name: "open".into(),
        write: ownership::Writers::Anyone,
    };
    let opened = chain
        .submit(signed(ADA), ownership::PROGRAM, &open)
        .unwrap();
    let relink_open = ownership::Op::Relink {
        program: crate::PROGRAM.into(),
        key: CHANNELS.key(&"general".to_owned()),
        policy: abi::decode(&opened).unwrap(),
    };
    chain
        .submit(signed(ADA), ownership::PROGRAM, &relink_open)
        .unwrap();
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

fn bo_posts(chain: &MockChain) -> u64 {
    let post = Op::PostMessage {
        channel_id: "general".into(),
        message_id: "bo-1".into(),
        blocks: crate::parse_message("bo's words"),
        thread: None,
    };
    run(chain, BO, &post).unwrap();
    1
}

fn strict(chain: &MockChain, key: &[u8]) -> ownership::PolicyId {
    let made = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(signed(key), ownership::PROGRAM, &made)
        .unwrap();
    abi::decode(&made).unwrap()
}

fn relink(chain: &MockChain, key: &[u8], at: Vec<u8>, policy: ownership::PolicyId) {
    let relink = ownership::Op::Relink {
        program: crate::PROGRAM.into(),
        key: at,
        policy,
    };
    chain
        .submit(signed(key), ownership::PROGRAM, &relink)
        .unwrap();
}

#[test]
fn a_messages_policy_decides_its_authors_edits_and_the_channels_decides_moderation() {
    let chain = chain();
    let seq = bo_posts(&chain);
    let id = MESSAGE_IDS.key(&"bo-1".to_owned());
    let query = ownership::Query::Record {
        program: crate::PROGRAM.into(),
        key: id.clone(),
    };
    let ownership::Reply::Record(Some(owned)) = chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("the guard recorded bo's message");
    };
    assert_eq!(owned.owner, ownership::Party::Account(2));
    assert_eq!(owned.kind.as_deref(), Some("chat.message"));
    let anyone = access::Op::Set {
        program: crate::PROGRAM.into(),
        key: crate::message_key("general", seq),
        list: Some(Ownership::Anyone),
    };
    chain.submit(signed(BO), "access", &anyone).unwrap();
    relink(&chain, BO, id, strict(&chain, BO));
    let edit = |text: &str| Op::EditMessage {
        channel_id: "general".into(),
        seq,
        blocks: crate::parse_message(text),
        base_rev: None,
    };
    assert_eq!(
        run(&chain, ADA, &edit("ada's words")),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(run(&chain, BO, &edit("bo's words, again")), Ok(()));
    let react = Op::AddReaction {
        channel_id: "general".into(),
        seq,
        emoji: "👍".into(),
    };
    assert_eq!(run(&chain, ADA, &react), Ok(()));
    let reply = Op::PostMessage {
        channel_id: "general".into(),
        message_id: "ada-1".into(),
        blocks: crate::parse_message("a reply"),
        thread: Some(seq),
    };
    assert_eq!(run(&chain, ADA, &reply), Ok(()));
    relink(
        &chain,
        ADA,
        CHANNELS.key(&"general".to_owned()),
        strict(&chain, ADA),
    );
    let delete = Op::DeleteMessage {
        channel_id: "general".into(),
        seq,
    };
    let moderated = run(&chain, ADA, &delete);
    assert_eq!(moderated, Ok(()));
    let ownership::Reply::Record(Some(kept)) = chain.query(ownership::PROGRAM, &query).unwrap()
    else {
        panic!("a moderated message keeps its author's record");
    };
    assert_eq!(kept.owner, ownership::Party::Account(2));
}

#[test]
fn anothers_delete_is_refused_where_the_channel_is_narrowed() {
    let chain = chain();
    let seq = bo_posts(&chain);
    let anyone = access::Op::Set {
        program: crate::PROGRAM.into(),
        key: crate::message_key("general", seq),
        list: Some(Ownership::Anyone),
    };
    chain.submit(signed(BO), "access", &anyone).unwrap();
    relink(
        &chain,
        ADA,
        CHANNELS.key(&"general".to_owned()),
        strict(&chain, ADA),
    );
    let delete = Op::DeleteMessage {
        channel_id: "general".into(),
        seq,
    };
    chain.hold(b"cy-key".to_vec(), 3);
    assert_eq!(
        run(&chain, b"cy-key", &delete),
        Err(code::UNAUTHORIZED.to_string())
    );
    assert_eq!(run(&chain, BO, &delete), Ok(()));
}

#[test]
fn adopt_owners_records_chats_rows_from_before_the_guard_as_access_enforces_them_today() {
    let mut chain = MockChain::default();
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.register(crate::PROGRAM, 900);
    chain.register("board", 950);
    for (key, number) in [(ADA, 1), (BO, 2)] {
        chain.hold(key.to_vec(), number);
    }
    for id in ["general", "j-1"] {
        let create = Op::CreateChannel {
            channel_id: id.into(),
            name: id.into(),
            post_policy: PostPolicy::Open,
        };
        run(&chain, ADA, &create).unwrap();
    }
    let board = access::Op::Set {
        program: crate::PROGRAM.into(),
        key: "j-1".into(),
        list: Some(Ownership::Listed(vec![::program::Party::Program(
            "board".into(),
        )])),
    };
    chain.submit(signed(ADA), "access", &board).unwrap();
    bo_posts(&chain);
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    for kind in ["chat.channel", "chat.message"] {
        let op = Op::AdoptOwners {
            kind: kind.into(),
            after: None,
            limit: 10,
        };
        let next = chain.submit(signed(BO), crate::PROGRAM, &op).unwrap();
        assert_eq!(abi::decode::<Option<Vec<u8>>>(&next).unwrap(), None);
    }
    let effective = |key: Vec<u8>| {
        let query = ownership::Query::Effective {
            program: crate::PROGRAM.into(),
            key,
        };
        let ownership::Reply::Policy(Some(policy)) =
            chain.query(ownership::PROGRAM, &query).unwrap()
        else {
            panic!("the row is recorded");
        };
        policy
    };
    let general = effective(CHANNELS.key(&"general".to_owned()));
    assert_eq!(
        (general.name.as_str(), &general.owner),
        (ownership::ONLY_ME, &ownership::Party::Account(1))
    );
    let listed = effective(CHANNELS.key(&"j-1".to_owned()));
    assert_eq!(listed.name, "chat j-1 (as before)");
    let program = ownership::Party::Program("board".into());
    assert_eq!(listed.write, ownership::Writers::Listed(vec![program]));
    let message = effective(MESSAGE_IDS.key(&"bo-1".to_owned()));
    assert_eq!(
        (message.name.as_str(), &message.owner),
        (ownership::ONLY_ME, &ownership::Party::Account(2))
    );
}

#[test]
fn adopt_owners_walks_on_by_itself_and_moves_what_the_guard_left_open_to_what_access_enforces() {
    let mut chain = MockChain::default();
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.seat::<scheduler::Scheduler>(scheduler::PROGRAM);
    chain.register(crate::PROGRAM, 900);
    for (key, number) in [(ADA, 1), (BO, 2)] {
        chain.hold(key.to_vec(), number);
    }
    let create = Op::CreateChannel {
        channel_id: "general".into(),
        name: "general".into(),
        post_policy: PostPolicy::Open,
    };
    run(&chain, ADA, &create).unwrap();
    let post = |id: &str| Op::PostMessage {
        channel_id: "general".into(),
        message_id: id.into(),
        blocks: crate::parse_message("bo's words"),
        thread: None,
    };
    run(&chain, BO, &post("bo-1")).unwrap();
    let open_to_anyone = |seq: u64| access::Op::Set {
        program: crate::PROGRAM.into(),
        key: crate::message_key("general", seq),
        list: Some(Ownership::Anyone),
    };
    chain
        .submit(signed(BO), "access", &open_to_anyone(1))
        .unwrap();
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    let adopt = |limit: u64| {
        let op = Op::AdoptOwners {
            kind: "chat.message".into(),
            after: None,
            limit,
        };
        chain.submit(signed(ADA), crate::PROGRAM, &op).unwrap();
    };
    adopt(10);
    let bo = ownership::Party::Account(2);
    let asked = ownership::Query::Open { owner: bo.clone() };
    let ownership::Reply::Open(Some(open)) = chain.query(ownership::PROGRAM, &asked).unwrap()
    else {
        panic!("bo-1, open to anyone, links to bo's open policy");
    };
    let default = |policy| ownership::Op::SetDefault {
        scope: ownership::Scope::Kind("chat.message".into()),
        policy,
    };
    chain
        .submit(signed(BO), ownership::PROGRAM, &default(Some(open)))
        .unwrap();
    for id in ["bo-2", "bo-3"] {
        run(&chain, BO, &post(id)).unwrap();
    }
    chain
        .submit(signed(BO), ownership::PROGRAM, &default(None))
        .unwrap();
    let policy = |id: &str| {
        let query = ownership::Query::Record {
            program: crate::PROGRAM.into(),
            key: MESSAGE_IDS.key(&id.to_owned()),
        };
        let ownership::Reply::Record(Some(record)) =
            chain.query(ownership::PROGRAM, &query).unwrap()
        else {
            panic!("{id} is recorded");
        };
        record.policy
    };
    let ids = ["bo-1", "bo-2", "bo-3"];
    assert_eq!(
        ids.map(policy),
        [open; 3],
        "as the guard's old fallback left them"
    );
    adopt(1);
    for height in 2..=6 {
        chain.at(height, height);
    }
    let asked = ownership::Query::OnlyMe { owner: bo };
    let ownership::Reply::OnlyMe(Some(only)) = chain.query(ownership::PROGRAM, &asked).unwrap()
    else {
        panic!("bo's messages moved to bo's Only me");
    };
    assert_eq!(ids.map(policy), [open, only, only]);
    for seq in [2, 3] {
        chain
            .submit(signed(BO), "access", &open_to_anyone(seq))
            .unwrap();
    }
    let edit = |seq: u64| Op::EditMessage {
        channel_id: "general".into(),
        seq,
        blocks: crate::parse_message("ada's words"),
        base_rev: None,
    };
    assert_eq!(
        run(&chain, ADA, &edit(2)),
        Err(code::UNAUTHORIZED.to_string()),
        "with access open, bo's own policy keeps ada out"
    );
    assert_eq!(
        run(&chain, ADA, &edit(1)),
        Ok(()),
        "a message access left open stays open"
    );
    assert_eq!(run(&chain, BO, &edit(3)), Ok(()));
    adopt(10);
    assert_eq!(
        ids.map(policy),
        [open, only, only],
        "a second walk changes nothing"
    );
}
