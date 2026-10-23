use super::*;

fn permit_enqueue(world: &World) {
    let set = access::Op::Set {
        program: forge::PROGRAM.into(),
        key: forge::repo_key(REPO),
        list: Some(::program::Ownership::Listed(vec![
            ::program::Party::Program(PROGRAM.into()),
        ])),
    };
    world
        .chain
        .submit(Origin::Signed(OWNER.to_vec()), "access", &set)
        .unwrap();
}

fn queue(world: &World) -> Vec<forge::QueueEntry> {
    let mut after = None;
    let mut entries = Vec::new();
    loop {
        let asked = forge::Query::Queue {
            repo: REPO.into(),
            branch: b"refs/heads/main".to_vec(),
            page: PageRequest { after, limit: None },
        };
        let forge::Reply::Queue { page, .. } = world.chain.query(forge::PROGRAM, &asked).unwrap()
        else {
            panic!("queue answers Queue");
        };
        entries.extend(page.items);
        match page.next {
            Some(next) => after = Some(next),
            None => return entries,
        }
    }
}

fn change(world: &World, n: u64) -> forge::Change {
    let asked = forge::Query::Change {
        repo: REPO.into(),
        n,
        page: PageRequest::first(1),
    };
    let forge::Reply::Change { change, .. } = world.chain.query(forge::PROGRAM, &asked).unwrap()
    else {
        panic!("change answers Change");
    };
    change
}

fn patch(world: &World, key: &[u8], n: u64, head: Oid, context: &str, state: CheckState) {
    let report = Op::ReportPatch {
        repo: REPO.into(),
        n,
        commit: head.to_hex(),
        context: context.into(),
        state,
        url: None,
    };
    world
        .chain
        .submit(Origin::Signed(key.to_vec()), PROGRAM, &report)
        .unwrap();
}

#[test]
fn a_required_patch_pass_queues_its_head_and_the_preview_lane_merges_without_a_hand_step() {
    let mut world = world();
    let (_, head) = opened(&mut world);
    permit_enqueue(&world);
    world.require();
    assert!(world.preview_of(1).is_none());

    patch_reviewed(&world, head).unwrap();

    let entries = queue(&world);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].n, 1);
    assert_eq!(entries[0].head, head.to_hex());
    assert_eq!(entries[0].by, forge::Principal::Account(REACH_ACCOUNT));
    assert_eq!(change(&world, 1).state, forge::ChangeState::Open);
    let preview = world.preview_of(1).expect("enqueue made the Preview");
    let preview = Oid::from_hex(Hash::Sha1, &preview).unwrap();
    assert!(world.statuses(preview).contains(&review_passed()));

    world.report(preview, LANE_REACH, None).unwrap();

    assert!(queue(&world).is_empty());
    let merged = change(&world, 1);
    assert_eq!(merged.state, forge::ChangeState::Merged);
    assert_eq!(merged.merge_oid, Some(preview.to_hex()));
}

#[test]
fn a_required_report_with_a_change_queues_once_without_waiting_for_the_other_check() {
    let mut world = world();
    let (_, head) = opened(&mut world);
    permit_enqueue(&world);
    world.require();

    world.report(head, LANE_REACH, Some(1)).unwrap();

    let before = queue(&world);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].head, head.to_hex());
    assert_eq!(change(&world, 1).state, forge::ChangeState::Open);
    world.chain.at(3, 0);
    world.report(head, LANE_REACH, Some(1)).unwrap();
    assert_eq!(queue(&world), before);
    patch_reviewed(&world, head).unwrap();
    assert_eq!(change(&world, 1).state, forge::ChangeState::Merged);
}

#[test]
fn an_unnamed_required_report_finds_unpreviewed_ref_and_oid_changes_past_empty_pages() {
    let mut world = world();
    let (_, head) = opened(&mut world);
    world
        .forge(&forge::Op::ChangeClose {
            repo: REPO.into(),
            n: 1,
        })
        .unwrap();
    world.open();
    world
        .forge(&forge::Op::ChangeOpen {
            repo: REPO.into(),
            from: forge::Revision::Oid(head.to_hex()),
            into: b"refs/heads/main".to_vec(),
            title: "Same head by oid".into(),
            body: String::new(),
            reviewers: Vec::new(),
        })
        .unwrap();
    let mut small_pages = bounds();
    small_pages.page_size = 1;
    world.chain.init(forge::PROGRAM, &small_pages).unwrap();
    permit_enqueue(&world);
    world.require();
    assert!(world.preview_of(2).is_none());
    assert!(world.preview_of(3).is_none());

    world.report(head, LANE_REACH, None).unwrap();

    let entries = queue(&world);
    let numbers: Vec<u64> = entries.iter().map(|entry| entry.n).collect();
    assert_eq!(numbers, [2, 3]);
    assert!(entries.iter().all(|entry| entry.head == head.to_hex()));
    assert_eq!(change(&world, 1).state, forge::ChangeState::Closed);
}

#[test]
fn the_branch_required_reporter_and_context_choose_who_can_trigger_enqueue() {
    let mut world = world();
    let (_, head) = opened(&mut world);
    permit_enqueue(&world);
    let require = forge::Op::RequireChecks {
        repo: REPO.into(),
        branch: b"refs/heads/main".to_vec(),
        required: Some(forge::Required {
            reporter: forge::Principal::Account(REACH_ACCOUNT),
            contexts: vec![format!("{LANE_ACCOUNT}/{REVIEW}"), lane_context()],
        }),
    };
    world.forge(&require).unwrap();

    patch_reviewed(&world, head).unwrap();
    assert!(queue(&world).is_empty());
    patch(&world, LANE, 1, head, "unrequired", CheckState::Passed);
    assert!(queue(&world).is_empty());
    patch(&world, LANE, 1, head, REVIEW, CheckState::Passed);
    assert_eq!(queue(&world)[0].head, head.to_hex());
}

#[test]
fn nonpasses_old_heads_preview_commits_and_closed_changes_do_not_enqueue() {
    let mut world = world();
    let (_, old) = opened(&mut world);
    permit_enqueue(&world);
    world.require();
    let preview = world.preview();
    for state in [CheckState::Pending, CheckState::Failed, CheckState::Errored] {
        patch(&world, REVIEWER, 1, old, REVIEW, state);
        assert!(queue(&world).is_empty());
    }
    patch(&world, REVIEWER, 1, preview, REVIEW, CheckState::Passed);
    assert!(queue(&world).is_empty());
    let head = world.commit(&[old], LOCKED_FEATURE);
    world.push(Some(old), head, "feature");
    patch_reviewed(&world, old).unwrap();
    assert!(queue(&world).is_empty());
    world
        .forge(&forge::Op::ChangeClose {
            repo: REPO.into(),
            n: 1,
        })
        .unwrap();
    patch_reviewed(&world, head).unwrap();
    assert!(queue(&world).is_empty());
}

#[test]
fn a_report_cannot_enqueue_for_a_branch_that_requires_another_status_reporter_or_no_checks() {
    let mut world = world();
    let (_, head) = opened(&mut world);
    permit_enqueue(&world);
    patch_reviewed(&world, head).unwrap();
    assert!(queue(&world).is_empty());
    let require = forge::Op::RequireChecks {
        repo: REPO.into(),
        branch: b"refs/heads/main".to_vec(),
        required: Some(forge::Required {
            reporter: forge::Principal::Account(OWNER_ACCOUNT),
            contexts: vec![review_context()],
        }),
    };
    world.forge(&require).unwrap();
    patch_reviewed(&world, head).unwrap();
    assert!(queue(&world).is_empty());
}

#[test]
fn an_enqueue_the_repository_refuses_keeps_the_pass_and_can_be_retried_by_reporting_again() {
    let mut world = world();
    let (_, head) = opened(&mut world);
    world.require();

    patch_reviewed(&world, head).unwrap();

    assert!(queue(&world).is_empty());
    let asked = Query::Reports {
        repo: REPO.into(),
        commit: head.to_hex(),
        page: PageRequest::default(),
    };
    let Reply::Reports(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("reports answers Reports");
    };
    assert!(page.items.iter().any(|report| {
        report.reporter == REVIEWER_ACCOUNT && report.state == CheckState::Passed
    }));

    permit_enqueue(&world);
    patch_reviewed(&world, head).unwrap();
    assert_eq!(queue(&world)[0].head, head.to_hex());
}
