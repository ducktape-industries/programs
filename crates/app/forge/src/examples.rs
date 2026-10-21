//! A working value of each forge op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use ::program::{BlobId, HashKind};

use crate::contract::*;

const REPO: &str = "notes";
const OID: &str = "0000000000000000000000000000000000000000";

fn repo() -> String {
    REPO.into()
}

fn oid() -> String {
    OID.into()
}

fn main() -> Vec<u8> {
    b"refs/heads/main".to_vec()
}

fn feature() -> Revision {
    Revision::Ref(b"refs/heads/feature".to_vec())
}

fn page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

pub(crate) fn create() -> Op {
    Op::Create {
        repo: repo(),
        hash: HashKind::Sha1,
    }
}

pub(crate) fn configure() -> Op {
    Op::Configure {
        repo: repo(),
        settings: Settings::default(),
    }
}

pub(crate) fn push() -> Op {
    Op::Push {
        repo: repo(),
        request: Vec::new(),
        pack: None,
    }
}

pub(crate) fn merge() -> Op {
    Op::Merge {
        repo: repo(),
        into: main(),
        from: feature(),
        expected_into: oid(),
        expected_from: oid(),
        result: oid(),
        change: Some(1),
    }
}

pub(crate) fn open() -> Op {
    Op::ChangeOpen {
        repo: repo(),
        from: feature(),
        into: main(),
        title: "notes: a first page".into(),
        body: "What changes and why.".into(),
        reviewers: vec![Principal::Account(17)],
    }
}

pub(crate) fn edit() -> Op {
    Op::ChangeEdit {
        repo: repo(),
        n: 1,
        title: Some("notes: the first page".into()),
        body: None,
        reviewers: None,
    }
}

pub(crate) fn close() -> Op {
    Op::ChangeClose { repo: repo(), n: 1 }
}

pub(crate) fn review() -> Op {
    let review = ReviewDraft {
        verdict: Verdict::Comment,
        body: "Reads well.".into(),
        commit_oid: oid(),
        base_oid: None,
        comments: Vec::new(),
    };
    Op::ReviewSubmit {
        repo: repo(),
        n: 1,
        review,
    }
}

pub(crate) fn stage() -> Op {
    Op::Stage {
        repo: repo(),
        pack: BlobId::Sha256([0; 32]),
    }
}

pub(crate) fn squash() -> Op {
    Op::SquashMerge {
        repo: repo(),
        into: main(),
        from: feature(),
        expected_into: oid(),
        expected_from: oid(),
        message: None,
        change: Some(1),
    }
}

pub(crate) fn rebase() -> Op {
    Op::Rebase {
        repo: repo(),
        n: 1,
        onto: Revision::Ref(main()),
        expected_head: oid(),
    }
}

pub(crate) fn repoint() -> Op {
    Op::ChangeRepoint {
        repo: repo(),
        n: 1,
        from: Revision::Ref(b"refs/heads/feature-2".to_vec()),
    }
}

pub(crate) fn delete_branch() -> Op {
    Op::DeleteBranch {
        repo: repo(),
        branch: b"refs/heads/feature".to_vec(),
        expected: oid(),
    }
}

pub(crate) fn protect() -> Op {
    Op::Protect {
        repo: repo(),
        branch: main(),
        protected: true,
    }
}

pub(crate) fn watch() -> Op {
    Op::Watch { deliver: vec![0] }
}

pub(crate) fn preview() -> Op {
    Op::Preview { repo: repo(), n: 1 }
}

pub(crate) fn status() -> Op {
    Op::Status {
        repo: repo(),
        commit: oid(),
        context: "gates.toml#gates".into(),
        state: CheckState::Passed,
        url: None,
    }
}

pub(crate) fn require() -> Op {
    let required = Required {
        reporter: Principal::Account(37),
        contexts: vec!["gates.toml#gates".into()],
    };
    Op::RequireChecks {
        repo: repo(),
        branch: main(),
        required: Some(required),
    }
}

pub(crate) fn enqueue() -> Op {
    Op::Enqueue {
        repo: repo(),
        n: 1,
        expected_head: oid(),
    }
}

pub(crate) fn dequeue() -> Op {
    Op::Dequeue { repo: repo(), n: 1 }
}

pub(crate) fn adopt() -> Op {
    Op::AdoptOwners {
        kind: "forge.change".into(),
        after: None,
        limit: 100,
    }
}

pub(crate) fn archive() -> Op {
    Op::Archive {
        repo: repo(),
        archived: true,
    }
}

pub(crate) fn dispatch() -> Op {
    Op::Dispatch {
        repo: repo(),
        event_type: "nightly".into(),
        payload: Vec::new(),
        at: Revision::Ref(main()),
    }
}

pub(crate) fn repos() -> Query {
    Query::Repos { page: page() }
}

pub(crate) fn one_repo() -> Query {
    Query::Repo { repo: repo() }
}

pub(crate) fn refs() -> Query {
    Query::Refs {
        repo: repo(),
        page: page(),
    }
}

pub(crate) fn advertise() -> Query {
    Query::Advertise {
        repo: repo(),
        service: Service::UploadPack,
    }
}

pub(crate) fn upload() -> Query {
    Query::Upload {
        repo: repo(),
        request: Vec::new(),
    }
}

pub(crate) fn log() -> Query {
    Query::Log {
        repo: repo(),
        from: feature(),
        exclude: Some(Revision::Ref(main())),
        page: page(),
    }
}

pub(crate) fn tree() -> Query {
    Query::Tree {
        repo: repo(),
        at: oid(),
        path: b"src".to_vec(),
        page: page(),
    }
}

pub(crate) fn blob() -> Query {
    Query::Blob {
        repo: repo(),
        oid: oid(),
        range: Some(ByteRange {
            offset: 0,
            len: 65536,
        }),
    }
}

pub(crate) fn diff() -> Query {
    Query::Diff {
        repo: repo(),
        base: Some(oid()),
        head: oid(),
        path: Some(b"src/lib.rs".to_vec()),
        page: page(),
    }
}

pub(crate) fn compare() -> Query {
    Query::Compare {
        repo: repo(),
        from: feature(),
        into: Revision::Ref(main()),
    }
}

pub(crate) fn compares() -> Query {
    Query::Compares {
        repo: repo(),
        into: Revision::Ref(main()),
        from: vec![feature()],
    }
}

pub(crate) fn changes() -> Query {
    let filter = ChangeFilter {
        author: None,
        involves: None,
        state: None,
    };
    Query::Changes {
        repo: repo(),
        filter,
        page: page(),
    }
}

pub(crate) fn change() -> Query {
    Query::Change {
        repo: repo(),
        n: 1,
        page: page(),
    }
}

pub(crate) fn judgment() -> Query {
    Query::Judgment {
        principal: Principal::Account(21),
        page: page(),
    }
}

pub(crate) fn closure() -> Query {
    Query::Closure {
        repo: repo(),
        walk: Walk::default(),
    }
}

pub(crate) fn protected() -> Query {
    Query::Protected {
        repo: repo(),
        page: page(),
    }
}

pub(crate) fn ref_names() -> Query {
    Query::RefNames {
        repo: repo(),
        prefix: "refs/heads/".into(),
        page: page(),
    }
}

pub(crate) fn diff_stat() -> Query {
    Query::DiffStat {
        repo: repo(),
        base: Some(oid()),
        head: oid(),
        page: page(),
    }
}

pub(crate) fn preview_of() -> Query {
    Query::Preview { repo: repo(), n: 1 }
}

pub(crate) fn statuses() -> Query {
    Query::Statuses {
        repo: repo(),
        commit: oid(),
        page: page(),
    }
}

pub(crate) fn required() -> Query {
    Query::Required {
        repo: repo(),
        page: page(),
    }
}

pub(crate) fn queue() -> Query {
    Query::Queue {
        repo: repo(),
        branch: main(),
        page: page(),
    }
}

pub(crate) fn archived() -> Query {
    Query::Archived { repo: repo() }
}

pub(crate) fn archived_repos() -> Query {
    Query::ArchivedRepos { page: page() }
}

pub(crate) fn history() -> Query {
    Query::History {
        repo: repo(),
        walk: Walk::default(),
    }
}

pub(crate) fn shallow() -> Query {
    Query::Shallow {
        repo: repo(),
        walk: DeepWalk {
            walk: Walk {
                objects: vec![oid()],
                ..Walk::default()
            },
            deepen: Some(1),
            frontier: vec![(oid(), 1)],
            ..DeepWalk::default()
        },
    }
}
