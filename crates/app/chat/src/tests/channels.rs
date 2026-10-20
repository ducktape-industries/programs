//! Channels: creating, dm rooms, renaming, archiving, membership, meta.
use std::collections::BTreeMap;

use super::*;
use crate::state::CHANNEL_META;
use crate::{ChannelInfo, dm_channel_id};

#[test]
fn a_channel_is_created_once_owned_by_its_creator() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let info = chat.ask_channel("general");
    assert_eq!((info.channel.owner, info.head_seq), (ADA, 0));
    let again = chat.refused(&BO, create("general", PostPolicy::Open));
    assert_eq!(again, code::ALREADY_EXISTS);
}

#[test]
fn a_channel_id_is_well_formed_and_a_colon_id_is_its_programs_alone() {
    let mut chat = Chat::default();
    for bad in ["", "a/b"] {
        assert_eq!(
            chat.refused(&ADA, create(bad, PostPolicy::Open)),
            code::INVALID_INPUT
        );
    }
    for who in [ADA, Principal::Account(901)] {
        let squat = chat.refused(&who, create("forge:repo:1", PostPolicy::Open));
        assert_eq!(squat, code::UNAUTHORIZED);
    }
    chat.ok(&FORGE, create("forge:repo:1", PostPolicy::Open));
    chat.ok(&Principal::Root, create("system:room", PostPolicy::Open));
    let blank = Op::CreateChannel {
        channel_id: "blank".into(),
        name: "  ".into(),
        post_policy: PostPolicy::Open,
    };
    assert_eq!(chat.refused(&ADA, blank), code::INVALID_INPUT);
}

#[test]
fn nobody_squats_a_dm_id_with_a_plain_create() {
    let dm = dm_channel_id(3, 5);
    for who in [7, 3, 5] {
        let mut chat = Chat::default();
        let who = Principal::Account(who);
        let op = create(&dm, PostPolicy::Open);
        assert_eq!(chat.refused(&who, op), code::UNAUTHORIZED, "{who:?}");
        assert!(chat.store.borrow().state.is_empty());
        chat.ok(&Principal::Account(3), open_dm(5));
        for peer in [3, 5] {
            assert!(is_member(&chat, &dm, Principal::Account(peer)));
        }
    }
}

#[test]
fn a_dm_seats_both_accounts_opens_once_and_keeps_others_out() {
    let mut chat = Chat::default();
    chat.ok(&ADA, open_dm(2));
    let dm = dm_channel_id(1, 2);
    let first = chat.store.borrow().state.clone();
    chat.ok(&BO, open_dm(1));
    assert_eq!(
        chat.store.borrow().state,
        first,
        "opening it again changes nothing"
    );
    chat.ok(&BO, post(&dm, "m1", "hi", None));
    assert_eq!(
        chat.refused(&CY, post(&dm, "m2", "me too", None)),
        code::UNAUTHORIZED
    );
    assert_eq!(chat.refused(&ADA, open_dm(1)), code::INVALID_INPUT);
    let program = Principal::Account(902);
    chat.ok(&program, open_dm(1));
    assert!(is_member(&chat, &dm_channel_id(1, 902), program));
}

#[test]
fn an_agent_opens_a_dm_through_the_program_that_runs_it() {
    let mut chat = Chat::default();
    chat.height += 1;
    let run_by_harness = chat.env(
        Origin::Program("harness".into()),
        Some(Principal::Account(AGENT)),
    );
    let ctx = chat.store.exec(run_by_harness);
    crate::Chat::execute(&ctx, open_dm(2)).unwrap();
    let dm = dm_channel_id(2, AGENT);
    assert!(is_member(&chat, &dm, Principal::Account(AGENT)));
    assert!(is_member(&chat, &dm, BO));
}

#[test]
fn a_dm_opens_to_any_account_that_exists() {
    let mut chat = Chat::default();
    for counterpart in [2, AGENT, 900, SUSPENDED, REVOKED] {
        chat.ok(&ADA, open_dm(counterpart));
    }
    assert_eq!(chat.refused(&ADA, open_dm(404)), code::INVALID_INPUT);
}

#[test]
fn a_dm_stays_open_once_its_counterpart_is_suspended() {
    let mut chat = Chat::default();
    chat.ok(&BO, open_dm(AGENT));
    let dm = dm_channel_id(2, AGENT);
    let mut roster = roster();
    for profile in &mut roster {
        if let crate::Kind::Program { standing, .. } = &mut profile.kind
            && profile.number == AGENT
        {
            *standing = crate::Standing::Suspended;
        }
    }
    chat.store.identity(roster);
    chat.ok(&CY, open_dm(AGENT));
    chat.ok(&BO, open_dm(AGENT));
    assert!(is_member(&chat, &dm, Principal::Account(AGENT)));
    chat.ok(&BO, post(&dm, "m1", "still here", None));
}

#[test]
fn neither_dm_peer_reshapes_the_room() {
    let mut chat = Chat::default();
    chat.ok(&ADA, open_dm(2));
    let dm = dm_channel_id(1, 2);
    let seat = |principal: Principal, member: bool| Op::SetMembership {
        channel_id: dm.clone(),
        principal,
        member,
    };
    for (who, principal, member) in [
        (&ADA, CY, true),
        (&ADA, BO, false),
        (&ADA, ADA, false),
        (&BO, CY, true),
        (&BO, ADA, false),
        (&CY, CY, true),
    ] {
        assert_eq!(
            chat.refused(who, seat(principal, member)),
            code::UNAUTHORIZED
        );
    }
    let rename = |name: &str| Op::RenameChannel {
        channel_id: dm.clone(),
        name: name.into(),
    };
    let archive = |archived| Op::SetChannelArchived {
        channel_id: dm.clone(),
        archived,
    };
    for who in [&ADA, &BO, &CY] {
        assert_eq!(chat.refused(who, rename("ours")), code::UNAUTHORIZED);
        assert_eq!(chat.refused(who, archive(true)), code::UNAUTHORIZED);
        assert_eq!(chat.refused(who, archive(false)), code::UNAUTHORIZED);
    }
    chat.ok(&BO, post(&dm, "m1", "still here", None));
    assert_eq!(
        chat.refused(&CY, post(&dm, "m2", "let me in", None)),
        code::UNAUTHORIZED
    );
}

#[test]
fn a_rename_takes_a_name_and_a_channel_that_exists() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, rename("General chat"));
    assert_eq!(chat.channel().name, "General chat");
    assert_eq!(chat.refused(&ADA, rename("")), code::INVALID_INPUT);
    let nowhere = Op::RenameChannel {
        channel_id: "nowhere".into(),
        name: "x".into(),
    };
    assert_eq!(chat.refused(&ADA, nowhere), code::NOT_FOUND);
}

#[test]
fn an_archived_channel_takes_no_writes_until_unarchived() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "hello", None);
    chat.ok(&ADA, archive(true));
    for op in [
        post("general", "m2", "late", None),
        react(1, "👍", true),
        edit(1, "changed"),
    ] {
        assert_eq!(chat.refused(&BO, op), code::WRONG_STATE);
    }
    chat.ok(&ADA, archive(false));
    chat.post(&BO, "m2", "back", None);
}

#[test]
fn members_are_seated_and_unseated() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    assert_eq!(
        chat.refused(&BO, post("general", "m1", "hi", None)),
        code::UNAUTHORIZED
    );
    chat.ok(&ADA, membership(BO, true));
    chat.post(&BO, "m1", "hi", None);
    let Reply::Members(members) = chat.ask(Query::Members {
        channel_id: "general".into(),
        page: PageRequest::default(),
    }) else {
        panic!("members answer members");
    };
    assert_eq!(members.items[0].principal, BO);
    chat.ok(&ADA, membership(BO, false));
    assert!(!is_member(&chat, "general", BO));
    assert_eq!(
        chat.refused(&BO, post("general", "m2", "hi", None)),
        code::UNAUTHORIZED
    );
}

#[test]
fn members_list_by_account_number_across_pages() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    for number in (1..=300).rev() {
        chat.ok(&ADA, membership(Principal::Account(number), true));
    }
    let mut listed = Vec::new();
    let mut after = None;
    loop {
        let Reply::Members(members) = chat.ask(Query::Members {
            channel_id: "general".into(),
            page: PageRequest { after, limit: None },
        }) else {
            panic!("members answer members");
        };
        listed.extend(members.items.into_iter().map(|row| row.principal));
        after = members.next;
        if after.is_none() {
            break;
        }
    }
    let numbers: Vec<Principal> = (1..=300).map(Principal::Account).collect();
    assert_eq!(listed, numbers);
}

#[test]
fn a_channel_keeps_whatever_meta_it_is_given_and_reads_it_back() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    assert!(meta_of(&chat, "general").is_empty(), "made without meta");
    chat.ok(&ADA, set_meta("category", "Jobs"));
    chat.ok(&ADA, set_meta("", "any key, any value"));
    chat.ok(&ADA, set_meta("category", "Teams"));
    let held = meta(&[("", "any key, any value"), ("category", "Teams")]);
    assert_eq!(meta_of(&chat, "general"), held);
    let Reply::Channels(listed) = chat.ask(Query::Channels {
        page: PageRequest::default(),
    }) else {
        panic!("channels answer channels");
    };
    assert_eq!(listed.items[0].meta, held);
    chat.ok(&ADA, remove_meta("category"));
    let left = meta(&[("", "any key, any value")]);
    assert_eq!(meta_of(&chat, "general"), left);
    chat.ok(&ADA, remove_meta(""));
    assert!(
        !CHANNEL_META.has(&chat.reads(), &"general".to_owned()),
        "the last key takes its row"
    );
    let before = chat.store.borrow().state.clone();
    chat.ok(&ADA, remove_meta("category"));
    assert_eq!(
        chat.store.borrow().state,
        before,
        "a key that is not there is no change"
    );
    chat.ok(&ADA, archive(true));
    chat.ok(&ADA, set_meta("category", "Old"));
    assert_eq!(meta_of(&chat, "general"), meta(&[("category", "Old")]));
}

#[test]
fn a_channel_created_with_meta_holds_it_and_passes_every_create_check() {
    let mut chat = Chat::default();
    let jobs = || meta(&[("category", "Jobs")]);
    chat.ok(&ADA, create_with_meta("j-020", jobs()));
    assert_eq!(meta_of(&chat, "j-020"), jobs());
    assert_eq!(chat.ask_channel("j-020").channel.owner, ADA);
    chat.ok(&ADA, create_with_meta("plain", BTreeMap::new()));
    assert!(meta_of(&chat, "plain").is_empty());
    assert!(!CHANNEL_META.has(&chat.reads(), &"plain".to_owned()));
    let dm = dm_channel_id(1, 2);
    for (id, refusal) in [
        ("j-020", code::ALREADY_EXISTS),
        ("forge:repo:1", code::UNAUTHORIZED),
        (dm.as_str(), code::UNAUTHORIZED),
        ("a/b", code::INVALID_INPUT),
    ] {
        let refused = chat.refused(&BO, create_with_meta(id, jobs()));
        assert_eq!(refused, refusal, "{id}");
    }
    let blank = Op::CreateChannelWithMeta {
        channel_id: "blank".into(),
        name: "  ".into(),
        post_policy: PostPolicy::Open,
        meta: jobs(),
    };
    assert_eq!(chat.refused(&ADA, blank), code::INVALID_INPUT);
}

#[test]
fn meta_takes_a_channel_that_exists_and_no_dm_takes_any() {
    let mut chat = Chat::default();
    chat.ok(&ADA, open_dm(2));
    let dm = dm_channel_id(1, 2);
    let set = |channel: &str| Op::SetChannelMeta {
        channel_id: channel.into(),
        key: "category".into(),
        value: "Jobs".into(),
    };
    let remove = |channel: &str| Op::RemoveChannelMeta {
        channel_id: channel.into(),
        key: "category".into(),
    };
    for who in [&ADA, &BO, &CY] {
        assert_eq!(chat.refused(who, set(&dm)), code::UNAUTHORIZED);
        assert_eq!(chat.refused(who, remove(&dm)), code::UNAUTHORIZED);
    }
    assert_eq!(chat.refused(&ADA, set("nowhere")), code::NOT_FOUND);
    assert_eq!(chat.refused(&ADA, remove("nowhere")), code::NOT_FOUND);
}

pub(super) fn open_dm(counterpart: u64) -> Op {
    Op::CreateDmChannel {
        counterpart,
        name: "dm".into(),
    }
}

fn rename(name: &str) -> Op {
    Op::RenameChannel {
        channel_id: "general".into(),
        name: name.into(),
    }
}

pub(super) fn archive(archived: bool) -> Op {
    Op::SetChannelArchived {
        channel_id: "general".into(),
        archived,
    }
}

fn membership(principal: Principal, member: bool) -> Op {
    Op::SetMembership {
        channel_id: "general".into(),
        principal,
        member,
    }
}

pub(super) fn edit(seq: u64, text: &str) -> Op {
    Op::EditMessage {
        channel_id: "general".into(),
        seq,
        blocks: parse_message(text),
        base_rev: None,
    }
}

fn set_meta(key: &str, value: &str) -> Op {
    Op::SetChannelMeta {
        channel_id: "general".into(),
        key: key.into(),
        value: value.into(),
    }
}

fn remove_meta(key: &str) -> Op {
    Op::RemoveChannelMeta {
        channel_id: "general".into(),
        key: key.into(),
    }
}

fn create_with_meta(id: &str, meta: BTreeMap<String, String>) -> Op {
    Op::CreateChannelWithMeta {
        channel_id: id.into(),
        name: id.into(),
        post_policy: PostPolicy::Open,
        meta,
    }
}

pub(super) fn meta(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn meta_of(chat: &Chat, id: &str) -> BTreeMap<String, String> {
    chat.ask_channel(id).meta
}

impl Chat {
    fn ask_channel(&self, id: &str) -> ChannelInfo {
        let Reply::Channel(Some(info)) = self.ask(Query::Channel {
            channel_id: id.into(),
        }) else {
            panic!("{id} reads back");
        };
        info
    }
}

fn is_member(chat: &Chat, channel: &str, principal: Principal) -> bool {
    crate::state::MEMBERS.has(&chat.reads(), &(channel.to_owned(), principal))
}
