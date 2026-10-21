//! The program on a `MockChain` beside `notes`, a program whose keys have owners and defaults it
//! declares: what the gate lets through, what a list allows, who sets one, and what a delete drops.

use ::program::{
    Error, ExecCtx, MockChain, MockHost, Need, Origin, Ownership, Party, Program, ProgramId,
    QueryCtx, Renamer, Reply as Wait, Resource, Writers, code,
};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{Access, Listed, Op, PROGRAM, PageRequest, Query, Reply};

fn renamed(chain: &mut MockChain, renames: &[(&str, &str)]) {
    chain.seat::<Renamer>("programs");
    chain.at(7, 0);
    let renames: Vec<(ProgramId, ProgramId)> = renames
        .iter()
        .map(|(from, to)| ((*from).to_owned(), (*to).to_owned()))
        .collect();
    chain.submit(signed(ADA), "programs", &renames).unwrap();
}

fn migrate(first: Option<(&str, &str)>, last: Option<(&str, &str)>) -> Op {
    migrate_at(7, first, last)
}

fn migrate_at(height: u64, first: Option<(&str, &str)>, last: Option<(&str, &str)>) -> Op {
    let bound = |bound: Option<(&str, &str)>| -> Option<(ProgramId, String)> {
        bound.map(|(program, key)| (program.to_owned(), key.to_owned()))
    };
    Op::Migrate {
        height,
        first: bound(first),
        last: bound(last),
    }
}

fn lists_of(chain: &MockChain, program: &str) -> Vec<(String, Ownership)> {
    let asked = Query::Lists {
        program: program.into(),
        page: PageRequest::first(10),
    };
    let Reply::Lists(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("lists");
    };
    page.items
        .into_iter()
        .map(|listed| (listed.key, listed.list))
        .collect()
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
enum Note {
    Own {
        key: String,
        default: Option<Ownership>,
    },
    Write {
        key: String,
    },
    Delete {
        key: String,
    },
    List {
        key: String,
        list: Option<Ownership>,
    },
}

struct Notes;

fn owner_key(key: &str) -> String {
    format!("owner/{key}")
}

fn default_key(key: &str) -> String {
    format!("default/{key}")
}

impl Program for Notes {
    const NAME: &'static str = "notes";
    type Op = Note;
    type Query = ();
    type Reply = ();

    fn init(_: &ExecCtx, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }

    fn execute(ctx: &ExecCtx, op: Note) -> Result<(), Error> {
        match op {
            Note::Own { key, default } => {
                let owner = Party::Account(ctx.sender_account()?);
                ctx.set(owner_key(&key), abi::encode(&owner));
                ctx.set(default_key(&key), abi::encode(&default));
            }
            Note::Write { key } => ctx.set(key, b"written".to_vec()),
            Note::Delete { key } => ctx.delete(key),
            Note::List { key, list } => {
                let set = Op::Set {
                    program: "notes".into(),
                    key,
                    list,
                };
                ctx.emit(PROGRAM, &set, Wait::Wanted);
            }
        }
        Ok(())
    }

    fn access(_: &QueryCtx, op: &Note) -> Result<Vec<Need>, Error> {
        Ok(match op {
            Note::Write { key } => vec![Need::write(key.clone())],
            Note::Delete { key } => vec![Need::delete(key.clone())],
            Note::Own { .. } | Note::List { .. } => Vec::new(),
        })
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<Resource, Error> {
        let read = |at: String| ctx.get(at.as_bytes());
        let owner = read(owner_key(key))
            .map(|bytes| abi::decode::<Party>(&bytes))
            .transpose()
            .map_err(::program::kernel::error_from)?;
        let default = read(default_key(key))
            .map(|bytes| abi::decode::<Option<Ownership>>(&bytes))
            .transpose()
            .map_err(::program::kernel::error_from)?
            .flatten();
        Ok(Resource { owner, default })
    }

    fn query(_: &QueryCtx, _: ()) -> Result<(), Error> {
        Ok(())
    }
}

const ADA: u64 = 1;
const BEA: u64 = 2;
const CY: u64 = 3;

fn signed(account: u64) -> Origin {
    Origin::Signed(vec![account as u8])
}

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Access>(PROGRAM);
    chain.seat::<Notes>("notes");
    chain.register(PROGRAM, 900);
    chain.register("notes", 901);
    for account in [ADA, BEA, CY] {
        chain.hold(vec![account as u8], account);
    }
    chain
}

fn note(chain: &MockChain, account: u64, op: Note) -> Result<Vec<u8>, Error> {
    chain.submit(signed(account), "notes", &op)
}

fn write(chain: &MockChain, account: u64, key: &str) -> Result<Vec<u8>, Error> {
    note(chain, account, Note::Write { key: key.into() })
}

fn set(
    chain: &MockChain,
    account: u64,
    key: &str,
    list: Option<Ownership>,
) -> Result<Vec<u8>, Error> {
    let op = Op::Set {
        program: "notes".into(),
        key: key.into(),
        list,
    };
    chain.submit(signed(account), PROGRAM, &op)
}

fn own(chain: &MockChain, account: u64, key: &str, default: Option<Ownership>) {
    let op = Note::Own {
        key: key.into(),
        default,
    };
    note(chain, account, op).unwrap();
}

fn refused<T: std::fmt::Debug>(result: Result<T, Error>) -> String {
    result.expect_err("refused").code
}

fn writers(chain: &MockChain, key: &str) -> Writers {
    let asked = Query::Writers {
        program: "notes".into(),
        key: key.into(),
    };
    match chain.query(PROGRAM, &asked).unwrap() {
        Reply::Writers(writers) => writers,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_key_no_list_covers_is_anyones_to_write() {
    let chain = chain();
    own(&chain, ADA, "docs", None);
    write(&chain, CY, "docs").unwrap();
    write(&chain, CY, "docs/a").unwrap();
    assert_eq!(writers(&chain, "docs/a"), Writers::Anyone);
}

#[test]
fn the_nearest_list_above_a_key_decides_and_its_owner_always_writes() {
    let chain = chain();
    own(&chain, ADA, "docs", None);
    set(
        &chain,
        ADA,
        "docs",
        Some(Ownership::Listed(vec![Party::Account(BEA)])),
    )
    .unwrap();
    write(&chain, ADA, "docs/a/b").unwrap();
    write(&chain, BEA, "docs/a/b").unwrap();
    assert_eq!(refused(write(&chain, CY, "docs/a/b")), code::UNAUTHORIZED);
    assert_eq!(
        writers(&chain, "docs/a/b"),
        Writers::Listed {
            at: "docs".into(),
            owner: Some(Party::Account(ADA)),
            list: Ownership::Listed(vec![Party::Account(BEA)]),
        }
    );
    own(&chain, CY, "docs/a", None);
    set(&chain, CY, "docs/a", Some(Ownership::Anyone)).unwrap();
    write(&chain, CY, "docs/a/b").unwrap();
}

#[test]
fn a_refused_op_writes_nothing() {
    let chain = chain();
    own(&chain, ADA, "docs", Some(Ownership::Owner));
    assert_eq!(refused(write(&chain, BEA, "docs")), code::UNAUTHORIZED);
    assert!(
        !chain
            .host("notes")
            .borrow()
            .state
            .contains_key(b"docs".as_slice())
    );
}

#[test]
fn a_default_the_owner_declares_holds_until_a_list_says_otherwise() {
    let chain = chain();
    own(&chain, ADA, "post", Some(Ownership::Owner));
    assert_eq!(refused(write(&chain, BEA, "post")), code::UNAUTHORIZED);
    write(&chain, ADA, "post").unwrap();
    set(&chain, ADA, "post", Some(Ownership::Anyone)).unwrap();
    write(&chain, BEA, "post").unwrap();
}

#[test]
fn only_the_owner_or_the_program_itself_sets_a_list() {
    let chain = chain();
    own(&chain, ADA, "docs", None);
    assert_eq!(
        refused(set(&chain, BEA, "docs", Some(Ownership::Owner))),
        code::UNAUTHORIZED
    );
    let by_program = Note::List {
        key: "docs".into(),
        list: Some(Ownership::Owner),
    };
    note(&chain, CY, by_program).unwrap();
    assert_eq!(refused(write(&chain, CY, "docs")), code::UNAUTHORIZED);
    set(&chain, ADA, "docs", None).unwrap();
    write(&chain, CY, "docs").unwrap();
}

#[test]
fn a_list_names_known_accounts_and_settles_without_its_owner() {
    let chain = chain();
    own(&chain, ADA, "docs", None);
    let unknown = Ownership::Listed(vec![Party::Account(77)]);
    assert_eq!(
        refused(set(&chain, ADA, "docs", Some(unknown))),
        code::NOT_FOUND
    );
    let own_name = Ownership::Listed(vec![Party::Account(ADA)]);
    set(&chain, ADA, "docs", Some(own_name)).unwrap();
    let asked = Query::List {
        program: "notes".into(),
        key: "docs".into(),
    };
    let listed: Reply = chain.query(PROGRAM, &asked).unwrap();
    assert_eq!(listed, Reply::List(Some(Ownership::Owner)));
}

#[test]
fn the_owner_above_a_key_deletes_it_and_a_delete_drops_every_list_below() {
    let chain = chain();
    own(&chain, ADA, "room", None);
    own(&chain, BEA, "room/1", Some(Ownership::Owner));
    own(&chain, BEA, "room/1/a", None);
    set(&chain, BEA, "room/1/a", Some(Ownership::Owner)).unwrap();
    set(&chain, BEA, "room/1", Some(Ownership::Owner)).unwrap();
    let delete = |account| {
        note(
            &chain,
            account,
            Note::Delete {
                key: "room/1".into(),
            },
        )
    };
    assert_eq!(refused(delete(CY)), code::UNAUTHORIZED);
    delete(ADA).unwrap();
    let asked = Query::Lists {
        program: "notes".into(),
        page: PageRequest::first(10),
    };
    let Reply::Lists(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("lists");
    };
    assert_eq!(page.items, Vec::<Listed>::new());
}

#[test]
fn only_the_kernel_runs_a_check() {
    let chain = chain();
    let check = Op::Check {
        program: "notes".into(),
        actor: vec![Party::Account(ADA)],
        needs: vec![Need::write("docs")],
    };
    assert_eq!(
        refused(chain.submit(signed(ADA), PROGRAM, &check)),
        code::UNAUTHORIZED
    );
}

#[test]
fn a_list_naming_a_program_lets_its_messages_through_whatever_account_they_act_as() {
    let mut chain = chain();
    chain.seat::<Notes>("relay");
    chain.register("relay", 902);
    own(&chain, ADA, "docs", None);
    set(
        &chain,
        ADA,
        "docs",
        Some(Ownership::Listed(vec![Party::Program("relay".into())])),
    )
    .unwrap();
    assert_eq!(refused(write(&chain, BEA, "docs")), code::UNAUTHORIZED);
    let relayed = Note::Write { key: "docs".into() };
    chain
        .submit(Origin::Program("relay".into()), "notes", &relayed)
        .unwrap();
}

#[test]
fn only_the_program_moves_the_lists_at_and_below_a_key() {
    let chain = chain();
    own(&chain, ADA, "a", None);
    own(&chain, ADA, "a/b", None);
    own(&chain, ADA, "ab", None);
    for key in ["a", "a/b", "ab"] {
        set(&chain, ADA, key, Some(Ownership::Owner)).unwrap();
    }
    let carried = Op::Move {
        program: "notes".into(),
        from: "a".into(),
        to: "z".into(),
    };
    assert_eq!(
        refused(chain.submit(signed(ADA), PROGRAM, &carried)),
        code::UNAUTHORIZED
    );
    chain
        .submit(Origin::Program("notes".into()), PROGRAM, &carried)
        .unwrap();
    let asked = Query::Lists {
        program: "notes".into(),
        page: PageRequest::first(10),
    };
    let Reply::Lists(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("lists");
    };
    let keys: Vec<_> = page
        .items
        .iter()
        .map(|listed| listed.key.as_str())
        .collect();
    assert_eq!(keys, ["ab", "z", "z/b"]);
}

#[test]
fn migrate_moves_a_renamed_programs_lists_and_renames_the_programs_lists_name() {
    let mut chain = chain();
    for (program, number) in [("relay", 902), ("keep", 903), ("o/notes", 904), ("p", 905)] {
        chain.seat::<Notes>(program);
        chain.register(program, number);
    }
    let relay = || Party::Program("relay".into());
    own(&chain, ADA, "docs", None);
    own(&chain, ADA, "post", None);
    set(&chain, ADA, "docs", Some(Ownership::Listed(vec![relay()]))).unwrap();
    set(&chain, ADA, "post", Some(Ownership::Owner)).unwrap();
    let kept_owner = Note::Own {
        key: "docs".into(),
        default: None,
    };
    chain.submit(signed(ADA), "keep", &kept_owner).unwrap();
    let kept = Op::Set {
        program: "keep".into(),
        key: "docs".into(),
        list: Some(Ownership::Listed(vec![
            relay(),
            Party::Program("p".into()),
            Party::Account(BEA),
        ])),
    };
    chain.submit(signed(ADA), PROGRAM, &kept).unwrap();
    let set_since = Op::Set {
        program: "o/notes".into(),
        key: "post".into(),
        list: Some(Ownership::Anyone),
    };
    chain
        .submit(Origin::Program("o/notes".into()), PROGRAM, &set_since)
        .unwrap();
    renamed(&mut chain, &[("notes", "o/notes"), ("relay", "o/relay")]);
    chain
        .submit(signed(CY), PROGRAM, &migrate(None, None))
        .unwrap();
    let o_relay = || Party::Program("o/relay".into());
    assert!(lists_of(&chain, "notes").is_empty());
    assert_eq!(
        lists_of(&chain, "o/notes"),
        [
            ("docs".to_owned(), Ownership::Listed(vec![o_relay()])),
            ("post".to_owned(), Ownership::Anyone),
        ]
    );
    assert_eq!(
        lists_of(&chain, "keep"),
        [(
            "docs".to_owned(),
            Ownership::Listed(vec![
                Party::Account(BEA),
                o_relay(),
                Party::Program("p".into()),
            ])
        )],
        "a list stays settled: sorted, each party once"
    );
    let access = chain.host(PROGRAM).clone();
    access.sibling::<Renamer>("programs", chain.host("programs"));
    let at_seven = ::program::Env {
        height: 7,
        ..MockHost::env(PROGRAM)
    };
    Access::execute(&access.exec(at_seven), migrate(None, None)).unwrap();
    assert!(
        access.borrow().touched.is_empty(),
        "a second run writes nothing"
    );
}

#[test]
fn migrate_rewrites_only_the_lists_inside_its_range() {
    let mut chain = chain();
    for key in ["a", "b", "c"] {
        own(&chain, ADA, key, None);
        set(&chain, ADA, key, Some(Ownership::Owner)).unwrap();
    }
    renamed(&mut chain, &[("notes", "o/notes")]);
    let only_b = migrate(Some(("notes", "b")), Some(("notes", "b")));
    chain.submit(signed(CY), PROGRAM, &only_b).unwrap();
    let keys = |program| {
        lists_of(&chain, program)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<Vec<_>>()
    };
    assert_eq!(keys("notes"), ["a", "c"]);
    assert_eq!(keys("o/notes"), ["b"]);
    chain
        .submit(signed(CY), PROGRAM, &migrate(None, None))
        .unwrap();
    assert!(keys("notes").is_empty());
    assert_eq!(keys("o/notes"), ["a", "b", "c"]);
}

#[test]
fn migrate_refuses_a_height_not_reached_and_leaves_a_rename_not_landed_alone() {
    let mut chain = chain();
    own(&chain, ADA, "docs", None);
    set(&chain, ADA, "docs", Some(Ownership::Owner)).unwrap();
    renamed(&mut chain, &[("notes", "o/notes")]);
    let ahead = migrate_at(8, None, None);
    assert_eq!(
        refused(chain.submit(signed(CY), PROGRAM, &ahead)),
        code::INVALID_INPUT
    );
    assert_eq!(lists_of(&chain, "notes").len(), 1);
    assert!(lists_of(&chain, "o/notes").is_empty());
}
