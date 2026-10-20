//! A change from a `j-NNN/…` branch talks in its job's room while chat holds that room open:
//! its lines name it there, nothing archives the room, and the room's other threads owe nothing.
mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const JOB: &str = "refs/heads/j-7/teach-a";
const FEATURE: &str = "refs/heads/feature";
const ROOM: &str = "j-7";
const OWN: &str = "forge:project:1";

// main at a root commit, and the job's branch and feature one commit ahead of it, every
// object pushed.
fn seeded() -> (MemorySandbox, Oid, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let ahead = file_commit(&mut store, &[root], 2, &[("a", b"2\n")]);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [
        (zero, root, MAIN),
        (zero, ahead, JOB),
        (zero, ahead, FEATURE),
    ];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    (sandbox, root, ahead)
}

// `key`'s chat op, in a frame of its own.
fn chat_as(sandbox: &MemorySandbox, key: &[u8], op: chat::Op) {
    let origin = Origin::Signed(key.to_vec());
    sandbox.chat_execute(origin, 1, TIME, op).unwrap();
}

// OWNER's room `j-7`, posted to as `post_policy` says.
fn job_room(sandbox: &MemorySandbox, post_policy: chat::PostPolicy) {
    let op = chat::Op::CreateChannel {
        channel_id: ROOM.into(),
        name: "Teach a".into(),
        post_policy,
    };
    chat_as(sandbox, OWNER, op);
}

fn archive_room(sandbox: &MemorySandbox) {
    let op = chat::Op::SetChannelArchived {
        channel_id: ROOM.into(),
        archived: true,
    };
    chat_as(sandbox, OWNER, op);
}

// The change WRITER opens from `from` into main.
fn opened(sandbox: &MemorySandbox, from: &str) -> u64 {
    let op = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(from.as_bytes().to_vec()),
        into: MAIN.as_bytes().to_vec(),
        title: "Teach a".into(),
        body: String::new(),
        reviewers: Vec::new(),
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

fn roots(sandbox: &MemorySandbox, room: &str) -> Vec<(u64, Option<String>)> {
    let query = chat::Query::Roots {
        channel_id: room.into(),
        page: PageRequest::first(64),
        viewer: Vec::new(),
    };
    let chat::Reply::Roots(page) = sandbox.chat_query(query).unwrap() else {
        panic!("roots");
    };
    let mut roots: Vec<(u64, Option<String>)> = page
        .items
        .into_iter()
        .map(|row| {
            let code = row
                .notice()
                .filter(|notice| notice.kind == forge::NOTICE)
                .map(|notice| notice.group.clone());
            (row.seq, code)
        })
        .collect();
    roots.sort();
    roots
}

// The code of every line forge posted into `room`, oldest first.
fn lines(sandbox: &MemorySandbox, room: &str) -> Vec<String> {
    roots(sandbox, room)
        .into_iter()
        .filter_map(|(_, code)| code)
        .collect()
}

// The archives and lines forge still waits on chat's answer to.
fn pending(sandbox: &MemorySandbox) -> (usize, usize) {
    let ctx = sandbox.reads(1);
    (
        forge::tables::ARCHIVING.all(&ctx).unwrap().len(),
        forge::tables::LOGGING.all(&ctx).unwrap().len(),
    )
}

fn close(sandbox: &MemorySandbox, n: u64) {
    let op = Op::ChangeClose {
        repo: "project".into(),
        n,
    };
    act(sandbox, WRITER, &op).unwrap();
}

fn squash(root: Oid, ahead: Oid, n: u64) -> Op {
    Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: forge::Revision::Ref(JOB.as_bytes().to_vec()),
        expected_into: root.to_hex(),
        expected_from: ahead.to_hex(),
        message: None,
        change: Some(n),
    }
}

// What WRITER owes: each change, and the review whose answered line it is.
fn owed(sandbox: &MemorySandbox) -> Vec<(u64, Option<u64>)> {
    let query = Query::Judgment {
        principal: person(WRITER),
        page: PageRequest::first(8),
    };
    let Reply::Judgment { page, .. } = abi::decode(&ask(sandbox, &query).unwrap()).unwrap() else {
        panic!("judgment");
    };
    page.items
        .iter()
        .map(|judgment| {
            let review = judgment.replies.as_ref().and_then(|replies| replies.review);
            (judgment.change.n, review)
        })
        .collect()
}

#[test]
fn a_job_branch_talks_in_its_open_room_and_its_lines_name_the_change() {
    let (sandbox, _, _) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, JOB);
    assert_eq!(change_of(&sandbox, n).channel, ROOM);
    assert!(channel(&sandbox, OWN).is_none(), "no room of its own");
    assert_eq!(lines(&sandbox, ROOM), ["opened project 1"]);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn closing_it_leaves_the_job_room_as_it_is() {
    let (sandbox, _, _) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, JOB);
    close(&sandbox, n);
    assert_eq!(
        lines(&sandbox, ROOM),
        ["opened project 1", "closed project 1"]
    );
    assert!(!channel(&sandbox, ROOM).unwrap().archived);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn merging_it_leaves_the_job_room_as_it_is() {
    let (sandbox, root, ahead) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, JOB);
    act(&sandbox, OWNER, &squash(root, ahead, n)).unwrap();
    assert_eq!(change_of(&sandbox, n).state, forge::ChangeState::Merged);
    assert_eq!(
        lines(&sandbox, ROOM),
        ["opened project 1", "merged project 1"]
    );
    assert!(!channel(&sandbox, ROOM).unwrap().archived);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn a_job_room_archived_after_it_opened_still_lets_it_merge() {
    let (sandbox, root, ahead) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, JOB);
    archive_room(&sandbox);
    act(&sandbox, OWNER, &squash(root, ahead, n)).unwrap();
    assert_eq!(change_of(&sandbox, n).state, forge::ChangeState::Merged);
    // chat refused the line into the archived room, and forge absorbed its answer
    assert_eq!(lines(&sandbox, ROOM), ["opened project 1"]);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn without_its_room_open_a_job_branch_talks_in_its_own() {
    let setups: [fn(&MemorySandbox); 3] = [
        |_| {},
        |sandbox| {
            job_room(sandbox, chat::PostPolicy::Open);
            archive_room(sandbox);
        },
        |sandbox| job_room(sandbox, chat::PostPolicy::MembersOnly),
    ];
    for (at, setup) in setups.into_iter().enumerate() {
        let (sandbox, _, _) = seeded();
        setup(&sandbox);
        let n = opened(&sandbox, JOB);
        assert_eq!(change_of(&sandbox, n).channel, OWN, "setup {at}");
        assert_eq!(lines(&sandbox, OWN), ["opened project 1"], "setup {at}");
    }
}

#[test]
fn another_branch_talks_in_its_own_room() {
    let (sandbox, _, _) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, FEATURE);
    assert_eq!(change_of(&sandbox, n).channel, OWN);
    assert!(lines(&sandbox, ROOM).is_empty());
}

#[test]
fn archive_ended_leaves_a_job_room() {
    let (sandbox, _, _) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, JOB);
    close(&sandbox, n);
    let op = Op::ArchiveEnded {
        repo: "project".into(),
        after: None,
        limit: 8,
    };
    match abi::decode(&act(&sandbox, STRANGER, &op).unwrap()).unwrap() {
        forge::OpReply::ArchivedEnded { archived, .. } => assert!(archived.is_empty()),
        other => panic!("{other:?}"),
    }
    assert!(!channel(&sandbox, ROOM).unwrap().archived);
}

#[test]
fn in_a_job_room_only_a_review_line_answered_is_owed() {
    let (sandbox, _, ahead) = seeded();
    job_room(&sandbox, chat::PostPolicy::Open);
    let n = opened(&sandbox, JOB);
    let post = |key: &[u8], id: &str, thread: Option<u64>| {
        let op = chat::Op::PostMessage {
            channel_id: ROOM.into(),
            message_id: id.into(),
            blocks: vec![chat::Block::paragraph("About a")],
            thread,
        };
        chat_as(&sandbox, key, op);
    };
    // WRITER's own thread in the room, answered: the job's talk, not the change's
    post(WRITER, "own-root", None);
    let (own, _) = roots(&sandbox, ROOM)
        .into_iter()
        .find(|(_, code)| code.is_none())
        .unwrap();
    post(STRANGER, "own-reply", Some(own));
    assert_eq!(owed(&sandbox), Vec::<(u64, Option<u64>)>::new());
    // WRITER's review, its line answered: that is owed
    let review = Op::ReviewSubmit {
        repo: "project".into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: ahead.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::Comment,
            body: "Looks right.".into(),
            comments: Vec::new(),
        },
    };
    act(&sandbox, WRITER, &review).unwrap();
    let line = format!("review 1 project {n}");
    let (root, _) = roots(&sandbox, ROOM)
        .into_iter()
        .find(|(_, code)| code.as_deref() == Some(line.as_str()))
        .unwrap();
    post(STRANGER, "review-reply", Some(root));
    assert_eq!(owed(&sandbox), [(n, Some(1))]);
}
