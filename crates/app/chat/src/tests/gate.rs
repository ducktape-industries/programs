//! Chat on a `MockChain` beside the access program: a channel is its opener's to rename,
//! archive and seat, a message its author's to edit, and a channel's owner deletes any message
//! in it; a list the owner sets in the access program lets others do the same.
use ::program::{MockChain, Origin, Ownership, Party, code};

use crate::{Op, PostPolicy, message_key};

const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";
const CY: &[u8] = b"cy-key";

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.register(crate::PROGRAM, 900);
    chain.register("access", 901);
    for (key, number) in [(ADA, 1), (BO, 2), (CY, 3)] {
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

fn post(chain: &MockChain, key: &[u8], id: &str) {
    let op = Op::PostMessage {
        channel_id: "general".into(),
        message_id: id.into(),
        blocks: crate::parse_message("hello"),
        thread: None,
    };
    run(chain, key, &op).unwrap();
}

fn rename(name: &str) -> Op {
    Op::RenameChannel {
        channel_id: "general".into(),
        name: name.into(),
    }
}

fn set_meta(value: &str) -> Op {
    Op::SetChannelMeta {
        channel_id: "general".into(),
        key: "category".into(),
        value: value.into(),
    }
}

fn remove_meta() -> Op {
    Op::RemoveChannelMeta {
        channel_id: "general".into(),
        key: "category".into(),
    }
}

fn edit(seq: u64) -> Op {
    Op::EditMessage {
        channel_id: "general".into(),
        seq,
        blocks: crate::parse_message("changed"),
        base_rev: None,
    }
}

fn clear() -> Op {
    Op::Break {
        channel_id: "general".into(),
        thread: None,
    }
}

fn delete(seq: u64) -> Op {
    Op::DeleteMessage {
        channel_id: "general".into(),
        seq,
    }
}

fn list(chain: &MockChain, key: &[u8], at: String, list: Ownership) -> Result<(), String> {
    let set = access::Op::Set {
        program: crate::PROGRAM.into(),
        key: at,
        list: Some(list),
    };
    chain
        .submit(signed(key), "access", &set)
        .map(drop)
        .map_err(|refusal| refusal.code)
}

#[test]
fn a_channel_is_its_openers_to_rename_archive_and_seat_until_it_lists_others() {
    let chain = chain();
    let unauthorized = Err(code::UNAUTHORIZED.to_owned());
    assert_eq!(run(&chain, BO, &rename("mine")), unauthorized);
    let archive = Op::SetChannelArchived {
        channel_id: "general".into(),
        archived: true,
    };
    assert_eq!(run(&chain, BO, &archive), unauthorized);
    let seat = Op::SetMembership {
        channel_id: "general".into(),
        principal: crate::Principal::Account(3),
        member: true,
    };
    assert_eq!(run(&chain, BO, &seat), unauthorized);
    run(&chain, ADA, &rename("General")).unwrap();
    let bo = Ownership::Listed(vec![Party::Account(2)]);
    assert_eq!(list(&chain, BO, "general".into(), bo.clone()), unauthorized);
    list(&chain, ADA, "general".into(), bo).unwrap();
    run(&chain, BO, &rename("Bo's")).unwrap();
    run(&chain, BO, &seat).unwrap();
}

#[test]
fn a_channels_meta_is_set_and_removed_and_its_places_cleared_by_whoever_may_rename_it() {
    let chain = chain();
    let unauthorized = Err(code::UNAUTHORIZED.to_owned());
    for op in [rename("mine"), set_meta("Jobs"), remove_meta(), clear()] {
        assert_eq!(run(&chain, BO, &op), unauthorized, "{op:?}");
    }
    for op in [rename("General"), set_meta("Jobs"), remove_meta(), clear()] {
        run(&chain, ADA, &op).unwrap();
    }
    let bo = Ownership::Listed(vec![Party::Account(2)]);
    list(&chain, ADA, "general".into(), bo).unwrap();
    for op in [rename("Bo's"), set_meta("Teams"), remove_meta(), clear()] {
        run(&chain, BO, &op).unwrap();
        assert_eq!(run(&chain, CY, &op), unauthorized, "{op:?}");
    }
    let opened = Op::CreateChannelWithMeta {
        channel_id: "bo".into(),
        name: "bo".into(),
        post_policy: PostPolicy::Open,
        meta: [("category".to_owned(), "Jobs".to_owned())].into(),
    };
    run(&chain, CY, &opened).unwrap();
}

#[test]
fn a_channels_posting_is_set_by_whoever_may_rename_it() {
    let chain = chain();
    let unauthorized = Err(code::UNAUTHORIZED.to_owned());
    let posting = |post_policy| Op::SetPostPolicy {
        channel_id: "general".into(),
        post_policy,
    };
    let hello = |id: &str| Op::PostMessage {
        channel_id: "general".into(),
        message_id: id.into(),
        blocks: crate::parse_message("hello"),
        thread: None,
    };
    assert_eq!(
        run(&chain, BO, &posting(PostPolicy::MembersOnly)),
        unauthorized
    );
    run(&chain, ADA, &posting(PostPolicy::MembersOnly)).unwrap();
    assert_eq!(run(&chain, CY, &hello("m1")), unauthorized);
    let bo = Ownership::Listed(vec![Party::Account(2)]);
    list(&chain, ADA, "general".into(), bo).unwrap();
    run(&chain, BO, &posting(PostPolicy::Open)).unwrap();
    run(&chain, CY, &hello("m2")).unwrap();
}

#[test]
fn a_message_is_its_authors_and_the_channels_owner_deletes_it() {
    let chain = chain();
    post(&chain, BO, "m1");
    post(&chain, BO, "m2");
    let unauthorized = Err(code::UNAUTHORIZED.to_owned());
    assert_eq!(run(&chain, CY, &edit(1)), unauthorized);
    assert_eq!(
        run(&chain, ADA, &edit(1)),
        unauthorized,
        "the owner edits no one's words"
    );
    assert_eq!(run(&chain, CY, &delete(1)), unauthorized);
    run(&chain, BO, &edit(1)).unwrap();
    run(&chain, ADA, &delete(1)).unwrap();
    list(
        &chain,
        BO,
        message_key("general", 2),
        Ownership::Listed(vec![Party::Account(3)]),
    )
    .unwrap();
    run(&chain, CY, &edit(2)).unwrap();
}
