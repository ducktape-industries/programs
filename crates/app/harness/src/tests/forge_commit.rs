//! `forge_commit` with forge seated: an agent's commit creates the repository it names, the
//! agent's, and the next one goes on top; a stale base, a repository someone else owns and a
//! commit over forge's bounds are refused in the tool's words, and a new repository's first
//! commit over them in forge's.
use ::program::{Ownership, Party};
use forge::{Bounds, CommitInfo, Revision, TreeInfo};

use super::*;

/// When the agent's calls run: the commit's time is this, in seconds.
const TIME_MS: u64 = 1_760_000_000_000;

fn bounds(max_objects: u64) -> Bounds {
    Bounds {
        max_objects,
        max_delta_depth: 64,
        max_object_size: 1 << 20,
        push_walk: 1000,
        fetch_walk: 1000,
        merge_cost: 100_000,
        page_size: 128,
        log_walk: 1000,
        tree_walk: 256,
        diff_bytes: 4 << 20,
        blob_bytes: 1 << 20,
        record_bytes: 64 << 10,
    }
}

fn forged(max_objects: u64) -> World {
    let mut world = world();
    world.chain.seat::<forge::Forge>(forge::PROGRAM);
    world
        .chain
        .init(forge::PROGRAM, &bounds(max_objects))
        .unwrap();
    world
}

/// One model turn of `forge_commit` calls at `height`, each run in the order given once its
/// relay comes back, a block later.
fn turn(chain: &MockChain, height: u64, calls: &[(&str, Value)]) {
    chain.at(height, TIME_MS);
    let uses: Vec<Value> = calls
        .iter()
        .map(|(id, input)| tool_use(id, "forge_commit", input.clone()))
        .collect();
    model_says(chain, Value::Array(uses), "tool_use");
    chain.at(height + 1, TIME_MS);
    for (id, _) in calls {
        relay_back(chain, id);
    }
}

fn file(path: &str, content: &str) -> Value {
    json!({ "path": path, "content": content })
}

fn ask(chain: &MockChain, query: forge::Query) -> forge::Reply {
    chain.query(forge::PROGRAM, &query).unwrap()
}

fn log(chain: &MockChain, repo: &str) -> Vec<CommitInfo> {
    let asked = forge::Query::Log {
        repo: repo.into(),
        from: Revision::Ref(b"refs/heads/main".to_vec()),
        exclude: None,
        page: PageRequest::first(16),
    };
    let forge::Reply::Log { page, .. } = ask(chain, asked) else {
        panic!("a log answers Log")
    };
    page.items
}

fn names(chain: &MockChain, repo: &str, at: &str, path: &str) -> Vec<(String, forge::EntryKind)> {
    let asked = forge::Query::Tree {
        repo: repo.into(),
        at: at.into(),
        path: path.as_bytes().to_vec(),
        page: PageRequest::first(16),
    };
    let forge::Reply::Tree { page, .. } = ask(chain, asked) else {
        panic!("a tree answers Tree")
    };
    page.items
        .into_iter()
        .map(|TreeInfo { name, kind, .. }| (String::from_utf8(name).unwrap(), kind))
        .collect()
}

#[test]
fn an_agent_commits_to_a_repository_it_creates_and_then_on_top_of_its_commit() {
    use forge::EntryKind::{Directory, Executable, File};
    let world = forged(10_000);
    let chain = &world.chain;
    let thread = world.opened("m1", "start a repo", Some(WINDOW));
    let start = json!({
        "repo": "notes",
        "files": [
            file("README.md", "# Notes\n"),
            { "path": "bin/run.sh", "content": "echo ok\n", "executable": true },
            file("src/main.rs", "fn main() {}\n"),
        ],
        "message": "Start the notes",
    });
    turn(chain, 3, &[("c1", start)]);
    let (text, failed) = result_of(chain, thread, "c1");
    assert!(!failed, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({ "ok": true, "pushed": "refs/heads/main" })
    );
    let forge::Reply::Repo { repo, .. } = ask(
        chain,
        forge::Query::Repo {
            repo: "notes".into(),
        },
    ) else {
        panic!()
    };
    assert_eq!(repo.repo.owner, Principal::Account(world.agent));
    let [started] = log(chain, "notes").try_into().unwrap();
    assert_eq!(started.parents, Vec::<String>::new());
    assert_eq!(started.message, b"Start the notes\n");
    assert_eq!(started.author, started.committer);
    assert_eq!(
        (
            started.author.name.as_slice(),
            started.author.email.clone(),
            started.author.time
        ),
        (
            b"Scout".as_slice(),
            format!("{}@net", world.agent).into_bytes(),
            1_760_000_000
        )
    );
    assert_eq!(
        names(chain, "notes", &started.oid, ""),
        [
            ("README.md".to_owned(), File),
            ("bin".to_owned(), Directory),
            ("src".to_owned(), Directory)
        ]
    );
    assert_eq!(
        names(chain, "notes", &started.oid, "bin"),
        [("run.sh".to_owned(), Executable)]
    );

    let next = json!({
        "repo": "notes",
        "files": [file("src/lib.rs", "pub fn notes() {}\n")],
        "remove": ["bin"],
        "message": "Code, no scripts",
    });
    let stale = json!({
        "repo": "notes",
        "files": [file("late.md", "late\n")],
        "message": "Late",
        "base": started.oid,
    });
    turn(chain, 5, &[("c2", next), ("c3", stale)]);
    let (text, failed) = result_of(chain, thread, "c2");
    assert!(!failed, "{text}");
    // both commits carry one time, so the log may list them in either order
    let log = log(chain, "notes");
    assert!(log.len() == 2 && log.contains(&started), "{log:?}");
    let second = log.iter().find(|commit| *commit != &started).unwrap();
    assert_eq!(second.parents, std::slice::from_ref(&started.oid));
    assert_eq!(
        names(chain, "notes", &second.oid, ""),
        [
            ("README.md".to_owned(), File),
            ("src".to_owned(), Directory)
        ]
    );
    assert_eq!(
        names(chain, "notes", &second.oid, "src"),
        [("lib.rs".to_owned(), File), ("main.rs".to_owned(), File)]
    );
    let (text, failed) = result_of(chain, thread, "c3");
    assert!(failed && text.starts_with(code::STALE), "{text}");
    assert!(text.contains(&second.oid), "{text}");
}

#[test]
fn a_repository_someone_else_owns_takes_the_agents_commit_once_its_list_allows_harness() {
    let world = forged(10_000);
    let chain = &world.chain;
    let create = forge::Op::Create {
        repo: "ada".into(),
        hash: ::program::HashKind::Sha1,
    };
    chain.submit(signed(ADA), forge::PROGRAM, &create).unwrap();
    let thread = world.opened("m1", "write to ada", Some(WINDOW));
    let commit = |message: &str| json!({ "repo": "ada", "files": [file("notes.md", message)], "message": message });
    turn(chain, 3, &[("c1", commit("not yet"))]);
    let (text, failed) = result_of(chain, thread, "c1");
    assert!(failed && text.starts_with(code::UNAUTHORIZED), "{text}");

    let harness = Ownership::Listed(vec![Party::Program(PROGRAM.into())]);
    set_list(chain, ADA, forge::PROGRAM, forge::repo_key("ada"), harness);
    turn(chain, 5, &[("c2", commit("now"))]);
    let (text, failed) = result_of(chain, thread, "c2");
    assert!(!failed, "{text}");
    let [commit] = log(chain, "ada").try_into().unwrap();
    assert_eq!(commit.message, b"now\n");
}

#[test]
fn a_commit_over_forges_bounds_is_refused_in_the_tools_words() {
    let world = forged(4);
    let chain = &world.chain;
    let thread = world.opened("m1", "write a lot", Some(WINDOW));
    let one = json!({ "repo": "notes", "files": [file("a.md", "a")], "message": "one" });
    turn(chain, 3, &[("c1", one)]);
    assert!(!result_of(chain, thread, "c1").1);
    let three = json!({
        "repo": "notes",
        "files": [file("b.md", "b"), file("c.md", "c"), file("d.md", "d")],
        "message": "three",
    });
    turn(chain, 5, &[("c2", three)]);
    let (text, failed) = result_of(chain, thread, "c2");
    assert!(failed && text.starts_with(code::CAPACITY), "{text}");
    assert!(text.contains("at most 4"), "{text}");
    assert_eq!(log(chain, "notes").len(), 1);
}

/// A new repository's bounds are unknown until it exists, so only forge's push checks its first
/// commit: over them, the call fails in forge's words, and the repository stays, the agent's and
/// empty.
#[test]
fn a_first_commit_over_forges_bounds_fails_and_leaves_its_new_repository_empty() {
    let world = forged(4);
    let chain = &world.chain;
    let thread = world.opened("m1", "write a lot", Some(WINDOW));
    let three = json!({
        "repo": "notes",
        "files": [file("a.md", "a"), file("b.md", "b"), file("c.md", "c")],
        "message": "three",
    });
    turn(chain, 3, &[("c1", three)]);
    let (text, failed) = result_of(chain, thread, "c1");
    assert!(failed && text.starts_with(code::CAPACITY), "{text}");
    assert!(text.contains("too many objects"), "{text}");
    let forge::Reply::Repo { repo, .. } = ask(
        chain,
        forge::Query::Repo {
            repo: "notes".into(),
        },
    ) else {
        panic!()
    };
    assert_eq!(
        (repo.repo.owner, repo.repo.refs_count),
        (Principal::Account(world.agent), 0)
    );
}
