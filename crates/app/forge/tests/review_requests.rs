//! `Query::ReviewRequests`: the open changes that ask an account's review at their current head,
//! read from forge's own tables, a page of the account's own changes at a time.
mod common;
use common::story::*;
use common::*;
use forge::{OpReply, Verdict};

fn asked(rig: &Rig, principal: Principal, limit: u64) -> Vec<u64> {
    let query = Query::ReviewRequests {
        principal,
        page: PageRequest::first(limit),
    };
    let Reply::ReviewRequests { page, .. } = abi::decode(&rig.query(&query).unwrap()).unwrap()
    else {
        panic!("ReviewRequests answers review requests");
    };
    page.items.iter().map(|summary| summary.n).collect()
}

fn opened(rig: &mut Rig, op: &Op) -> u64 {
    let OpReply::Change { n, .. } = abi::decode(&rig.execute(op).unwrap()).unwrap() else {
        panic!("ChangeOpen answers the change");
    };
    n
}

fn reviewed(rig: &mut Rig, story: &Story, verdict: Verdict) {
    rig.actor = b"reviewer".to_vec();
    rig.execute(&review(&story.feature, &story.root, verdict))
        .unwrap();
    rig.actor = TESTER.to_vec();
}

#[test]
fn a_review_is_asked_at_the_head_until_it_is_given_there() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let mut story = Story::pushed(&mut rig);
    let n = opened(&mut rig, &story.open("Feature"));
    assert_eq!(asked(&rig, Principal::Account(2), 8), [n]);
    assert!(
        asked(&rig, Principal::Account(1), 8).is_empty(),
        "its author is asked nothing"
    );
    reviewed(&mut rig, &story, Verdict::Comment);
    assert!(
        asked(&rig, Principal::Account(2), 8).is_empty(),
        "reviewed at the head"
    );
    story.push_follow_up(&mut rig, "follow-up.txt", b"follow-up\n");
    assert_eq!(asked(&rig, Principal::Account(2), 8), [n], "the head moved");
    rig.execute(&Op::ChangeClose {
        repo: REPO.into(),
        n,
    })
    .unwrap();
    assert!(asked(&rig, Principal::Account(2), 8).is_empty(), "closed");
}

#[test]
fn a_reply_under_my_review_asks_nothing_of_me() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let n = opened(&mut rig, &story.open("Feature"));
    reviewed(&mut rig, &story, Verdict::Comment);
    let Reply::Change { reviews, .. } = abi::decode(&rig.query(&change(n)).unwrap()).unwrap()
    else {
        panic!("Change answers the change");
    };
    let chat::Reply::Message(Some(root)) = rig
        .sandbox
        .chat_query(chat::Query::MessageById {
            message_id: reviews.items[0].message_id.clone(),
        })
        .unwrap()
    else {
        panic!("the review's line is in chat");
    };
    rig.chat_execute(
        Principal::Account(1),
        chat::Op::PostMessage {
            channel_id: format!("forge:{REPO}:{n}"),
            message_id: "a-reply".into(),
            blocks: vec![chat::Block::paragraph("A reply under the review")],
            thread: Some(root.seq),
        },
    );
    assert!(
        asked(&rig, Principal::Account(2), 8).is_empty(),
        "a reply is chat's to tell"
    );
}

#[test]
fn a_page_holds_the_readers_own_changes_never_everyone_elses() {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let Op::ChangeOpen {
        repo,
        from,
        into,
        title,
        body,
        ..
    } = story.open("Asks nobody")
    else {
        panic!("open is a ChangeOpen");
    };
    let nobody = opened(
        &mut rig,
        &Op::ChangeOpen {
            repo,
            from,
            into,
            title,
            body,
            reviewers: Vec::new(),
        },
    );
    let mine = opened(&mut rig, &story.open("Asks the reviewer"));
    assert!(nobody < mine);
    assert_eq!(asked(&rig, Principal::Account(2), 1), [mine]);
}
