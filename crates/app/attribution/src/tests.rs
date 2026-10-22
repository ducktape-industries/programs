//! attribution on a `MockChain`: a program reporting its own objects, and each change handed
//! only to the registered program its recipient's script calls.
use ::program::{Error, ExecCtx, MockChain, Origin, Program, QueryCtx, Reply as Wanted, code};
use abi::role::identity::{Category, Kind as Account, Profile, Script, Standing};
use borsh::{BorshDeserialize, BorshSerialize};
use store::Item;

use crate::*;

const ADA: &[u8] = b"ada-key";
const REPORTER: &str = "reporter";
const LISTENER: &str = "listener";
const OTHER: &str = "other";

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Report {
    Send(Op),
}

struct Reporter;

impl Program for Reporter {
    const NAME: &'static str = REPORTER;
    type Op = Report;
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, op: Report) -> Result<(), Error> {
        let Report::Send(op) = op;
        ctx.emit(PROGRAM, &op, Wanted::None);
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Listen {
    Register,
    Told(Box<Change>),
    Refuse,
}

const TOLD: Item<Vec<Change>> = Item::new("told");
const REFUSING: Item<bool> = Item::new("refusing");

struct Listener;

impl Program for Listener {
    const NAME: &'static str = LISTENER;
    type Op = Listen;
    type Query = ();
    type Reply = Vec<Change>;

    fn execute(ctx: &ExecCtx, op: Listen) -> Result<(), Error> {
        match op {
            Listen::Register => {
                let deliver = vec![1];
                ctx.emit(PROGRAM, &Op::Register { deliver }, Wanted::None);
                Ok(())
            }
            Listen::Told(change) => {
                ctx.env().sent_by(PROGRAM)?;
                if REFUSING.get(ctx)?.unwrap_or(false) {
                    return Err(::program::invalid("not now"));
                }
                let mut told = TOLD.get(ctx)?.unwrap_or_default();
                told.push(*change);
                TOLD.put(ctx, &told);
                Ok(())
            }
            Listen::Refuse => {
                REFUSING.put(ctx, &true);
                Ok(())
            }
        }
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<Vec<Change>, Error> {
        Ok(TOLD.get(ctx)?.unwrap_or_default())
    }
}

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Attribution>(PROGRAM);
    chain.seat::<Reporter>(REPORTER);
    chain.seat::<Listener>(LISTENER);
    chain.seat::<Listener>(OTHER);
    for (program, number) in [
        (PROGRAM, 900),
        (REPORTER, 902),
        (LISTENER, 903),
        (OTHER, 904),
    ] {
        chain.register(program, number);
    }
    chain.hold(ADA.to_vec(), 1);
    chain
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn source(id: &str) -> Source {
    Source {
        program: REPORTER.into(),
        object: Object {
            kind: "task".into(),
            id: id.into(),
        },
    }
}

fn relations(chain: &MockChain, source: Source) -> Option<Relations> {
    let Reply::Relations(relations) = chain.query(PROGRAM, &Query::Relations { source }).unwrap()
    else {
        panic!()
    };
    relations
}

fn changes_to(chain: &MockChain, recipient: AccountNumber) -> Vec<(String, Reason, Kind)> {
    let asked = Query::ChangesTo {
        recipient,
        page: PageRequest::default(),
    };
    let Reply::Changes(page) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
        .into_iter()
        .map(|change| (change.source.object.id, change.reason, change.kind))
        .collect()
}

fn change_at(chain: &MockChain, seq: u64) -> Option<Change> {
    let Reply::Change(change) = chain.query(PROGRAM, &Query::Change { seq }).unwrap() else {
        panic!()
    };
    change
}

fn update(id: &str, revision: u64, relations: Vec<Relation>, transfers: Vec<Transfer>) -> Op {
    Op::Attribute(Update {
        object: Object {
            kind: "task".into(),
            id: id.into(),
        },
        revision,
        actor: Actor::System,
        relations,
        transfers,
    })
}

fn owner(recipient: AccountNumber) -> Relation {
    Relation {
        recipient,
        reason: Reason::Ownership,
        detail: b"since today".to_vec(),
    }
}

fn report(chain: &MockChain, op: Op) -> Result<Vec<u8>, Error> {
    chain.submit(signed(ADA), REPORTER, &Report::Send(op))
}

#[test]
fn a_program_reports_only_its_own_objects_and_only_forward() {
    let chain = chain();
    let signed_by_hand = chain.submit(
        signed(ADA),
        PROGRAM,
        &update("t", 1, Vec::new(), Vec::new()),
    );
    assert_eq!(signed_by_hand.unwrap_err().code, code::UNAUTHORIZED);

    report(&chain, update("t", 5, vec![owner(1)], Vec::new())).unwrap();
    assert_eq!(
        relations(&chain, source("t")).unwrap().relations,
        [owner(1)]
    );
    let again = report(&chain, update("t", 5, Vec::new(), Vec::new()));
    assert_eq!(again.unwrap_err().code, code::STALE);

    let unmatched = Transfer {
        reason: Reason::Ownership,
        from: 1,
        to: 3,
    };
    let refused = report(&chain, update("t", 6, vec![owner(2)], vec![unmatched]));
    assert_eq!(refused.unwrap_err().code, code::INVALID_INPUT);
    let handed = Transfer {
        reason: Reason::Ownership,
        from: 1,
        to: 2,
    };
    report(&chain, update("t", 6, vec![owner(2)], vec![handed])).unwrap();
    assert_eq!(
        changes_to(&chain, 1),
        [
            ("t".into(), Reason::Ownership, Kind::Added),
            (
                "t".into(),
                Reason::Ownership,
                Kind::TransferredOut { to: 2 }
            )
        ]
    );
    assert_eq!(
        changes_to(&chain, 2),
        [(
            "t".into(),
            Reason::Ownership,
            Kind::TransferredIn { from: 1 }
        )]
    );

    for bad in [
        update("a/b", 1, Vec::new(), Vec::new()),
        update("u", 1, vec![owner(0)], Vec::new()),
        update("u", 1, vec![owner(1), owner(1)], Vec::new()),
        update(
            "u",
            1,
            vec![Relation {
                recipient: 1,
                reason: Reason::Defined(String::new()),
                detail: Vec::new(),
            }],
            Vec::new(),
        ),
    ] {
        assert_eq!(report(&chain, bad).unwrap_err().code, code::INVALID_INPUT);
    }
}

fn told(chain: &MockChain, program: &str) -> Vec<(AccountNumber, Kind)> {
    let told: Vec<Change> = chain.query(program, &()).unwrap();
    told.into_iter()
        .map(|change| (change.recipient, change.kind))
        .collect()
}

const USER: AccountNumber = 1;
const SCRIPTED: AccountNumber = 2;
const UNREGISTERED: AccountNumber = 3;
const SUSPENDED: AccountNumber = 4;

fn program_account(number: AccountNumber, calls: &str, standing: Standing) -> Profile {
    Profile {
        number,
        name: format!("agent {number}"),
        kind: Account::Program {
            manager: USER,
            category: Category::Agent,
            standing,
            script: Some(Script {
                calls: calls.into(),
                config: "/shared/agent.md".into(),
            }),
        },
    }
}

fn scripted_chain() -> MockChain {
    let chain = chain();
    chain.profile(Profile {
        number: USER,
        name: "Ada".into(),
        kind: Account::User,
    });
    chain.profile(program_account(SCRIPTED, LISTENER, Standing::Active));
    chain.profile(program_account(UNREGISTERED, "nobody", Standing::Active));
    chain.profile(program_account(SUSPENDED, LISTENER, Standing::Suspended));
    for program in [LISTENER, OTHER] {
        chain
            .submit(signed(ADA), program, &Listen::Register)
            .unwrap();
    }
    chain
}

#[test]
fn a_program_registers_itself_only() {
    let chain = scripted_chain();
    let Reply::Registered(registered) = chain.query(PROGRAM, &Query::Registered).unwrap() else {
        panic!()
    };
    assert_eq!(registered, [LISTENER.to_owned(), OTHER.to_owned()]);
    let by_hand = chain.submit(signed(ADA), PROGRAM, &Op::Register { deliver: vec![1] });
    assert_eq!(by_hand.unwrap_err().code, code::UNAUTHORIZED);
}

#[test]
fn a_change_about_a_scripted_account_reaches_only_the_program_its_script_calls() {
    let chain = scripted_chain();
    report(&chain, update("t", 1, vec![owner(SCRIPTED)], Vec::new())).unwrap();
    assert_eq!(told(&chain, LISTENER), [(SCRIPTED, Kind::Added)]);
    assert_eq!(told(&chain, OTHER), []);
    let forged = Listen::Told(Box::new(
        chain.query::<_, Vec<Change>>(LISTENER, &()).unwrap()[0].clone(),
    ));
    assert_eq!(
        chain
            .submit(signed(ADA), LISTENER, &forged)
            .unwrap_err()
            .code,
        code::UNAUTHORIZED
    );
}

#[test]
fn a_users_change_and_an_unregistered_or_suspended_scripts_change_reach_no_program() {
    let chain = scripted_chain();
    let recipients = vec![owner(USER), owner(UNREGISTERED), owner(SUSPENDED)];
    report(&chain, update("t", 1, recipients, Vec::new())).unwrap();
    assert_eq!(told(&chain, LISTENER), []);
    assert_eq!(told(&chain, OTHER), []);
    let recorded: Vec<AccountNumber> = [USER, UNREGISTERED, SUSPENDED]
        .into_iter()
        .filter(|recipient| {
            changes_to(&chain, *recipient) == [("t".into(), Reason::Ownership, Kind::Added)]
        })
        .collect();
    assert_eq!(recorded, [USER, UNREGISTERED, SUSPENDED]);
}

#[test]
fn a_refusal_by_the_program_handed_a_change_changes_nothing() {
    let chain = scripted_chain();
    chain
        .submit(signed(ADA), LISTENER, &Listen::Refuse)
        .unwrap();
    report(&chain, update("t", 1, vec![owner(SCRIPTED)], Vec::new())).unwrap();
    assert_eq!(told(&chain, LISTENER), []);
    assert_eq!(
        relations(&chain, source("t")).unwrap().relations,
        [owner(SCRIPTED)]
    );
    assert_eq!(
        changes_to(&chain, SCRIPTED),
        [("t".into(), Reason::Ownership, Kind::Added)]
    );
}

#[test]
fn a_change_reads_back_by_its_seq() {
    let chain = chain();
    report(&chain, update("t", 1, vec![owner(1), owner(2)], Vec::new())).unwrap();
    let second = change_at(&chain, 2).unwrap();
    let read = (second.seq, second.recipient, second.source.object.id);
    assert_eq!(read, (2, 2, "t".to_owned()));
    assert_eq!(second.kind, Kind::Added);
    assert_eq!(change_at(&chain, 3), None);
}

#[test]
fn migrate_moves_a_renamed_programs_objects_their_changes_and_its_registration() {
    let mut chain = scripted_chain();
    report(&chain, update("t", 1, vec![owner(SCRIPTED)], Vec::new())).unwrap();
    report(&chain, update("u", 1, vec![owner(USER)], Vec::new())).unwrap();
    chain.seat::<Reporter>("o/reporter");
    chain.register("o/reporter", 905);
    chain.seat::<::program::Renamer>("programs");
    let renames: Vec<(ProgramId, ProgramId)> = [(REPORTER, "o/reporter"), (LISTENER, "o/listener")]
        .into_iter()
        .map(|(from, to)| (from.to_owned(), to.to_owned()))
        .collect();
    chain.submit(signed(ADA), "programs", &renames).unwrap();
    let since = Report::Send(update("t", 1, vec![owner(USER)], Vec::new()));
    chain.submit(signed(ADA), "o/reporter", &since).unwrap();
    let migrate = |rows: Rows| {
        let op = Op::Migrate { height: 0, rows };
        chain.submit(signed(ADA), PROGRAM, &op).unwrap();
    };
    let at = |program: &str, id: &str| Source {
        program: program.into(),
        object: Object {
            kind: "task".into(),
            id: id.into(),
        },
    };
    let key = |id: &str| Some((REPORTER.to_owned(), "task".to_owned(), id.to_owned()));
    migrate(Rows::Objects {
        first: key("u"),
        last: key("u"),
    });
    assert_eq!(relations(&chain, at(REPORTER, "u")), None, "u moved");
    let moved = relations(&chain, at("o/reporter", "u")).unwrap();
    assert_eq!(moved.source, at("o/reporter", "u"));
    assert_eq!(moved.relations, [owner(USER)]);
    assert!(
        relations(&chain, at(REPORTER, "t")).is_some(),
        "t is outside the range"
    );
    let objects = || Rows::Objects {
        first: None,
        last: None,
    };
    migrate(objects());
    assert_eq!(relations(&chain, at(REPORTER, "t")), None);
    assert_eq!(
        relations(&chain, at("o/reporter", "t")).unwrap().relations,
        [owner(USER)],
        "the object reported under the new id since is kept"
    );
    let of = || Rows::Of {
        first: None,
        last: None,
    };
    migrate(of());
    let changes_of = |source: Source| -> Vec<(u64, ProgramId)> {
        let asked = Query::ChangesOf {
            source,
            page: PageRequest::default(),
        };
        let Reply::Changes(page) = chain.query(PROGRAM, &asked).unwrap() else {
            panic!()
        };
        page.items
            .into_iter()
            .map(|change| (change.seq, change.source.program))
            .collect()
    };
    assert_eq!(
        changes_of(at("o/reporter", "t")),
        [(1, REPORTER.to_owned()), (3, "o/reporter".to_owned())],
        "each change keeps the id it was recorded with"
    );
    assert_eq!(changes_of(at(REPORTER, "t")), []);
    let registered = || Rows::Registered {
        first: None,
        last: None,
    };
    migrate(registered());
    let Reply::Registered(listed) = chain.query(PROGRAM, &Query::Registered).unwrap() else {
        panic!()
    };
    assert_eq!(listed, ["o/listener".to_owned(), OTHER.to_owned()]);
    let before = chain.host(PROGRAM).borrow().state.clone();
    for rows in [objects(), of(), registered()] {
        migrate(rows);
    }
    assert_eq!(
        chain.host(PROGRAM).borrow().state,
        before,
        "a second run writes nothing"
    );
}
