//! Actual program reply bytes replayed by forge-view.
//! FORGE_REGENERATE_FIXTURES=1 rewrites fixtures/replies.{bin,idx}.
mod common;
#[path = "../fixtures/loader.rs"]
mod loader;

use std::path::{Path, PathBuf};

use common::story::*;
use common::*;
use forge::*;
use sha2::{Digest, Sha256};

struct Captured {
    name: &'static str,
    request: Vec<u8>,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Tape(Vec<Captured>);

impl Tape {
    fn save(&mut self, name: &'static str, request: Vec<u8>, bytes: Vec<u8>) {
        self.0.push(Captured {
            name,
            request,
            bytes,
        });
    }

    fn capture(&mut self, rig: &Rig, name: &'static str, q: Query) -> Reply {
        let bytes = rig.query(&q).unwrap();
        let reply: Reply = abi::decode(&bytes).unwrap();
        assert_eq!(abi::encode(&reply), bytes);
        self.save(name, abi::encode(&q), bytes);
        reply
    }

    fn output(&mut self, rig: &mut Rig, name: &'static str, op: Op) -> OpReply {
        let bytes = rig.execute(&op).unwrap();
        let reply: OpReply = abi::decode(&bytes).unwrap();
        assert_eq!(abi::encode(&reply), bytes);
        self.save(name, abi::encode(&op), bytes);
        reply
    }

    fn refusal(&mut self, rig: &Rig, name: &'static str, q: Query) -> program::Error {
        let refusal = rig.query(&q).unwrap_err();
        self.save(name, abi::encode(&q), abi::encode(&refusal));
        refusal
    }

    fn git_reply(&mut self, rig: &Rig, name: &'static str, q: Query) {
        let bytes = rig.query(&q).unwrap();
        assert!(!bytes.is_empty());
        self.save(name, abi::encode(&q), bytes);
    }
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

fn write(tape: &Tape) {
    let mut bin = Vec::new();
    let mut idx = String::new();
    for c in &tape.0 {
        idx += &format!(
            "{} {} {} {} {}\n",
            c.name,
            bin.len(),
            c.bytes.len(),
            abi::hex(&c.request),
            abi::hex(&Sha256::digest(&c.bytes))
        );
        bin.extend_from_slice(&c.bytes);
    }
    std::fs::write(dir().join("replies.bin"), bin).unwrap();
    std::fs::write(dir().join("replies.idx"), idx).unwrap();
}

fn check(tape: &Tape) {
    let index = loader::index(&dir());
    let all = std::fs::read(dir().join("replies.bin")).unwrap();
    assert_eq!(
        index.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        tape.0.iter().map(|c| c.name).collect::<Vec<_>>(),
        "the committed shapes, in order"
    );
    for (f, c) in index.iter().zip(&tape.0) {
        let stored = &all[f.offset..f.offset + f.len];
        assert_eq!(f.request, c.request, "{}: request", c.name);
        assert_eq!(stored, c.bytes, "{}: actual program bytes", c.name);
        assert_eq!(
            f.sha256,
            abi::hex(&Sha256::digest(stored)),
            "{}: sha256",
            c.name
        );
    }
}

fn replay(tape: &mut Tape) {
    let bounds = fixture_bounds();
    let mut rig = Rig::start(bounds, HashKind::Sha1);
    let empty = MemorySandbox::default();
    Forge::init(&empty.exec(0), &abi::encode(&bounds)).unwrap();
    let q = Query::Repos {
        page: PageRequest::first(2),
    };
    let bytes = Forge::answer(&empty.reads(0), q.clone()).unwrap();
    let _: Reply = abi::decode(&bytes).unwrap();
    tape.save("repos-empty", abi::encode(&q), bytes);
    tape.capture(
        &rig,
        "refs-empty",
        Query::Refs {
            repo: REPO.into(),
            page: PageRequest::first(2),
        },
    );
    tape.refusal(
        &rig,
        "log-unborn",
        Query::Log {
            repo: REPO.into(),
            from: reference("main"),
            exclude: None,
            page: PageRequest::first(2),
        },
    );
    tape.capture(&rig, "changes-empty", changes());
    tape.capture(
        &rig,
        "review-requests-empty",
        Query::ReviewRequests {
            principal: Principal::Account(2),
            page: PageRequest::first(2),
        },
    );
    let mut story = Story::pushed(&mut rig);
    tape.capture(
        &rig,
        "repos",
        Query::Repos {
            page: PageRequest::first(2),
        },
    );
    tape.capture(&rig, "repo", Query::Repo { repo: REPO.into() });
    let Reply::Refs { page, .. } = tape.capture(
        &rig,
        "refs",
        Query::Refs {
            repo: REPO.into(),
            page: PageRequest::first(2),
        },
    ) else {
        panic!();
    };
    tape.capture(
        &rig,
        "refs-next",
        Query::Refs {
            repo: REPO.into(),
            page: PageRequest::resume(page.next, 64),
        },
    );
    let Reply::Log { page, .. } = tape.capture(
        &rig,
        "log",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: None,
            page: PageRequest::first(1),
        },
    ) else {
        panic!();
    };
    tape.capture(
        &rig,
        "log-next",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: None,
            page: PageRequest::resume(page.next, 1),
        },
    );
    let Reply::Tree { page, .. } = tape.capture(
        &rig,
        "tree",
        Query::Tree {
            repo: REPO.into(),
            at: story.feature.clone(),
            path: vec![],
            page: PageRequest::first(2),
        },
    ) else {
        panic!();
    };
    tape.capture(
        &rig,
        "tree-next",
        Query::Tree {
            repo: REPO.into(),
            at: story.feature.clone(),
            path: vec![],
            page: PageRequest::resume(page.next, 64),
        },
    );
    tape.capture(
        &rig,
        "tree-directory",
        Query::Tree {
            repo: REPO.into(),
            at: story.feature.clone(),
            path: b"src".to_vec(),
            page: PageRequest::first(2),
        },
    );
    for (name, path) in [
        ("blob", "src/lib.rs"),
        ("blob-binary", "image.bin"),
        ("blob-oversize", "large.txt"),
        ("blob-empty", "empty.txt"),
    ] {
        tape.capture(
            &rig,
            name,
            Query::Blob {
                repo: REPO.into(),
                oid: story.oid(path),
                range: None,
            },
        );
    }
    tape.capture(
        &rig,
        "blob-range",
        Query::Blob {
            repo: REPO.into(),
            oid: story.oid("src/lib.rs"),
            range: Some(ByteRange { offset: 4, len: 6 }),
        },
    );
    let Reply::Diff { page, .. } = tape.capture(
        &rig,
        "diff",
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.feature.clone(),
            path: None,
            page: PageRequest::first(2),
        },
    ) else {
        panic!();
    };
    tape.capture(
        &rig,
        "diff-next",
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.feature.clone(),
            path: None,
            page: PageRequest::resume(page.next, 2),
        },
    );
    for (name, path) in [
        ("diff-text", "src/lib.rs"),
        ("diff-mode", "mode.sh"),
        ("diff-binary", "image.bin"),
        ("diff-oversize", "large.txt"),
        ("diff-gitlink", "vendor"),
        ("diff-deleted", "gone.txt"),
        ("diff-added", "new.txt"),
    ] {
        tape.capture(
            &rig,
            name,
            Query::Diff {
                repo: REPO.into(),
                base: Some(story.root.clone()),
                head: story.feature.clone(),
                path: Some(path.as_bytes().to_vec()),
                page: PageRequest::first(2),
            },
        );
    }
    tape.capture(
        &rig,
        "diff-root",
        Query::Diff {
            repo: REPO.into(),
            base: None,
            head: story.root.clone(),
            path: Some(b"README.md".to_vec()),
            page: PageRequest::first(2),
        },
    );
    tape.capture(
        &rig,
        "diff-empty",
        Query::Diff {
            repo: REPO.into(),
            base: Some(story.root.clone()),
            head: story.root.clone(),
            path: None,
            page: PageRequest::first(2),
        },
    );
    for (name, from, into) in [
        ("compare", "feature", "main"),
        ("compare-up-to-date", "main", "feature"),
        ("compare-diverged", "feature", "clean"),
        ("compare-unrelated", "feature", "unrelated"),
    ] {
        tape.capture(&rig, name, compare(from, into));
    }
    tape.capture(
        &rig,
        "compares",
        Query::Compares {
            repo: REPO.into(),
            into: reference("main"),
            from: ["feature", "clean", "conflict", "unrelated"]
                .map(reference)
                .to_vec(),
        },
    );
    tape.git_reply(
        &rig,
        "advertise-receive",
        Query::Advertise {
            repo: REPO.into(),
            service: Service::ReceivePack,
        },
    );
    tape.git_reply(
        &rig,
        "advertise-upload",
        Query::Advertise {
            repo: REPO.into(),
            service: Service::UploadPack,
        },
    );
    let mut request = Vec::new();
    pktline::push_line(&mut request, b"command=ls-refs");
    request.extend_from_slice(b"0001");
    pktline::push_line(&mut request, b"symrefs");
    request.extend_from_slice(b"0000");
    tape.git_reply(
        &rig,
        "upload-refs",
        Query::Upload {
            repo: REPO.into(),
            request,
        },
    );
    tape.refusal(
        &rig,
        "refused-object-not-held",
        Query::Blob {
            repo: REPO.into(),
            oid: "f".repeat(40),
            range: None,
        },
    );
    tape.refusal(
        &rig,
        "refused-not-found",
        Query::Repo {
            repo: "absent".into(),
        },
    );
    tape.refusal(
        &rig,
        "refused-invalid-input",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: None,
            page: PageRequest::resume(Some(vec![1]), 1),
        },
    );
    let Reply::Refs { page, .. } = tape.capture(
        &rig,
        "refs-before-update",
        Query::Refs {
            repo: REPO.into(),
            page: PageRequest::first(1),
        },
    ) else {
        panic!();
    };
    rig.advance();
    rig.execute(&Op::Configure {
        repo: REPO.into(),
        settings: Settings::default(),
    })
    .unwrap();
    tape.refusal(
        &rig,
        "refused-stale",
        Query::Refs {
            repo: REPO.into(),
            page: PageRequest::resume(page.next.clone(), 1),
        },
    );
    tape.refusal(
        &rig,
        "refused-other-listing",
        Query::Refs {
            repo: "other".into(),
            page: PageRequest::resume(page.next, 1),
        },
    );
    tape.output(&mut rig, "op-change-open", story.open("Review this change"));
    tape.capture(&rig, "change", change(1));
    tape.capture(&rig, "changes", changes());
    tape.capture(
        &rig,
        "review-requests",
        Query::ReviewRequests {
            principal: Principal::Account(2),
            page: PageRequest::first(2),
        },
    );
    tape.output(
        &mut rig,
        "op-change-edit",
        Op::ChangeEdit {
            repo: REPO.into(),
            n: 1,
            title: Some("Review this edited change".into()),
            body: Some("An edited body on the forge record.".into()),
            reviewers: None,
        },
    );
    rig.actor = b"reviewer".to_vec();
    for (name, verdict) in [
        ("op-review-comment", Verdict::Comment),
        ("op-review-request-changes", Verdict::RequestChanges),
        ("op-review-approve", Verdict::Approve),
    ] {
        tape.output(&mut rig, name, review(&story.feature, &story.root, verdict));
    }
    rig.advance();
    let Reply::Change { reviews, .. } = tape.capture(
        &rig,
        "change-reviewed",
        Query::Change {
            repo: REPO.into(),
            n: 1,
            page: PageRequest::first(2),
        },
    ) else {
        panic!();
    };
    tape.capture(
        &rig,
        "change-reviews-next",
        Query::Change {
            repo: REPO.into(),
            n: 1,
            page: PageRequest::resume(reviews.next, 2),
        },
    );
    tape.capture(
        &rig,
        "review-requests-reviewed",
        Query::ReviewRequests {
            principal: Principal::Account(2),
            page: PageRequest::first(2),
        },
    );
    rig.actor = TESTER.to_vec();
    let tip = story.push_follow_up(&mut rig, "follow-up.txt", b"follow-up\n");
    tape.capture(&rig, "change-outdated", change(1));
    tape.capture(
        &rig,
        "review-requests-head-moved",
        Query::ReviewRequests {
            principal: Principal::Account(2),
            page: PageRequest::first(2),
        },
    );
    tape.output(
        &mut rig,
        "op-change-open-second",
        story.open("Close this change"),
    );
    tape.output(
        &mut rig,
        "op-change-close",
        Op::ChangeClose {
            repo: REPO.into(),
            n: 2,
        },
    );
    tape.capture(&rig, "change-closed", change(2));
    tape.capture(
        &rig,
        "changes-filtered",
        Query::Changes {
            repo: REPO.into(),
            filter: ChangeFilter {
                state: Some(ChangeState::Closed),
                ..Default::default()
            },
            page: PageRequest::first(2),
        },
    );
    tape.output(
        &mut rig,
        "op-merge",
        Op::Merge {
            repo: REPO.into(),
            into: b"refs/heads/main".to_vec(),
            from: reference("feature"),
            expected_into: story.root.clone(),
            expected_from: tip.clone(),
            result: tip,
            change: Some(1),
        },
    );
    tape.capture(&rig, "change-merged", change(1));
    let mut narrow = Rig::start(
        Bounds {
            log_walk: 1,
            ..bounds
        },
        HashKind::Sha1,
    );
    Story::pushed(&mut narrow);
    tape.refusal(
        &narrow,
        "refused-capacity",
        Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: None,
            page: PageRequest::first(1),
        },
    );
    tape.capture(&rig, "unarchived", Query::Archived { repo: REPO.into() });
    rig.execute(&Op::Archive {
        repo: REPO.into(),
        archived: true,
    })
    .unwrap();
    tape.capture(&rig, "archived", Query::Archived { repo: REPO.into() });
    tape.capture(
        &rig,
        "archived-repos",
        Query::ArchivedRepos {
            page: PageRequest::first(2),
        },
    );
    tape.capture(
        &rig,
        "repos-archived",
        Query::Repos {
            page: PageRequest::first(2),
        },
    );
    let mut checked = Rig::start(bounds, HashKind::Sha1);
    let story = Story::pushed(&mut checked);
    checked.execute(&story.open("Checked change")).unwrap();
    checked.advance();
    let reviewer: &[u8] = b"reviewer";
    for (actor, commit, context, state, url) in [
        (
            TESTER,
            &story.feature,
            "build.sh#lane",
            CheckState::Passed,
            Some("desktop:20261021-081734-1"),
        ),
        (
            reviewer,
            &story.feature,
            "check.toml#check",
            CheckState::Pending,
            None,
        ),
    ] {
        checked.actor = actor.to_vec();
        checked
            .execute(&Op::Status {
                repo: REPO.into(),
                commit: commit.clone(),
                context: context.into(),
                state,
                url: url.map(Into::into),
            })
            .unwrap();
    }
    checked.advance();
    tape.capture(
        &checked,
        "statuses",
        Query::Statuses {
            repo: REPO.into(),
            commit: story.feature.clone(),
            page: PageRequest::first(128),
        },
    );
}

#[test]
fn replay_fixtures_are_the_programs_real_bytes() {
    let tape = &mut Tape::default();
    replay(tape);
    if std::env::var_os("FORGE_REGENERATE_FIXTURES").as_deref() == Some(std::ffi::OsStr::new("1")) {
        write(tape);
    } else {
        check(tape);
    }
}
