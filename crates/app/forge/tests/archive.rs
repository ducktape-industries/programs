mod common;
use common::*;

const MAIN: &str = "refs/heads/main";
const FEATURE: &str = "refs/heads/feature";

fn seeded() -> (MemorySandbox, Oid, Oid) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut store, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut store, &[root], 2, &[("a", b"2\n")]);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, root, MAIN), (zero, feature, FEATURE)];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    (sandbox, root, feature)
}

fn opened(sandbox: &MemorySandbox) -> u64 {
    let op = Op::ChangeOpen {
        repo: "project".into(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
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

fn close(n: u64) -> Op {
    Op::ChangeClose {
        repo: "project".into(),
        n,
    }
}

fn channel_id(n: u64) -> String {
    format!("forge:project:{n}")
}

fn channel(sandbox: &MemorySandbox, n: u64) -> Option<chat::ChannelRow> {
    let query = chat::Query::Channel {
        channel_id: channel_id(n),
    };
    match sandbox.chat_query(query).unwrap() {
        chat::Reply::Channel(info) => info.map(|info| info.channel),
        other => panic!("{other:?}"),
    }
}

fn archived(sandbox: &MemorySandbox, n: u64) -> bool {
    channel(sandbox, n).expect("the change's channel").archived
}

fn rewrite(sandbox: &MemorySandbox, n: u64, change: impl FnOnce(&mut chat::ChannelRow)) {
    let ctx = sandbox.chat.exec(MockHost::env("chat"));
    let mut row = channel(sandbox, n).unwrap();
    change(&mut row);
    chat::tables::CHANNELS.put(&ctx, &row.id, &row);
}

fn pending(sandbox: &MemorySandbox) -> (usize, usize) {
    let ctx = sandbox.reads(1);
    let archiving = forge::tables::ARCHIVING.all(&ctx).unwrap();
    (
        archiving.len(),
        forge::tables::LOGGING.all(&ctx).unwrap().len(),
    )
}

fn notices(sandbox: &MemorySandbox, n: u64) -> Vec<chat::Notice> {
    let query = chat::Query::Roots {
        channel_id: channel_id(n),
        page: PageRequest::first(64),
        viewer: Vec::new(),
    };
    let chat::Reply::Roots(page) = sandbox.chat_query(query).unwrap() else {
        panic!("roots");
    };
    let mut rows = page.items;
    rows.sort_by_key(|row| row.seq);
    rows.iter()
        .filter_map(|row| row.notice().cloned())
        .collect()
}

fn lines(sandbox: &MemorySandbox, n: u64) -> Vec<String> {
    notices(sandbox, n)
        .into_iter()
        .map(|notice| notice.group)
        .collect()
}

fn state_of(sandbox: &MemorySandbox, n: u64) -> forge::ChangeState {
    let query = Query::Change {
        repo: "project".into(),
        n,
        page: PageRequest::first(8),
    };
    let reply: Reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
    let Reply::Change { change, .. } = reply else {
        panic!("{reply:?}");
    };
    change.state
}

#[test]
fn a_merge_archives_the_change_channel() {
    let (sandbox, root, feature) = seeded();
    let n = opened(&sandbox);
    assert!(!archived(&sandbox, n));
    let merge = Op::Merge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
        expected_into: root.to_hex(),
        expected_from: feature.to_hex(),
        result: feature.to_hex(),
        change: Some(n),
    };
    act(&sandbox, OWNER, &merge).unwrap();
    assert_eq!(state_of(&sandbox, n), forge::ChangeState::Merged);
    assert!(archived(&sandbox, n));
    assert_eq!(lines(&sandbox, n), ["opened project 1", "merged project 1"]);
}

#[test]
fn a_squash_merge_archives_the_change_channel() {
    let (sandbox, root, feature) = seeded();
    let n = opened(&sandbox);
    let squash = Op::SquashMerge {
        repo: "project".into(),
        into: MAIN.as_bytes().to_vec(),
        from: forge::Revision::Ref(FEATURE.as_bytes().to_vec()),
        expected_into: root.to_hex(),
        expected_from: feature.to_hex(),
        message: None,
        change: Some(n),
    };
    act(&sandbox, OWNER, &squash).unwrap();
    assert_eq!(state_of(&sandbox, n), forge::ChangeState::Merged);
    assert!(archived(&sandbox, n));
}

#[test]
fn a_close_archives_the_change_channel() {
    let (sandbox, _, _) = seeded();
    let n = opened(&sandbox);
    act(&sandbox, WRITER, &close(n)).unwrap();
    assert_eq!(state_of(&sandbox, n), forge::ChangeState::Closed);
    assert!(archived(&sandbox, n));
    assert_eq!(lines(&sandbox, n), ["opened project 1", "closed project 1"]);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn a_refused_archive_still_closes_and_is_said_in_the_channel() {
    let (sandbox, _, _) = seeded();
    let n = opened(&sandbox);
    rewrite(&sandbox, n, |row| row.owner = Principal::Account(13));
    act(&sandbox, WRITER, &close(n)).unwrap();
    assert_eq!(state_of(&sandbox, n), forge::ChangeState::Closed);
    assert!(!archived(&sandbox, n));
    let notices = notices(&sandbox, n);
    let groups: Vec<&str> = notices.iter().map(|notice| notice.group.as_str()).collect();
    assert_eq!(
        groups,
        [
            "opened project 1",
            "closed project 1",
            "archive refused project 1"
        ]
    );
    let refused = &notices[2];
    assert!(
        refused.detail.starts_with("archive refused: "),
        "{refused:?}"
    );
    assert_eq!(refused.status, chat::message::Status::Failed);
    assert_eq!(refused.concerns, [12]);
    assert_eq!(pending(&sandbox), (0, 0));
}

#[test]
fn a_change_room_opens_under_pr_and_an_old_name_takes_it_at_the_next_notice() {
    let (sandbox, _, _) = seeded();
    let n = opened(&sandbox);
    let named = format!("pr/project#{n} Teach a");
    assert_eq!(channel(&sandbox, n).unwrap().name, named);
    rewrite(&sandbox, n, |row| row.name = format!("project#{n}"));
    act(&sandbox, WRITER, &close(n)).unwrap();
    assert_eq!(channel(&sandbox, n).unwrap().name, named);
    assert_eq!(pending(&sandbox), (0, 0));
}
