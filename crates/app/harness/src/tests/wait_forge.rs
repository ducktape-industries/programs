//! Waits on forge with forge seated: a ref that moves, a change whose source moves, that closes
//! or merges, each wake their conversation once; a change already ended is refused saying so.
use super::forge_tools::{answered, head, refusal, seeded, turn, world};
use super::*;

const TIME_MS: u64 = 1_760_000_000_000;

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

fn waited(chain: &MockChain, thread: u64, call: &str) -> u64 {
    answered(chain, thread, call)["wait"].as_u64().unwrap()
}

fn commit(branch: &str, path: &str) -> Value {
    json!({
        "repo": "notes",
        "branch": branch,
        "files": [{ "path": path, "content": "more\n" }],
        "message": path,
    })
}

#[test]
fn a_wait_for_a_ref_wakes_once_when_it_moves() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "tell me when main moves", Some(WINDOW));
    seeded(&world, thread);
    let wanted = json!({ "event": { "ref": { "repo": "notes", "ref": "main" } } });
    turn(chain, 7, &[("w1", "wait_for", wanted)]);
    let wait = waited(chain, thread, "w1");
    let words = json!([{ "type": "text", "text": "Committed; waiting for main." }]);
    model_says(chain, words, "end_turn");
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the turn that armed it ends once the result before it is heard"
    );
    let before = head(chain, "notes", "main");

    chain.at(9, TIME_MS);
    world.mention("m2", "commit to dev, then main");
    turn(
        chain,
        10,
        &[("c1", "forge_commit", commit("dev", "dev.md"))],
    );
    assert!(wakes(chain, thread).is_empty(), "another ref wakes nothing");
    turn(
        chain,
        12,
        &[("c2", "forge_commit", commit("main", "main.md"))],
    );
    let after = head(chain, "notes", "main");
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let words = format!(
        "Wait {wait} is over: refs/heads/main in notes moved from {before} to {after} by account {}.",
        world.agent
    );
    assert_eq!(wake, words);
    turn(
        chain,
        14,
        &[("c3", "forge_commit", commit("main", "again.md"))],
    );
    assert_eq!(wakes(chain, thread).len(), 1, "it wakes once");
}

#[test]
fn a_wait_for_a_change_wakes_when_its_source_moves_and_when_it_closes_and_an_ended_one_is_refused()
{
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "watch my change", Some(WINDOW));
    seeded(&world, thread);
    let open = json!({ "repo": "notes", "from": "dev", "into": "main", "title": "dev's own" });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    let change = json!({ "event": { "change": { "repo": "notes", "n": 1 } } });
    turn(chain, 9, &[("w1", "wait_for", change.clone())]);
    let first = waited(chain, thread, "w1");
    let before = head(chain, "notes", "dev");

    chain.at(11, TIME_MS);
    world.mention("m2", "push to dev");
    turn(
        chain,
        12,
        &[("c1", "forge_commit", commit("dev", "more.md"))],
    );
    let after = head(chain, "notes", "dev");
    let [moved] = wakes(chain, thread).try_into().expect("one wake");
    let words = format!(
        "Wait {first} is over: change 1 in notes: its source refs/heads/dev moved from {before} to {after} by account {}.",
        world.agent
    );
    assert_eq!(moved, words);

    turn(chain, 14, &[("w2", "wait_for", change.clone())]);
    let second = waited(chain, thread, "w2");
    chain.at(16, TIME_MS);
    world.mention("m3", "close it");
    let close = json!({ "repo": "notes", "number": 1 });
    turn(chain, 17, &[("x1", "forge_change_close", close)]);
    let told = wakes(chain, thread);
    let closed = format!(
        "Wait {second} is over: change 1 in notes was closed by account {}.",
        world.agent
    );
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[1].starts_with(&closed), "{told:?}");

    turn(chain, 19, &[("w3", "wait_for", change)]);
    let text = refusal(chain, thread, "w3");
    assert!(
        text.contains("change 1 in notes is already closed"),
        "{text}"
    );
}

#[test]
fn a_change_merged_ends_its_wait_once_forge_tells_wake() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "watch my change", Some(WINDOW));
    seeded(&world, thread);
    let open = json!({ "repo": "notes", "from": "dev", "into": "main", "title": "dev's own" });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    let change = json!({ "event": { "change": { "repo": "notes", "n": 1 } } });
    turn(chain, 9, &[("w1", "wait_for", change)]);
    let wait = waited(chain, thread, "w1");
    chain.at(11, TIME_MS);
    assert!(wakes(chain, thread).is_empty());
    let heard = wake::Op::Forged(forge::Watched::Merged {
        repo: "notes".into(),
        n: 1,
        oid: "abc123".into(),
        by: Principal::Account(world.ada),
    });
    let by_forge = Origin::Program(forge::PROGRAM.into());
    chain.submit(by_forge, wake::PROGRAM, &heard).unwrap();
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let words = format!(
        "Wait {wait} is over: change 1 in notes was merged by account {} as abc123.",
        world.ada
    );
    assert_eq!(wake, words);
}

#[test]
fn a_wait_until_merged_hears_no_push_and_ends_when_the_change_merges() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "wait for the merge", Some(WINDOW));
    seeded(&world, thread);
    let open = json!({ "repo": "notes", "from": "dev", "into": "main", "title": "dev's own" });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    let until =
        |until: &str| json!({ "event": { "change": { "repo": "notes", "n": 1, "until": until } } });
    turn(
        chain,
        9,
        &[
            ("w1", "wait_for", until("merged")),
            ("w2", "wait_for", until("closed")),
        ],
    );
    let merged = waited(chain, thread, "w1");
    let closed = waited(chain, thread, "w2");

    chain.at(11, TIME_MS);
    world.mention("m2", "push twice");
    turn(
        chain,
        12,
        &[("c1", "forge_commit", commit("dev", "one.md"))],
    );
    turn(
        chain,
        14,
        &[("c2", "forge_commit", commit("dev", "two.md"))],
    );
    assert!(
        wakes(chain, thread).is_empty(),
        "a push wakes no wait until merged"
    );

    chain.at(16, TIME_MS);
    let merge = wake::Op::Forged(forge::Watched::Merged {
        repo: "notes".into(),
        n: 1,
        oid: "abc123".into(),
        by: Principal::Account(world.ada),
    });
    chain
        .submit(
            Origin::Program(forge::PROGRAM.into()),
            wake::PROGRAM,
            &merge,
        )
        .unwrap();
    chain.at(18, TIME_MS);
    let told = wakes(chain, thread);
    let ada = world.ada;
    let by_merge =
        format!("Wait {merged} is over: change 1 in notes was merged by account {ada} as abc123.");
    let not_closed = format!(
        "Wait {closed} is over: change 1 in notes was merged by account {ada} as abc123, so it will not be closed"
    );
    assert!(
        told.iter().any(|wake| wake.starts_with(&by_merge)),
        "{told:?}"
    );
    assert!(
        told.iter().any(|wake| wake.starts_with(&not_closed)),
        "{told:?}"
    );
    assert_eq!(told.len(), 2, "each wait wakes once: {told:?}");
}

#[test]
fn a_change_forge_takes_out_of_its_merge_queue_ends_its_waits_saying_why() {
    let world = world();
    let chain = &world.chain;
    let create = forge::Op::Create {
        repo: "notes".into(),
        hash: ::program::HashKind::Sha1,
    };
    chain.submit(signed(ADA), forge::PROGRAM, &create).unwrap();
    let anyone = ::program::Ownership::Anyone;
    set_list(chain, ADA, forge::PROGRAM, forge::repo_key("notes"), anyone);
    let thread = world.opened("m1", "queue my change", Some(WINDOW));
    seeded(&world, thread);
    let open = json!({ "repo": "notes", "from": "dev", "into": "main", "title": "dev's own" });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    chain.at(9, TIME_MS);
    let require = forge::Op::RequireChecks {
        repo: "notes".into(),
        branch: b"refs/heads/main".to_vec(),
        required: Some(forge::Required {
            reporter: Principal::Account(world.ada),
            contexts: vec!["gates".into()],
        }),
    };
    chain.submit(signed(ADA), forge::PROGRAM, &require).unwrap();
    let enqueue = forge::Op::Enqueue {
        repo: "notes".into(),
        n: 1,
        expected_head: head(chain, "notes", "dev"),
    };
    chain.submit(signed(ADA), forge::PROGRAM, &enqueue).unwrap();
    let merged = json!({ "event": { "change": { "repo": "notes", "n": 1, "until": "merged" } } });
    let moved = json!({ "event": { "change": { "repo": "notes", "n": 1 } } });
    turn(
        chain,
        11,
        &[("w1", "wait_for", merged), ("w2", "wait_for", moved)],
    );
    let until_merged = waited(chain, thread, "w1");
    let any_move = waited(chain, thread, "w2");
    assert!(wakes(chain, thread).is_empty());

    chain.at(13, TIME_MS);
    let preview = forge::Query::Log {
        repo: "notes".into(),
        from: forge::Revision::Ref(b"refs/changes/1/merge".to_vec()),
        exclude: None,
        page: PageRequest::first(1),
    };
    let forge::Reply::Log { tip: merge, .. } = chain.query(forge::PROGRAM, &preview).unwrap()
    else {
        panic!("a log answers Log")
    };
    let failed = forge::Op::Status {
        repo: "notes".into(),
        commit: merge,
        context: "gates".into(),
        state: forge::CheckState::Failed,
        url: None,
    };
    chain.submit(signed(ADA), forge::PROGRAM, &failed).unwrap();
    chain.at(15, TIME_MS);
    let told = wakes(chain, thread);
    let left = "change 1 in notes left the merge queue of refs/heads/main: gates failed";
    for wait in [until_merged, any_move] {
        let words = format!("Wait {wait} is over: {left}");
        let woke = told.iter().filter(|wake| wake.starts_with(&words)).count();
        assert_eq!(woke, 1, "wait {wait} wakes once, saying why: {told:?}");
    }
    assert_eq!(told.len(), 2, "{told:?}");
}
