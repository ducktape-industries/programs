//! A merge queue lands its changes one after another, each as the very Preview commit its checks
//! passed on, and an entry that goes red or moves leaves with a line naming who queued it.
mod common;
use common::*;
use forge::{CheckState, OpReply, PreviewInfo, Required};

const MAIN: &str = "refs/heads/main";
const ONE: &str = "refs/heads/one";
const TWO: &str = "refs/heads/two";
const GATES: &str = "gates.toml#gates";

fn branch(name: &str) -> forge::Revision {
    forge::Revision::Ref(name.as_bytes().to_vec())
}

struct Repo {
    sandbox: MemorySandbox,
    store: MemoryObjects,
    root: Oid,
    one: Oid,
    two: Oid,
}

fn repo() -> Repo {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"1\n"), ("c", b"1\n")];
    let root = file_commit(&mut store, &[], 1, files);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"2\n"), ("c", b"1\n")];
    let one = file_commit(&mut store, &[root], 2, files);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"1\n"), ("c", b"2\n")];
    let two = file_commit(&mut store, &[root], 3, files);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [(zero, root, MAIN), (zero, one, ONE), (zero, two, TWO)];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    for from in [ONE, TWO] {
        let open = Op::ChangeOpen {
            repo: "project".into(),
            from: branch(from),
            into: MAIN.as_bytes().to_vec(),
            title: format!("Teach {from}"),
            body: String::new(),
            reviewers: Vec::new(),
        };
        act(&sandbox, WRITER, &open).unwrap();
    }
    Repo {
        sandbox,
        store,
        root,
        one,
        two,
    }
}

impl Repo {
    fn require(&self) {
        let op = Op::RequireChecks {
            repo: "project".into(),
            branch: MAIN.as_bytes().to_vec(),
            required: Some(Required {
                reporter: person(STRANGER),
                contexts: vec![GATES.into()],
            }),
        };
        act(&self.sandbox, OWNER, &op).unwrap();
    }

    fn enqueue(&self, n: u64, head: Oid) -> Result<Vec<u8>, program::Error> {
        let op = Op::Enqueue {
            repo: "project".into(),
            n,
            expected_head: head.to_hex(),
        };
        act(&self.sandbox, OWNER, &op)
    }

    fn queue(&self) -> Vec<u64> {
        let asked = Query::Queue {
            repo: "project".into(),
            branch: MAIN.as_bytes().to_vec(),
            page: PageRequest::first(16),
        };
        let reply: Reply = abi::decode(&ask(&self.sandbox, &asked).unwrap()).unwrap();
        let Reply::Queue { page, .. } = reply else {
            panic!("{reply:?}");
        };
        page.items.iter().map(|entry| entry.n).collect()
    }

    fn preview(&self, n: u64) -> Option<PreviewInfo> {
        let asked = Query::Preview {
            repo: "project".into(),
            n,
        };
        let reply: Reply = abi::decode(&ask(&self.sandbox, &asked).unwrap()).unwrap();
        let Reply::Preview { preview, .. } = reply else {
            panic!("{reply:?}");
        };
        preview
    }

    fn status(&self, commit: &str, state: CheckState) {
        let op = Op::Status {
            repo: "project".into(),
            commit: commit.into(),
            context: GATES.into(),
            state,
            url: Some("/shared/actions/log".into()),
        };
        act(&self.sandbox, STRANGER, &op).unwrap();
    }

    fn main(&self) -> String {
        refs_of(&self.sandbox, "project")[MAIN].clone()
    }

    fn state(&self, n: u64) -> forge::ChangeState {
        let asked = Query::Change {
            repo: "project".into(),
            n,
            page: PageRequest::first(1),
        };
        let reply: Reply = abi::decode(&ask(&self.sandbox, &asked).unwrap()).unwrap();
        let Reply::Change { change, .. } = reply else {
            panic!("{reply:?}");
        };
        change.state
    }

    fn said(&self, n: u64) -> Vec<String> {
        let query = chat::Query::Roots {
            channel_id: chat::namespace::id("forge", &format!("project:{n}")),
            page: PageRequest::first(64),
            viewer: Vec::new(),
        };
        let chat::Reply::Roots(page) = self.sandbox.chat_query(query).unwrap() else {
            panic!("roots");
        };
        page.items.into_iter().map(|row| row.text).collect()
    }
}

#[test]
fn a_queue_lands_each_change_as_the_preview_its_checks_passed_on_one_after_another() {
    let repo = repo();
    repo.require();
    repo.enqueue(1, repo.one).unwrap();
    repo.enqueue(2, repo.two).unwrap();
    assert_eq!(repo.queue(), [1, 2]);
    let first = repo
        .preview(1)
        .expect("the first in line is previewed as it joins");
    assert_eq!(first.into_head, repo.root.to_hex());
    assert!(
        repo.preview(2).is_none(),
        "only the first in line is previewed"
    );
    repo.status(&first.merge, CheckState::Pending);
    assert_eq!(repo.main(), repo.root.to_hex(), "pending lands nothing");
    repo.status(&first.merge, CheckState::Passed);
    assert_eq!(
        repo.main(),
        first.merge,
        "the tested commit is the one that lands"
    );
    assert_eq!(repo.state(1), forge::ChangeState::Merged);
    assert_eq!(repo.queue(), [2]);
    let second = repo
        .preview(2)
        .expect("the next in line is previewed on the new head");
    assert_eq!(second.into_head, first.merge);
    repo.status(&second.merge, CheckState::Passed);
    assert_eq!(repo.main(), second.merge);
    assert!(repo.queue().is_empty());
}

#[test]
fn an_entry_that_goes_red_or_moves_leaves_and_its_channel_hears_why_naming_who_queued_it() {
    let mut repo = repo();
    repo.require();
    repo.enqueue(1, repo.one).unwrap();
    repo.enqueue(2, repo.two).unwrap();
    let first = repo.preview(1).unwrap();
    repo.status(&first.merge, CheckState::Failed);
    assert_eq!(repo.queue(), [2]);
    assert_eq!(repo.state(1), forge::ChangeState::Open);
    assert_eq!(repo.main(), repo.root.to_hex());
    let said = repo.said(1).join("\n");
    assert!(said.contains("<@11>"), "{said}");
    assert!(said.contains("left the merge queue"), "{said}");
    assert!(
        said.contains("gates.toml#gates failed (/shared/actions/log)"),
        "{said}"
    );
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"1\n"), ("c", b"3\n")];
    let moved = file_commit(&mut repo.store, &[repo.two], 4, files);
    let pack = pack_of(&repo.store, &all_ids(&repo.store));
    push(
        &repo.sandbox,
        OWNER,
        "project",
        &[(repo.two, moved, TWO)],
        &pack,
    )
    .unwrap();
    assert!(repo.queue().is_empty(), "its head moved");
    let said = repo.said(2).join("\n");
    assert!(said.contains("its head moved"), "{said}");
}

#[test]
fn a_change_joins_a_queue_of_a_branch_that_requires_checks_at_its_head_and_once() {
    let repo = repo();
    let unrequired = repo.enqueue(1, repo.one).unwrap_err();
    assert!(
        unrequired.message.contains("requires no checks"),
        "{}",
        unrequired.message
    );
    repo.require();
    let stale = repo.enqueue(1, repo.two).unwrap_err();
    assert!(stale.message.contains("head moved"), "{}", stale.message);
    repo.enqueue(1, repo.one).unwrap();
    let twice = repo.enqueue(1, repo.one).unwrap_err();
    assert_eq!(twice.code, code::WRONG_STATE);
    let dequeue = Op::Dequeue {
        repo: "project".into(),
        n: 1,
    };
    act(&repo.sandbox, OWNER, &dequeue).unwrap();
    assert!(repo.queue().is_empty());
    assert_eq!(
        refused(&repo.sandbox, OWNER, &dequeue).code,
        code::NOT_FOUND
    );
    let previewed = Op::Preview {
        repo: "project".into(),
        n: 1,
    };
    let made = act(&repo.sandbox, STRANGER, &previewed).unwrap();
    assert!(matches!(
        abi::decode(&made).unwrap(),
        OpReply::Previewed { .. }
    ));
}
