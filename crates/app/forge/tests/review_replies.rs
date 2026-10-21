//! A reply under a review's line in its change's room reaches the account that reviewed, by
//! chat's own rule for who hears a thread.
mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";
const ROOM: &str = "forge:project:1";

fn seeded() -> (MemorySandbox, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let ahead = file_commit(&mut store, &[root], 2, &[("a", b"2\n")]);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, root, MAIN), (zero, ahead, FEATURE)];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    (sandbox, ahead)
}

fn meant_for(sandbox: &MemorySandbox, account: u64) -> u64 {
    let query = chat::Query::Unread {
        account,
        rooms: vec![(ROOM.into(), 0)],
    };
    let chat::Reply::Unread(counts) = sandbox.chat_query(query).unwrap() else {
        panic!("Unread answers a count per room");
    };
    counts[0].meant
}

fn review_line(sandbox: &MemorySandbox) -> u64 {
    let query = chat::Query::Roots {
        channel_id: ROOM.into(),
        page: PageRequest::first(64),
        viewer: Vec::new(),
    };
    let chat::Reply::Roots(page) = sandbox.chat_query(query).unwrap() else {
        panic!("roots");
    };
    let line = |row: &&chat::MsgRow| {
        row.notice()
            .is_some_and(|notice| notice.group == "review 1 project 1")
    };
    page.items.iter().find(line).expect("the review's line").seq
}

#[test]
fn a_reply_under_a_review_line_is_meant_for_its_reviewer() {
    let (sandbox, ahead) = seeded();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach a".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    act(&sandbox, WRITER, &open).unwrap();
    let review = Op::ReviewSubmit {
        repo: "project".into(),
        n: 1,
        review: forge::ReviewDraft {
            commit_oid: ahead.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::Comment,
            body: "Looks right.".into(),
            comments: Vec::new(),
        },
    };
    act(&sandbox, OWNER, &review).unwrap();
    let root = review_line(&sandbox);
    let before = meant_for(&sandbox, 11);
    let reply = chat::Op::PostMessage {
        channel_id: ROOM.into(),
        message_id: "reply".into(),
        blocks: vec![chat::Block::paragraph("Why this way?")],
        thread: Some(root),
    };
    let origin = Origin::Signed(STRANGER.to_vec());
    sandbox.chat_execute(origin, 1, TIME, reply).unwrap();
    assert_eq!(meant_for(&sandbox, 11), before + 1);
}
