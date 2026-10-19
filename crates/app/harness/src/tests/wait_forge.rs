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
    assert!(
        assigned(chain, SERVICE).is_empty(),
        "the turn that armed it ends"
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
fn a_change_merged_ends_its_wait_and_only_forge_tells_it() {
    let world = world();
    let chain = &world.chain;
    let thread = world.opened("m1", "watch my change", Some(WINDOW));
    seeded(&world, thread);
    let open = json!({ "repo": "notes", "from": "dev", "into": "main", "title": "dev's own" });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    let change = json!({ "event": { "change": { "repo": "notes", "n": 1 } } });
    turn(chain, 9, &[("w1", "wait_for", change)]);
    let wait = waited(chain, thread, "w1");
    let merged = Op::Forged(forge::Watched::Merged {
        repo: "notes".into(),
        n: 1,
        oid: "abc123".into(),
        by: Principal::Account(world.ada),
    });
    chain.at(11, TIME_MS);
    let error = chain.submit(signed(ADA), PROGRAM, &merged).unwrap_err();
    assert_eq!(error.code, code::UNAUTHORIZED, "{error:?}");
    assert!(wakes(chain, thread).is_empty());
    let by_forge = Origin::Program(forge::PROGRAM.into());
    chain.submit(by_forge, PROGRAM, &merged).unwrap();
    let [wake] = wakes(chain, thread).try_into().expect("one wake");
    let words = format!(
        "Wait {wait} is over: change 1 in notes was merged by account {} as abc123.",
        world.ada
    );
    assert_eq!(wake, words);
}
