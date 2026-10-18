//! Forge names people by account: one person with two device keys is one
//! owner, author, reviewer and writer; a key that holds no account writes
//! nothing. The system lines forge posts into chat name no one.

mod common;
use common::story::*;
use common::*;
use forge::{ChangeFilter, ChangeState, OpReply, Verdict};

const LAPTOP: &[u8] = b"tester-laptop";
const PHONE: &[u8] = b"reviewer-phone";

fn story() -> (Rig, Story) {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    rig.sandbox.hold(LAPTOP, 1);
    rig.sandbox.hold(PHONE, 2);
    let story = Story::pushed(&mut rig);
    (rig, story)
}

fn as_key(rig: &mut Rig, key: &[u8], op: &Op) -> Vec<u8> {
    rig.actor = key.to_vec();
    let output = rig.execute(op).unwrap();
    rig.actor = TESTER.to_vec();
    output
}

fn reply(rig: &Rig, query: Query) -> Reply {
    abi::decode(&rig.query(&query).unwrap()).unwrap()
}

fn record(rig: &Rig, n: u64) -> (forge::Change, Vec<forge::Review>) {
    let Reply::Change {
        change, reviews, ..
    } = reply(rig, change(n))
    else {
        panic!()
    };
    (change, reviews.items)
}

fn owed(rig: &Rig, principal: Principal) -> Vec<(u64, bool)> {
    let Reply::Judgment { page, .. } = reply(
        rig,
        Query::Judgment {
            principal,
            page: PageRequest::first(8),
        },
    ) else {
        panic!()
    };
    page.items
        .iter()
        .map(|judgment| (judgment.change.n, judgment.requested))
        .collect()
}

fn involving(rig: &Rig, principal: Principal) -> Vec<u64> {
    let Reply::Changes { page, .. } = reply(
        rig,
        Query::Changes {
            repo: REPO.into(),
            filter: ChangeFilter {
                involves: Some(principal),
                ..ChangeFilter::default()
            },
            page: PageRequest::first(8),
        },
    ) else {
        panic!()
    };
    page.items.iter().map(|summary| summary.n).collect()
}

#[test]
fn one_person_with_two_keys_is_one_owner_author_and_reviewer() {
    let (mut rig, story) = story();
    let Reply::Repos { page, .. } = reply(
        &rig,
        Query::Repos {
            page: PageRequest::first(2),
        },
    ) else {
        panic!()
    };
    assert_eq!(page.items[0].repo.owner, Principal::Account(1));
    as_key(
        &mut rig,
        LAPTOP,
        &Op::Configure {
            repo: REPO.into(),
            settings: Settings::default(),
        },
    );

    let output = as_key(&mut rig, LAPTOP, &story.open("From the laptop"));
    let OpReply::Change { n, .. } = abi::decode(&output).unwrap() else {
        panic!()
    };
    let retitle = Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: Some("Edited from the desk".into()),
        body: None,
        reviewers: None,
    };
    rig.execute(&retitle).unwrap();
    assert_eq!(record(&rig, n).0.author, Principal::Account(1));
    assert_eq!(involving(&rig, Principal::Account(1)), [n]);

    assert_eq!(owed(&rig, Principal::Account(2)), [(n, true)]);
    let approve = review(&story.feature, &story.root, Verdict::Approve);
    as_key(&mut rig, PHONE, &approve);
    as_key(&mut rig, b"reviewer", &approve);
    let (_, reviews) = record(&rig, n);
    assert!(reviews.iter().all(|r| r.author == Principal::Account(2)));
    assert_eq!(
        owed(&rig, Principal::Account(2)),
        [],
        "reviewed at the head"
    );
    assert_eq!(involving(&rig, Principal::Account(2)), [n]);

    let close = Op::ChangeClose {
        repo: REPO.into(),
        n,
    };
    as_key(&mut rig, LAPTOP, &close);
    assert_eq!(record(&rig, n).0.closed_by, Some(Principal::Account(1)));
}

/// A key that holds no account writes nothing, whatever the op, and leaves
/// forge as it was; once identity seats it in an account, the same key
/// writes as that account.
#[test]
fn a_key_writes_only_once_it_holds_an_account() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let ops = [
        Op::Create {
            repo: "other".into(),
            hash: HashKind::Sha1,
        },
        Op::Push {
            repo: REPO.into(),
            request: push_request(&[]),
            pack: None,
        },
        Op::Configure {
            repo: REPO.into(),
            settings: Settings::default(),
        },
        Op::Grant {
            repo: REPO.into(),
            principal: Principal::Account(3),
        },
        Op::Revoke {
            repo: REPO.into(),
            principal: Principal::Account(3),
        },
        story.open("From a key with no account"),
        Op::ChangeEdit {
            repo: REPO.into(),
            n,
            title: Some("Renamed".into()),
            body: None,
            reviewers: None,
        },
        review(&story.feature, &story.root, Verdict::Approve),
        Op::Merge {
            repo: REPO.into(),
            into: b"refs/heads/main".to_vec(),
            from: reference("feature"),
            expected_into: story.root.clone(),
            expected_from: story.feature.clone(),
            result: story.feature.clone(),
            change: Some(n),
        },
        Op::ChangeClose {
            repo: REPO.into(),
            n,
        },
    ];
    rig.actor = b"loose".to_vec();
    for op in &ops {
        let refusal = rig.refused(op);
        assert_eq!(refusal.code, code::UNAUTHORIZED, "{op:?}");
    }
    rig.sandbox.hold(b"loose", 5);
    let close = Op::ChangeClose {
        repo: REPO.into(),
        n,
    };
    assert_eq!(
        rig.refused(&close).code,
        code::UNAUTHORIZED,
        "not a writer yet"
    );
    rig.sandbox
        .list_writers(TESTER, REPO, &[Principal::Account(5)])
        .unwrap();
    rig.actor = b"loose".to_vec();
    rig.execute(&close).unwrap();
    assert_eq!(record(&rig, n).0.closed_by, Some(Principal::Account(5)));
}

#[test]
fn an_ended_change_is_not_edited() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    rig.execute(&Op::ChangeClose {
        repo: REPO.into(),
        n,
    })
    .unwrap();
    assert_eq!(record(&rig, n).0.state, ChangeState::Closed);
    let retitle = Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: Some("Too late".into()),
        body: None,
        reviewers: None,
    };
    assert_eq!(rig.refused(&retitle).code, code::WRONG_STATE);
}

/// Each line is an event code in a `forge` block: no key, no name, no
/// sentence. Who and what are on forge's records.
#[test]
fn system_lines_carry_an_event_code_and_no_name() {
    let (mut rig, story) = story();
    let n = opened(&mut rig, &story);
    let approve = review(&story.feature, &story.root, Verdict::Approve);
    as_key(&mut rig, PHONE, &approve);
    rig.execute(&Op::ChangeClose {
        repo: REPO.into(),
        n,
    })
    .unwrap();
    let code = |text: &str| chat::Block::Code {
        lang: Some("forge".into()),
        text: text.into(),
    };
    let chat::Reply::Roots(page) = rig
        .sandbox
        .chat_query(chat::Query::Roots {
            channel_id: format!("forge:{REPO}:{n}"),
            viewer: Vec::new(),
            page: PageRequest::first(8),
        })
        .unwrap()
    else {
        panic!()
    };
    // each line landed in its op's own frame; newest first
    let texts: Vec<&str> = page.items.iter().map(|row| row.text.as_str()).collect();
    assert_eq!(texts, ["closed", "review 1", "opened"]);
    assert_eq!(page.items[0].blocks, [code("closed")]);
}

fn opened(rig: &mut Rig, story: &Story) -> u64 {
    let output = rig.execute(&story.open("Feature")).unwrap();
    let OpReply::Change { n, .. } = abi::decode(&output).unwrap() else {
        panic!()
    };
    n
}

#[test]
fn a_programs_account_is_asked_to_review_as_any_account_is() {
    let (mut rig, story) = story();
    let chat = Principal::Account(sandbox::PROGRAMS[1].1);
    let nobody = Principal::Account(404);
    let Op::ChangeOpen {
        repo,
        from,
        into,
        title,
        body,
        ..
    } = story.open("Asks chat")
    else {
        panic!()
    };
    let open = |reviewers: Vec<Principal>| Op::ChangeOpen {
        repo: repo.clone(),
        from: from.clone(),
        into: into.clone(),
        title: title.clone(),
        body: body.clone(),
        reviewers,
    };
    assert_eq!(
        rig.refused(&open(vec![nobody.clone()])).code,
        code::INVALID_INPUT
    );
    let output = rig
        .execute(&open(vec![Principal::Account(2), chat.clone()]))
        .unwrap();
    let OpReply::Change { n, .. } = abi::decode(&output).unwrap() else {
        panic!()
    };
    assert_eq!(
        record(&rig, n).0.reviewers,
        [Principal::Account(2), chat.clone()]
    );
    let edit = |reviewers: Vec<Principal>| Op::ChangeEdit {
        repo: REPO.into(),
        n,
        title: None,
        body: None,
        reviewers: Some(reviewers),
    };
    assert_eq!(rig.refused(&edit(vec![nobody])).code, code::INVALID_INPUT);
    rig.execute(&edit(vec![chat.clone()])).unwrap();
    assert_eq!(record(&rig, n).0.reviewers, [chat]);
}
