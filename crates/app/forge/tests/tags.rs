mod common;
use borsh::BorshSerialize;
use borsh::schema::Definition;
use common::*;

const REPO: &str = "project";

const LIVE_OPS: [(&str, i64); 20] = [
    ("Create", 0),
    ("Configure", 1),
    ("Push", 4),
    ("Merge", 5),
    ("ChangeOpen", 6),
    ("ChangeEdit", 7),
    ("ChangeClose", 8),
    ("ReviewSubmit", 9),
    ("Stage", 10),
    ("SquashMerge", 11),
    ("Rebase", 12),
    ("ChangeRepoint", 13),
    ("DeleteBranch", 14),
    ("Protect", 15),
    ("Watch", 16),
    ("Status", 19),
    ("AdoptOwners", 23),
    ("Archive", 24),
    ("Dispatch", 25),
    ("Migrate", 27),
];

const GRANT_TAG: u8 = 2;
const REVOKE_TAG: u8 = 3;
const ARCHIVE_ENDED_TAG: u8 = 17;
const ACTIVITY_TAG: u8 = 10;
const JUDGMENT_TAG: u8 = 13;
const LIST_INVOLVED_TAG: u8 = 26;

const LIVE_QUERIES: [(&str, i64); 4] = [
    ("Changes", 11),
    ("Change", 12),
    ("Closure", 14),
    ("ReviewRequests", 27),
];
const LIVE_REPLIES: [(&str, i64); 4] = [
    ("Changes", 9),
    ("Change", 10),
    ("Closure", 12),
    ("ReviewRequests", 24),
];

fn encoded<T: BorshSerialize>(value: &T) -> Vec<u8> {
    borsh::to_vec(value).unwrap()
}

fn op_bytes(tag: u8, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![tag];
    for field in fields {
        bytes.extend_from_slice(field);
    }
    bytes
}

fn sent_as_bytes(
    sandbox: &MemorySandbox,
    key: &[u8],
    bytes: &[u8],
) -> Result<Vec<u8>, program::Error> {
    sandbox.chain.at(1, TIME);
    sandbox
        .chain
        .submit_raw(Origin::Signed(key.to_vec()), "forge", bytes)
}

fn repo_read(sandbox: &MemorySandbox) -> Vec<u8> {
    ask(sandbox, &Query::Repo { repo: REPO.into() }).unwrap()
}

fn is_archived(sandbox: &MemorySandbox) -> bool {
    let answer = ask(sandbox, &Query::Archived { repo: REPO.into() }).unwrap();
    match abi::decode::<Reply>(&answer).unwrap() {
        Reply::Archived { archived, .. } => archived,
        other => panic!("{other:?}"),
    }
}

fn head_of(sandbox: &MemorySandbox) -> Vec<u8> {
    match abi::decode::<Reply>(&repo_read(sandbox)).unwrap() {
        Reply::Repo { repo, .. } => repo.repo.settings.head,
        other => panic!("{other:?}"),
    }
}

#[test]
fn each_live_op_keeps_its_tag_in_the_schema() {
    let schema = program::schema::interface::<Forge>();
    let op = program::schema::roots(&schema).unwrap().op;
    let Some(Definition::Enum { variants, .. }) = schema.get_definition(op) else {
        panic!("forge's op is an enum");
    };
    let tags: Vec<(&str, i64)> = variants
        .iter()
        .map(|(tag, name, _)| (name.as_str(), *tag))
        .collect();
    for live in LIVE_OPS {
        assert!(tags.contains(&live), "{live:?} in {tags:?}");
    }
}

#[test]
fn an_op_written_as_bytes_on_either_side_of_the_retired_tags_lands() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let settings = Settings {
        head: b"refs/heads/dev".to_vec(),
        ..Settings::default()
    };
    let configure = op_bytes(1, &[encoded(&REPO.to_owned()), encoded(&settings)]);
    sent_as_bytes(&sandbox, OWNER, &configure).unwrap();
    assert_eq!(head_of(&sandbox), b"refs/heads/dev");
    let archive = op_bytes(24, &[encoded(&REPO.to_owned()), encoded(&true)]);
    sent_as_bytes(&sandbox, OWNER, &archive).unwrap();
    assert!(is_archived(&sandbox));
    let restore = op_bytes(24, &[encoded(&REPO.to_owned()), encoded(&false)]);
    sent_as_bytes(&sandbox, OWNER, &restore).unwrap();
    assert!(!is_archived(&sandbox));
}

#[test]
fn a_grant_or_revoke_tag_is_refused_and_writes_nothing() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let before = repo_read(&sandbox);
    for tag in [GRANT_TAG, REVOKE_TAG] {
        let bytes = op_bytes(tag, &[encoded(&REPO.to_owned()), encoded(&person(WRITER))]);
        let refused = sandbox
            .forge
            .refused(|| sent_as_bytes(&sandbox, OWNER, &bytes));
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
        let undecoded = refused.message.starts_with("forge: Op did not decode: ");
        assert!(undecoded, "{refused}");
    }
    assert_eq!(repo_read(&sandbox), before);
}

#[test]
fn an_archive_ended_tag_is_refused_and_writes_nothing() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let before = repo_read(&sandbox);
    let no_after: Option<u64> = None;
    let fields = [
        encoded(&REPO.to_owned()),
        encoded(&no_after),
        encoded(&8u64),
    ];
    let bytes = op_bytes(ARCHIVE_ENDED_TAG, &fields);
    let refused = sandbox
        .forge
        .refused(|| sent_as_bytes(&sandbox, STRANGER, &bytes));
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    let undecoded = refused.message.starts_with("forge: Op did not decode: ");
    assert!(undecoded, "{refused}");
    assert_eq!(repo_read(&sandbox), before);
}

#[test]
fn a_repos_last_activity_reads_off_repo_at_the_height_of_its_last_op() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let settings = Settings {
        head: b"refs/heads/dev".to_vec(),
        ..Settings::default()
    };
    let configure = Op::Configure {
        repo: REPO.into(),
        settings,
    };
    sandbox.chain.at(7, TIME);
    let origin = Origin::Signed(OWNER.to_vec());
    let encoded = abi::encode(&configure);
    sandbox.chain.submit_raw(origin, "forge", encoded).unwrap();
    let Reply::Repo { repo, .. } = abi::decode::<Reply>(&repo_read(&sandbox)).unwrap() else {
        panic!("Repo answers the repository");
    };
    assert_eq!(repo.repo.last_activity, 7);
}

#[test]
fn an_activity_tag_is_no_query() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let bytes = op_bytes(ACTIVITY_TAG, &[encoded(&REPO.to_owned())]);
    let refused = program::query::<Forge>(&sandbox.reads(1), &bytes).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    let undecoded = refused.message.starts_with("forge: Query did not decode: ");
    assert!(undecoded, "{refused}");
}

#[test]
fn a_judgment_tag_is_no_query() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let fields = [
        encoded(&Principal::Account(2)),
        encoded(&PageRequest::first(2)),
    ];
    let bytes = op_bytes(JUDGMENT_TAG, &fields);
    let refused = program::query::<Forge>(&sandbox.reads(1), &bytes).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    let undecoded = refused.message.starts_with("forge: Query did not decode: ");
    assert!(undecoded, "{refused}");
}

#[test]
fn the_queries_and_replies_beside_the_retired_judgment_keep_their_tags() {
    let schema = program::schema::interface::<Forge>();
    let roots = program::schema::roots(&schema).unwrap();
    for (root, live) in [(roots.query, LIVE_QUERIES), (roots.reply, LIVE_REPLIES)] {
        let Some(Definition::Enum { variants, .. }) = schema.get_definition(root) else {
            panic!("{root} is an enum");
        };
        let tags: Vec<(&str, i64)> = variants
            .iter()
            .map(|(tag, name, _)| (name.as_str(), *tag))
            .collect();
        for live in live {
            assert!(tags.contains(&live), "{live:?} in {tags:?}");
        }
        let judged = tags.iter().any(|(name, _)| *name == "Judgment");
        assert!(!judged, "{root}: {tags:?}");
    }
}

#[test]
fn a_list_involved_tag_is_refused_and_writes_nothing() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let before = repo_read(&sandbox);
    let no_after: Option<Vec<u8>> = None;
    let bytes = op_bytes(LIST_INVOLVED_TAG, &[encoded(&no_after)]);
    let refused = sandbox
        .forge
        .refused(|| sent_as_bytes(&sandbox, STRANGER, &bytes));
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    let undecoded = refused.message.starts_with("forge: Op did not decode: ");
    assert!(undecoded, "{refused}");
    assert_eq!(repo_read(&sandbox), before);
}

#[test]
fn merge_scheduling_tags_are_refused_without_writes() {
    let sandbox = founded();
    create(&sandbox, REPO, HashKind::Sha1);
    let before = repo_read(&sandbox);
    let repo = encoded(&REPO.to_owned());
    let branch = encoded(&b"refs/heads/main".to_vec());
    let number = encoded(&1u64);
    let bytes = [
        op_bytes(18, &[repo.clone(), number.clone()]),
        op_bytes(20, &[repo.clone(), branch.clone(), encoded(&None::<()>)]),
        op_bytes(
            21,
            &[repo.clone(), number.clone(), encoded(&"a".repeat(40))],
        ),
        op_bytes(22, &[repo.clone(), number]),
        op_bytes(28, &[repo, branch, encoded(&None::<forge::Train>)]),
    ];
    for bytes in bytes {
        let refused = sandbox
            .forge
            .refused(|| sent_as_bytes(&sandbox, OWNER, &bytes));
        assert!(
            [code::WRONG_STATE, code::INVALID_INPUT].contains(&refused.code.as_str()),
            "{refused}"
        );
        assert_eq!(repo_read(&sandbox), before);
    }
}

#[test]
fn merge_notice_tags_cannot_be_reused() {
    let retired = [
        op_bytes(
            2,
            &[
                encoded(&REPO),
                encoded(&1_u64),
                encoded(&"a".repeat(40)),
                encoded(&person(OWNER)),
            ],
        ),
        op_bytes(
            6,
            &[
                encoded(&REPO),
                encoded(&1_u64),
                encoded(&b"refs/heads/main".to_vec()),
                encoded(&"retired"),
                encoded(&person(OWNER)),
            ],
        ),
    ];
    for bytes in retired {
        assert!(borsh::from_slice::<forge::Watched>(&bytes).is_err());
    }
    let live = [
        forge::Watched::Closed {
            repo: REPO.into(),
            n: 1,
            by: person(OWNER),
        },
        forge::Watched::Dispatched {
            repo: REPO.into(),
            event_type: "manual".into(),
            payload: Vec::new(),
            commit: "a".repeat(40),
            by: person(OWNER),
        },
    ];
    for (event, tag) in live.into_iter().zip([3, 7]) {
        let bytes = encoded(&event);
        assert_eq!(bytes[0], tag);
        assert_eq!(borsh::from_slice::<forge::Watched>(&bytes).unwrap(), event);
    }
}
