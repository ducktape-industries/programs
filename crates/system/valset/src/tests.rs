// The program natively over `program::MockHost`: what the founding suite checks on the host, without the host.

use ::program::{Env, Error, MockHost, Origin, Principal, Program, Roster, code};
use abi::role::identity::{AccountNumber, Category, Kind, Profile, Standing};
use programs::{Change, Scheduled};
use store::PageRequest;

use crate::{
    Genesis, Grant, INVITE_NAMESPACE, Invite, Member, Membership, Motion, Op, Pending, Query,
    QueryPolicy, Reply, Role, Valset, Voted,
};

const NETWORK: &[u8] = b"net";
const TIME: u64 = 1_000;

fn key(n: u8) -> Vec<u8> {
    vec![n; 32]
}

fn env(origin: Origin) -> Env {
    Env {
        chain_id: NETWORK.to_vec(),
        height: 3,
        time: TIME,
        origin,
        sender: None,
        ..MockHost::env(crate::PROGRAM)
    }
}

fn halo(n: u8) -> String {
    format!("node-{n}.example.org:9100")
}

fn member(n: u8) -> Member {
    Member {
        key: key(n),
        address: Some(format!("node-{n}")),
        halo: halo(n),
    }
}

fn membership(n: u8, role: Role) -> Membership {
    Membership {
        key: key(n),
        address: Some(format!("node-{n}")),
        halo: halo(n),
        role,
    }
}

fn found(validators: &[u8], member_cap: u32) -> Result<MockHost, Error> {
    let store = MockHost::default();
    store.borrow_mut().verifier = Some(Box::new(|_, key, namespace, message, signature| {
        namespace == INVITE_NAMESPACE && signature == [key, message].concat()
    }));
    let genesis = abi::encode(&Genesis {
        validators: validators.iter().rev().map(|n| member(*n)).collect(),
        member_cap,
        queries: abi::role::validators::QueryPolicy::Signed,
    });
    Valset::init(&store.exec(env(Origin::Root)), &genesis)?;
    Ok(store)
}

fn founded() -> MockHost {
    found(&[1, 2], 8).unwrap()
}

fn run(store: &MockHost, signer: u8, op: Op) -> Result<(), Error> {
    store.attempt(|| Valset::execute(&store.exec(env(Origin::Signed(key(signer)))), op))
}

fn refused(store: &MockHost, signer: u8, op: Op) -> String {
    store
        .refused(|| Valset::execute(&store.exec(env(Origin::Signed(key(signer)))), op))
        .code
}

fn enroll(n: u8, invite: Option<Invite>) -> Op {
    Op::Enroll {
        address: Some(format!("node-{n}")),
        halo: halo(n),
        invite,
    }
}

fn vote(store: &MockHost, voter: u8, motion: Motion) -> Result<Voted, Error> {
    run(store, voter, Op::Vote(motion))?;
    Ok(abi::decode(&store.take_output()).unwrap())
}

fn pass(store: &MockHost, voters: &[u8], motion: Motion) {
    let (last, first) = voters.split_last().unwrap();
    for voter in first {
        let counted = vote(store, *voter, motion.clone()).unwrap();
        assert!(matches!(counted, Voted::Counted { .. }), "{counted:?}");
    }
    assert_eq!(vote(store, *last, motion).unwrap(), Voted::Enacted);
}

fn signed(issuer: u8, grant: Grant) -> Invite {
    let signature = [key(issuer), grant.preimage()].concat();
    Invite {
        issuer: key(issuer),
        grant,
        signature,
    }
}

fn invite(issuer: u8, nonce: u8) -> Invite {
    signed(
        issuer,
        Grant {
            network: NETWORK.to_vec(),
            nonce: [nonce; 16],
            expires: TIME + 1,
        },
    )
}

fn ask(store: &MockHost, query: Query) -> Reply {
    Valset::query(&store.query(env(Origin::Root)), query).unwrap()
}

fn held(store: &MockHost, n: u8) -> Option<Membership> {
    match ask(store, Query::Membership { key: key(n) }) {
        Reply::Membership(membership) => membership,
        other => panic!("{other:?}"),
    }
}

fn validators(store: &MockHost) -> Vec<Vec<u8>> {
    match ask(store, Query::Validators) {
        Reply::Validators(validators) => validators,
        other => panic!("{other:?}"),
    }
}

#[test]
fn founding_seats_the_validators_in_key_order() {
    let store = founded();
    assert_eq!(validators(&store), [key(1), key(2)]);
    let Reply::Members(members) = ask(&store, Query::Members) else {
        panic!()
    };
    assert_eq!(members[0], member(1));
}

#[test]
fn founding_more_validators_than_the_cap_is_refused() {
    let refused = found(&[1, 2], 1).err();
    assert_eq!(refused.map(|error| error.code), Some(code::CAPACITY.into()));
}

#[test]
fn founding_a_validator_at_a_halo_address_no_member_dials_is_refused() {
    for unregistrable in [
        "0.0.0.0:9100",
        "127.0.0.1:9100",
        "[::1]:9100",
        "localhost:9100",
    ] {
        let genesis = abi::encode(&Genesis {
            validators: vec![
                member(1),
                Member {
                    halo: unregistrable.into(),
                    ..member(2)
                },
            ],
            member_cap: 8,
            queries: abi::role::validators::QueryPolicy::Signed,
        });
        let store = MockHost::default();
        let refused = Valset::init(&store.exec(env(Origin::Root)), &genesis).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{unregistrable}");
        assert!(refused.message.starts_with("halo: "), "{refused:?}");
    }
}

#[test]
fn only_a_signed_frame_writes() {
    let store = founded();
    for origin in [Origin::Root, Origin::Program("probe".into())] {
        let refusal = store.refused(|| Valset::execute(&store.exec(env(origin)), enroll(3, None)));
        assert_eq!(refusal.code, code::UNAUTHORIZED);
    }
}

#[test]
fn the_closed_door_refuses_an_uninvited_stranger() {
    let store = founded();
    assert_eq!(refused(&store, 3, enroll(3, None)), code::UNAUTHORIZED);
    assert_eq!(held(&store, 3), None);
}

#[test]
fn an_invite_admits_a_resident_once() {
    let store = founded();
    let ticket = invite(1, 7);
    run(&store, 3, enroll(3, Some(ticket.clone()))).unwrap();
    assert_eq!(held(&store, 3), Some(membership(3, Role::Resident)));
    assert_eq!(validators(&store), [key(1), key(2)]);
    assert_eq!(
        refused(&store, 4, enroll(4, Some(ticket))),
        code::WRONG_STATE
    );
    run(&store, 4, enroll(4, Some(invite(2, 7)))).unwrap();
}

#[test]
fn an_expired_foreign_forged_or_resident_issued_invite_is_refused() {
    let store = founded();
    let expired = signed(
        1,
        Grant {
            expires: TIME,
            ..invite(1, 1).grant
        },
    );
    assert_eq!(
        refused(&store, 3, enroll(3, Some(expired))),
        code::INVALID_INPUT
    );
    let foreign = signed(
        1,
        Grant {
            network: b"elsewhere".to_vec(),
            ..invite(1, 2).grant
        },
    );
    assert_eq!(
        refused(&store, 3, enroll(3, Some(foreign))),
        code::INVALID_INPUT
    );
    let forged = Invite {
        signature: invite(9, 3).signature,
        ..invite(1, 3)
    };
    assert_eq!(
        refused(&store, 3, enroll(3, Some(forged))),
        code::UNAUTHORIZED
    );
    run(&store, 3, enroll(3, Some(invite(1, 4)))).unwrap();
    assert_eq!(
        refused(&store, 4, enroll(4, Some(invite(3, 5)))),
        code::UNAUTHORIZED
    );
    let stranger = invite(9, 6);
    assert_eq!(
        refused(&store, 4, enroll(4, Some(stranger))),
        code::UNAUTHORIZED
    );
}

#[test]
fn an_open_door_admits_without_an_invite() {
    let store = founded();
    pass(&store, &[1, 2], Motion::Door { open: true });
    run(&store, 3, enroll(3, None)).unwrap();
    assert_eq!(held(&store, 3), Some(membership(3, Role::Resident)));
    let again = vote(&store, 1, Motion::Door { open: true }).unwrap_err();
    assert_eq!(again.code, code::WRONG_STATE);
    pass(&store, &[1, 2], Motion::Door { open: false });
    assert_eq!(refused(&store, 4, enroll(4, None)), code::UNAUTHORIZED);
}

fn queries(store: &MockHost) -> QueryPolicy {
    match ask(store, Query::QueryPolicy) {
        Reply::QueryPolicy(policy) => policy,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_vote_sets_which_queries_the_network_answers() {
    let store = founded();
    assert_eq!(queries(&store), QueryPolicy::Signed);
    let open = || Motion::Queries {
        policy: QueryPolicy::Open,
    };
    pass(&store, &[1, 2], open());
    assert_eq!(queries(&store), QueryPolicy::Open);
    let again = vote(&store, 1, open()).unwrap_err();
    assert_eq!(again.code, code::WRONG_STATE);
    let signed = Motion::Queries {
        policy: QueryPolicy::Signed,
    };
    pass(&store, &[1, 2], signed);
    assert_eq!(queries(&store), QueryPolicy::Signed);
}

#[test]
fn a_network_founded_before_the_query_policy_answers_signed_queries_only() {
    let store = founded();
    let open = Motion::Queries {
        policy: QueryPolicy::Open,
    };
    pass(&store, &[1, 2], open);
    let held = store.borrow_mut().state.remove(b"queries".as_slice());
    assert!(held.is_some(), "the policy is the row `queries`");
    assert_eq!(queries(&store), QueryPolicy::Signed);
}

#[test]
fn a_member_enrolling_again_moves_its_address_and_keeps_its_role() {
    let store = founded();
    let moved = Op::Enroll {
        address: Some("elsewhere".into()),
        halo: halo(1),
        invite: None,
    };
    run(&store, 1, moved).unwrap();
    assert_eq!(
        held(&store, 1),
        Some(Membership {
            address: Some("elsewhere".into()),
            ..membership(1, Role::Validator)
        })
    );
}

#[test]
fn a_member_enrolling_without_an_address_is_held_without_one() {
    let store = founded();
    let undialed = Op::Enroll {
        address: None,
        halo: halo(1),
        invite: None,
    };
    run(&store, 1, undialed).unwrap();
    assert_eq!(
        held(&store, 1),
        Some(Membership {
            address: None,
            ..membership(1, Role::Validator)
        })
    );
}

#[test]
fn a_full_network_refuses_a_newcomer_and_a_key_is_32_bytes() {
    let store = found(&[1, 2], 3).unwrap();
    run(&store, 3, enroll(3, Some(invite(1, 1)))).unwrap();
    assert_eq!(
        refused(&store, 4, enroll(4, Some(invite(1, 2)))),
        code::CAPACITY
    );
    run(&store, 3, enroll(3, None)).unwrap();
    let short = store.refused(|| {
        Valset::execute(
            &store.exec(env(Origin::Signed(vec![5, 5]))),
            enroll(5, Some(invite(1, 3))),
        )
    });
    assert_eq!(short.code, code::INVALID_INPUT);
}

#[test]
fn only_a_validator_votes() {
    let store = founded();
    run(&store, 3, enroll(3, Some(invite(1, 1)))).unwrap();
    let promote = Motion::Promote { key: key(3) };
    assert_eq!(
        refused(&store, 3, Op::Vote(promote.clone())),
        code::UNAUTHORIZED
    );
    assert_eq!(refused(&store, 9, Op::Vote(promote)), code::UNAUTHORIZED);
}

#[test]
fn a_motion_passes_at_the_quorum_of_the_current_validators() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    run(&store, 5, enroll(5, Some(invite(1, 1)))).unwrap();
    let promote = Motion::Promote { key: key(5) };
    let counted = |votes| Voted::Counted { votes, needed: 3 };
    assert_eq!(vote(&store, 1, promote.clone()).unwrap(), counted(1));
    assert_eq!(vote(&store, 1, promote.clone()).unwrap(), counted(1));
    assert_eq!(vote(&store, 2, promote.clone()).unwrap(), counted(2));
    assert_eq!(held(&store, 5).unwrap().role, Role::Resident);
    assert_eq!(vote(&store, 3, promote.clone()).unwrap(), Voted::Enacted);
    assert_eq!(held(&store, 5).unwrap().role, Role::Validator);
    let again = vote(&store, 4, promote).unwrap_err();
    assert_eq!(again.code, code::WRONG_STATE);
}

#[test]
fn a_voter_no_longer_a_validator_is_dropped_at_tally() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    assert_eq!(
        vote(&store, 4, Motion::Remove { key: key(1) }).unwrap(),
        Voted::Counted {
            votes: 1,
            needed: 3
        }
    );
    pass(&store, &[1, 2, 3], Motion::Remove { key: key(4) });
    assert_eq!(
        vote(&store, 2, Motion::Remove { key: key(1) }).unwrap(),
        Voted::Counted {
            votes: 1,
            needed: 3
        }
    );
}

#[test]
fn a_removal_clears_the_open_motions_that_name_the_member() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    run(&store, 5, enroll(5, Some(invite(1, 1)))).unwrap();
    vote(&store, 1, Motion::Promote { key: key(5) }).unwrap();
    vote(&store, 2, Motion::Promote { key: key(5) }).unwrap();
    pass(&store, &[1, 2, 3], Motion::Remove { key: key(5) });
    assert_eq!(held(&store, 5), None);
    run(&store, 5, enroll(5, Some(invite(1, 2)))).unwrap();
    assert_eq!(
        vote(&store, 3, Motion::Promote { key: key(5) }).unwrap(),
        Voted::Counted {
            votes: 1,
            needed: 3
        }
    );
}

#[test]
fn a_motion_on_the_wrong_role_or_a_stranger_is_refused() {
    let store = founded();
    let promote_a_validator = Op::Vote(Motion::Promote { key: key(1) });
    assert_eq!(refused(&store, 1, promote_a_validator), code::WRONG_STATE);
    run(&store, 3, enroll(3, Some(invite(1, 1)))).unwrap();
    let demote_a_resident = Op::Vote(Motion::Demote { key: key(3) });
    assert_eq!(refused(&store, 1, demote_a_resident), code::WRONG_STATE);
    let remove_a_stranger = Op::Vote(Motion::Remove { key: key(8) });
    assert_eq!(refused(&store, 1, remove_a_stranger), code::NOT_FOUND);
}

#[test]
fn the_last_validator_is_held() {
    let store = founded();
    pass(&store, &[1, 2], Motion::Demote { key: key(2) });
    assert_eq!(validators(&store), [key(1)]);
    assert_eq!(held(&store, 2).unwrap().role, Role::Resident);
    let demote = Op::Vote(Motion::Demote { key: key(1) });
    assert_eq!(refused(&store, 1, demote), code::WRONG_STATE);
    let remove = Op::Vote(Motion::Remove { key: key(1) });
    assert_eq!(refused(&store, 1, remove), code::WRONG_STATE);
    assert_eq!(refused(&store, 1, Op::Leave), code::WRONG_STATE);
    assert_eq!(validators(&store), [key(1)]);
}

#[test]
fn a_member_leaves_and_a_stranger_cannot() {
    let store = founded();
    run(&store, 3, enroll(3, Some(invite(1, 1)))).unwrap();
    run(&store, 3, Op::Leave).unwrap();
    assert_eq!(held(&store, 3), None);
    run(&store, 2, Op::Leave).unwrap();
    assert_eq!(validators(&store), [key(1)]);
    assert_eq!(refused(&store, 4, Op::Leave), code::NOT_FOUND);
}

#[test]
fn a_role_motion_sends_its_op_to_the_role_once_enacted_and_takes_its_refusal() {
    let store = founded();
    let op = abi::encode(&programs::Op::Cancel {
        height: 1_000,
        program: "chat".into(),
    });
    let motion = Motion::Role {
        role: abi::role::PROGRAMS.into(),
        op: op.clone(),
    };
    vote(&store, 1, motion.clone()).unwrap();
    assert!(store.take_emissions().is_empty());
    assert_eq!(vote(&store, 2, motion).unwrap(), Voted::Enacted);
    let sent = store.take_emissions();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].target, MockHost::role(abi::role::PROGRAMS));
    assert_eq!(sent[0].payload, op);
    assert!(sent[0].reply);
    let ctx = store.exec(env(Origin::Program(MockHost::role(abi::role::PROGRAMS))));
    let refused = ::program::Outcome::Rejected(Error::new(code::NOT_FOUND, "no such change"));
    let id = ::program::MessageId {
        program: crate::PROGRAM.into(),
        seq: 0,
    };
    assert_eq!(Valset::reply(&ctx, &id, &refused), Ok(()));
    assert!(motions(&store, 3).1.is_empty());
}

#[test]
fn a_halo_role_motion_sends_its_op_to_the_halo_once_enacted() {
    let store = founded();
    let lanes = MockHost::default();
    let founding = halo::Genesis { lanes: Vec::new() };
    halo::Halo::init(
        &lanes.exec(MockHost::env(halo::PROGRAM).root()),
        &abi::encode(&founding),
    )
    .unwrap();
    store.sibling::<halo::Halo>(MockHost::role(abi::role::HALO), &lanes);
    let op = abi::encode(&halo::Op::Open {
        name: "chat".into(),
        cap: halo::Class::Member,
    });
    let motion = Motion::Role {
        role: abi::role::HALO.into(),
        op: op.clone(),
    };
    vote(&store, 1, motion.clone()).unwrap();
    assert!(store.take_emissions().is_empty());
    assert_eq!(vote(&store, 2, motion).unwrap(), Voted::Enacted);
    let sent = store.take_emissions();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].target, MockHost::role(abi::role::HALO));
    assert_eq!(sent[0].payload, op);
    assert!(sent[0].reply);
}

#[test]
fn a_member_registers_a_halo_address_other_members_dial_and_moves_it_alone() {
    let store = founded();
    for unregistrable in [
        "0.0.0.0:9100",
        "[::]:9100",
        "127.0.0.1:9100",
        "[::1]:9100",
        "localhost:9100",
        "node-3.example.org",
    ] {
        let enroll = Op::Enroll {
            address: None,
            halo: unregistrable.into(),
            invite: Some(invite(1, 1)),
        };
        let refusal =
            store.refused(|| Valset::execute(&store.exec(env(Origin::Signed(key(3)))), enroll));
        assert_eq!(refusal.code, code::INVALID_INPUT, "{unregistrable}");
        assert!(refusal.message.starts_with("halo: "), "{refusal:?}");
        let moved = Op::SetHalo {
            halo: unregistrable.into(),
        };
        assert_eq!(refused(&store, 1, moved), code::INVALID_INPUT);
    }
    assert_eq!(held(&store, 3), None);

    let moved = Op::SetHalo {
        halo: "203.0.113.7:9100".into(),
    };
    run(&store, 1, moved.clone()).unwrap();
    assert_eq!(
        held(&store, 1),
        Some(Membership {
            halo: "203.0.113.7:9100".into(),
            ..membership(1, Role::Validator)
        })
    );
    assert_eq!(refused(&store, 7, moved), code::NOT_FOUND);
}

fn halo_cleared(store: &MockHost) -> Vec<Vec<u8>> {
    match ask(store, Query::HaloCleared) {
        Reply::HaloCleared(cleared) => cleared,
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_explicit_member_action_clears_a_direct_halo() {
    let store = founded();
    assert!(halo_cleared(&store).is_empty());
    run(&store, 1, Op::SetHalo { halo: halo(1) }).unwrap();
    assert!(halo_cleared(&store).is_empty());
    assert_eq!(refused(&store, 7, Op::ClearHalo), code::NOT_FOUND);
    run(&store, 2, Op::ClearHalo).unwrap();
    run(&store, 1, Op::ClearHalo).unwrap();
    run(&store, 1, Op::ClearHalo).unwrap();
    assert_eq!(halo_cleared(&store), [key(1), key(2)]);
    assert_eq!(held(&store, 1), Some(membership(1, Role::Validator)));
    run(&store, 1, Op::SetHalo { halo: halo(1) }).unwrap();
    assert_eq!(halo_cleared(&store), [key(2)]);
    run(&store, 2, enroll(2, None)).unwrap();
    assert!(halo_cleared(&store).is_empty());
}

#[test]
fn leaving_or_removal_erases_a_members_explicit_halo_choice() {
    let store = founded();
    run(&store, 3, enroll(3, Some(invite(1, 1)))).unwrap();
    run(&store, 3, Op::ClearHalo).unwrap();
    run(&store, 3, Op::Leave).unwrap();
    assert!(halo_cleared(&store).is_empty());
    run(&store, 3, enroll(3, Some(invite(1, 2)))).unwrap();
    assert!(halo_cleared(&store).is_empty());
    run(&store, 3, Op::ClearHalo).unwrap();
    pass(&store, &[1, 2], Motion::Remove { key: key(3) });
    assert_eq!(held(&store, 3), None);
    assert!(halo_cleared(&store).is_empty());
}

#[test]
fn halo_cleared_uses_the_existing_current_core_role_codec() {
    assert_eq!(
        abi::encode(&Query::HaloCleared),
        abi::encode(&abi::role::validators::Query::HaloCleared)
    );
    assert_eq!(
        abi::encode(&Reply::HaloCleared(vec![key(1)])),
        abi::encode(&abi::role::validators::Reply::HaloCleared(vec![key(1)]))
    );
}

#[test]
fn memberships_page_in_key_order_at_the_answering_height() {
    let store = founded();
    run(&store, 3, enroll(3, Some(invite(1, 1)))).unwrap();
    let Reply::Memberships(first) = ask(
        &store,
        Query::Memberships {
            page: PageRequest::first(2),
        },
    ) else {
        panic!()
    };
    assert_eq!(first.height, 3);
    assert_eq!(
        first.items.iter().map(|m| m.key[0]).collect::<Vec<_>>(),
        [1, 2]
    );
    let Reply::Memberships(rest) = ask(
        &store,
        Query::Memberships {
            page: PageRequest::resume(first.next, 2),
        },
    ) else {
        panic!()
    };
    assert_eq!(rest.items, [membership(3, Role::Resident)]);
    assert_eq!(rest.next, None);
}

#[test]
fn role_asks_the_program_bound_to_the_validators_role() {
    let store = founded();
    store.sibling::<Valset>("third-party-valset", &store);
    let mut bound = env(Origin::Root);
    bound.roles = MockHost::roles_with(abi::role::VALIDATORS, "third-party-valset");
    assert_eq!(
        crate::role(&store.query(bound), &key(1)).unwrap(),
        Some(Role::Validator)
    );
}

const ADA: AccountNumber = 10;
const BOB: AccountNumber = 14;

fn profile(number: AccountNumber, kind: Kind) -> Profile {
    Profile {
        number,
        name: format!("account {number}"),
        kind,
    }
}

fn agent(number: AccountNumber, standing: Standing) -> Profile {
    profile(
        number,
        Kind::Program {
            script: None,
            manager: ADA,
            category: Category::Agent,
            standing,
        },
    )
}

fn roster(held: &[(u8, AccountNumber)], agents: Vec<Profile>) -> Roster {
    let mut roster = Roster::from(agents);
    for user in [ADA, BOB] {
        roster.profiles.insert(user, profile(user, Kind::User));
    }
    roster.keys.insert(key(20), ADA);
    roster.keys.insert(key(21), BOB);
    for (n, account) in held {
        roster.keys.insert(key(*n), *account);
    }
    roster
}

fn delegated() -> MockHost {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    store.identity(roster(&[(1, 11)], vec![agent(11, Standing::Active)]));
    store
}

fn device(store: &MockHost, n: u8, account: AccountNumber, motion: Motion) -> Result<Voted, Error> {
    let frame = Env {
        sender: Some(Principal::Account(account)),
        ..env(Origin::Signed(key(n)))
    };
    store.attempt(|| Valset::execute(&store.exec(frame), Op::Vote(motion)))?;
    Ok(abi::decode(&store.take_output()).unwrap())
}

fn voices(store: &MockHost, n: u8) -> Vec<Vec<u8>> {
    match ask(store, Query::Voices { key: key(n) }) {
        Reply::Voices(voices) => voices,
        other => panic!("{other:?}"),
    }
}

fn motions(store: &MockHost, height: u64) -> (u64, Vec<Pending>) {
    let at = Env {
        height,
        ..env(Origin::Root)
    };
    let query = Query::Motions {
        page: PageRequest::first(16),
    };
    match Valset::query(&store.query(at), query).unwrap() {
        Reply::Motions { needed, page } => (needed, page.items),
        other => panic!("{other:?}"),
    }
}

fn counted(votes: u64) -> Voted {
    Voted::Counted { votes, needed: 3 }
}

fn programs_role(op: &programs::Op) -> Motion {
    Motion::Role {
        role: abi::role::PROGRAMS.into(),
        op: abi::encode(op),
    }
}

fn schedule(height: u64) -> Motion {
    let op = programs::Op::ScheduleOver {
        scheduled: Scheduled {
            height,
            change: Change::Remove("chat".into()),
        },
        expected: None,
    };
    programs_role(&op)
}

#[test]
fn a_validator_votes_as_itself_and_asks_identity_nothing() {
    let store = founded();
    let open = Motion::Door { open: true };
    let one = Voted::Counted {
        votes: 1,
        needed: 2,
    };
    assert_eq!(vote(&store, 1, open.clone()).unwrap(), one);
    assert_eq!(vote(&store, 2, open).unwrap(), Voted::Enacted);
    assert_eq!(voices(&store, 1), [key(1)]);
}

#[test]
fn a_managers_device_votes_for_the_validator_on_its_active_agent() {
    let store = delegated();
    let open = Motion::Door { open: true };
    assert_eq!(device(&store, 20, ADA, open.clone()).unwrap(), counted(1));
    let (_, pending) = motions(&store, 3);
    assert_eq!(pending[0].voters, [key(1)]);
    vote(&store, 2, open.clone()).unwrap();
    assert_eq!(vote(&store, 3, open).unwrap(), Voted::Enacted);
}

#[test]
fn a_suspended_agent_adds_no_voice_and_any_other_refusal_fails_the_vote() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    store.identity(roster(&[(1, 11)], vec![agent(11, Standing::Suspended)]));
    let open = Motion::Door { open: true };
    let alone = device(&store, 20, ADA, open.clone()).unwrap_err();
    assert_eq!(alone.code, code::UNAUTHORIZED);
    let agents = vec![agent(11, Standing::Active), agent(12, Standing::Suspended)];
    store.identity(roster(&[(1, 11), (2, 12)], agents));
    assert_eq!(device(&store, 20, ADA, open.clone()).unwrap(), counted(1));
    assert_eq!(voices(&store, 20), [key(1)]);
    store.borrow_mut().siblings.insert(
        MockHost::role(abi::role::IDENTITY),
        Box::new(|_, _| Err(Error::new(code::UNSUPPORTED, "not answered here"))),
    );
    let misbound = device(&store, 20, ADA, open).unwrap_err();
    assert_eq!(misbound.code, code::UNSUPPORTED);
}

#[test]
fn a_stranger_is_refused_and_a_node_key_on_a_user_votes_only_as_itself() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    store.identity(roster(&[(2, ADA)], vec![]));
    let open = Motion::Door { open: true };
    let bob = device(&store, 21, BOB, open.clone()).unwrap_err();
    assert_eq!(bob.code, code::UNAUTHORIZED);
    let ada = device(&store, 20, ADA, open.clone()).unwrap_err();
    assert_eq!(ada.code, code::UNAUTHORIZED);
    assert_eq!(device(&store, 2, ADA, open).unwrap(), counted(1));
    assert_eq!(voices(&store, 2), [key(2)]);
    assert!(voices(&store, 20).is_empty());
}

#[test]
fn one_frame_casts_every_voice_and_a_validator_counts_once() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    let agents = vec![agent(11, Standing::Active), agent(12, Standing::Active)];
    store.identity(roster(&[(1, 11), (2, 12)], agents));
    let open = Motion::Door { open: true };
    assert_eq!(device(&store, 20, ADA, open.clone()).unwrap(), counted(2));
    assert_eq!(vote(&store, 1, open.clone()).unwrap(), counted(2));
    assert_eq!(device(&store, 20, ADA, open.clone()).unwrap(), counted(2));
    assert_eq!(vote(&store, 3, open).unwrap(), Voted::Enacted);
}

#[test]
fn a_key_taken_off_the_agent_casts_no_new_votes_and_its_votes_stand() {
    let store = delegated();
    let open = Motion::Door { open: true };
    assert_eq!(device(&store, 20, ADA, open.clone()).unwrap(), counted(1));
    store.identity(roster(&[], vec![agent(11, Standing::Active)]));
    let gone = device(&store, 20, ADA, open.clone()).unwrap_err();
    assert_eq!(gone.code, code::UNAUTHORIZED);
    assert!(voices(&store, 20).is_empty());
    assert_eq!(vote(&store, 2, open).unwrap(), counted(2));
}

fn cancel(height: u64) -> Motion {
    let op = programs::Op::Cancel {
        height,
        program: "chat".into(),
    };
    programs_role(&op)
}

#[test]
fn a_role_motion_is_counted_unread_and_refused_only_for_an_unbound_role() {
    let store = delegated();
    assert_eq!(vote(&store, 1, schedule(2)).unwrap(), counted(1));
    assert_eq!(vote(&store, 1, cancel(1)).unwrap(), counted(1));
    let bad = programs_role(&programs::Op::ScheduleOver {
        scheduled: Scheduled {
            height: 9,
            change: Change::Remove("valset".into()),
        },
        expected: None,
    });
    assert_eq!(vote(&store, 1, bad).unwrap(), counted(1));
    let garbage = Motion::Role {
        role: abi::role::PROGRAMS.into(),
        op: b"not an op".to_vec(),
    };
    assert_eq!(vote(&store, 1, garbage).unwrap(), counted(1));
    assert_eq!(motions(&store, 3).1.len(), 4);
    let unbound = Motion::Role {
        role: "ledger".into(),
        op: Vec::new(),
    };
    assert_eq!(
        refused(&store, 1, Op::Vote(unbound.clone())),
        code::UNSUPPORTED
    );
    let store = founded();
    let unread = device(&store, 20, ADA, unbound).unwrap_err();
    assert_eq!(unread.code, code::UNSUPPORTED);
}

#[test]
fn an_identity_that_answers_the_account_but_refuses_its_profile_fails_the_vote() {
    let store = found(&[1, 2, 3, 4], 8).unwrap();
    let roster = roster(&[(1, 11)], vec![agent(11, Standing::Active)]);
    store.borrow_mut().siblings.insert(
        MockHost::role(abi::role::IDENTITY),
        Box::new(move |_, request| match abi::decode(request) {
            Ok(abi::role::identity::Query::Profile(_)) => {
                Err(Error::new(code::UNSUPPORTED, "no profiles here"))
            }
            _ => ::program::identity_role(&roster, request),
        }),
    );
    let open = Motion::Door { open: true };
    let refused = device(&store, 20, ADA, open).unwrap_err();
    assert_eq!(refused.code, code::UNSUPPORTED);
    assert!(motions(&store, 3).1.is_empty());
}

#[test]
fn a_page_of_motions_cut_short_resumes_past_what_it_answered() {
    let store = delegated();
    for height in [4, 5, 9] {
        vote(&store, 1, schedule(height)).unwrap();
    }
    let at = Env {
        height: 3,
        ..env(Origin::Root)
    };
    let first = Query::Motions {
        page: PageRequest::first(2),
    };
    let Reply::Motions { page, .. } = Valset::query(&store.query(at.clone()), first).unwrap()
    else {
        panic!()
    };
    assert_eq!(page.items.len(), 2);
    let rest = Query::Motions {
        page: PageRequest {
            after: page.next,
            limit: Some(2),
        },
    };
    let Reply::Motions { page, .. } = Valset::query(&store.query(at), rest).unwrap() else {
        panic!()
    };
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.next, None);
}

#[test]
fn motions_list_what_vote_stored_and_voices_are_the_set_vote_uses() {
    let store = delegated();
    let open = Motion::Door { open: true };
    device(&store, 20, ADA, open.clone()).unwrap();
    vote(&store, 4, open.clone()).unwrap();
    vote(&store, 2, schedule(5)).unwrap();
    let (needed, pending) = motions(&store, 3);
    assert_eq!(needed, 3);
    assert_eq!(
        pending,
        [
            Pending {
                motion: open.clone(),
                voters: vec![key(1), key(4)],
            },
            Pending {
                motion: schedule(5),
                voters: vec![key(2)],
            },
        ]
    );
    assert_eq!(motions(&store, 9).1.len(), 2);
    pass(&store, &[1, 2, 3], Motion::Remove { key: key(4) });
    let (needed, pending) = motions(&store, 3);
    assert_eq!(needed, 3);
    assert_eq!(pending[0].voters, [key(1)]);
    assert_eq!(voices(&store, 20), [key(1)]);
    assert_eq!(voices(&store, 1), [key(1)]);
    assert!(voices(&store, 21).is_empty());
}

#[test]
fn a_programs_vote_is_described_in_the_programss_words() {
    let Motion::Role { op, .. } = schedule(9) else {
        unreachable!()
    };
    let programs = programs::describe(&abi::decode(&op).unwrap());
    let described = crate::describe(&Op::Vote(schedule(9)));
    assert_eq!(described.title, "Vote · Schedule · chat");
    assert_eq!(described.fields, programs.fields);
    let garbage = Op::Vote(Motion::Role {
        role: abi::role::PROGRAMS.into(),
        op: b"not an op".to_vec(),
    });
    let described = crate::describe(&garbage);
    assert_eq!(described.title, "Vote · programs");
    assert_eq!(described.fields[0].label, "op");
}

#[test]
fn a_halo_vote_is_described_in_the_halos_words() {
    let op = halo::Op::Close {
        name: "chat".into(),
    };
    let described = crate::describe(&Op::Vote(Motion::Role {
        role: abi::role::HALO.into(),
        op: abi::encode(&op),
    }));
    assert_eq!(described.title, "Vote · Close lane · chat");
    assert_eq!(described.fields, halo::describe(&op).fields);
    let garbage = Op::Vote(Motion::Role {
        role: abi::role::HALO.into(),
        op: b"not an op".to_vec(),
    });
    let described = crate::describe(&garbage);
    assert_eq!(described.title, "Vote · halo");
    assert_eq!(described.fields[0].label, "op");
}

#[test]
fn another_roles_op_is_described_by_the_role_name() {
    let title = |motion: Motion| crate::describe(&Op::Vote(motion)).title;
    let other = Motion::Role {
        role: abi::role::EGRESS.into(),
        op: vec![1],
    };
    assert_eq!(title(other), "Vote · egress");
}
