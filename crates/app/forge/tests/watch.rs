//! A program that watches forge hears each ref a push or a merge moves and each change opened,
//! reviewed, closed, merged or taken out of a merge queue, and a watcher that refuses what it
//! hears never refuses forge's write.
mod common;

use borsh::{BorshDeserialize, BorshSerialize};
use common::*;
use forge::{Revision, Watched};
use program::{Error, ExecCtx, QueryCtx, Reply as Wanted};
use store::Item;

const WATCHER: &str = "watcher";
const HEARS: u8 = 1;
const HEARD: Item<Vec<Watched>> = Item::new("heard");
const DEAF: Item<bool> = Item::new("deaf");

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Ask {
    Watch(Vec<u8>),
    Heard(Watched),
    Refuse,
}

struct Watcher;

impl Program for Watcher {
    const NAME: &'static str = WATCHER;
    type Op = Ask;
    type Query = ();
    type Reply = Vec<Watched>;

    fn execute(ctx: &ExecCtx, ask: Ask) -> Result<(), Error> {
        match ask {
            Ask::Watch(deliver) => {
                ctx.emit(forge::PROGRAM, &Op::Watch { deliver }, Wanted::None);
            }
            Ask::Heard(event) => {
                ctx.env().sent_by(forge::PROGRAM)?;
                if DEAF.get(ctx)?.unwrap_or(false) {
                    return Err(program::invalid("the watcher refuses what it hears"));
                }
                let mut heard = HEARD.get(ctx)?.unwrap_or_default();
                heard.push(event);
                HEARD.put(ctx, &heard);
            }
            Ask::Refuse => DEAF.put(ctx, &true),
        }
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<Vec<Watched>, Error> {
        Ok(HEARD.get(ctx)?.unwrap_or_default())
    }
}

fn watched() -> MemorySandbox {
    let mut sandbox = founded();
    sandbox.chain.seat::<Watcher>(WATCHER);
    sandbox.chain.register(WATCHER, 903);
    ask_watcher(&sandbox, Ask::Watch(vec![HEARS])).unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    sandbox
}

fn ask_watcher(sandbox: &MemorySandbox, ask: Ask) -> Result<Vec<u8>, Error> {
    let anyone = Origin::Signed(STRANGER.to_vec());
    sandbox.chain.submit(anyone, WATCHER, &ask)
}

fn heard(sandbox: &MemorySandbox) -> Vec<Watched> {
    sandbox.chain.query(WATCHER, &()).unwrap()
}

fn moved(reference: &str, old: Option<Oid>, new: Option<Oid>) -> Watched {
    Watched::Moved {
        repo: "project".into(),
        reference: reference.as_bytes().to_vec(),
        old: old.map(|oid| oid.to_hex()),
        new: new.map(|oid| oid.to_hex()),
        by: Some(person(OWNER)),
    }
}

fn branch(name: &str) -> Revision {
    Revision::Ref(format!("refs/heads/{name}").into_bytes())
}

#[test]
fn a_watcher_hears_each_ref_moved_and_each_change_opened() {
    let sandbox = watched();
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut source, &[root], 2, &[("a", b"2\n")]);
    let zero = Hash::Sha1.zero();
    let pack = pack_of(&source, &all_ids(&source));
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &pack,
    )
    .unwrap();
    let to_feature = [(zero, feature, "refs/heads/feature")];
    push(&sandbox, OWNER, "project", &to_feature, b"").unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: branch("feature"),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    act(&sandbox, OWNER, &open).unwrap();
    let merge = Op::Merge {
        repo: "project".into(),
        into: b"refs/heads/main".to_vec(),
        from: branch("feature"),
        expected_into: root.to_hex(),
        expected_from: feature.to_hex(),
        result: feature.to_hex(),
        change: Some(1),
    };
    act(&sandbox, OWNER, &merge).unwrap();
    let opened = Watched::Opened {
        repo: "project".into(),
        n: 1,
        from: branch("feature"),
        into: b"refs/heads/main".to_vec(),
        head: feature.to_hex(),
        by: person(OWNER),
    };
    let expected = [
        moved("refs/heads/main", None, Some(root)),
        moved("refs/heads/feature", None, Some(feature)),
        opened,
        Watched::Merged {
            repo: "project".into(),
            n: 1,
            oid: feature.to_hex(),
            by: person(OWNER),
        },
        moved("refs/heads/main", Some(root), Some(feature)),
    ];
    assert_eq!(heard(&sandbox), expected);
}

#[test]
fn a_watcher_hears_a_change_closed() {
    let sandbox = watched();
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut source, &[root], 2, &[("a", b"2\n")]);
    let zero = Hash::Sha1.zero();
    let pack = pack_of(&source, &all_ids(&source));
    let made = [
        (zero, root, "refs/heads/main"),
        (zero, feature, "refs/heads/feature"),
    ];
    push(&sandbox, OWNER, "project", &made, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: branch("feature"),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    act(&sandbox, OWNER, &open).unwrap();
    let close = Op::ChangeClose {
        repo: "project".into(),
        n: 1,
    };
    act(&sandbox, OWNER, &close).unwrap();
    let closed = Watched::Closed {
        repo: "project".into(),
        n: 1,
        by: person(OWNER),
    };
    assert_eq!(heard(&sandbox).last(), Some(&closed));
}

#[test]
fn a_watcher_that_refuses_what_it_hears_never_refuses_the_push() {
    let sandbox = watched();
    ask_watcher(&sandbox, Ask::Refuse).unwrap();
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1\n")]);
    let zero = Hash::Sha1.zero();
    let pack = pack_of(&source, &all_ids(&source));
    let report = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &pack,
    );
    assert_eq!(report.unwrap(), ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        root.to_hex()
    );
    assert!(heard(&sandbox).is_empty());
}

#[test]
fn a_program_watches_and_an_empty_prefix_stops_it() {
    let sandbox = watched();
    let by_account = refused(
        &sandbox,
        OWNER,
        &Op::Watch {
            deliver: vec![HEARS],
        },
    );
    assert_eq!(by_account.code, code::UNAUTHORIZED);
    ask_watcher(&sandbox, Ask::Watch(Vec::new())).unwrap();
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1\n")]);
    let zero = Hash::Sha1.zero();
    let pack = pack_of(&source, &all_ids(&source));
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &pack,
    )
    .unwrap();
    assert!(heard(&sandbox).is_empty());
}

fn open_feature(sandbox: &MemorySandbox) -> Oid {
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1\n")]);
    let feature = file_commit(&mut source, &[root], 2, &[("a", b"2\n")]);
    let zero = Hash::Sha1.zero();
    let pack = pack_of(&source, &all_ids(&source));
    let made = [
        (zero, root, "refs/heads/main"),
        (zero, feature, "refs/heads/feature"),
    ];
    push(sandbox, OWNER, "project", &made, &pack).unwrap();
    let open = Op::ChangeOpen {
        repo: "project".into(),
        from: branch("feature"),
        into: b"refs/heads/main".to_vec(),
        title: "Feature".into(),
        body: String::new(),
        reviewers: Vec::new(),
    };
    act(sandbox, OWNER, &open).unwrap();
    feature
}

#[test]
fn a_watcher_hears_a_review_submitted() {
    let sandbox = watched();
    let feature = open_feature(&sandbox);
    let review = Op::ReviewSubmit {
        repo: "project".into(),
        n: 1,
        review: forge::ReviewDraft {
            commit_oid: feature.to_hex(),
            base_oid: None,
            verdict: forge::Verdict::RequestChanges,
            body: "not yet".into(),
            comments: Vec::new(),
        },
    };
    act(&sandbox, OWNER, &review).unwrap();
    let reviewed = Watched::Reviewed {
        repo: "project".into(),
        n: 1,
        id: 1,
        verdict: forge::Verdict::RequestChanges,
        by: person(OWNER),
    };
    assert_eq!(heard(&sandbox).last(), Some(&reviewed));
}

#[test]
fn a_watcher_hears_a_change_forge_takes_out_of_a_merge_queue() {
    let sandbox = watched();
    let feature = open_feature(&sandbox);
    let require = Op::RequireChecks {
        repo: "project".into(),
        branch: b"refs/heads/main".to_vec(),
        required: Some(forge::Required {
            reporter: person(STRANGER),
            contexts: vec!["gates".into()],
        }),
    };
    act(&sandbox, OWNER, &require).unwrap();
    let enqueue = Op::Enqueue {
        repo: "project".into(),
        n: 1,
        expected_head: feature.to_hex(),
    };
    act(&sandbox, OWNER, &enqueue).unwrap();
    let merge = heard(&sandbox)
        .into_iter()
        .find_map(|event| match event {
            Watched::Previewed { merge, .. } => Some(merge),
            _ => None,
        })
        .expect("the change is previewed as it joins the queue");
    let failed = Op::Status {
        repo: "project".into(),
        commit: merge,
        context: "gates".into(),
        state: forge::CheckState::Failed,
        url: None,
    };
    act(&sandbox, STRANGER, &failed).unwrap();
    let Some(Watched::LeftQueue {
        repo,
        n,
        into,
        why,
        queued_by,
    }) = heard(&sandbox).pop()
    else {
        panic!("{:#?}", heard(&sandbox));
    };
    assert_eq!(
        (repo.as_str(), n, into.as_slice(), queued_by),
        ("project", 1, b"refs/heads/main".as_slice(), person(OWNER))
    );
    assert!(why.contains("gates failed"), "{why}");
}
