//! A change's conversation at forge's interface: ChangeOpen opens the change's own room, its
//! notices land there and its end archives it; a change whose channel is another room keeps it.
//! Its own room is listed for who takes part: its author, the reviewers asked and who reviews.
mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";
const JOB_NAMED: &str = "refs/heads/j-7/teach-a";
const OWN: &str = "forge:project:1";
const ELSEWHERE: &str = "elsewhere";

fn seeded() -> (MemorySandbox, Oid, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let ahead = file_commit(&mut store, &[root], 2, &[("a", b"2\n")]);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [
        (zero, root, MAIN),
        (zero, ahead, FEATURE),
        (zero, ahead, JOB_NAMED),
    ];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    (sandbox, root, ahead)
}

fn opened(sandbox: &MemorySandbox, from: &str) -> u64 {
    let op = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(from.as_bytes().to_vec()),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach a".into(),
        body: String::new(),
        reviewers: vec![person(STRANGER)],
    };
    match abi::decode(&act(sandbox, WRITER, &op).unwrap()).unwrap() {
        forge::OpReply::Change { n, .. } => n,
        other => panic!("{other:?}"),
    }
}

fn change_of(sandbox: &MemorySandbox, n: u64) -> forge::Change {
    let query = Query::Change {
        repo: "project".into(),
        n,
        page: PageRequest::first(8),
    };
    match abi::decode(&ask(sandbox, &query).unwrap()).unwrap() {
        Reply::Change { change, .. } => change,
        other => panic!("{other:?}"),
    }
}

fn channel(sandbox: &MemorySandbox, id: &str) -> Option<chat::ChannelRow> {
    let query = chat::Query::Channel {
        channel_id: id.into(),
    };
    match sandbox.chat_query(query).unwrap() {
        chat::Reply::Channel(info) => info.map(|info| info.channel),
        other => panic!("{other:?}"),
    }
}

fn lines(sandbox: &MemorySandbox, room: &str) -> Vec<String> {
    let query = chat::Query::Roots {
        channel_id: room.into(),
        page: PageRequest::first(64),
        viewer: Vec::new(),
    };
    let chat::Reply::Roots(page) = sandbox.chat_query(query).unwrap() else {
        panic!("roots");
    };
    let mut rows = page.items;
    rows.sort_by_key(|row| row.seq);
    rows.iter()
        .filter_map(|row| row.notice())
        .filter(|notice| notice.kind == forge::NOTICE)
        .map(|notice| notice.group.clone())
        .collect()
}

fn pending(sandbox: &MemorySandbox) -> (usize, usize) {
    let ctx = sandbox.reads(1);
    (
        forge::tables::ARCHIVING.all(&ctx).unwrap().len(),
        forge::tables::LOGGING.all(&ctx).unwrap().len(),
    )
}

fn review(sandbox: &MemorySandbox, n: u64, ahead: Oid) {
    let op = Op::ReviewSubmit {
        repo: "project".into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: ahead.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::Approve,
            body: "Looks right.".into(),
            comments: Vec::new(),
        },
    };
    act(sandbox, STRANGER, &op).unwrap();
}

fn squash(sandbox: &MemorySandbox, from: &str, root: Oid, ahead: Oid, n: u64) {
    let op = Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: forge::Revision::Ref(from.as_bytes().to_vec()),
        expected_into: root.to_hex(),
        expected_from: ahead.to_hex(),
        message: None,
        change: Some(n),
    };
    act(sandbox, OWNER, &op).unwrap();
}

fn close(sandbox: &MemorySandbox, n: u64) {
    let op = Op::ChangeClose {
        repo: "project".into(),
        n,
    };
    act(sandbox, WRITER, &op).unwrap();
}

fn chat_as(sandbox: &MemorySandbox, key: &[u8], op: chat::Op) {
    let origin = Origin::Signed(key.to_vec());
    sandbox.chat_execute(origin, 1, TIME, op).unwrap();
}

fn open_room(sandbox: &MemorySandbox, id: &str) {
    let op = chat::Op::CreateChannel {
        channel_id: id.into(),
        name: id.into(),
        post_policy: chat::PostPolicy::Open,
    };
    chat_as(sandbox, OWNER, op);
}

fn archive_room(sandbox: &MemorySandbox, id: &str) {
    let op = chat::Op::SetChannelArchived {
        channel_id: id.into(),
        archived: true,
    };
    chat_as(sandbox, OWNER, op);
}

fn talking_elsewhere(sandbox: &MemorySandbox, n: u64) {
    open_room(sandbox, ELSEWHERE);
    let ctx = sandbox.forge.exec(MockHost::env("forge"));
    let key = ("project".to_owned(), n);
    let mut change = forge::tables::CHANGES.get(&ctx, &key).unwrap().unwrap();
    change.channel = ELSEWHERE.into();
    forge::tables::CHANGES.put(&ctx, &key, &change);
}

#[test]
fn a_change_opens_its_own_room_named_under_pr_and_says_so_there() {
    let (sandbox, _, _) = seeded();
    let n = opened(&sandbox, FEATURE);
    assert_eq!(change_of(&sandbox, n).channel, OWN);
    let room = channel(&sandbox, OWN).expect("its own room");
    assert_eq!(room.name, "pr/project#1 Teach a");
    assert!(!room.archived);
    assert!(!room.members_only());
    assert_eq!(lines(&sandbox, OWN), ["opened project 1"]);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn its_review_and_merge_are_said_in_its_own_room_and_the_merge_archives_it() {
    let (sandbox, root, ahead) = seeded();
    let n = opened(&sandbox, FEATURE);
    review(&sandbox, n, ahead);
    squash(&sandbox, FEATURE, root, ahead, n);
    assert_eq!(change_of(&sandbox, n).state, forge::ChangeState::Merged);
    assert_eq!(
        lines(&sandbox, OWN),
        ["opened project 1", "review 1 project 1", "merged project 1"]
    );
    assert!(channel(&sandbox, OWN).unwrap().archived);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn its_close_is_said_in_its_own_room_and_archives_it() {
    let (sandbox, _, _) = seeded();
    let n = opened(&sandbox, FEATURE);
    close(&sandbox, n);
    assert_eq!(change_of(&sandbox, n).state, forge::ChangeState::Closed);
    assert_eq!(
        lines(&sandbox, OWN),
        ["opened project 1", "closed project 1"]
    );
    assert!(channel(&sandbox, OWN).unwrap().archived);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn a_branch_named_like_a_room_chat_does_not_hold_talks_in_its_own_room() {
    let (sandbox, root, ahead) = seeded();
    let n = opened(&sandbox, JOB_NAMED);
    assert_eq!(change_of(&sandbox, n).channel, OWN);
    assert!(channel(&sandbox, "j-7").is_none());
    squash(&sandbox, JOB_NAMED, root, ahead, n);
    assert_eq!(
        lines(&sandbox, OWN),
        ["opened project 1", "merged project 1"]
    );
    assert!(channel(&sandbox, OWN).unwrap().archived);
}

#[test]
fn a_branch_named_like_an_open_room_still_talks_in_its_own_room() {
    let (sandbox, _, _) = seeded();
    open_room(&sandbox, "j-7");
    let n = opened(&sandbox, JOB_NAMED);
    assert_eq!(change_of(&sandbox, n).channel, OWN);
    assert_eq!(lines(&sandbox, OWN), ["opened project 1"]);
    assert!(lines(&sandbox, "j-7").is_empty());
    close(&sandbox, n);
    assert!(channel(&sandbox, OWN).unwrap().archived);
    assert!(!channel(&sandbox, "j-7").unwrap().archived);
}

#[test]
fn a_change_whose_channel_is_another_room_says_its_end_there_and_leaves_the_room_open() {
    let (sandbox, _, _) = seeded();
    let n = opened(&sandbox, FEATURE);
    talking_elsewhere(&sandbox, n);
    close(&sandbox, n);
    assert_eq!(lines(&sandbox, ELSEWHERE), ["closed project 1"]);
    assert!(!channel(&sandbox, ELSEWHERE).unwrap().archived);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn a_change_whose_other_room_was_archived_still_merges() {
    let (sandbox, root, ahead) = seeded();
    let n = opened(&sandbox, FEATURE);
    talking_elsewhere(&sandbox, n);
    archive_room(&sandbox, ELSEWHERE);
    squash(&sandbox, FEATURE, root, ahead, n);
    assert_eq!(change_of(&sandbox, n).state, forge::ChangeState::Merged);
    assert!(lines(&sandbox, ELSEWHERE).is_empty());
    assert_eq!(pending(&sandbox), (0, 0));
}

fn listed(sandbox: &MemorySandbox, room: &str) -> Option<String> {
    let query = chat::Query::Channel {
        channel_id: room.into(),
    };
    match sandbox.chat_query(query).unwrap() {
        chat::Reply::Channel(info) => info.and_then(|info| info.meta.get(chat::LISTED).cloned()),
        other => panic!("{other:?}"),
    }
}

fn seats(sandbox: &MemorySandbox, room: &str) -> BTreeMap<u64, u64> {
    let ctx = sandbox.chat.exec(MockHost::env("chat"));
    let rows = chat::tables::MEMBERS.all(&ctx).unwrap();
    rows.into_iter()
        .filter(|((id, _), _)| id == room)
        .filter_map(|((_, principal), row)| {
            principal.account().map(|account| (account, row.height))
        })
        .filter(|(account, _)| sandbox::program_of(*account).is_none())
        .collect()
}

fn seated(sandbox: &MemorySandbox, room: &str) -> BTreeSet<u64> {
    seats(sandbox, room).into_keys().collect()
}

fn unseat(sandbox: &MemorySandbox, room: &str, account: u64) {
    let ctx = sandbox.chat.exec(MockHost::env("chat"));
    chat::tables::MEMBERS.remove(&ctx, &(room.to_owned(), Principal::Account(account)));
}

fn as_opened_before_listing(sandbox: &MemorySandbox, room: &str) {
    for account in seated(sandbox, room) {
        unseat(sandbox, room, account);
    }
    let ctx = sandbox.chat.exec(MockHost::env("chat"));
    chat::tables::CHANNEL_META.remove(&ctx, &room.to_owned());
}

fn review_as(sandbox: &MemorySandbox, key: &[u8], height: u64, n: u64, ahead: Oid) {
    let op = Op::ReviewSubmit {
        repo: "project".into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: ahead.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::Comment,
            body: "A thought.".into(),
            comments: Vec::new(),
        },
    };
    signed_op(sandbox, key, height, &op).unwrap();
}

#[test]
fn its_room_is_listed_for_who_takes_part_seating_its_author_and_the_reviewers_asked() {
    let (sandbox, _, _) = seeded();
    opened(&sandbox, FEATURE);
    assert_eq!(listed(&sandbox, OWN).as_deref(), Some(chat::INVOLVED));
    assert_eq!(seated(&sandbox, OWN), BTreeSet::from([12, 13]));
}

#[test]
fn a_reviewer_asked_later_or_one_who_reviews_unasked_is_seated_and_a_seat_held_keeps_its_block() {
    let (sandbox, _, ahead) = seeded();
    sandbox.hold(b"fourth-key", 14);
    let n = opened(&sandbox, FEATURE);
    let ask = Op::ChangeEdit {
        repo: "project".into(),
        n,
        title: None,
        body: None,
        reviewers: Some(vec![person(STRANGER), person(OWNER)]),
    };
    signed_op(&sandbox, WRITER, 3, &ask).unwrap();
    assert_eq!(seated(&sandbox, OWN), BTreeSet::from([11, 12, 13]));
    review_as(&sandbox, b"fourth-key", 4, n, ahead);
    review_as(&sandbox, STRANGER, 5, n, ahead);
    assert_eq!(
        seats(&sandbox, OWN),
        BTreeMap::from([(11, 3), (12, 1), (13, 1), (14, 4)]),
        "each seat keeps the block that first seated it"
    );
}

#[test]
fn list_involved_is_retired_refused_and_lists_nothing() {
    let (mut sandbox, _, _) = seeded();
    sandbox
        .chain
        .seat::<scheduler::Scheduler>(scheduler::PROGRAM);
    opened(&sandbox, FEATURE);
    as_opened_before_listing(&sandbox, OWN);
    let walk = Op::ListInvolved { after: None };
    let refused = sandbox
        .forge
        .refused(|| signed_op(&sandbox, STRANGER, 2, &walk));
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    sandbox.chain.at(10, TIME);
    assert_eq!(listed(&sandbox, OWN), None);
    assert!(seated(&sandbox, OWN).is_empty());
}

fn line(sandbox: &MemorySandbox, room: &str, group: &str) -> chat::MsgRow {
    let query = chat::Query::Roots {
        channel_id: room.into(),
        page: PageRequest::first(64),
        viewer: Vec::new(),
    };
    let chat::Reply::Roots(page) = sandbox.chat_query(query).unwrap() else {
        panic!("roots");
    };
    let mut rows = page.items.into_iter();
    rows.find(|row| row.notice().is_some_and(|notice| notice.group == group))
        .unwrap_or_else(|| panic!("no line {group}"))
}

fn reviewed(sandbox: &MemorySandbox, n: u64, ahead: Oid, review: (forge::Verdict, &str)) {
    let (verdict, body) = review;
    let comments = match body.is_empty() {
        true => Vec::new(),
        false => vec![forge::LineComment {
            path: b"a".to_vec(),
            side: forge::Side::New,
            line: 1,
            body: "Why 2?".into(),
        }],
    };
    let op = Op::ReviewSubmit {
        repo: "project".into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: ahead.to_hex(),
            base_oid: None,
            verdict,
            body: body.into(),
            comments,
        },
    };
    act(sandbox, STRANGER, &op).unwrap();
}

#[test]
fn a_review_says_its_body_and_line_comments_under_its_line_and_other_lines_stay_one_line() {
    let (sandbox, _, ahead) = seeded();
    let n = opened(&sandbox, FEATURE);
    reviewed(
        &sandbox,
        n,
        ahead,
        (forge::Verdict::Comment, "Name the **reason**."),
    );
    let row = line(&sandbox, OWN, "review 1 project 1");
    let notice = row.notice().expect("a review's line is a notice");
    assert_eq!(notice.detail, "review 1: commented");
    let body = chat::parse_message("Name the **reason**.\n`a:1`\nWhy 2?");
    assert_eq!(row.blocks[1..], body[..]);
    reviewed(&sandbox, n, ahead, (forge::Verdict::Approve, ""));
    assert_eq!(line(&sandbox, OWN, "review 2 project 1").blocks.len(), 1);
    assert_eq!(line(&sandbox, OWN, "opened project 1").blocks.len(), 1);
}
