//! The program's own path, run natively: the sender the host resolved, a
//! the identity role's profiles paged through chat. Each
//! refusal leaves the store as it was.
use ::program::{Env, Origin};

use super::*;
use crate::{Category, Kind, Profile, Standing};

/// Ada's key; it holds account 1.
const ADA_KEY: [u8; 32] = [1; 32];
/// Bo's key: it holds account 2 once identity seats it.
const LONE_KEY: [u8; 32] = [2; 32];

/// The identity role's roster: three accounts, the third an agent Ada manages.
fn identity() -> Vec<Profile> {
    let profile = |number: u64, kind| Profile {
        number,
        name: format!("user{number}"),
        kind,
    };
    vec![
        profile(1, Kind::User),
        profile(2, Kind::User),
        profile(
            3,
            Kind::Program {
                script: None,
                manager: 1,
                category: Category::Agent,
                standing: Standing::Active,
            },
        ),
    ]
}

/// A store with identity beside it.
fn store() -> MockHost {
    let store = MockHost::default();
    store.identity(identity());
    store
}

fn env(origin: Origin, sender: Option<Principal>) -> Env {
    Env {
        time: 1000,
        origin,
        sender,
        ..MockHost::env(crate::PROGRAM)
    }
}

/// A frame signed by `key`, acting as the account it holds (if any), as the
/// host resolves it.
fn key(key: &[u8], account: Option<u64>) -> Env {
    Env {
        time: 1000,
        ..MockHost::env(crate::PROGRAM).signed(key, account)
    }
}

fn ada() -> Env {
    key(&ADA_KEY, Some(1))
}

fn root() -> Env {
    env(Origin::Root, Some(Principal::Root))
}

/// The refusal's reason; the store is untouched by it.
#[track_caller]
fn refused(store: &MockHost, env: Env, op: Op) -> String {
    let ctx = store.exec(env);
    store.refused(|| crate::Chat::execute(&ctx, op)).code
}

/// A read of `store`.
fn reads(store: &MockHost) -> ::program::QueryCtx {
    store.query(env(Origin::Root, None))
}

fn owner(store: &MockHost, id: &str) -> Principal {
    crate::state::channel(&reads(store), id).unwrap().owner
}

#[test]
fn a_write_acts_as_its_sender() {
    let store = store();
    crate::Chat::execute(&store.exec(ada()), create("a", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "a"), Principal::Account(1));
    // forge's own frame acts as forge's account
    let forge = env(Origin::Program("forge".into()), Some(FORGE));
    crate::Chat::execute(&store.exec(forge), create("forge:c", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "forge:c"), FORGE);
    crate::Chat::execute(&store.exec(root()), create("d", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "d"), Principal::Root);
}

/// Every op, one each, as a key would send it into `#general`.
fn every_op() -> Vec<Op> {
    let general = || "general".to_string();
    vec![
        create("room", PostPolicy::Open),
        Op::CreateDmChannel {
            counterpart: 1,
            name: "ada".into(),
        },
        Op::RenameChannel {
            channel_id: general(),
            name: "renamed".into(),
        },
        Op::SetChannelArchived {
            channel_id: general(),
            archived: true,
        },
        Op::PostMessage {
            channel_id: general(),
            message_id: "m2".into(),
            blocks: parse_message("hi"),
            thread: None,
        },
        Op::EditMessage {
            channel_id: general(),
            seq: 1,
            blocks: parse_message("edited"),
            base_rev: None,
        },
        Op::DeleteMessage {
            channel_id: general(),
            seq: 1,
        },
        Op::AddReaction {
            channel_id: general(),
            seq: 1,
            emoji: "👍".into(),
        },
        Op::RemoveReaction {
            channel_id: general(),
            seq: 1,
            emoji: "👍".into(),
        },
        Op::SetMembership {
            channel_id: general(),
            principal: Principal::Account(3),
            member: true,
        },
        Op::SetNotices {
            channel_id: general(),
            notices: Some(crate::Notices::Off),
        },
    ]
}

/// A key that holds no account writes nothing, whatever the op; once
/// identity hands it an account, the same key writes as that account.
#[test]
fn a_key_writes_only_once_it_holds_an_account() {
    let store = store();
    crate::Chat::execute(&store.exec(ada()), create("general", PostPolicy::Open)).unwrap();
    crate::Chat::execute(&store.exec(ada()), post("general", "m1", "hello", None)).unwrap();
    for op in every_op() {
        let why = format!("{op:?}");
        assert_eq!(
            refused(&store, key(&LONE_KEY, None), op),
            code::UNAUTHORIZED,
            "{why}"
        );
    }
    let bo = || key(&LONE_KEY, Some(2));
    crate::Chat::execute(&store.exec(bo()), create("bo", PostPolicy::Open)).unwrap();
    assert_eq!(owner(&store, "bo"), Principal::Account(2));
    crate::Chat::execute(&store.exec(bo()), post("general", "m2", "now I can", None)).unwrap();
    let row = crate::state::message(&reads(&store), "general", 2).unwrap();
    assert_eq!(row.author, Principal::Account(2));
}

#[test]
fn the_roster_pages_through_the_identity_role() {
    let store = store();
    let page = |after: Option<Vec<u8>>| {
        let asked = Query::Accounts {
            page: PageRequest {
                after,
                limit: Some(2),
            },
        };
        let Reply::Accounts(page) = crate::Chat::query(&reads(&store), asked).unwrap() else {
            panic!("accounts answer accounts");
        };
        page
    };
    let first = page(None);
    let numbers = |rows: &[Profile]| rows.iter().map(|row| row.number).collect::<Vec<_>>();
    assert_eq!(numbers(&first.items), [1, 2]);
    assert_eq!(first.items[0].name, "user1");
    let rest = page(first.next);
    assert_eq!(numbers(&rest.items), [3]);
    assert!(matches!(
        rest.items[0].kind,
        Kind::Program { manager: 1, .. }
    ));
    assert_eq!(rest.next, None);
}
