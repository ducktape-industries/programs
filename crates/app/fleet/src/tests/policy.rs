//! fleet placing runs of a kind by that kind's policy in resources: admitted machines only, the
//! policy's class order and fair share, quotas, a bounded walk, Promote and a run's limit.
use resources::{Class, Counter, Group, KindSpec, Meter, Policy, PolicyId, Quota, Share};

use super::*;

const OWNER: &[u8] = b"kind-owner-key";
const OWNER_ACCOUNT: u64 = 41;
const CY: &[u8] = b"cy-key";
const CY_ACCOUNT: u64 = 51;
const BUILD: &str = "fleet.build";

fn kinded() -> World {
    let mut world = world();
    world.chain.seat::<resources::Resources>(resources::PROGRAM);
    world.chain.register(resources::PROGRAM, 905);
    world.chain.hold(OWNER, OWNER_ACCOUNT);
    world.chain.hold(CY, CY_ACCOUNT);
    world
}

fn class(name: &str, may_use: Option<PolicyId>) -> Class {
    Class {
        name: name.into(),
        may_use,
        notarized: false,
    }
}

impl World {
    fn resources(&self, op: &resources::Op) -> Result<Vec<u8>, Error> {
        self.chain
            .submit(Origin::Signed(OWNER.to_vec()), resources::PROGRAM, op)
    }

    fn register(&self, walk: u64) {
        let spec = KindSpec {
            meters: vec![Meter {
                name: BLOCKS.into(),
                unit: "block".into(),
                reported_by: Counter::Chain,
                checked_by: None,
            }],
            slot: "one build".into(),
            lapse: None,
            notary: None,
            walk,
        };
        let register = resources::Op::RegisterKind {
            name: BUILD.into(),
            spec,
        };
        self.resources(&register).unwrap();
    }

    fn admit(&self, provider: u64) {
        let admit = resources::Op::Admit {
            kind: BUILD.into(),
            provider,
        };
        self.resources(&admit).unwrap();
    }

    fn set_policy(&self, policy: Policy) {
        let set = resources::Op::SetPolicy {
            kind: BUILD.into(),
            policy,
        };
        self.resources(&set).unwrap();
    }

    fn members(&self, accounts: &[u64]) -> PolicyId {
        let create = ownership::Op::Create {
            name: "members".into(),
            write: ownership::Writers::Listed(
                accounts
                    .iter()
                    .map(|account| ownership::Party::Account(*account))
                    .collect(),
            ),
        };
        let made = self
            .chain
            .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &create)
            .unwrap();
        abi::decode(&made).unwrap()
    }

    fn ready_in(&self, key: &[u8], name: &str, slots: u64) {
        let ready = Op::ReadyIn {
            name: name.into(),
            labels: Vec::new(),
            slots,
            serves: Serves::Anyone,
            kind: Some(BUILD.into()),
        };
        self.act(key, &ready).unwrap();
    }

    fn ask_in(&self, key: &[u8], class: &str, limit: Option<u64>) -> Result<u64, Error> {
        let ask = Op::AskIn {
            spec: job(),
            kind: Some(BUILD.into()),
            class: class.into(),
            limit,
        };
        let made = self.act(key, &ask)?;
        Ok(abi::decode(&made).unwrap())
    }

    fn next(&self) {
        self.to(self.height.get() + 1);
    }

    fn filed(&self, run: u64) -> Filed {
        let Reply::Filed(Some(filed)) = self.chain.query(PROGRAM, &Query::Filed { run }).unwrap()
        else {
            panic!("no filing for run {run}");
        };
        filed
    }

    fn used(&self, group: &str) -> Vec<resources::LedgerRow> {
        let asked = Query::Ledger {
            kind: BUILD.into(),
            group: group.into(),
            page: PageRequest::default(),
        };
        let Reply::Ledger(page) = self.chain.query(PROGRAM, &asked).unwrap() else {
            panic!("Ledger answers rows");
        };
        page.items
    }
}

fn two_classes() -> Policy {
    Policy {
        classes: vec![class("high", None), class("low", None)],
        ..Policy::default()
    }
}

#[test]
fn a_run_of_a_kind_goes_only_to_a_machine_of_that_kind_its_owner_admitted() {
    let world = kinded();
    world.to(2);
    world.register(10);
    world.ready(B, "plain", &[], 1);
    world.ready_in(A, "a", 1);
    world.next();
    let run = world.ask_in(ADA, "", None).unwrap();
    world.next();
    world.next();
    assert_eq!(world.placed(run), (State::Queued, None, 0));
    assert_eq!(world.queued(), [run]);
    world.admit(A_ACCOUNT);
    world.next();
    assert_eq!(
        world.run(run).state,
        State::Queued,
        "an Admit alone tells fleet nothing"
    );
    world.act(BO, &Op::Walk { kind: BUILD.into() }).unwrap();
    assert_eq!(
        world.placed(run),
        (State::Assigned, Some(machine(A_ACCOUNT, "a")), 1)
    );
    let untyped = world.ask(job());
    assert_eq!(world.placed(untyped).1, Some(machine(B_ACCOUNT, "plain")));
    let filed = world.filed(run);
    assert_eq!(filed.kind.as_deref(), Some(BUILD));
    let Reply::Kind(kind) = world
        .chain
        .query(
            PROGRAM,
            &Query::KindOf {
                machine: machine(A_ACCOUNT, "a"),
            },
        )
        .unwrap()
    else {
        panic!("KindOf answers a kind");
    };
    assert_eq!(kind.as_deref(), Some(BUILD));
    world.ready(A, "a", &[], 2);
    let Reply::Kind(kept) = world
        .chain
        .query(
            PROGRAM,
            &Query::KindOf {
                machine: machine(A_ACCOUNT, "a"),
            },
        )
        .unwrap()
    else {
        panic!("KindOf answers a kind");
    };
    assert_eq!(
        kept.as_deref(),
        Some(BUILD),
        "a plain Ready keeps the kind it finds"
    );
}

#[test]
fn an_ask_names_a_kind_and_a_class_that_admits_its_asker() {
    let world = kinded();
    world.to(2);
    let unknown = world.ask_in(ADA, "", None).unwrap_err();
    assert_eq!(unknown.code, code::NOT_FOUND);
    world.register(10);
    let bo = world.members(&[BO_ACCOUNT]);
    world.set_policy(Policy {
        classes: vec![class("front", Some(bo)), class("low", None)],
        ..Policy::default()
    });
    world.next();
    let refused = world.ask_in(ADA, "front", None).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let missing = world.ask_in(ADA, "nope", None).unwrap_err();
    assert_eq!(missing.code, code::INVALID_INPUT);
    let classless = Op::AskIn {
        spec: job(),
        kind: None,
        class: "low".into(),
        limit: None,
    };
    let untyped = world.act(ADA, &classless).unwrap_err();
    assert_eq!(untyped.code, code::INVALID_INPUT);
    world.ask_in(BO, "front", None).unwrap();
    world.ask_in(ADA, "low", None).unwrap();
}

#[test]
fn highs_are_placed_before_older_lows_on_two_machines() {
    let world = kinded();
    world.to(2);
    world.register(10);
    world.set_policy(two_classes());
    world.admit(A_ACCOUNT);
    world.admit(B_ACCOUNT);
    world.ready_in(A, "a", 1);
    world.ready_in(B, "b", 1);
    world.next();
    let fill_a = world.ask_in(ADA, "low", None).unwrap();
    let fill_b = world.ask_in(ADA, "low", None).unwrap();
    world.next();
    world.next();
    let low_1 = world.ask_in(ADA, "low", None).unwrap();
    let low_2 = world.ask_in(BO, "low", None).unwrap();
    world.next();
    let high_1 = world.ask_in(BO, "high", None).unwrap();
    let high_2 = world.ask_in(ADA, "high", None).unwrap();
    world.next();
    world.next();
    assert_eq!(world.queued(), [high_1, high_2, low_1, low_2]);
    for (key, run) in [(A, fill_a), (B, fill_b)] {
        let attempt = world.run(run).attempt;
        world.claim(key, run, attempt).unwrap();
        world.report(key, run, attempt, State::Passed).unwrap();
    }
    world.walked();
    assert_eq!(world.run(high_1).state, State::Assigned);
    assert_eq!(world.run(high_2).state, State::Assigned);
    assert_eq!(world.queued(), [low_1, low_2]);
}

#[test]
fn a_policy_set_before_an_ask_rules_the_walk_that_ask_starts_with_no_walk_sent() {
    let world = kinded();
    world.to(2);
    world.register(10);
    world.set_policy(two_classes());
    world.ready_in(A, "a", 1);
    world.next();
    let low = world.ask_in(ADA, "low", None).unwrap();
    let high = world.ask_in(ADA, "high", None).unwrap();
    world.next();
    world.next();
    assert_eq!(world.queued(), [high, low], "a is not admitted yet");
    world.admit(A_ACCOUNT);
    world.set_policy(Policy {
        classes: vec![class("low", None), class("high", None)],
        ..Policy::default()
    });
    world.next();
    let later = world.ask_in(ADA, "high", None).unwrap();
    world.next();
    world.next();
    assert_eq!(world.placed(low).1, Some(machine(A_ACCOUNT, "a")));
    assert_eq!(world.queued(), [high, later]);
}

#[test]
fn groups_share_by_the_blocks_each_has_run() {
    let world = kinded();
    world.to(2);
    world.register(10);
    let ada = world.members(&[ADA_ACCOUNT]);
    let bo = world.members(&[BO_ACCOUNT]);
    world.set_policy(Policy {
        groups: vec![
            Group {
                name: "ada".into(),
                members: ada,
                weight: 1,
            },
            Group {
                name: "bo".into(),
                members: bo,
                weight: 1,
            },
        ],
        share: Share {
            meter: Some(BLOCKS.into()),
            window: None,
        },
        ..Policy::default()
    });
    world.admit(A_ACCOUNT);
    world.ready_in(A, "a", 1);
    world.next();
    let first = world.ask_in(ADA, "", None).unwrap();
    world.next();
    world.next();
    world.claim(A, first, 1).unwrap();
    let ada_next = world.ask_in(ADA, "", None).unwrap();
    world.next();
    let bo_next = world.ask_in(BO, "", None).unwrap();
    world.to(world.height.get() + 10);
    world.report(A, first, 1, State::Passed).unwrap();
    let blocks: u64 = world
        .used("ada")
        .iter()
        .filter(|row| row.meter == BLOCKS)
        .map(|row| row.used)
        .sum();
    assert_eq!(blocks, 11);
    world.walked();
    assert_eq!(world.placed(bo_next).1, Some(machine(A_ACCOUNT, "a")));
    assert_eq!(world.queued(), [ada_next]);
    assert_eq!(world.filed(ada_next).group, "ada");
}

#[test]
fn a_group_over_its_quota_waits_for_the_window_to_end() {
    let world = kinded();
    world.to(2);
    world.register(10);
    let ada = world.members(&[ADA_ACCOUNT]);
    world.set_policy(Policy {
        groups: vec![Group {
            name: "ada".into(),
            members: ada,
            weight: 1,
        }],
        quotas: vec![Quota {
            group: "ada".into(),
            meter: String::new(),
            window: 20,
            most: 1,
        }],
        ..Policy::default()
    });
    world.admit(A_ACCOUNT);
    world.ready_in(A, "a", 2);
    world.next();
    let first = world.ask_in(ADA, "", None).unwrap();
    world.next();
    world.next();
    assert_eq!(world.run(first).state, State::Assigned);
    let over = world.ask_in(ADA, "", None).unwrap();
    world.next();
    world.next();
    assert_eq!(world.run(over).state, State::Queued);
    world.to(21);
    assert_eq!(world.run(over).state, State::Assigned);
}

#[test]
fn a_step_examines_at_most_the_kinds_walk() {
    let world = kinded();
    world.to(2);
    world.register(2);
    world.admit(A_ACCOUNT);
    world.ready_in(A, "a", 0);
    world.next();
    let runs: Vec<u64> = (0..5)
        .map(|_| world.ask_in(ADA, "", None).unwrap())
        .collect();
    world.next();
    world.next();
    world.ready_in(A, "a", 5);
    let mut placed = Vec::new();
    for _ in 0..4 {
        world.next();
        let count = runs
            .iter()
            .filter(|run| world.run(**run).state == State::Assigned)
            .count();
        placed.push(count);
    }
    assert_eq!(placed, [0, 2, 4, 5]);
}

#[test]
fn promote_moves_a_queued_run_to_a_class_that_admits_the_sender() {
    let world = kinded();
    world.to(2);
    world.register(10);
    let bo = world.members(&[BO_ACCOUNT]);
    world.set_policy(Policy {
        classes: vec![class("front", Some(bo)), class("low", None)],
        ..Policy::default()
    });
    world.admit(A_ACCOUNT);
    world.ready_in(A, "a", 0);
    world.next();
    let older = world.ask_in(ADA, "low", None).unwrap();
    let mine = world.ask_in(BO, "low", None).unwrap();
    world.next();
    world.next();
    let refused = world
        .act(
            ADA,
            &Op::Promote {
                run: older,
                class: "front".into(),
            },
        )
        .unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    world
        .act(
            BO,
            &Op::Promote {
                run: mine,
                class: "front".into(),
            },
        )
        .unwrap();
    assert_eq!(world.queued(), [mine, older]);
    world.ready_in(A, "a", 1);
    world.walked();
    assert_eq!(world.placed(mine).1, Some(machine(A_ACCOUNT, "a")));
    assert_eq!(world.filed(mine).class, "front");
}

#[test]
fn promote_is_refused_to_whom_the_runs_claim_policy_does_not_admit() {
    let world = kinded();
    world.to(2);
    world.register(10);
    let bo = world.members(&[BO_ACCOUNT, CY_ACCOUNT]);
    world.set_policy(Policy {
        classes: vec![class("front", Some(bo)), class("low", None)],
        ..Policy::default()
    });
    world.next();
    let theirs = world.ask_in(CY, "low", None).unwrap();
    world.walked();
    let outsider = world
        .act(
            BO,
            &Op::Promote {
                run: theirs,
                class: "front".into(),
            },
        )
        .unwrap_err();
    assert_eq!(outsider.code, code::UNAUTHORIZED);
    assert_eq!(world.filed(theirs).class, "low");
    world
        .act(
            CY,
            &Op::Promote {
                run: theirs,
                class: "front".into(),
            },
        )
        .unwrap();
    assert_eq!(world.filed(theirs).class, "front");
}

#[test]
fn a_run_past_its_limit_is_stopped_by_its_machine() {
    let world = kinded();
    world.to(2);
    world.register(10);
    world.admit(A_ACCOUNT);
    world.ready_in(A, "a", 1);
    world.next();
    let run = world.ask_in(ADA, "", Some(5)).unwrap();
    world.next();
    world.next();
    world.claim(A, run, 1).unwrap();
    let claimed = world.height.get();
    world.to(claimed + 3);
    world.act(BO, &Op::Expire { run, attempt: 1 }).unwrap();
    assert_eq!(
        world.run(run).state,
        State::Running,
        "an Expire before the limit stops nothing"
    );
    world.to(claimed + 5);
    assert_eq!(world.run(run).state, State::Running);
    world.to(claimed + 6);
    let stopping = world.run(run);
    assert_eq!(stopping.state, State::Stopping);
    assert_eq!(stopping.summary, "past its limit of 5 blocks");
    world.report(A, run, 1, State::Cancelled).unwrap();
    assert_eq!(world.run(run).state, State::Cancelled);
}

#[test]
fn a_lower_tier_takes_every_run_it_can_before_a_higher_tier_takes_the_overflow() {
    let world = kinded();
    world.to(2);
    world.register(10);
    world.admit(A_ACCOUNT);
    world.admit(B_ACCOUNT);
    let rank = resources::Op::Rank {
        kind: BUILD.into(),
        provider: B_ACCOUNT,
        tier: 1,
    };
    world.resources(&rank).unwrap();
    world.ready_in(B, "zk-dev", 4);
    world.next();
    world.ready_in(A, "lat", 2);
    world.next();
    let runs: Vec<u64> = (0..3)
        .map(|_| world.ask_in(ADA, "", None).unwrap())
        .collect();
    world.walked();
    let lat = machine(A_ACCOUNT, "lat");
    let zk_dev = machine(B_ACCOUNT, "zk-dev");
    let placed: Vec<Option<MachineId>> = runs.iter().map(|run| world.placed(*run).1).collect();
    assert_eq!(
        placed,
        [Some(lat.clone()), Some(lat.clone()), Some(zk_dev.clone())],
        "zk-dev was free longer, yet takes only what lat has no slot for"
    );
    let first = runs[0];
    world.claim(A, first, 1).unwrap();
    world.report(A, first, 1, State::Passed).unwrap();
    let next = world.ask_in(ADA, "", None).unwrap();
    world.walked();
    assert_eq!(world.placed(next).1, Some(lat));
}
