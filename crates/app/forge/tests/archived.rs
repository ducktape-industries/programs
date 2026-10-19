//! `Archive`: an archived repository leaves `Repos`, refuses every op but `Archive` and still
//! answers every read and clone; un-archived, it is listed and takes writes again.
mod common;
use common::*;
use forge::Revision;

const REPO: &str = "project";

fn archive(archived: bool) -> Op {
    Op::Archive {
        repo: REPO.into(),
        archived,
    }
}

fn reply(sandbox: &MemorySandbox, query: Query) -> Reply {
    abi::decode(&ask(sandbox, &query).unwrap()).unwrap()
}

fn listed(sandbox: &MemorySandbox) -> Vec<String> {
    let page = PageRequest::first(16);
    let Reply::Repos { page, .. } = reply(sandbox, Query::Repos { page }) else {
        panic!("Repos answers Repos");
    };
    page.items.into_iter().map(|info| info.name).collect()
}

fn archived_ones(sandbox: &MemorySandbox) -> Vec<String> {
    let page = PageRequest::first(16);
    let Reply::ArchivedRepos { page, .. } = reply(sandbox, Query::ArchivedRepos { page }) else {
        panic!("ArchivedRepos answers ArchivedRepos");
    };
    page.items.into_iter().map(|info| info.name).collect()
}

fn is_archived(sandbox: &MemorySandbox) -> bool {
    let query = Query::Archived { repo: REPO.into() };
    let Reply::Archived { archived, .. } = reply(sandbox, query) else {
        panic!("Archived answers Archived");
    };
    archived
}

// `project` with `main` and `feature` (one commit past main) pushed, and `other` beside it;
// the source's objects and the two tips.
fn pushed() -> (MemorySandbox, MemoryObjects, [String; 2]) {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    create(&sandbox, "other", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let main = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let feature = file_commit(&mut source, &[main], 2, &[("a", b"2")]);
    let commands = [
        (Hash::Sha1.zero(), main, "refs/heads/main"),
        (Hash::Sha1.zero(), feature, "refs/heads/feature"),
    ];
    let pack = pack_of(&source, &all_ids(&source));
    push(&sandbox, OWNER, REPO, &commands, &pack).unwrap();
    (sandbox, source, [main.to_hex(), feature.to_hex()])
}

#[test]
fn an_archived_repository_leaves_the_list_and_comes_back() {
    let (sandbox, _, _) = pushed();
    let mut both = listed(&sandbox);
    both.sort();
    assert_eq!(both, ["other", REPO]);
    assert!(!is_archived(&sandbox));
    act(&sandbox, OWNER, &archive(true)).unwrap();
    assert_eq!(listed(&sandbox), ["other"]);
    assert_eq!(archived_ones(&sandbox), [REPO]);
    assert!(is_archived(&sandbox));
    act(&sandbox, OWNER, &archive(false)).unwrap();
    assert_eq!(listed(&sandbox).len(), 2, "listed again");
    assert!(archived_ones(&sandbox).is_empty());
    assert!(!is_archived(&sandbox));
}

#[test]
fn an_archived_repository_refuses_every_write() {
    let (sandbox, _, [main, feature]) = pushed();
    act(&sandbox, OWNER, &archive(true)).unwrap();
    let branch = |name: &str| format!("refs/heads/{name}").into_bytes();
    let tip = gitcore::Oid::from_hex(Hash::Sha1, &main).unwrap();
    let writes = [
        Op::Push {
            repo: REPO.into(),
            request: push_request(&[(Hash::Sha1.zero(), tip, "refs/heads/copy")]),
            pack: None,
        },
        Op::ChangeOpen {
            repo: REPO.into(),
            from: Revision::Ref(branch("feature")),
            into: branch("main"),
            title: "Land the feature".into(),
            body: "Why it lands".into(),
            reviewers: Vec::new(),
        },
        Op::Configure {
            repo: REPO.into(),
            settings: Settings::default(),
        },
        Op::Protect {
            repo: REPO.into(),
            branch: branch("feature"),
            protected: true,
        },
        Op::DeleteBranch {
            repo: REPO.into(),
            branch: branch("feature"),
            expected: feature.clone(),
        },
        Op::Status {
            repo: REPO.into(),
            commit: feature,
            context: "gates".into(),
            state: forge::CheckState::Passed,
            url: None,
        },
    ];
    for op in &writes {
        let refusal = refused(&sandbox, OWNER, op);
        assert_eq!(refusal.code, code::WRONG_STATE, "{op:?}");
        assert!(refusal.message.contains("is archived"), "{refusal:?}");
    }
    let again = refused(&sandbox, OWNER, &archive(true));
    assert_eq!(again.code, code::WRONG_STATE);
    let create = Op::Create {
        repo: REPO.into(),
        hash: HashKind::Sha1,
    };
    let taken = refused(&sandbox, OWNER, &create);
    assert_eq!(taken.code, code::ALREADY_EXISTS, "the name stays taken");
    act(&sandbox, OWNER, &archive(false)).unwrap();
    for op in &writes[..2] {
        act(&sandbox, OWNER, op).unwrap();
    }
    assert!(refs_of(&sandbox, REPO).contains_key("refs/heads/copy"));
    let restored = refused(&sandbox, OWNER, &archive(false));
    assert_eq!(restored.code, code::WRONG_STATE);
}

#[test]
fn an_archived_repository_still_serves_reads_and_clones() {
    let (sandbox, source, [main, feature]) = pushed();
    act(&sandbox, OWNER, &archive(true)).unwrap();
    let Reply::Repo { repo, .. } = reply(&sandbox, Query::Repo { repo: REPO.into() }) else {
        panic!("Repo answers Repo");
    };
    assert_eq!(repo.name, REPO);
    let refs = refs_of(&sandbox, REPO);
    assert_eq!(refs.get("refs/heads/main"), Some(&main));
    let log = Query::Log {
        repo: REPO.into(),
        from: Revision::Ref(b"refs/heads/feature".to_vec()),
        exclude: None,
        page: PageRequest::first(16),
    };
    let Reply::Log { page, .. } = reply(&sandbox, log) else {
        panic!("Log answers Log");
    };
    assert_eq!(page.items.len(), 2);
    let upload = Query::Advertise {
        repo: REPO.into(),
        service: Service::UploadPack,
    };
    assert!(!ask(&sandbox, &upload).unwrap().is_empty());
    let ls_refs = Query::Upload {
        repo: REPO.into(),
        request: v2_request("ls-refs", Hash::Sha1, &["symrefs".into()]),
    };
    let lines = pkt_lines(&ask(&sandbox, &ls_refs).unwrap());
    assert!(
        lines.iter().any(|line| line.starts_with(&feature)),
        "{lines:?}"
    );
    let tip = gitcore::Oid::from_hex(Hash::Sha1, &feature).unwrap();
    let cloned = closure_of(&sandbox, REPO, HashKind::Sha1, &[tip], &[]);
    let mut ids: Vec<_> = cloned.iter().map(|item| item.id).collect();
    let mut held = all_ids(&source);
    ids.sort();
    held.sort();
    assert_eq!(ids, held, "a clone gets every object");
}

#[test]
fn the_repositorys_settings_list_decides_who_archives() {
    let (sandbox, _, _) = pushed();
    let settings = forge::settings_key(REPO);
    sandbox.list_on(OWNER, &settings, &[]).unwrap();
    let refusal = refused(&sandbox, STRANGER, &archive(true));
    assert_eq!(refusal.code, code::UNAUTHORIZED);
    assert!(!is_archived(&sandbox));
    sandbox
        .list_on(OWNER, &settings, &[person(WRITER)])
        .unwrap();
    act(&sandbox, WRITER, &archive(true)).unwrap();
    assert!(is_archived(&sandbox));
}

#[test]
fn archiving_is_described_in_words() {
    let title = |archived| forge::describe(&archive(archived)).title;
    assert_eq!(title(true), "Archive repository · project");
    assert_eq!(title(false), "Un-archive repository · project");
}
