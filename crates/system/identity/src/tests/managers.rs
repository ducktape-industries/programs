// SetManager: a manager hands a program account to another account, or to itself, at once; one
// that manages itself runs its own day, and only the validators' vote changes its standing.

use super::*;

fn hand(account: u64, manager: u64) -> Op {
    Op::SetManager { account, manager }
}

fn managed_by(store: &MockHost, by: u64) -> Vec<u64> {
    let asked = Query::Managed {
        by,
        page: PageRequest::first(10),
    };
    let Reply::Accounts(managed) = query(store, asked) else {
        panic!("Managed answers accounts");
    };
    managed.items.iter().map(|account| account.number).collect()
}

fn manager_of(store: &MockHost, number: u64) -> u64 {
    match get(store, number).control {
        Control::Program { manager, .. } => manager,
        other => panic!("{other:?}"),
    }
}

#[track_caller]
fn cleared(store: &MockHost, number: u64) {
    let sent: Vec<(::program::ProgramId, access::Op)> = store
        .take_emissions()
        .iter()
        .map(|message| {
            let op = abi::decode(&message.payload).expect("an access op");
            (message.target.clone(), op)
        })
        .collect();
    let clear = access::Op::Set {
        program: MockHost::role(abi::role::IDENTITY),
        key: crate::account_key(number),
        list: None,
    };
    assert_eq!(
        sent,
        [(MockHost::role(abi::role::ACCESS), clear)],
        "the list on the account's key goes with the move"
    );
}

fn writers(store: &MockHost, number: u64) -> ::program::Writers {
    let ctx = store.query(env(Origin::Root, None, 100));
    abi::role::access::writers::<::program::Error>(
        &crate::account_key(number),
        |_| Ok(None),
        |at| Identity::resource(&ctx, at),
    )
    .unwrap()
}

#[test]
fn a_manager_hands_an_agent_to_another_account_at_once() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    let bob = create(&store, BOB, "Bob");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let narrow = Op::SetOwnership {
        account: agent,
        ownership: Ownership::Listed(vec![Party::Account(bob)]),
    };
    run(&store, &signed(&store, ALICE), narrow).unwrap();
    assert!(hears(&store, agent, alice).unwrap());
    assert!(hears(&store, agent, bob).unwrap());
    assert!(
        needs(&store, &hand(agent, bob)).is_empty(),
        "identity checks the sender itself"
    );

    run(&store, &signed(&store, ALICE), hand(agent, bob)).unwrap();
    cleared(&store, agent);
    assert_eq!(manager_of(&store, agent), bob);
    assert!(managed_by(&store, alice).is_empty());
    assert_eq!(managed_by(&store, bob), [agent]);
    assert_eq!(owner_of(&store, agent), Some(Party::Account(bob)));
    assert_eq!(
        answers(&store, agent),
        Some(Ownership::Owner),
        "settled against its new manager"
    );
    assert!(hears(&store, agent, bob).unwrap());
    assert!(
        !hears(&store, agent, alice).unwrap(),
        "its old manager is heard no more"
    );

    let old = refused(&store, &signed(&store, ALICE), hand(agent, alice));
    assert_eq!(old.code, code::UNAUTHORIZED);
    run(&store, &signed(&store, BOB), hand(agent, alice)).unwrap();
    cleared(&store, agent);
    assert_eq!(manager_of(&store, agent), alice);
    assert_eq!(managed_by(&store, alice), [agent]);
    assert!(managed_by(&store, bob).is_empty());
}

#[test]
fn a_move_is_refused_where_nothing_would_move() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    let bob = create(&store, BOB, "Bob");
    let forge_account = account_of(&store, "forge");
    let forge = from_program(&store, "forge");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();
    let by_alice = signed(&store, ALICE);
    let by_bob = signed(&store, BOB);
    let by_itself = signed(&store, BOT);
    let cases = [
        (
            &by_alice,
            hand(alice, bob),
            code::WRONG_STATE,
            "a user has no manager",
        ),
        (
            &forge,
            hand(forge_account, bob),
            code::WRONG_STATE,
            "nor a system account",
        ),
        (
            &by_alice,
            hand(agent, 99),
            code::NOT_FOUND,
            "no such account",
        ),
        (
            &by_alice,
            hand(agent, alice),
            code::WRONG_STATE,
            "already its manager",
        ),
        (
            &by_bob,
            hand(agent, bob),
            code::UNAUTHORIZED,
            "not its manager",
        ),
        (
            &by_itself,
            hand(agent, agent),
            code::UNAUTHORIZED,
            "a managed agent itself",
        ),
    ];
    for (env, op, wanted, why) in cases {
        assert_eq!(refused(&store, env, op).code, wanted, "{why}");
    }

    let gone = create_agent(&store, ALICE, "Gone");
    run(&store, &by_alice, Op::Revoke { account: gone }).unwrap();
    let to_revoked = refused(&store, &by_alice, hand(agent, gone));
    assert_eq!(
        to_revoked.code,
        code::WRONG_STATE,
        "a revoked account manages nothing"
    );
    let of_revoked = refused(&store, &by_alice, hand(gone, bob));
    assert_eq!(
        of_revoked.code,
        code::WRONG_STATE,
        "a revoked account moves no more"
    );
    assert!(
        store.take_emissions().is_empty(),
        "a refused move clears no list"
    );

    run(&store, &by_alice, Op::Suspend { account: agent }).unwrap();
    run(&store, &by_alice, hand(agent, bob)).unwrap();
    cleared(&store, agent);
    assert_eq!(
        manager_of(&store, agent),
        bob,
        "a suspended agent moves too"
    );
}

#[test]
fn an_agent_that_manages_itself_runs_its_day_and_only_the_vote_changes_its_standing() {
    let store = memory();
    let alice = create(&store, ALICE, "Alice");
    let validators = MockHost::role(abi::role::VALIDATORS);
    account_of(&store, &validators);
    account_of(&store, "harness");
    account_of(&store, "forge");
    let agent = create_agent(&store, ALICE, "Scout");
    add_agent_key(&store, ALICE, agent, BOT).unwrap();

    run(&store, &signed(&store, ALICE), hand(agent, agent)).unwrap();
    cleared(&store, agent);
    assert_eq!(manager_of(&store, agent), agent);
    assert_eq!(managed_by(&store, agent), [agent]);
    assert!(managed_by(&store, alice).is_empty());
    assert_eq!(
        owner_of(&store, agent),
        Some(Party::Program(validators.clone()))
    );
    let own_key = writers(&store, agent);
    assert!(
        own_key.allows(&[Party::Account(agent)]),
        "it writes its own key"
    );
    assert!(
        own_key.allows(&[Party::Program(validators.clone())]),
        "and so does the vote"
    );
    assert!(
        !own_key.allows(&[Party::Account(alice)]),
        "its old manager no more"
    );

    let itself = signed(&store, BOT);
    let rename = Op::SetName {
        account: agent,
        name: "Scout II".into(),
    };
    run(&store, &itself, rename).unwrap();
    run(&store, &itself, run_by(agent, Some("harness"))).unwrap();
    let answers_only_itself = Op::SetOwnership {
        account: agent,
        ownership: Ownership::Owner,
    };
    run(&store, &itself, answers_only_itself).unwrap();
    assert_eq!(get(&store, agent).card.name, "Scout II");
    assert_eq!(runs(&store, "harness", agent).unwrap(), Some(agent));

    let standing = [
        (itself.clone(), Op::Suspend { account: agent }),
        (itself.clone(), Op::Resume { account: agent }),
        (itself.clone(), Op::Revoke { account: agent }),
        (itself.clone(), hand(agent, alice)),
        (itself.clone(), agent_key(agent, SPARE, 0)),
        (signed(&store, SPARE), consented_by(BOT, agent, SPARE, 0)),
        (signed(&store, ALICE), Op::Suspend { account: agent }),
        (signed(&store, ALICE), hand(agent, alice)),
        (
            from_program(&store, "forge"),
            Op::Suspend { account: agent },
        ),
    ];
    for (env, op) in standing {
        let why = format!("{op:?}");
        let refusal = refused(&store, &env, op);
        assert_eq!(refusal.code, code::UNAUTHORIZED, "{why}");
        assert_eq!(
            refusal.message,
            format!(
                "account {agent} manages itself: only the validators' vote changes its standing"
            ),
            "{why}"
        );
    }
    assert_eq!(keys_of(&get(&store, agent)), [BOT.to_vec()]);
    assert!(store.take_emissions().is_empty());

    let vote = from_program(&store, &validators);
    run(&store, &vote, Op::Suspend { account: agent }).unwrap();
    assert_eq!(design::note(&get(&store, agent).kind()), Some("suspended"));
    run(&store, &vote, Op::Resume { account: agent }).unwrap();
    assert_eq!(design::note(&get(&store, agent).kind()), None);
    run(&store, &vote, agent_key(agent, SPARE, 0)).unwrap();
    assert_eq!(keys_of(&get(&store, agent)), [BOT.to_vec(), SPARE.to_vec()]);
    run(&store, &vote, hand(agent, alice)).unwrap();
    cleared(&store, agent);
    assert_eq!(manager_of(&store, agent), alice);
    assert_eq!(owner_of(&store, agent), Some(Party::Account(alice)));
    run(
        &store,
        &signed(&store, ALICE),
        Op::Suspend { account: agent },
    )
    .unwrap();
    assert_eq!(design::note(&get(&store, agent).kind()), Some("suspended"));
}
