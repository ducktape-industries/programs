mod common;
use borsh::BorshSerialize;
use borsh::schema::Definition;
use common::*;

const REPO: &str = "project";

const LIVE_OPS: [(&str, i64); 22] = [
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
    ("Preview", 18),
    ("Status", 19),
    ("RequireChecks", 20),
    ("Enqueue", 21),
    ("Dequeue", 22),
    ("AdoptOwners", 23),
    ("Archive", 24),
];

const GRANT_TAG: u8 = 2;
const REVOKE_TAG: u8 = 3;
const ARCHIVE_ENDED_TAG: u8 = 17;

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
