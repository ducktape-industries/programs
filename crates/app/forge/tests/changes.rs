//! Every op on the story's repository: what it does, and what it refuses
//! (the wrong signer, the wrong state, a duplicate, an unbounded input).
//! Every refusal leaves forge's store as it was.

mod common;
use common::story::*;
use common::*;
use forge::{
    ChangeFilter, ChangeState, LineComment, OpReply, ReviewDraft, Revision, Side, Verdict,
};

const REVIEWER: &[u8] = b"reviewer";

fn story() -> (Rig, Story) {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    (rig, story)
}

/// The op signed by `who`; the rig signs as the owner again afterwards.
fn signed(rig: &mut Rig, who: &[u8], op: &Op) -> Result<Vec<u8>, program::Error> {
    rig.actor = who.to_vec();
    let result = rig.execute(op);
    rig.actor = TESTER.to_vec();
    result
}

/// The reason `who`'s op is refused; the store is untouched by it.
#[track_caller]
fn refused_as(rig: &mut Rig, who: &[u8], op: &Op) -> String {
    rig.actor = who.to_vec();
    let refusal = rig.refused(op);
    rig.actor = TESTER.to_vec();
    refusal.code
}

fn reply(rig: &Rig, query: &Query) -> Reply {
    abi::decode(&rig.query(query).unwrap()).unwrap()
}

fn record(rig: &Rig, n: u64) -> forge::Change {
    let Reply::Change { change, .. } = reply(rig, &change(n)) else {
        panic!()
    };
    change
}

fn involving(rig: &Rig, who: &[u8]) -> Vec<u64> {
    involving_principal(rig, rig.principal(who))
}

fn involving_principal(rig: &Rig, principal: Principal) -> Vec<u64> {
    let query = Query::Changes {
        repo: REPO.into(),
        filter: ChangeFilter {
            involves: Some(principal),
            ..ChangeFilter::default()
        },
        page: PageRequest::first(128),
    };
    let Reply::Changes { page, .. } = reply(rig, &query) else {
        panic!()
    };
    page.items.iter().map(|summary| summary.n).collect()
}

fn opened(rig: &mut Rig, story: &Story) -> u64 {
    let output = rig.execute(&story.open("Feature")).unwrap();
    let OpReply::Change { n, .. } = abi::decode(&output).unwrap() else {
        panic!()
    };
    n
}

fn edit_reviewers(n: u64, reviewers: Vec<Principal>) -> Op {
    Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: None,
        body: None,
        reviewers: Some(reviewers),
    }
}

// ------------------------------------------------------------ repository

#[test]
fn configure_is_the_owners_and_takes_a_ref_for_head() {
    let (mut rig, _) = story();
    let settings = |head: &[u8]| Op::Configure {
        repo: REPO.into(),
        settings: Settings {
            head: head.to_vec(),
            allow_force: true,
            allow_delete: false,
        },
    };
    rig.execute(&settings(b"refs/heads/feature")).unwrap();
    let Reply::Repo { repo, .. } = reply(&rig, &Query::Repo { repo: REPO.into() }) else {
        panic!()
    };
    assert_eq!(repo.repo.settings.head, b"refs/heads/feature");
    assert!(repo.repo.settings.allow_force);

    let stranger = refused_as(&mut rig, STRANGER, &settings(b"refs/heads/main"));
    assert_eq!(stranger, code::UNAUTHORIZED);
    assert_eq!(rig.refused(&settings(b"main")).code, code::INVALID_INPUT);
}

#[test]
fn grant_and_revoke_are_retired_and_the_owner_lists_writers_in_access() {
    let (mut rig, _) = story();
    let grant = Op::Grant {
        repo: REPO.into(),
        principal: person(WRITER),
    };
    assert_eq!(rig.refused(&grant).code, code::UNAUTHORIZED);
    let configure = Op::Configure {
        repo: REPO.into(),
        settings: Settings::default(),
    };
    rig.sandbox
        .list_writers(TESTER, REPO, &[person(WRITER)])
        .unwrap();
    assert_eq!(
        refused_as(&mut rig, WRITER, &configure),
        code::UNAUTHORIZED,
        "a writer pushes, the owner configures"
    );
    assert_eq!(
        rig.sandbox
            .list_writers(WRITER, REPO, &[person(STRANGER)])
            .unwrap_err()
            .code,
        code::UNAUTHORIZED
    );
    assert_eq!(
        rig.sandbox
            .list_writers(TESTER, REPO, &[Principal::Account(404)])
            .unwrap_err()
            .code,
        code::NOT_FOUND
    );
}

// ---------------------------------------------------------------- changes

#[test]
fn open_numbers_changes_and_refuses_what_cannot_merge() {
    let (mut rig, story) = story();
    assert_eq!(opened(&mut rig, &story), 1);
    assert_eq!(opened(&mut rig, &story), 2, "numbers are never reused");
    let feature = record(&rig, 1);
    assert_eq!(feature.state, ChangeState::Open);
    assert_eq!(feature.author, Principal::Account(1));
    assert_eq!(feature.channel, "forge:project:1");

    let open =
        |from: Revision, into: &[u8], title: &str, reviewers: Vec<Principal>| Op::ChangeOpen {
            repo: REPO.into(),
            from,
            into: into.to_vec(),
            title: title.into(),
            body: String::new(),
            reviewers,
        };
    let main = b"refs/heads/main";
    let same = open(reference("main"), main, "Nothing", vec![]);
    assert_eq!(rig.refused(&same).code, code::WRONG_STATE);
    let tag = open(reference("feature"), b"refs/tags/v1", "Into a tag", vec![]);
    assert_eq!(rig.refused(&tag).code, code::INVALID_INPUT);
    let blank = open(reference("feature"), main, "  ", vec![]);
    assert_eq!(rig.refused(&blank).code, code::INVALID_INPUT);
    let twice = open(
        reference("feature"),
        main,
        "Twice",
        vec![Principal::Account(2), Principal::Account(2)],
    );
    assert_eq!(rig.refused(&twice).code, code::INVALID_INPUT);
    let gone = open(reference("nowhere"), main, "Gone", vec![]);
    assert_eq!(rig.refused(&gone).code, code::NOT_FOUND);
}

/// Chat's refusal of a message forge sent without a reply fails forge's
/// whole frame, as the kernel fails it: the change is never opened.
#[test]
fn chat_refusing_the_channel_fails_the_open_it_came_from() {
    let (mut rig, story) = story();
    // forge itself, outside any change: the channel the change will want
    let forge = Principal::Account(sandbox::PROGRAMS[0].1);
    rig.chat_execute(
        forge,
        chat::Op::CreateChannel {
            channel_id: "forge:project:1".into(),
            name: "squat".into(),
            post_policy: chat::PostPolicy::Open,
        },
    );
    let before = rig.sandbox.forge.borrow().state.clone();
    let refusal = rig.execute(&story.open("Feature")).unwrap_err();
    assert_eq!(refusal.code, code::ALREADY_EXISTS);
    assert_eq!(rig.sandbox.forge.borrow().state, before);
}

#[test]
fn edit_is_the_authors_and_drops_the_reviewers_it_unasks() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    assert_eq!(involving(&rig, REVIEWER), [n]);

    let retitle = Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: Some("Renamed".into()),
        body: None,
        reviewers: None,
    };
    rig.execute(&retitle).unwrap();
    assert_eq!(record(&rig, n).title, "Renamed");
    assert_eq!(
        record(&rig, n).body,
        "The author's body.",
        "None keeps a field"
    );
    assert_eq!(refused_as(&mut rig, REVIEWER, &retitle), code::UNAUTHORIZED);
    rig.sandbox
        .list_writers(TESTER, REPO, &[person(WRITER)])
        .unwrap();
    assert_eq!(
        refused_as(&mut rig, WRITER, &retitle),
        code::UNAUTHORIZED,
        "a repository's writers do not edit another's change"
    );
    rig.sandbox
        .list_on(TESTER, &forge::change_key(REPO, n), &[person(WRITER)])
        .unwrap();
    signed(&mut rig, WRITER, &retitle).unwrap();
    assert_eq!(
        rig.sandbox
            .list_on(WRITER, &forge::change_key(REPO, n), &[])
            .unwrap_err()
            .code,
        code::UNAUTHORIZED,
        "only the author lists who edits a change"
    );

    // Asking a fleet of accounts and unasking them leaves nothing behind.
    for i in 0..(3 * 64u64) {
        rig.sandbox.hold(&i.to_be_bytes(), 100 + i);
    }
    for round in 0..3u8 {
        let fleet = (0..64)
            .map(|i| Principal::Account(100 + 64 * round as u64 + i))
            .collect();
        rig.execute(&edit_reviewers(n, fleet)).unwrap();
    }
    rig.execute(&edit_reviewers(n, vec![])).unwrap();
    assert!(involving(&rig, REVIEWER).is_empty());
    assert!(involving_principal(&rig, Principal::Account(100)).is_empty());
    assert_eq!(involving(&rig, TESTER), [n], "the author stays involved");

    let missing = edit_reviewers(n + 1, vec![]);
    assert_eq!(rig.refused(&missing).code, code::NOT_FOUND);
}

#[test]
fn a_reviewer_who_reviewed_stays_involved_when_unasked() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let approve = review(&story.feature, &story.root, Verdict::Approve);
    signed(&mut rig, REVIEWER, &approve).unwrap();
    rig.execute(&edit_reviewers(n, vec![])).unwrap();
    assert_eq!(involving(&rig, REVIEWER), [n]);
}

#[test]
fn close_is_the_authors_or_a_writers_and_happens_once() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let close = Op::ChangeClose {
        repo: REPO.into(),
        n,
    };
    assert_eq!(refused_as(&mut rig, STRANGER, &close), code::UNAUTHORIZED);
    rig.execute(&close).unwrap();
    let closed = record(&rig, n);
    assert_eq!(closed.state, ChangeState::Closed);
    assert_eq!(closed.closed_by, Some(Principal::Account(1)));
    assert_eq!(closed.merged_by, None);
    assert_eq!(rig.refused(&close).code, code::WRONG_STATE);

    let second = opened(&mut rig, &story);
    rig.sandbox
        .list_writers(TESTER, REPO, &[person(WRITER)])
        .unwrap();
    let by_writer = Op::ChangeClose {
        repo: REPO.into(),
        n: second,
    };
    signed(&mut rig, WRITER, &by_writer).unwrap();
    let closed = record(&rig, second);
    assert_eq!(closed.state, ChangeState::Closed);
    assert_eq!(closed.closed_by, Some(person(WRITER)));
}

#[test]
fn a_review_counts_once_and_its_anchors_are_checked() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let output = signed(
        &mut rig,
        REVIEWER,
        &review(&story.feature, &story.root, Verdict::RequestChanges),
    )
    .unwrap();
    assert!(matches!(
        abi::decode(&output).unwrap(),
        OpReply::Review { id: 1, .. }
    ));
    let change = record(&rig, n);
    assert_eq!(change.review_count, 1);
    assert_eq!(change.comment_count, 2);
    assert_eq!(change.verdicts.request_changes, 1);

    let draft = |base: Option<&str>, verdict, comments: Vec<(Side, u64, &str)>| Op::ReviewSubmit {
        repo: REPO.into(),
        n,
        review: ReviewDraft {
            commit_oid: story.feature.clone(),
            base_oid: base.map(str::to_owned),
            verdict,
            body: String::new(),
            comments: comments
                .into_iter()
                .map(|(side, line, body)| LineComment {
                    path: b"src/lib.rs".to_vec(),
                    side,
                    line,
                    body: body.into(),
                })
                .collect(),
        },
    };
    let root = Some(story.root.as_str());
    let twice = draft(
        root,
        Verdict::Comment,
        vec![(Side::New, 2, "a"), (Side::New, 2, "b")],
    );
    assert_eq!(rig.refused(&twice).code, code::INVALID_INPUT);
    let baseless = draft(None, Verdict::Comment, vec![(Side::Old, 2, "old")]);
    assert_eq!(rig.refused(&baseless).code, code::INVALID_INPUT);
    let silent = draft(root, Verdict::Comment, vec![]);
    assert_eq!(rig.refused(&silent).code, code::INVALID_INPUT);
    let line_zero = draft(root, Verdict::Approve, vec![(Side::New, 0, "zero")]);
    assert_eq!(rig.refused(&line_zero).code, code::INVALID_INPUT);
    let mut escape = draft(root, Verdict::Comment, vec![(Side::New, 1, "up")]);
    if let Op::ReviewSubmit { review, .. } = &mut escape {
        review.comments[0].path = b"../etc/passwd".to_vec();
    }
    assert_eq!(rig.refused(&escape).code, code::INVALID_INPUT);
    assert_eq!(
        record(&rig, n).review_count,
        1,
        "a refused review counts nothing"
    );
}

#[test]
fn a_merge_lands_its_change_once_and_only_over_the_heads_it_read() {
    let (mut rig, mut story) = story();
    let n = opened(&mut rig, &story);
    let merge_over = |expected_into: &str, from: &str| Op::Merge {
        repo: REPO.into(),
        into: b"refs/heads/main".to_vec(),
        from: reference("feature"),
        expected_into: expected_into.into(),
        expected_from: from.into(),
        result: from.into(),
        change: Some(n),
    };
    let merge = |expected_into: &str| merge_over(expected_into, &story.feature);
    assert_eq!(
        refused_as(&mut rig, STRANGER, &merge(&story.root)),
        code::UNAUTHORIZED
    );
    assert_eq!(rig.refused(&merge(&story.clean)).code, code::STALE);

    let output = rig.execute(&merge(&story.root)).unwrap();
    assert!(matches!(
        abi::decode(&output).unwrap(),
        OpReply::Merged {
            change: Some(1),
            ..
        }
    ));
    let change = record(&rig, n);
    assert_eq!(change.state, ChangeState::Merged);
    assert_eq!(change.merge_oid.as_deref(), Some(story.feature.as_str()));
    assert_eq!(change.merged_by, Some(Principal::Account(1)));
    assert_eq!(change.closed_by, None);
    assert_eq!(
        refs_of(&rig.sandbox, REPO)["refs/heads/main"],
        story.feature
    );

    // The feature moves on; its change is merged already.
    let main = refs_of(&rig.sandbox, REPO)["refs/heads/main"].clone();
    let merged = story.feature.clone();
    let tip = story.push_follow_up(&mut rig, "later.txt", b"later\n");
    let again = merge_over(&main, &tip);
    assert_eq!(rig.refused(&again).code, code::WRONG_STATE);
    // ...and still reads the two heads it merged, not the moved refs
    let Reply::Change {
        source_head,
        target_head,
        ..
    } = reply(&rig, &crate::change(n))
    else {
        panic!()
    };
    assert_eq!(
        (source_head.as_deref(), target_head.as_deref()),
        (Some(merged.as_str()), Some(story.root.as_str()))
    );
}
