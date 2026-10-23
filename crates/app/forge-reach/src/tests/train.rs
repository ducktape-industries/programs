use super::*;

const SPLIT_BASE: Files = &[
    ("Cargo.lock", "lock 1\n"),
    ("a/x.rs", "x 1\n"),
    ("b/y.rs", "y 1\n"),
];
const BREAKS_A: Files = &[
    ("Cargo.lock", "lock 1\n"),
    ("a/x.rs", "x broken\n"),
    ("b/y.rs", "y 1\n"),
];
const EDITS_B: Files = &[
    ("Cargo.lock", "lock 1\n"),
    ("a/x.rs", "x 1\n"),
    ("b/y.rs", "y 2\n"),
];

fn oid(hex: &str) -> Oid {
    Oid::from_hex(Hash::Sha1, hex).unwrap()
}

fn verdict_of(world: &World, n: u64) -> PreviewVerdict {
    let asked = Query::Verdict {
        repo: REPO.into(),
        n,
    };
    let Reply::Verdict(Some(verdict)) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("change {n} has a verdict");
    };
    verdict
}

fn lane_on(world: &World, commit: Oid) -> Option<CheckState> {
    world
        .statuses(commit)
        .into_iter()
        .find(|(context, _, _)| *context == lane_context())
        .map(|(_, state, _)| state)
}

fn a_train_whose_front_breaks_what_its_tip_never_touched(world: &mut World) -> Oid {
    let base = world.commit(&[], SPLIT_BASE);
    let breaks = world.commit(&[base], BREAKS_A);
    let edits = world.commit(&[base], EDITS_B);
    world.push(None, base, "main");
    world.push(None, breaks, "breaks");
    world.push(None, edits, "edits");
    world.open_from("breaks");
    world.open_from("edits");
    world.require();
    let train = forge::Op::SetTrain {
        repo: REPO.into(),
        branch: b"refs/heads/main".to_vec(),
        train: Some(forge::Train {
            length: 2,
            tip: vec![lane_context()],
        }),
    };
    world.forge(&train).unwrap();
    for (head, read) in [(breaks, "a/x.rs"), (edits, "b/y.rs")] {
        world
            .say(REVIEWER, head, REVIEW, CheckState::Passed, &[read])
            .unwrap();
    }
    world
        .report(edits, &["Cargo.lock", "b/y.rs"], Some(2))
        .unwrap();
    for (n, head) in [(1, breaks), (2, edits)] {
        let enqueue = forge::Op::Enqueue {
            repo: REPO.into(),
            n,
            expected_head: head.to_hex(),
        };
        world.forge(&enqueue).unwrap();
    }
    oid(&world.preview_of(2).expect("the tip has a Preview"))
}

#[test]
fn a_lane_on_the_tip_changes_own_head_is_pending_on_the_tip_naming_what_the_front_changes() {
    let mut world = world();
    let tip = a_train_whose_front_breaks_what_its_tip_never_touched(&mut world);
    for n in [1, 2] {
        assert_eq!(train_change_state(&world, n), forge::ChangeState::Open);
    }
    assert_eq!(train_formed(&world), vec![1, 2]);
    assert_eq!(lane_on(&world, tip), Some(CheckState::Pending));
    let verdict = verdict_of(&world, 2);
    let lane = verdict
        .carried
        .iter()
        .find(|carried| carried.context == lane_context())
        .expect("the head's lane is judged on the tip");
    assert_eq!(lane.differs, vec!["a/x.rs".to_string()]);
}

#[test]
fn a_red_lane_on_the_tip_lands_the_change_that_broke_nothing_and_leaves_the_front_out() {
    let mut world = world();
    let tip = a_train_whose_front_breaks_what_its_tip_never_touched(&mut world);
    world
        .say(LANE, tip, CONTEXT, CheckState::Failed, &[""])
        .unwrap();
    let front = oid(&world.preview_of(1).expect("the front has its own Preview"));
    world
        .say(LANE, front, CONTEXT, CheckState::Failed, &[""])
        .unwrap();
    assert_eq!(train_change_state(&world, 1), forge::ChangeState::Open);
    assert_eq!(train_change_state(&world, 2), forge::ChangeState::Merged);
}
