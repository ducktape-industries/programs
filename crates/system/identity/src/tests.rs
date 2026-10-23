// The program natively over `program::MockHost`: what the founding suite checks on the host, without the host.

mod managers;
mod renames;

use ::program::{Env, Origin, Principal, Scheme, code};
use ::program::{MockHost, Program};
use store::PageRequest;

use crate::{
    Account, Admission, CONSENT_NAMESPACE, Category, Consent, Control, Identity, Kind, Life, Op,
    Ownership, Party, Query, Reply, Script, Standing,
};

const ALICE: &[u8] = b"alice-key";
const SECOND: &[u8] = b"alice-second-key";
const BOB: &[u8] = b"bob";
const BOT: &[u8] = b"bot-key";
const SPARE: &[u8] = b"bot-spare-key";

fn env(origin: Origin, sender: Option<Principal>, time: u64) -> Env {
    Env {
        height: 7,
        time,
        origin,
        sender,
        ..MockHost::env(crate::PROGRAM)
    }
}

fn signed_at(store: &MockHost, key: &[u8], time: u64) -> Result<Env, ::program::Error> {
    let origin = Origin::Signed(key.to_vec());
    let asked = Query::OfKey { key: key.to_vec() };
    let sender = match Identity::query(&store.query(env(origin.clone(), None, time)), asked)? {
        Reply::Number(number) => number.map(Principal::Account),
        other => panic!("{other:?}"),
    };
    Ok(env(origin, sender, time))
}

fn signed(store: &MockHost, key: &[u8]) -> Env {
    signed_at(store, key, 100).unwrap()
}

fn from_program(store: &MockHost, program: &str) -> Env {
    let asked = Query::OfProgram {
        program: program.into(),
    };
    let Reply::Number(number) = query(store, asked) else {
        panic!("OfProgram answers a number");
    };
    env(
        Origin::Program(program.into()),
        number.map(Principal::Account),
        100,
    )
}

fn root() -> Env {
    env(Origin::Root, Some(Principal::Root), 100)
}

fn memory() -> MockHost {
    let host = MockHost::default();
    host.borrow_mut().verifier = Some(Box::new(|_, key, namespace, message, signature| {
        signature == proof(namespace, message) && !key.is_empty()
    }));
    host
}

fn proof(namespace: &[u8], preimage: &[u8]) -> Vec<u8> {
    [namespace, preimage].concat()
}

fn run(store: &MockHost, env: &Env, op: Op) -> Result<u64, ::program::Error> {
    Identity::execute(&store.exec(env.clone()), op)?;
    let output = store.take_output();
    Ok(if output.is_empty() {
        0
    } else {
        abi::decode(&output).unwrap()
    })
}

#[track_caller]
fn refused(store: &MockHost, env: &Env, op: Op) -> ::program::Error {
    store.refused(|| run(store, env, op))
}

fn query(store: &MockHost, query: Query) -> Reply {
    Identity::query(&store.query(env(Origin::Root, None, 100)), query).unwrap()
}

fn get(store: &MockHost, number: u64) -> Account {
    match query(store, Query::Get { number }) {
        Reply::Account(account) => account.expect("the account exists"),
        other => panic!("{other:?}"),
    }
}

fn create(store: &MockHost, key: &[u8], name: &str) -> u64 {
    run(
        store,
        &signed(store, key),
        Op::Create {
            name: name.into(),
            scheme: Scheme::Ed25519,
        },
    )
    .unwrap()
}

fn create_agent(store: &MockHost, manager: &[u8], name: &str) -> u64 {
    let op = Op::CreateAgent { name: name.into() };
    run(store, &signed(store, manager), op).unwrap()
}

fn add_agent_key(
    store: &MockHost,
    manager: &[u8],
    agent: u64,
    key: &[u8],
) -> Result<u64, ::program::Error> {
    run(store, &signed(store, manager), agent_key(agent, key, 0))
}

fn agent_key(agent: u64, key: &[u8], generation: u64) -> Op {
    consented_by(key, agent, key, generation)
}

fn consented_by(consenting: &[u8], account: u64, key: &[u8], generation: u64) -> Op {
    let admission = Admission {
        network: b"net".to_vec(),
        scheme: Scheme::Ed25519,
        key: key.to_vec(),
        generation,
        account,
        expires_at: 200,
    };
    Op::AddKey {
        scheme: Scheme::Ed25519,
        label: Some("sandbox".into()),
        consent: Consent {
            key: consenting.to_vec(),
            account,
            expires_at: 200,
            proof: proof(CONSENT_NAMESPACE, &admission.preimage()),
        },
    }
}

fn owner_of(store: &MockHost, number: u64) -> Option<Party> {
    let ctx = store.query(env(Origin::Root, None, 100));
    Identity::resource(&ctx, &crate::account_key(number))
        .unwrap()
        .owner
}

fn needs(store: &MockHost, op: &Op) -> Vec<::program::Need> {
    Identity::access(&store.query(env(Origin::Root, None, 100)), op).unwrap()
}

fn keys_of(account: &Account) -> Vec<Vec<u8>> {
    account.keys().iter().map(|key| key.key.clone()).collect()
}

#[test]
fn the_empty_key_of_an_unsigned_query_holds_no_account() {
    let store = memory();
    assert_eq!(create(&store, ALICE, "Alice"), 1);
    let unsigned = env(Origin::Signed(Vec::new()), None, 100);
    let create = Op::Create {
        name: "Nobody".into(),
        scheme: Scheme::Ed25519,
    };
    assert_eq!(refused(&store, &unsigned, create).code, code::UNAUTHORIZED);
    assert_eq!(
        query(&store, Query::OfKey { key: Vec::new() }),
        Reply::Number(None)
    );
}

#[test]
fn accounts_are_numbered_from_one_and_a_key_holds_one_account() {
    let store = memory();
    assert_eq!(create(&store, ALICE, "  Alice "), 1);
    let alice = get(&store, 1);
    assert_eq!(alice.card.name, "Alice");
    assert!(matches!(alice.control, Control::User { .. }));
    assert_eq!(create(&store, BOB, "Bob"), 2);
    let again = refused(
        &store,
        &signed(&store, ALICE),
        Op::Create {
            name: "Twice".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(again.code, code::ALREADY_EXISTS);
    assert_eq!(
        query(
            &store,
            Query::OfKey {
                key: ALICE.to_vec()
            }
        ),
        Reply::Number(Some(1))
    );
    let unsigned = refused(
        &store,
        &root(),
        Op::Create {
            name: "x".into(),
            scheme: Scheme::Ed25519,
        },
    );
    assert_eq!(unsigned.code, code::UNAUTHORIZED);
}

#[test]
fn a_key_joins_by_consent_and_leaves_only_junior_to_its_remover() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let admission = Admission {
        network: b"net".to_vec(),
        scheme: Scheme::Ed25519,
        key: SECOND.to_vec(),
        generation: 0,
        account: 1,
        expires_at: 200,
    };
    let consent = |proof: Vec<u8>, expires_at: u64| Consent {
        key: ALICE.to_vec(),
        account: 1,
        expires_at,
        proof,
    };
    let add = |proof: Vec<u8>, expires_at: u64| Op::AddKey {
        scheme: Scheme::Ed25519,
        label: Some("laptop".into()),
        consent: consent(proof, expires_at),
    };
    let consented = proof(CONSENT_NAMESPACE, &admission.preimage());
    let second = |time| signed_at(&store, SECOND, time).unwrap();
    let forged = refused(&store, &second(100), add(b"nope".to_vec(), 200));
    assert_eq!(forged.code, code::UNAUTHORIZED);
    let expired = refused(&store, &second(300), add(consented.clone(), 200));
    assert_eq!(expired.code, code::UNAUTHORIZED);
    run(&store, &second(150), add(consented, 200)).unwrap();
    assert_eq!(get(&store, 1).keys().len(), 2);
    assert_eq!(
        query(
            &store,
            Query::Generation {
                key: SECOND.to_vec()
            }
        ),
        Reply::Generation(1)
    );
    let remove = |key: &[u8]| Op::RemoveKey {
        account: 1,
        key: key.to_vec(),
    };
    let senior = refused(&store, &second(150), remove(ALICE));
    assert_eq!(senior.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), remove(SECOND)).unwrap();
    assert!(!get(&store, 1).holds(SECOND));
    let last = refused(&store, &signed(&store, ALICE), remove(ALICE));
    assert_eq!(last.code, code::WRONG_STATE);
}

#[test]
fn a_user_manages_an_agent_and_its_keys() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    assert_eq!(agent, 3);
    assert_eq!(
        get(&store, agent).control,
        Control::Program {
            script: None,
            manager: 1,
            category: Category::Agent,
            life: Life::Active { keys: Vec::new() },
        }
    );

    let stranger = refused(&store, &signed(&store, BOB), agent_key(agent, BOT, 0));
    assert_eq!(stranger.code, code::UNAUTHORIZED);
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));

    let remove = Op::RemoveKey {
        account: agent,
        key: BOT.to_vec(),
    };
    let by_stranger = refused(&store, &signed(&store, BOB), remove.clone());
    assert_eq!(by_stranger.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, ALICE), remove).unwrap();
    assert_eq!(signed(&store, BOT).sender, None);
}

#[test]
fn an_agent_adds_and_removes_its_own_keys_as_a_user_does() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let spare = || signed_at(&store, SPARE, 150).unwrap();

    let forged = Op::AddKey {
        scheme: Scheme::Ed25519,
        label: None,
        consent: Consent {
            key: BOT.to_vec(),
            account: agent,
            expires_at: 200,
            proof: b"nope".to_vec(),
        },
    };
    assert_eq!(refused(&store, &spare(), forged).code, code::UNAUTHORIZED);
    run(&store, &spare(), consented_by(BOT, agent, SPARE, 0)).unwrap();
    assert_eq!(keys_of(&get(&store, agent)), [BOT.to_vec(), SPARE.to_vec()]);
    assert_eq!(spare().sender, Some(Principal::Account(agent)));

    let remove = |key: &[u8]| Op::RemoveKey {
        account: agent,
        key: key.to_vec(),
    };
    let senior = refused(&store, &spare(), remove(BOT));
    assert_eq!(senior.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, BOT), remove(SPARE)).unwrap();
    let last = refused(&store, &signed(&store, BOT), remove(BOT));
    assert_eq!(last.code, code::WRONG_STATE);

    run(
        &store,
        &signed(&store, ALICE),
        Op::Suspend { account: agent },
    )
    .unwrap();
    let while_suspended = refused(&store, &spare(), consented_by(BOT, agent, SPARE, 1));
    assert_eq!(while_suspended.code, code::UNAUTHORIZED);
    assert_eq!(
        while_suspended.message,
        format!("account {agent} is suspended")
    );
    assert_eq!(keys_of(&get(&store, agent)), [BOT.to_vec()]);
}

#[test]
fn an_agent_suspends_itself_and_its_manager_resumes_it() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    run(&store, &signed(&store, BOT), Op::Suspend { account: agent }).unwrap();
    assert_eq!(design::note(&get(&store, agent).kind()), Some("suspended"));
    let its_key = signed_at(&store, BOT, 100).unwrap_err();
    assert_eq!(its_key.code, code::UNAUTHORIZED);
    run(
        &store,
        &signed(&store, ALICE),
        Op::Resume { account: agent },
    )
    .unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));
}

#[test]
fn an_agent_creates_an_agent_it_manages() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let scout = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, scout, BOT).unwrap();
    let helper = create_agent(&store, BOT, "Helper");
    assert_eq!(
        get(&store, helper).control,
        Control::Program {
            script: None,
            manager: scout,
            category: Category::Agent,
            life: Life::Active { keys: Vec::new() },
        }
    );
    assert_eq!(owner_of(&store, helper), Some(Party::Account(scout)));
    let Reply::Accounts(managed) = query(
        &store,
        Query::Managed {
            by: scout,
            page: PageRequest::first(10),
        },
    ) else {
        panic!("Managed answers accounts");
    };
    assert_eq!(
        managed.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [helper]
    );

    add_agent_key(&store, BOT, helper, SPARE).unwrap();
    assert_eq!(
        signed(&store, SPARE).sender,
        Some(Principal::Account(helper))
    );
    run(
        &store,
        &signed(&store, BOT),
        Op::Suspend { account: helper },
    )
    .unwrap();
    assert_eq!(design::note(&get(&store, helper).kind()), Some("suspended"));
    run(&store, &signed(&store, BOT), Op::Resume { account: helper }).unwrap();
    assert_eq!(design::note(&get(&store, helper).kind()), None);
    run(&store, &signed(&store, BOT), Op::Revoke { account: helper }).unwrap();
    assert_eq!(design::note(&get(&store, helper).kind()), Some("revoked"));
}

#[test]
fn a_system_account_creates_an_agent_it_manages() {
    let store = memory();
    run(
        &store,
        &root(),
        Op::RegisterProgram {
            program: "forge".into(),
        },
    )
    .unwrap();
    let forge = from_program(&store, "forge");
    let Some(Principal::Account(forge_account)) = forge.sender else {
        panic!("forge acts as its account");
    };
    let agent = run(&store, &forge, Op::CreateAgent { name: "Bot".into() }).unwrap();
    assert_eq!(
        get(&store, agent).control,
        Control::Program {
            script: None,
            manager: forge_account,
            category: Category::Agent,
            life: Life::Active { keys: Vec::new() },
        }
    );
    run(&store, &forge, agent_key(agent, BOT, 0)).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));
}

#[test]
fn a_suspended_or_revoked_agent_creates_no_agent() {
    let store = memory();
    create(&store, ALICE, "Alice");
    run(
        &store,
        &root(),
        Op::RegisterProgram {
            program: "harness".into(),
        },
    )
    .unwrap();
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    run(
        &store,
        &signed(&store, ALICE),
        run_by(agent, Some("harness")),
    )
    .unwrap();
    assert_eq!(runs(&store, "harness", agent).unwrap(), Some(agent));

    run(
        &store,
        &signed(&store, ALICE),
        Op::Suspend { account: agent },
    )
    .unwrap();
    let signed_frame = signed_at(&store, BOT, 100).unwrap_err();
    assert_eq!(signed_frame.code, code::UNAUTHORIZED);
    let run_frame = runs(&store, "harness", agent).unwrap_err();
    assert_eq!(run_frame.code, code::UNAUTHORIZED);

    run(
        &store,
        &signed(&store, ALICE),
        Op::Revoke { account: agent },
    )
    .unwrap();
    let by_its_old_key = refused(
        &store,
        &signed(&store, BOT),
        Op::CreateAgent { name: "x".into() },
    );
    assert_eq!(by_its_old_key.code, code::UNAUTHORIZED);
    let run_frame = runs(&store, "harness", agent).unwrap_err();
    assert_eq!(run_frame.code, code::WRONG_STATE);
    let Reply::Accounts(managed) = query(
        &store,
        Query::Managed {
            by: agent,
            page: PageRequest::first(10),
        },
    ) else {
        panic!("Managed answers accounts");
    };
    assert!(managed.items.is_empty());
}

#[test]
fn a_card_is_edited_by_its_user_its_agent_its_manager_or_its_program() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    run(
        &store,
        &root(),
        Op::RegisterProgram {
            program: "forge".into(),
        },
    )
    .unwrap();
    let forge = from_program(&store, "forge");
    let Some(Principal::Account(forge_account)) = forge.sender else {
        panic!("forge acts as its account");
    };
    let bio = |account: u64, bio: &str| Op::SetProfile {
        account,
        avatar: None,
        bio: Some(bio.into()),
    };
    let rename = |account: u64, name: &str| Op::SetName {
        account,
        name: name.into(),
    };
    run(&store, &signed(&store, ALICE), bio(1, " mine ")).unwrap();
    assert_eq!(get(&store, 1).card.bio.as_deref(), Some("mine"));
    run(&store, &signed(&store, ALICE), bio(agent, "managed")).unwrap();
    run(&store, &signed(&store, ALICE), rename(agent, "Scout II")).unwrap();
    assert_eq!(get(&store, agent).card.bio.as_deref(), Some("managed"));
    assert_eq!(get(&store, agent).card.name, "Scout II");
    run(&store, &forge, rename(forge_account, "Forge")).unwrap();
    run(&store, &forge, bio(forge_account, "the forge")).unwrap();
    assert_eq!(get(&store, forge_account).card.name, "Forge");
    assert_eq!(owner_of(&store, 1), Some(Party::Account(1)));
    assert_eq!(owner_of(&store, agent), Some(Party::Account(1)));
    let writers = |number| {
        let ctx = store.query(env(Origin::Root, None, 100));
        abi::role::access::writers::<::program::Error>(
            &crate::account_key(number),
            |_| Ok(None),
            |at| Identity::resource(&ctx, at),
        )
        .unwrap()
    };
    let agents_card = writers(agent);
    assert!(agents_card.allows(&[Party::Account(agent)]));
    assert!(agents_card.allows(&[Party::Account(1)]));
    assert!(!agents_card.allows(&[Party::Account(2)]));
    assert!(!writers(1).allows(&[Party::Account(agent)]));
    run(&store, &signed(&store, BOT), rename(agent, "Scout III")).unwrap();
    assert_eq!(get(&store, agent).card.name, "Scout III");
    assert_eq!(
        owner_of(&store, forge_account),
        Some(Party::Account(forge_account))
    );
    for account in [1, agent, forge_account] {
        let key = crate::account_key(account);
        assert_eq!(
            needs(&store, &bio(account, "x")),
            [::program::Need::write(key.clone())]
        );
        assert_eq!(
            needs(&store, &rename(account, "x")),
            [::program::Need::write(key)]
        );
    }
}

#[test]
fn an_agent_is_suspended_resumed_or_revoked_and_its_manager_owns_it() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let suspend = || Op::Suspend { account: agent };
    let resume = || Op::Resume { account: agent };
    let revoke = || Op::Revoke { account: agent };
    for op in [suspend(), resume(), revoke()] {
        assert_eq!(
            needs(&store, &op),
            [::program::Need::write(crate::account_key(agent))]
        );
    }
    assert_eq!(owner_of(&store, agent), Some(Party::Account(1)));
    let already = refused(&store, &signed(&store, ALICE), resume());
    assert_eq!(already.code, code::WRONG_STATE);

    run(&store, &signed(&store, ALICE), suspend()).unwrap();
    let refused_key = signed_at(&store, BOT, 100).unwrap_err();
    assert_eq!(refused_key.code, code::UNAUTHORIZED);
    assert_eq!(keys_of(&get(&store, agent)), [BOT.to_vec()]);
    assert_eq!(design::note(&get(&store, agent).kind()), Some("suspended"));
    let again = refused(&store, &signed(&store, ALICE), suspend());
    assert_eq!(again.code, code::WRONG_STATE);
    run(
        &store,
        &signed(&store, ALICE),
        agent_key(agent, b"second-bot", 0),
    )
    .unwrap();
    run(
        &store,
        &signed(&store, ALICE),
        Op::SetName {
            account: agent,
            name: "Idle".into(),
        },
    )
    .unwrap();
    run(&store, &signed(&store, ALICE), resume()).unwrap();
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));

    run(&store, &signed(&store, ALICE), revoke()).unwrap();
    let account = get(&store, agent);
    assert_eq!(design::note(&account.kind()), Some("revoked"));
    assert!(account.keys().is_empty(), "revoked, it keeps no key");
    assert_eq!(
        query(&store, Query::OfKey { key: BOT.to_vec() }),
        Reply::Number(None),
        "its key holds nothing"
    );
    let ops = [
        suspend(),
        resume(),
        revoke(),
        agent_key(agent, b"third-bot", 0),
        Op::RemoveKey {
            account: agent,
            key: BOT.to_vec(),
        },
        Op::SetName {
            account: agent,
            name: "Back".into(),
        },
        Op::SetProfile {
            account: agent,
            avatar: None,
            bio: Some("back".into()),
        },
    ];
    for op in ops {
        let why = format!("{op:?}");
        let refused = refused(&store, &signed(&store, ALICE), op);
        assert_eq!(refused.code, code::WRONG_STATE, "{why}");
    }
    create(&store, BOT, "Reborn");
    let Reply::Accounts(managed) = query(
        &store,
        Query::Managed {
            by: 1,
            page: PageRequest::first(10),
        },
    ) else {
        panic!("Managed answers accounts");
    };
    assert_eq!(
        managed.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [agent]
    );

    let alice = refused(&store, &signed(&store, ALICE), Op::Suspend { account: 1 });
    assert_eq!(alice.code, code::WRONG_STATE);
    let bob = refused(&store, &signed(&store, BOB), Op::Suspend { account: 1 });
    assert_eq!(bob.code, code::UNAUTHORIZED);
}

#[test]
fn a_stranger_changes_no_account_it_neither_is_nor_manages() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let rename = |account: u64| Op::SetName {
        account,
        name: "Mallory".into(),
    };
    let bio = |account: u64| Op::SetProfile {
        account,
        avatar: None,
        bio: Some("taken".into()),
    };
    let ops = [
        rename(1),
        bio(1),
        rename(agent),
        bio(agent),
        Op::SetOwnership {
            account: agent,
            ownership: Ownership::Owner,
        },
        Op::SetScript {
            account: agent,
            script: None,
        },
        Op::Suspend { account: agent },
        Op::Resume { account: agent },
        Op::Revoke { account: agent },
    ];
    for op in ops {
        let why = format!("{op:?}");
        let refused = refused(&store, &signed(&store, BOB), op);
        assert_eq!(refused.code, code::UNAUTHORIZED, "{why}");
    }
    let its_manager = refused(&store, &signed(&store, BOT), rename(1));
    assert_eq!(its_manager.code, code::UNAUTHORIZED);
    assert_eq!(get(&store, 1).card.name, "Alice");
    assert_eq!(get(&store, agent).card.name, "Scout");
    assert_eq!(signed(&store, BOT).sender, Some(Principal::Account(agent)));
    run(
        &store,
        &signed(&store, ALICE),
        Op::Revoke { account: agent },
    )
    .unwrap();
    assert_eq!(design::note(&get(&store, agent).kind()), Some("revoked"));
}

#[test]
fn the_system_registers_a_program_which_alone_names_its_account() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let register = Op::RegisterProgram {
        program: "forge".into(),
    };
    run(&store, &root(), register).unwrap();
    let forge = from_program(&store, "forge");
    assert_eq!(forge.sender, Some(Principal::Account(2)));
    let account = get(&store, 2);
    assert_eq!(account.card.name, "forge");
    assert_eq!(
        account.control,
        Control::System {
            program: "forge".into()
        }
    );
    assert!(account.keys().is_empty());

    let key = refused(&store, &signed(&store, ALICE), agent_key(2, BOT, 0));
    assert_eq!(key.code, code::WRONG_STATE);
    let key = refused(&store, &forge, agent_key(2, BOT, 0));
    assert_eq!(key.code, code::WRONG_STATE);
    let remove = refused(
        &store,
        &forge,
        Op::RemoveKey {
            account: 2,
            key: BOT.to_vec(),
        },
    );
    assert_eq!(remove.code, code::WRONG_STATE);
    let suspend = refused(&store, &forge, Op::Suspend { account: 2 });
    assert_eq!(suspend.code, code::WRONG_STATE);
    let stranger = refused(&store, &signed(&store, ALICE), Op::Suspend { account: 2 });
    assert_eq!(stranger.code, code::UNAUTHORIZED);
}

#[test]
fn lists_page_in_number_order_and_managed_lists_one_manager() {
    let store = memory();
    for n in 0..11u8 {
        create(&store, &[n], &format!("a{n}"));
    }
    for _ in 0..3 {
        create_agent(&store, &[1], "agent");
    }
    create_agent(&store, &[10], "other");
    let list = |asked| match query(&store, asked) {
        Reply::Accounts(page) => page,
        other => panic!("{other:?}"),
    };
    let first = list(Query::List {
        page: PageRequest::first(10),
    });
    assert_eq!(first.height, 7);
    assert_eq!(first.items.len(), 10);
    let rest = list(Query::List {
        page: PageRequest::resume(first.next, 10),
    });
    assert_eq!(
        rest.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [11, 12, 13, 14, 15],
        "numeric order across the ten boundary"
    );
    let managed = list(Query::Managed {
        by: 2,
        page: PageRequest::first(2),
    });
    assert_eq!(
        managed.items.iter().map(|a| a.number).collect::<Vec<_>>(),
        [12, 13]
    );
    let more = list(Query::Managed {
        by: 2,
        page: PageRequest::resume(managed.next, 2),
    });
    assert_eq!(
        (
            more.items.iter().map(|a| a.number).collect::<Vec<_>>(),
            more.next
        ),
        (vec![14], None),
        "the page stays under one manager"
    );
    assert_eq!(
        query(
            &store,
            Query::Resolve {
                references: vec![
                    crate::Reference::Account(15),
                    crate::Reference::Account(99),
                    crate::Reference::Key(vec![3]),
                ],
            }
        ),
        Reply::Resolved(vec![Some(15), None, Some(4)])
    );
}

#[test]
fn profiles_page_in_number_order_and_say_what_each_is() {
    let store = memory();
    create(&store, ALICE, "Alice");
    create_agent(&store, ALICE, "Scout");
    run(
        &store,
        &root(),
        Op::RegisterProgram {
            program: "chat".into(),
        },
    )
    .unwrap();
    let page = |after, limit| match query(&store, Query::Profiles { after, limit }) {
        Reply::Profiles { profiles, next } => (profiles, next),
        other => panic!("{other:?}"),
    };
    let (first, next) = page(None, 2);
    let what: Vec<_> = first
        .iter()
        .map(|p| (p.number, p.name.as_str(), p.kind.clone()))
        .collect();
    assert_eq!(
        what,
        [
            (1, "Alice", Kind::User),
            (
                2,
                "Scout",
                Kind::Program {
                    script: None,
                    manager: 1,
                    category: Category::Agent,
                    standing: Standing::Active
                }
            )
        ]
    );
    assert_eq!(next, Some(2));
    let (rest, next) = page(next, 2);
    assert_eq!(
        rest.iter()
            .map(|p| (p.number, p.kind.clone()))
            .collect::<Vec<_>>(),
        [(3, Kind::System("chat".into()))]
    );
    assert_eq!(next, None);
    let one = |number| match query(&store, Query::Profile { number }) {
        Reply::Profile(profile) => profile,
        other => panic!("{other:?}"),
    };
    assert_eq!(one(2).map(|p| p.name), Some("Scout".into()));
    assert_eq!(one(9), None);
}

#[test]
fn an_agents_key_consent_expires_and_is_not_replayed() {
    let store = memory();
    create(&store, ALICE, "Alice");
    let agent = create_agent(&store, ALICE, "Scout");
    let late = signed_at(&store, ALICE, 300).unwrap();
    let expired = refused(&store, &late, agent_key(agent, BOT, 0));
    assert_eq!(expired.code, code::UNAUTHORIZED);
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let remove = Op::RemoveKey {
        account: agent,
        key: BOT.to_vec(),
    };
    run(&store, &signed(&store, ALICE), remove).unwrap();
    let replayed = refused(&store, &signed(&store, ALICE), agent_key(agent, BOT, 0));
    assert_eq!(replayed.code, code::UNAUTHORIZED);
    let fresh = agent_key(agent, BOT, 1);
    run(&store, &signed(&store, ALICE), fresh).unwrap();
    assert!(get(&store, agent).holds(BOT));
}

fn answers(store: &MockHost, account: u64) -> Option<Ownership> {
    match query(store, Query::Answers { account }) {
        Reply::Answers(answers) => answers,
        other => panic!("{other:?}"),
    }
}

fn hears(store: &MockHost, account: u64, from: u64) -> Result<bool, ::program::Error> {
    let asked = Query::Hears { account, from };
    match Identity::query(&store.query(env(Origin::Root, None, 100)), asked)? {
        Reply::Hears(hears) => Ok(hears),
        other => panic!("{other:?}"),
    }
}

fn account_of(store: &MockHost, program: &str) -> u64 {
    run(
        store,
        &root(),
        Op::RegisterProgram {
            program: program.into(),
        },
    )
    .unwrap();
    let Some(Principal::Account(number)) = from_program(store, program).sender else {
        panic!("{program} acts as its account");
    };
    number
}

#[test]
fn an_agent_answers_anyone_until_it_or_its_manager_narrows_it() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    let bob = create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let pods = account_of(&store, "pods");
    assert_eq!(answers(&store, agent), Some(Ownership::Anyone));
    for from in [alice, bob, pods] {
        assert!(hears(&store, agent, from).unwrap(), "{from}");
    }

    let set = |ownership| Op::SetOwnership {
        account: agent,
        ownership,
    };
    // the account's key: its manager and the agent itself write it (the card test shows who)
    assert_eq!(
        needs(&store, &set(Ownership::Owner)),
        [::program::Need::write(crate::account_key(agent))]
    );
    let pods_party = || Party::Program("pods".into());
    let listed = vec![pods_party(), Party::Account(alice), pods_party()];
    run(
        &store,
        &signed(&store, ALICE),
        set(Ownership::Listed(listed)),
    )
    .unwrap();
    assert_eq!(
        answers(&store, agent),
        Some(Ownership::Listed(vec![pods_party()])),
        "settled: once each, its manager left out"
    );
    assert!(hears(&store, agent, alice).unwrap(), "its manager, always");
    assert!(hears(&store, agent, pods).unwrap(), "a program's account");
    assert!(!hears(&store, agent, bob).unwrap());

    let manager_only = Ownership::Listed(vec![Party::Account(alice)]);
    run(&store, &signed(&store, BOT), set(manager_only)).unwrap();
    assert_eq!(answers(&store, agent), Some(Ownership::Owner));
    assert!(!hears(&store, agent, pods).unwrap());
    assert!(hears(&store, agent, alice).unwrap());

    run(&store, &signed(&store, BOT), set(Ownership::Anyone)).unwrap();
    assert_eq!(answers(&store, agent), Some(Ownership::Anyone));
    assert!(hears(&store, agent, bob).unwrap());
}

fn narrowed(store: &MockHost) -> Vec<(u64, Ownership)> {
    let page = crate::PageRequest {
        after: None,
        limit: None,
    };
    match query(store, Query::Narrowed { page }) {
        Reply::Narrowed(page) => page.items,
        other => panic!("{other:?}"),
    }
}

// one page answers every agent that narrowed whom it answers, settled as Answers says it; the rest,
// and one opened again to anyone, answer Anyone and are not listed
#[test]
fn every_narrowed_agent_is_one_page() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    let bob = create(&store, BOB, "Bob");
    let scout = create_agent(&store, ALICE, "Scout");
    let atlas = create_agent(&store, ALICE, "Atlas");
    let sage = create_agent(&store, ALICE, "Sage");
    assert_eq!(narrowed(&store), []);
    let set = |account, ownership| Op::SetOwnership { account, ownership };
    let listed = Ownership::Listed(vec![Party::Account(bob), Party::Account(alice)]);
    for (account, ownership) in [(sage, listed), (scout, Ownership::Owner)] {
        run(&store, &signed(&store, ALICE), set(account, ownership)).unwrap();
    }
    let bob_only = Ownership::Listed(vec![Party::Account(bob)]);
    assert_eq!(
        narrowed(&store),
        [(scout, Ownership::Owner), (sage, bob_only.clone())]
    );
    for (account, ownership) in narrowed(&store) {
        assert_eq!(answers(&store, account), Some(ownership));
    }
    assert_eq!(answers(&store, atlas), Some(Ownership::Anyone));
    run(
        &store,
        &signed(&store, ALICE),
        set(scout, Ownership::Anyone),
    )
    .unwrap();
    assert_eq!(narrowed(&store), [(sage, bob_only)]);
}

#[test]
fn only_a_program_account_answers_and_not_once_revoked() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    let agent = create_agent(&store, ALICE, "Scout");
    let forge = account_of(&store, "forge");
    let narrow = |account| Op::SetOwnership {
        account,
        ownership: Ownership::Owner,
    };
    for (account, by) in [
        (alice, signed(&store, ALICE)),
        (forge, from_program(&store, "forge")),
    ] {
        let why = refused(&store, &by, narrow(account));
        assert_eq!(why.code, code::WRONG_STATE, "{account}");
        assert_eq!(answers(&store, account), None);
        let asked = hears(&store, account, alice).unwrap_err();
        assert_eq!(asked.code, code::WRONG_STATE);
    }
    assert_eq!(answers(&store, 99), None);
    assert_eq!(hears(&store, 99, alice).unwrap_err().code, code::NOT_FOUND);

    run(
        &store,
        &signed(&store, ALICE),
        Op::Revoke { account: agent },
    )
    .unwrap();
    let revoked = refused(&store, &signed(&store, ALICE), narrow(agent));
    assert_eq!(revoked.code, code::WRONG_STATE);
}

fn runs(store: &MockHost, program: &str, account: u64) -> Result<Option<u64>, ::program::Error> {
    let asked = Query::Runs {
        program: program.into(),
        account,
    };
    match Identity::query(&store.query(env(Origin::Root, None, 100)), asked)? {
        Reply::Number(number) => Ok(number),
        other => panic!("{other:?}"),
    }
}

fn script(calls: &str) -> Script {
    Script {
        calls: calls.into(),
        config: "answer in the forge".into(),
    }
}

fn run_by(account: u64, calls: Option<&str>) -> Op {
    Op::SetScript {
        account,
        script: calls.map(script),
    }
}

#[test]
fn a_manager_hands_an_agent_to_a_program_that_then_alone_runs_it() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    run(
        &store,
        &root(),
        Op::RegisterProgram {
            program: "harness".into(),
        },
    )
    .unwrap();
    assert_eq!(
        runs(&store, "harness", agent).unwrap_err().code,
        code::UNAUTHORIZED
    );

    let unknown = refused(
        &store,
        &signed(&store, ALICE),
        run_by(agent, Some("nobody")),
    );
    assert_eq!(unknown.code, code::NOT_FOUND);
    assert_eq!(
        needs(&store, &run_by(agent, Some("harness"))),
        [::program::Need::write(crate::account_key(agent))]
    );
    let user = refused(
        &store,
        &signed(&store, ALICE),
        run_by(alice, Some("harness")),
    );
    assert_eq!(user.code, code::WRONG_STATE);

    run(
        &store,
        &signed(&store, ALICE),
        run_by(agent, Some("harness")),
    )
    .unwrap();
    assert_eq!(runs(&store, "harness", agent).unwrap(), Some(agent));
    assert_eq!(
        runs(&store, "chat", agent).unwrap_err().code,
        code::UNAUTHORIZED
    );
    assert_eq!(
        runs(&store, "harness", alice).unwrap_err().code,
        code::UNAUTHORIZED
    );
    let Reply::Profile(Some(profile)) = query(&store, Query::Profile { number: agent }) else {
        panic!()
    };
    assert_eq!(
        profile.kind,
        Kind::Program {
            manager: alice,
            category: Category::Agent,
            standing: Standing::Active,
            script: Some(script("harness")),
        }
    );

    run(
        &store,
        &signed(&store, ALICE),
        Op::Suspend { account: agent },
    )
    .unwrap();
    assert_eq!(
        runs(&store, "harness", agent).unwrap_err().code,
        code::UNAUTHORIZED
    );
    run(
        &store,
        &signed(&store, ALICE),
        Op::Resume { account: agent },
    )
    .unwrap();
    run(&store, &signed(&store, ALICE), run_by(agent, None)).unwrap();
    assert_eq!(
        runs(&store, "harness", agent).unwrap_err().code,
        code::UNAUTHORIZED
    );

    run(
        &store,
        &signed(&store, ALICE),
        run_by(agent, Some("harness")),
    )
    .unwrap();
    run(
        &store,
        &signed(&store, ALICE),
        Op::Revoke { account: agent },
    )
    .unwrap();
    assert_eq!(
        runs(&store, "harness", agent).unwrap_err().code,
        code::WRONG_STATE
    );
    let revoked = refused(&store, &signed(&store, ALICE), run_by(agent, None));
    assert_eq!(revoked.code, code::WRONG_STATE);
}
