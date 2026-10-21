//! A change's new head is a notice to the reviewers asked of it, even a change opened before forge
//! kept its index: a push, a repoint and a rebase each tell them, never the account that moved it;
//! asking a review tells the reviewers it adds; and what concerns no one posts nothing.

mod common;
use common::story::*;
use common::*;
use forge::OpReply;

fn opened(rig: &mut Rig, reviewers: Vec<Principal>) -> u64 {
    let open = Op::ChangeOpen {
        repo: REPO.into(),
        from: reference("feature"),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: "The author's body.".into(),
        reviewers,
    };
    let OpReply::Change { n, .. } = abi::decode(&rig.execute(&open).unwrap()).unwrap() else {
        panic!("an open answers Change")
    };
    n
}

fn notices(rig: &Rig, n: u64, word: &str) -> Vec<(Vec<u64>, String)> {
    let roots = chat::Query::Roots {
        channel_id: format!("forge:{REPO}:{n}"),
        viewer: Vec::new(),
        page: PageRequest::first(32),
    };
    let chat::Reply::Roots(page) = rig.sandbox.chat_query(roots).unwrap() else {
        panic!("Roots answers Roots")
    };
    let group = format!("{word} {REPO} {n}");
    page.items
        .iter()
        .filter_map(|row| row.notice())
        .filter(|notice| notice.group == group)
        .map(|notice| {
            assert_eq!(notice.status, chat::message::Status::Running);
            (notice.concerns.clone(), notice.detail.clone())
        })
        .collect()
}

fn asked(n: u64, reviewers: &[u64]) -> Op {
    Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: None,
        body: None,
        reviewers: Some(reviewers.iter().copied().map(Principal::Account).collect()),
    }
}

#[test]
fn a_push_to_a_changes_branch_tells_its_reviewers_and_never_the_pusher() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let mut story = Story::pushed(&mut rig);
    let n = opened(&mut rig, vec![Principal::Account(2), Principal::Account(4)]);
    let tip = story.push_follow_up(&mut rig, "more.txt", b"more\n");
    let first = (vec![2, 4], format!("new head {tip}"));
    assert_eq!(notices(&rig, n, "head"), std::slice::from_ref(&first));
    rig.sandbox
        .list_writers(TESTER, REPO, &[Principal::Account(2)])
        .unwrap();
    rig.actor = b"reviewer".to_vec();
    let next = story.push_follow_up(&mut rig, "again.txt", b"again\n");
    let second = (vec![4], format!("new head {next}"));
    assert_eq!(notices(&rig, n, "head"), [second, first]);
}

#[test]
fn a_repoint_and_a_rebase_tell_the_reviewers_of_the_head_they_make() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let mut story = Story::pushed(&mut rig);
    let n = opened(&mut rig, vec![Principal::Account(2)]);
    let repoint = Op::ChangeRepoint {
        repo: REPO.into(),
        n,
        from: reference("conflict"),
    };
    rig.execute(&repoint).unwrap();
    let repointed = (vec![2], format!("new head {}", story.conflict));
    assert_eq!(notices(&rig, n, "head"), std::slice::from_ref(&repointed));
    story.push_follow_up(&mut rig, "more.txt", b"more\n");
    assert_eq!(
        notices(&rig, n, "head"),
        std::slice::from_ref(&repointed),
        "a push to the branch it left tells no one"
    );
    let rebase = Op::Rebase {
        repo: REPO.into(),
        n,
        onto: reference("clean"),
        expected_head: story.conflict.clone(),
    };
    let OpReply::Rebased { oid, .. } = abi::decode(&rig.execute(&rebase).unwrap()).unwrap() else {
        panic!("a rebase answers Rebased")
    };
    let rebased = (vec![2], format!("new head {oid}"));
    assert_eq!(notices(&rig, n, "head"), [rebased, repointed]);
}

#[test]
fn asking_a_review_of_an_open_change_tells_the_reviewers_it_adds() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    Story::pushed(&mut rig);
    let n = opened(&mut rig, Vec::new());
    rig.execute(&asked(n, &[2])).unwrap();
    rig.execute(&asked(n, &[2, 4])).unwrap();
    rig.execute(&asked(n, &[2, 4])).unwrap();
    let requested = |who: Vec<u64>| (who, "review requested".to_owned());
    assert_eq!(
        notices(&rig, n, "requested"),
        [requested(vec![4]), requested(vec![2])]
    );
}

#[test]
fn a_head_or_a_request_that_concerns_no_one_but_its_actor_posts_nothing() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let mut story = Story::pushed(&mut rig);
    let n = opened(&mut rig, Vec::new());
    story.push_follow_up(&mut rig, "more.txt", b"more\n");
    assert!(notices(&rig, n, "head").is_empty(), "no reviewer is asked");
    rig.execute(&asked(n, &[1])).unwrap();
    story.push_follow_up(&mut rig, "again.txt", b"again\n");
    assert!(
        notices(&rig, n, "requested").is_empty(),
        "the tester asked itself"
    );
    assert!(notices(&rig, n, "head").is_empty(), "the tester pushed");
}

#[test]
fn a_change_opened_before_the_index_existed_is_told_its_new_head() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let mut story = Story::pushed(&mut rig);
    let n = opened(&mut rig, vec![Principal::Account(2)]);
    let unindexed = |key: &Vec<u8>| !key.starts_with(b"j/") && !key.starts_with(b"t/");
    let mut forge = rig.sandbox.forge.borrow_mut();
    forge.state.retain(|key, _| unindexed(key));
    drop(forge);
    let tip = story.push_follow_up(&mut rig, "more.txt", b"more\n");
    let told = (vec![2], format!("new head {tip}"));
    assert_eq!(notices(&rig, n, "head"), [told]);
}

#[test]
fn a_merged_changes_branch_moves_without_a_notice() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let mut story = Story::pushed(&mut rig);
    let n = opened(&mut rig, vec![Principal::Account(2)]);
    let merge = Op::Merge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: reference("feature"),
        expected_into: story.root.clone(),
        expected_from: story.feature.clone(),
        result: story.feature.clone(),
        change: Some(n),
    };
    rig.execute(&merge).unwrap();
    story.push_follow_up(&mut rig, "more.txt", b"more\n");
    assert!(notices(&rig, n, "head").is_empty());
}
