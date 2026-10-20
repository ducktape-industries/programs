//! Waits on a files path: a commit that changes the path (or anything below a directory), makes it
//! or removes it wakes the conversation once; a commit elsewhere wakes nothing, and harness hears
//! files' commits only while such a wait is open, and only from files.
use super::schedule::{answered, refused, schemed, turn};
use super::*;

fn waited(chain: &MockChain, thread: u64, call: &str) -> u64 {
    answered(chain, thread, call)["wait"].as_u64().unwrap()
}

fn wakes(chain: &MockChain, thread: u64) -> Vec<String> {
    frames(chain, thread)
        .into_iter()
        .chain(held(chain, thread))
        .filter_map(|body| match body {
            Body::Post {
                author: Principal::Root,
                text,
            } => Some(text),
            _ => None,
        })
        .collect()
}

fn file(path: &str) -> Value {
    json!({ "event": { "file": { "path": path } } })
}

fn watching(chain: &MockChain) -> bool {
    let host = chain.host(files::PROGRAM);
    host.borrow()
        .state
        .keys()
        .any(|key| key.starts_with(b"watchers/"))
}

fn remove(chain: &MockChain, key: &[u8], path: &str) -> BlobId {
    let base = match chain.query(files::PROGRAM, &files::Query::Head).unwrap() {
        files::Reply::Head(head) => head.map(|row| row.id),
        other => panic!("{other:?}"),
    };
    let commit = files::Op::Commit {
        base,
        message: "tidy".into(),
        changes: vec![files::Change::Remove { path: path.into() }],
    };
    abi::decode(&chain.submit(signed(key), files::PROGRAM, &commit).unwrap()).unwrap()
}

fn words(
    wait: u64,
    path: &str,
    how: &str,
    snapshot: BlobId,
    by: AccountNumber,
    said: &str,
) -> String {
    let snapshot = abi::hex(snapshot.digest());
    format!(
        "Wait {wait} is over: {path} {how} in files snapshot {snapshot} by account {by} (\"{said}\")."
    )
}

#[test]
fn a_wait_for_a_file_wakes_once_when_a_commit_changes_it() {
    let world = schemed();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/tools/warm-pod", "duckpond-1\n");
    let thread = world.opened("m1", "tell me when the warm pod changes", Some(WINDOW));
    assert!(!watching(chain), "no file wait, no watch");
    turn(
        &world,
        3,
        &[("w1", "wait_for", file("/shared/tools/warm-pod"))],
    );
    let wait = waited(chain, thread, "w1");
    assert!(
        watching(chain),
        "harness hears files while the wait is open"
    );

    chain.at(5, 0);
    commit(chain, ADA, "/shared/tools/other", "x");
    assert!(
        wakes(chain, thread).is_empty(),
        "a commit elsewhere wakes nothing"
    );
    let snapshot = commit(chain, ADA, "/shared/tools/warm-pod", "duckpond-2\n");
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let path = "/shared/tools/warm-pod";
    let said = words(wait, path, "changed", snapshot, world.ada, "configure");
    assert_eq!(wake, said);
    assert!(
        !watching(chain),
        "the last file wait over, harness stops hearing files"
    );

    chain.at(6, 0);
    commit(chain, ADA, "/shared/tools/warm-pod", "duckpond-3\n");
    assert_eq!(wakes(chain, thread).len(), 1, "it wakes once");
}

#[test]
fn a_wait_for_a_directory_wakes_on_a_change_below_it_and_one_for_a_path_when_it_is_made() {
    let world = schemed();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/jobs/a.md", "a");
    let thread = world.opened("m1", "watch the jobs", Some(WINDOW));
    let calls = [
        ("w1", "wait_for", file("/shared/jobs")),
        ("w2", "wait_for", file("/shared/new.md")),
    ];
    turn(&world, 3, &calls);
    let (dir, made) = (waited(chain, thread, "w1"), waited(chain, thread, "w2"));

    chain.at(5, 0);
    let below = commit(chain, ADA, "/shared/jobs/b/brief.md", "b");
    let made_at = commit(chain, ADA, "/shared/new.md", "new");
    let told = wakes(chain, thread);
    let first = words(
        dir,
        "/shared/jobs",
        "changed",
        below,
        world.ada,
        "configure",
    );
    let still = format!(
        "(This conversation still waits: wait {made} for /shared/new.md to change in files.)"
    );
    let expected = [
        format!("{first}\n{still}"),
        words(
            made,
            "/shared/new.md",
            "was made",
            made_at,
            world.ada,
            "configure",
        ),
    ];
    assert_eq!(told, expected);
}

#[test]
fn a_removed_path_wakes_its_wait_and_a_cancelled_one_leaves_harness_hearing_files_no_more() {
    let world = schemed();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/tools/warm-pod", "duckpond-1\n");
    let thread = world.opened("m1", "watch two paths", Some(WINDOW));
    let calls = [
        ("w1", "wait_for", file("/shared/tools/warm-pod")),
        ("w2", "wait_for", file("/shared/other")),
    ];
    turn(&world, 3, &calls);
    let (gone, other) = (waited(chain, thread, "w1"), waited(chain, thread, "w2"));

    chain.at(5, 0);
    world.mention("m2", "drop the second");
    turn(
        &world,
        6,
        &[("c1", "wait_cancel", json!({ "wait": other }))],
    );
    assert_eq!(answered(chain, thread, "c1"), json!({ "ok": true }));
    assert!(watching(chain), "the first wait still needs files");

    chain.at(8, 0);
    let removed = remove(chain, ADA, "/shared/tools/warm-pod");
    let path = "/shared/tools/warm-pod";
    let said = words(gone, path, "was removed", removed, world.ada, "tidy");
    assert_eq!(wakes(chain, thread), [said]);
    assert!(!watching(chain));
}

#[test]
fn only_files_tells_harness_of_a_commit_and_an_agent_moves_no_files_watcher() {
    let world = schemed();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/a.md", "a");
    let thread = world.opened("m1", "forge a commit", Some(WINDOW));
    let watch = json!({ "program": "files", "op": { "Watch": { "deliver": "" } } });
    turn(&world, 3, &[("f1", "network_call", watch)]);
    let text = refused(chain, thread, "f1");
    let refusal = text.starts_with(code::UNAUTHORIZED) && text.contains("does not send");
    assert!(refusal, "{text}");
    let files::Reply::Head(Some(row)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!("files holds a head")
    };
    let error = chain
        .submit(signed(ADA), PROGRAM, &Op::Filed(row))
        .unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
}

#[test]
fn the_files_prefix_decodes_as_filed() {
    let world = schemed();
    let chain = &world.chain;
    commit(chain, ADA, "/shared/a.md", "a");
    let files::Reply::Head(Some(row)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!("files holds a head")
    };
    let mut delivered = filed_as();
    delivered.extend(abi::encode(&row));
    assert_eq!(abi::decode::<Op>(&delivered).unwrap(), Op::Filed(row));
}
