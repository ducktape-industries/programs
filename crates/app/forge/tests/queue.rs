//! A merge queue lands its changes one after another, each as the very Preview commit its checks
//! passed on, and an entry that goes red or moves leaves with a notice for whoever queued it. A
//! train lands several at once, each previewed on the one ahead and judged on the last.
mod common;
use common::*;
use forge::{CheckState, Formed, OpReply, PreviewInfo, Required, Train};

const MAIN: &str = "refs/heads/main";
const ONE: &str = "refs/heads/one";
const TWO: &str = "refs/heads/two";
const THREE: &str = "refs/heads/three";
const FOUR: &str = "refs/heads/four";
const GATES: &str = "gates.toml#gates";
const REVIEW: &str = "review";

fn branch(name: &str) -> forge::Revision {
    forge::Revision::Ref(name.as_bytes().to_vec())
}

struct Repo {
    sandbox: MemorySandbox,
    store: MemoryObjects,
    root: Oid,
    one: Oid,
    two: Oid,
    three: Oid,
    four: Oid,
}

fn repo() -> Repo {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"1\n"), ("c", b"1\n")];
    let root = file_commit(&mut store, &[], 1, files);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"2\n"), ("c", b"1\n")];
    let one = file_commit(&mut store, &[root], 2, files);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"1\n"), ("c", b"2\n")];
    let two = file_commit(&mut store, &[root], 3, files);
    let files: &[(&str, &[u8])] = &[("a", b"1\n"), ("b", b"3\n"), ("c", b"1\n")];
    let three = file_commit(&mut store, &[root], 5, files);
    let files: &[(&str, &[u8])] = &[("a", b"2\n"), ("b", b"1\n"), ("c", b"1\n")];
    let four = file_commit(&mut store, &[root], 6, files);
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let zero = Hash::Sha1.zero();
    let refs = [
        (zero, root, MAIN),
        (zero, one, ONE),
        (zero, two, TWO),
        (zero, three, THREE),
        (zero, four, FOUR),
    ];
    let pack = pack_of(&store, &all_ids(&store));
    push(&sandbox, OWNER, "project", &refs, &pack).unwrap();
    for from in [ONE, TWO, THREE, FOUR] {
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
        three,
        four,
    }
}

fn train_of(length: u64, tip: &[&str]) -> Op {
    Op::SetTrain {
        repo: "project".into(),
        branch: MAIN.as_bytes().to_vec(),
        train: Some(Train {
            length,
            tip: tip.iter().map(|context| context.to_string()).collect(),
        }),
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
        self.report(1, commit, GATES, state);
    }

    fn report(&self, height: u64, commit: &str, context: &str, state: CheckState) {
        let op = Op::Status {
            repo: "project".into(),
            commit: commit.into(),
            context: context.into(),
            state,
            url: Some("/shared/actions/log".into()),
        };
        let sandbox = &self.sandbox;
        let reported = sandbox
            .forge
            .attempt(|| signed_op(sandbox, STRANGER, height, &op));
        reported.unwrap();
    }

    fn require_reviewed(&self) {
        let op = Op::RequireChecks {
            repo: "project".into(),
            branch: MAIN.as_bytes().to_vec(),
            required: Some(Required {
                reporter: person(STRANGER),
                contexts: vec![GATES.into(), REVIEW.into()],
            }),
        };
        act(&self.sandbox, OWNER, &op).unwrap();
    }

    fn train(&self, length: u64) {
        act(&self.sandbox, OWNER, &train_of(length, &[GATES])).unwrap();
    }

    fn trained(&self) -> (Option<Train>, Option<Formed>) {
        let asked = Query::Train {
            repo: "project".into(),
            branch: MAIN.as_bytes().to_vec(),
        };
        let reply: Reply = abi::decode(&ask(&self.sandbox, &asked).unwrap()).unwrap();
        let Reply::Train { train, formed, .. } = reply else {
            panic!("{reply:?}");
        };
        (train, formed)
    }

    fn formed(&self) -> Vec<u64> {
        let (_, formed) = self.trained();
        formed
            .map(|formed| formed.members.iter().map(|member| member.n).collect())
            .unwrap_or_default()
    }

    fn reviewed_pair(&self) -> (PreviewInfo, PreviewInfo) {
        self.require_reviewed();
        self.train(2);
        self.enqueue(1, self.one).unwrap();
        self.enqueue(2, self.two).unwrap();
        let first = self.preview(1).unwrap();
        self.report(1, &first.merge, REVIEW, CheckState::Passed);
        let second = self.preview(2).unwrap();
        self.report(1, &second.merge, REVIEW, CheckState::Passed);
        (first, second)
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

    fn rows(&self, n: u64) -> Vec<chat::MsgRow> {
        let query = chat::Query::Roots {
            channel_id: chat::namespace::id("forge", &format!("project:{n}")),
            page: PageRequest::first(64),
            viewer: Vec::new(),
        };
        let chat::Reply::Roots(page) = self.sandbox.chat_query(query).unwrap() else {
            panic!("roots");
        };
        page.items
    }

    fn said(&self, n: u64) -> Vec<String> {
        self.rows(n).into_iter().map(|row| row.text).collect()
    }

    fn left_for(&self, n: u64) -> Vec<Vec<u64>> {
        self.rows(n)
            .iter()
            .filter_map(|row| row.notice())
            .filter(|notice| notice.group.starts_with("left queue "))
            .map(|notice| notice.concerns.clone())
            .collect()
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
    assert_eq!(repo.left_for(1), [vec![11]], "{said}");
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

#[test]
fn a_train_stacks_each_preview_on_the_one_ahead_and_lands_them_all_when_its_tip_passes() {
    let repo = repo();
    repo.require_reviewed();
    repo.train(2);
    repo.enqueue(1, repo.one).unwrap();
    repo.enqueue(2, repo.two).unwrap();
    let first = repo.preview(1).unwrap();
    assert_eq!(first.into_head, repo.root.to_hex());
    let second = repo
        .preview(2)
        .expect("the second in line is previewed on the first, in the same op");
    assert_eq!(second.into_head, first.merge);
    assert!(
        repo.formed().is_empty(),
        "nothing forms behind a front whose own review is pending"
    );
    repo.report(1, &first.merge, REVIEW, CheckState::Passed);
    assert_eq!(repo.formed(), [1], "the second's own review is pending");
    repo.report(1, &second.merge, REVIEW, CheckState::Passed);
    assert_eq!(
        repo.formed(),
        [1, 2],
        "a review in the block the train formed in joins it"
    );
    repo.report(2, &first.merge, GATES, CheckState::Passed);
    assert_eq!(
        repo.main(),
        repo.root.to_hex(),
        "the train is judged on its tip, not its front"
    );
    repo.report(2, &second.merge, GATES, CheckState::Passed);
    assert_eq!(repo.main(), second.merge, "the branch moves to the tip");
    assert_eq!(repo.state(1), forge::ChangeState::Merged);
    assert_eq!(repo.state(2), forge::ChangeState::Merged);
    assert!(repo.queue().is_empty());
    assert!(repo.formed().is_empty());
}

#[test]
fn a_review_after_the_block_a_train_formed_in_waits_for_the_next_train() {
    let repo = repo();
    repo.require_reviewed();
    repo.train(2);
    repo.enqueue(1, repo.one).unwrap();
    repo.enqueue(2, repo.two).unwrap();
    let first = repo.preview(1).unwrap();
    repo.report(1, &first.merge, REVIEW, CheckState::Passed);
    let second = repo.preview(2).unwrap();
    repo.report(2, &second.merge, REVIEW, CheckState::Passed);
    assert_eq!(
        repo.formed(),
        [1],
        "the train holds its members after its block"
    );
    repo.report(2, &first.merge, GATES, CheckState::Passed);
    assert_eq!(
        repo.main(),
        first.merge,
        "the train lands only its own member"
    );
    assert_eq!(repo.state(2), forge::ChangeState::Open);
    assert_eq!(repo.queue(), [2]);
    assert_eq!(repo.formed(), [2], "the next train takes it");
}

#[test]
fn a_red_tip_sends_the_front_on_alone_and_a_red_there_takes_it_out() {
    let repo = repo();
    let (first, second) = repo.reviewed_pair();
    assert_eq!(repo.formed(), [1, 2]);
    repo.report(2, &second.merge, GATES, CheckState::Failed);
    assert_eq!(repo.formed(), [1], "the front goes on alone");
    assert_eq!(repo.queue(), [1, 2], "a red tip takes no one out");
    assert_eq!(repo.main(), repo.root.to_hex());
    repo.report(2, &first.merge, GATES, CheckState::Failed);
    assert_eq!(repo.queue(), [2]);
    assert_eq!(repo.state(1), forge::ChangeState::Open);
    let said = repo.said(1).join("\n");
    assert!(said.contains("left the merge queue"), "{said}");
    let again = repo.preview(2).unwrap();
    assert_eq!(
        again.into_head,
        repo.root.to_hex(),
        "the next front is previewed on the head"
    );
}

#[test]
fn a_front_that_passes_alone_lands_and_the_red_tip_then_takes_the_change_behind_it_out() {
    let repo = repo();
    let (first, second) = repo.reviewed_pair();
    repo.report(2, &second.merge, GATES, CheckState::Failed);
    repo.report(2, &first.merge, GATES, CheckState::Passed);
    assert_eq!(repo.main(), first.merge, "the front lands by itself");
    assert_eq!(repo.state(1), forge::ChangeState::Merged);
    assert!(
        repo.queue().is_empty(),
        "on the landed front the tip is the same red commit"
    );
    assert_eq!(repo.state(2), forge::ChangeState::Open);
    let said = repo.said(2).join("\n");
    assert!(said.contains("left the merge queue"), "{said}");
}

#[test]
fn a_change_whose_squash_conflicts_on_the_one_ahead_ends_the_train_and_keeps_its_place() {
    let repo = repo();
    repo.require_reviewed();
    repo.train(3);
    repo.enqueue(1, repo.one).unwrap();
    repo.enqueue(3, repo.three).unwrap();
    repo.enqueue(2, repo.two).unwrap();
    let first = repo.preview(1).unwrap();
    repo.report(1, &first.merge, REVIEW, CheckState::Passed);
    assert!(
        repo.preview(3).is_none(),
        "its squash on the first conflicts"
    );
    assert!(repo.preview(2).is_none(), "the train ends at the conflict");
    assert_eq!(repo.formed(), [1]);
    assert_eq!(repo.queue(), [1, 3, 2]);
}

#[test]
fn a_train_needs_required_checks_a_length_and_tip_contexts_the_branch_requires() {
    let repo = repo();
    let unrequired = refused(&repo.sandbox, OWNER, &train_of(2, &[GATES]));
    assert_eq!(unrequired.code, code::WRONG_STATE, "{unrequired}");
    repo.require_reviewed();
    let empty = refused(&repo.sandbox, OWNER, &train_of(0, &[GATES]));
    assert_eq!(empty.code, code::INVALID_INPUT, "{empty}");
    let stray = refused(&repo.sandbox, OWNER, &train_of(2, &["lint"]));
    assert_eq!(stray.code, code::INVALID_INPUT, "{stray}");
    assert!(stray.message.contains("lint"), "{stray}");
    repo.train(2);
    let (train, _) = repo.trained();
    assert_eq!(
        train,
        Some(Train {
            length: 2,
            tip: vec![GATES.into()],
        })
    );
    let dropped = Op::RequireChecks {
        repo: "project".into(),
        branch: MAIN.as_bytes().to_vec(),
        required: None,
    };
    act(&repo.sandbox, OWNER, &dropped).unwrap();
    assert_eq!(
        repo.trained(),
        (None, None),
        "dropping the checks drops the train"
    );
}

#[test]
fn a_preview_of_a_train_member_answers_the_queues_preview_and_leaves_the_tip() {
    let repo = repo();
    let (first, second) = repo.reviewed_pair();
    let asked = Op::Preview {
        repo: "project".into(),
        n: 2,
    };
    let made = act(&repo.sandbox, STRANGER, &asked).unwrap();
    let OpReply::Previewed { oid, .. } = abi::decode::<OpReply>(&made).unwrap() else {
        panic!("a Preview answers Previewed");
    };
    assert_eq!(oid, second.merge);
    let kept = repo.preview(2).unwrap();
    assert_eq!(
        kept.into_head, first.merge,
        "the queue's Preview, on the change ahead"
    );
    let (_, formed) = repo.trained();
    let tip = formed.and_then(|formed| formed.members.last().map(|member| member.merge.clone()));
    assert_eq!(tip, Some(second.merge));
}

#[test]
fn a_member_that_leaves_a_held_train_re_forms_it_from_the_changes_eligible_then() {
    let repo = repo();
    repo.require_reviewed();
    repo.train(3);
    repo.enqueue(1, repo.one).unwrap();
    repo.enqueue(2, repo.two).unwrap();
    repo.enqueue(4, repo.four).unwrap();
    let first = repo.preview(1).unwrap();
    repo.report(1, &first.merge, REVIEW, CheckState::Passed);
    let second = repo.preview(2).unwrap();
    repo.report(1, &second.merge, REVIEW, CheckState::Passed);
    let fourth = repo.preview(4).unwrap();
    assert_eq!(fourth.into_head, second.merge);
    repo.report(2, &fourth.merge, REVIEW, CheckState::Passed);
    assert_eq!(repo.formed(), [1, 2], "held from the block after it formed");
    let dequeue = Op::Dequeue {
        repo: "project".into(),
        n: 2,
    };
    let sandbox = &repo.sandbox;
    let left = sandbox
        .forge
        .attempt(|| signed_op(sandbox, OWNER, 3, &dequeue));
    left.unwrap();
    let moved = repo.preview(4).unwrap();
    assert_eq!(
        moved.into_head, first.merge,
        "the change behind is previewed on the front"
    );
    assert_eq!(
        repo.formed(),
        [1],
        "its review was of the Preview it no longer has"
    );
    repo.report(3, &moved.merge, REVIEW, CheckState::Passed);
    assert_eq!(
        repo.formed(),
        [1, 4],
        "the train re-formed in this block, so a review in it joins"
    );
}
