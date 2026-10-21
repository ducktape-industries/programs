//! The guard over a native chain: `notes` writes the keys each op names (claiming each `n/` key as a
//! `test.note`, a shared `c/` counter), and `bot` sends `notes` the same op as its own account.

use ::program::{Error, ExecCtx, MockChain, Origin, Program, QueryCtx, Reply as Wanted, code};
use abi::role::ownership::Write;

use crate::*;

type Writes = Vec<(String, Option<String>)>;

struct Notes;

impl Program for Notes {
    const NAME: &'static str = "notes";
    type Op = Writes;
    type Query = ();
    type Reply = ();
    fn execute(ctx: &ExecCtx, writes: Writes) -> Result<(), Error> {
        for (key, value) in writes {
            let claim = key.starts_with("n/").then_some("test.note");
            match value {
                Some(value) => ctx.set_with(key, value, claim),
                None => ctx.delete(key),
            }
        }
        let count: u64 = ctx.record("c/count")?.unwrap_or(0);
        ctx.put("c/count", &(count + 1));
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

struct Bot;

impl Program for Bot {
    const NAME: &'static str = "bot";
    type Op = Writes;
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, writes: Writes) -> Result<(), Error> {
        ctx.emit("notes", &writes, Wanted::None);
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

const ADA: Party = Party::Account(1);
const BOB: Party = Party::Account(2);

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Notes>("notes");
    chain.seat::<Bot>("bot");
    chain.seat::<Ownership>(PROGRAM);
    chain.register("notes", 100);
    chain.register("bot", 101);
    chain.hold(b"ada".to_vec(), 1);
    chain.hold(b"bob".to_vec(), 2);
    chain
}

fn by(who: &str) -> Origin {
    Origin::Signed(who.as_bytes().to_vec())
}

fn note(chain: &MockChain, who: &str, key: &str, value: Option<&str>) -> Result<(), Error> {
    let writes: Writes = vec![(key.into(), value.map(str::to_owned))];
    chain.submit(by(who), "notes", &writes).map(drop)
}

fn act(chain: &MockChain, who: &str, op: &Op) -> Result<Vec<u8>, Error> {
    chain.submit(by(who), PROGRAM, op)
}

fn made(chain: &MockChain, who: &str, name: &str, write: Writers) -> PolicyId {
    let op = Op::Create {
        name: name.into(),
        write,
    };
    abi::decode(&act(chain, who, &op).unwrap()).unwrap()
}

fn ask(chain: &MockChain, query: Query) -> Reply {
    chain.query(PROGRAM, &query).unwrap()
}

fn record(chain: &MockChain, key: &str) -> Option<Record> {
    let query = Query::Record {
        program: "notes".into(),
        key: key.as_bytes().to_vec(),
    };
    let Reply::Record(record) = ask(chain, query) else {
        panic!("Record answers a record")
    };
    record
}

fn effective(chain: &MockChain, key: &str) -> Option<Policy> {
    let query = Query::Effective {
        program: "notes".into(),
        key: key.as_bytes().to_vec(),
    };
    let Reply::Policy(policy) = ask(chain, query) else {
        panic!("Effective answers a policy")
    };
    policy
}

fn default(chain: &MockChain, who: &str, scope: Scope, policy: Option<PolicyId>) {
    act(chain, who, &Op::SetDefault { scope, policy }).unwrap();
}

fn notes(chain: &MockChain, key: &str) -> Option<Vec<u8>> {
    chain
        .host("notes")
        .borrow()
        .state
        .get(key.as_bytes())
        .cloned()
}

#[test]
fn a_new_key_is_its_writers_and_only_theirs_until_they_choose() {
    let chain = chain();
    note(&chain, "ada", "n/a", Some("hers")).unwrap();
    let policy = effective(&chain, "n/a").unwrap();
    let kind = Some("test.note".to_owned());
    assert_eq!(
        record(&chain, "n/a"),
        Some(Record {
            owner: ADA,
            policy: policy.id,
            kind
        })
    );
    assert_eq!(
        (policy.name.as_str(), &policy.write, &policy.owner),
        (ONLY_ME, &Writers::Owner, &ADA)
    );
    let Reply::OnlyMe(only_me) = ask(&chain, Query::OnlyMe { owner: ADA }) else {
        panic!("OnlyMe answers OnlyMe")
    };
    assert_eq!(only_me, Some(policy.id));
    let refused = note(&chain, "bob", "n/a", Some("his")).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert_eq!(notes(&chain, "n/a"), Some(b"hers".to_vec()));
    assert_eq!(record(&chain, "c/count"), None, "a shared key has no owner");
    let page = PageRequest::default();
    let Reply::Linked(linked) = ask(
        &chain,
        Query::Linked {
            policy: policy.id,
            page,
        },
    ) else {
        panic!()
    };
    let item = Linked {
        program: "notes".into(),
        key: b"n/a".to_vec(),
    };
    assert_eq!(linked.items, [item]);
}

#[test]
fn a_narrowed_default_refuses_others_and_their_whole_frame() {
    let chain = chain();
    let mine = made(&chain, "ada", "mine", Writers::Owner);
    default(&chain, "ada", Scope::Global, Some(mine));
    note(&chain, "ada", "n/b", Some("hers")).unwrap();
    assert_eq!(record(&chain, "n/b").unwrap().policy, mine);
    let count = notes(&chain, "c/count");
    for change in [Some("his"), None] {
        let refused = note(&chain, "bob", "n/b", change).unwrap_err();
        assert_eq!(refused.code, code::UNAUTHORIZED);
        assert!(refused.message.contains("mine"), "{refused}");
    }
    assert_eq!(notes(&chain, "n/b"), Some(b"hers".to_vec()));
    assert_eq!(
        notes(&chain, "c/count"),
        count,
        "the shared write went with the frame"
    );
    note(&chain, "bob", "n/c", Some("his")).unwrap();
    assert_eq!(record(&chain, "n/c").unwrap().owner, BOB);
    note(&chain, "ada", "n/b", Some("again")).unwrap();
    note(&chain, "ada", "n/b", None).unwrap();
    assert_eq!(
        record(&chain, "n/b"),
        None,
        "a deleted key drops its record"
    );
    let page = PageRequest::default();
    let Reply::Linked(linked) = ask(&chain, Query::Linked { policy: mine, page }) else {
        panic!()
    };
    assert!(linked.items.is_empty());
}

#[test]
fn the_most_specific_default_wins() {
    let chain = chain();
    let [global, program, kind] =
        ["global", "program", "kind"].map(|name| made(&chain, "ada", name, Writers::Owner));
    default(&chain, "ada", Scope::Global, Some(global));
    default(&chain, "ada", Scope::Program("notes".into()), Some(program));
    default(&chain, "ada", Scope::Kind("test.note".into()), Some(kind));
    let linked = |key: &str| {
        note(&chain, "ada", key, Some("x")).unwrap();
        record(&chain, key).unwrap().policy
    };
    assert_eq!(linked("n/1"), kind);
    default(&chain, "ada", Scope::Kind("test.note".into()), None);
    assert_eq!(linked("n/2"), program);
    default(&chain, "ada", Scope::Program("notes".into()), None);
    assert_eq!(linked("n/3"), global);
    let page = PageRequest::default();
    let Reply::Defaults(defaults) = ask(&chain, Query::Defaults { owner: ADA, page }) else {
        panic!()
    };
    let only = ScopeDefault {
        scope: Scope::Global,
        policy: global,
    };
    assert_eq!(defaults.items, [only]);
}

#[test]
fn a_listed_program_counts_only_as_its_own_account() {
    let chain = chain();
    let write = Writers::Listed(vec![Party::Program("bot".into())]);
    let listed = made(&chain, "ada", "bot", write);
    default(&chain, "ada", Scope::Global, Some(listed));
    note(&chain, "ada", "n/d", Some("hers")).unwrap();
    let through_bot: Writes = vec![("n/d".into(), Some("bot's".into()))];
    chain.submit(by("bob"), "bot", &through_bot).unwrap();
    assert_eq!(notes(&chain, "n/d"), Some(b"bot's".to_vec()));
    let refused = note(&chain, "bob", "n/d", Some("his")).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let op = Op::Edit {
        policy: listed,
        name: "bob".into(),
        write: Writers::Listed(vec![BOB]),
    };
    assert_eq!(
        act(&chain, "bob", &op).unwrap_err().code,
        code::UNAUTHORIZED
    );
    act(&chain, "ada", &op).unwrap();
    note(&chain, "bob", "n/d", Some("his")).unwrap();
    let refused = chain.submit(by("bob"), "bot", &through_bot).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
}

#[test]
fn a_retired_policy_still_protects_until_its_items_are_switched() {
    let chain = chain();
    note(&chain, "ada", "n/first", Some("x")).unwrap();
    let strict = made(&chain, "ada", "strict", Writers::Owner);
    default(&chain, "ada", Scope::Global, Some(strict));
    note(&chain, "ada", "n/e", Some("hers")).unwrap();
    act(&chain, "ada", &Op::Retire { policy: strict }).unwrap();
    let page = PageRequest::default();
    let Reply::Defaults(defaults) = ask(&chain, Query::Defaults { owner: ADA, page }) else {
        panic!()
    };
    assert!(
        defaults.items.is_empty(),
        "retiring drops the defaults naming it"
    );
    assert_eq!(
        note(&chain, "bob", "n/e", Some("his")).unwrap_err().code,
        code::UNAUTHORIZED
    );
    let again = Op::SetDefault {
        scope: Scope::Global,
        policy: Some(strict),
    };
    assert_eq!(
        act(&chain, "ada", &again).unwrap_err().code,
        code::WRONG_STATE
    );
    let loose = made(&chain, "ada", "loose", Writers::Anyone);
    let switch = Op::Switch {
        from: strict,
        to: loose,
    };
    assert_eq!(
        act(&chain, "bob", &switch).unwrap_err().code,
        code::UNAUTHORIZED
    );
    act(&chain, "ada", &switch).unwrap();
    assert_eq!(effective(&chain, "n/e").unwrap().id, loose);
    note(&chain, "bob", "n/e", Some("his")).unwrap();
    assert_eq!(
        act(&chain, "ada", &switch).unwrap_err().code,
        code::WRONG_STATE
    );
}

#[test]
fn only_an_items_owner_relinks_or_transfers_it() {
    let chain = chain();
    note(&chain, "ada", "n/f", Some("hers")).unwrap();
    let strict = made(&chain, "ada", "strict", Writers::Owner);
    let relink = Op::Relink {
        program: "notes".into(),
        key: b"n/f".to_vec(),
        policy: strict,
    };
    assert_eq!(
        act(&chain, "bob", &relink).unwrap_err().code,
        code::UNAUTHORIZED
    );
    act(&chain, "ada", &relink).unwrap();
    assert_eq!(
        note(&chain, "bob", "n/f", Some("his")).unwrap_err().code,
        code::UNAUTHORIZED
    );
    let transfer = |to| Op::Transfer {
        program: "notes".into(),
        key: b"n/f".to_vec(),
        to,
    };
    assert_eq!(
        act(&chain, "bob", &transfer(BOB)).unwrap_err().code,
        code::UNAUTHORIZED
    );
    let nobody = transfer(Party::Account(999));
    assert_eq!(
        act(&chain, "ada", &nobody).unwrap_err().code,
        code::NOT_FOUND
    );
    act(&chain, "ada", &transfer(BOB)).unwrap();
    assert_eq!(record(&chain, "n/f").unwrap().owner, BOB);
    note(&chain, "bob", "n/f", Some("his")).unwrap();
    assert_eq!(
        note(&chain, "ada", "n/f", Some("hers")).unwrap_err().code,
        code::UNAUTHORIZED
    );
    assert_eq!(
        act(&chain, "bob", &relink).unwrap_err().code,
        code::INVALID_INPUT
    );
}

#[test]
fn the_kernel_reads_a_record_at_the_key_records_keep_it() {
    for (program, key) in [("chat", b"m\0/7".as_slice()), ("", b""), ("notes", b"n/a")] {
        let at = (program.to_owned(), key.to_vec());
        assert_eq!(
            crate::rules::RECORDS.key(&at),
            abi::role::ownership::record_key(program, key)
        );
    }
}

#[test]
fn a_key_created_with_no_claim_has_no_owner() {
    let chain = chain();
    note(&chain, "ada", "c/loose", Some("hers")).unwrap();
    assert_eq!(record(&chain, "c/loose"), None);
    note(&chain, "bob", "c/loose", Some("his")).unwrap();
    note(&chain, "bob", "c/loose", None).unwrap();
}

#[test]
fn only_the_kernel_guards_and_its_bytes_are_the_kernels() {
    let chain = chain();
    let guard = abi::role::ownership::Guard {
        program: "notes".into(),
        origin: abi::Origin::External(b"ada".to_vec()),
        sender: Some(abi::Principal::Account(1)),
        writes: vec![Write {
            key: b"n/z".to_vec(),
            claimed: Some("test.note".into()),
            existed: false,
            kept: true,
        }],
    };
    let ours = abi::encode(&Op::Guard(guard.clone()));
    assert_eq!(
        ours,
        abi::encode(&abi::role::ownership::Op::Guard(guard.clone()))
    );
    let refused = act(&chain, "ada", &Op::Guard(guard)).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    assert_eq!(record(&chain, "n/z"), None);
}

#[derive(Debug, borsh::BorshSerialize, borsh::BorshDeserialize)]
enum Adopting {
    Write(String),
    Adopt(Vec<Adopted>),
    Page(String),
    Relay(AccountNumber, String),
}

struct Adopter;

impl Program for Adopter {
    const NAME: &'static str = "adopter";
    type Op = Adopting;
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, op: Adopting) -> Result<(), Error> {
        match op {
            Adopting::Write(key) => ctx.set_with(key, b"v".to_vec(), Some("test.old")),
            Adopting::Adopt(items) => crate::adopt(ctx, items)?,
            Adopting::Page(key) => match crate::by_itself(ctx)? {
                true => ctx.set_with(key, b"v".to_vec(), Some("test.old")),
                false => {
                    ctx.emit("adopter", &Adopting::Page(key), Wanted::None);
                }
            },
            Adopting::Relay(account, key) => {
                ctx.emit_as(account, "adopter", &Adopting::Page(key), Wanted::None);
            }
        }
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

fn adopting() -> MockChain {
    let mut chain = chain();
    chain.seat::<Adopter>("adopter");
    chain.register("adopter", 102);
    chain
}

fn item(key: &str, owner: Party, rule: Rule) -> Adopted {
    Adopted {
        key: key.as_bytes().to_vec(),
        owner,
        kind: Some("test.old".into()),
        rule,
    }
}

fn adopted(chain: &MockChain, key: &str) -> (Record, Policy) {
    let query = Query::Record {
        program: "adopter".into(),
        key: key.as_bytes().to_vec(),
    };
    let Reply::Record(Some(record)) = ask(chain, query) else {
        panic!("{key} is recorded");
    };
    let Reply::Policy(Some(policy)) = ask(chain, Query::Policy { id: record.policy }) else {
        panic!("its policy is kept");
    };
    (record, policy)
}

fn send(chain: &MockChain, op: &Adopting) -> Result<Vec<u8>, Error> {
    chain.submit(by("ada"), "adopter", op)
}

#[test]
fn a_program_records_owners_from_before_the_guard_once_and_on_todays_rule() {
    let chain = adopting();
    let room = Rule::List {
        at: "room".into(),
        write: Writers::Listed(vec![Party::Program("bot".into())]),
        by: ADA,
    };
    let items = vec![
        item("n/mine", ADA, Rule::OwnerOnly),
        item("n/open", ADA, Rule::Open),
        item("n/room-1", ADA, room.clone()),
        item("n/room-2", BOB, room),
    ];
    send(&chain, &Adopting::Adopt(items)).unwrap();
    let (mine, only) = adopted(&chain, "n/mine");
    assert_eq!(
        (&mine.owner, mine.kind.as_deref()),
        (&ADA, Some("test.old"))
    );
    assert_eq!(
        (only.name.as_str(), &only.write, &only.owner),
        (ONLY_ME, &Writers::Owner, &ADA)
    );
    let open = adopted(&chain, "n/open").1;
    assert_eq!(open.name, OPEN_POLICY);
    for (built_in, other) in [(open.id, only.id), (only.id, open.id)] {
        for op in [
            Op::Retire { policy: built_in },
            Op::Switch {
                from: built_in,
                to: other,
            },
            Op::Edit {
                policy: built_in,
                name: "changed".into(),
                write: Writers::Listed(vec![BOB]),
            },
        ] {
            let refused = act(&chain, "ada", &op).unwrap_err();
            assert_eq!(refused.code, code::WRONG_STATE, "{op:?}");
        }
    }
    let (first, listed) = adopted(&chain, "n/room-1");
    let (second, _) = adopted(&chain, "n/room-2");
    assert_eq!(first.policy, second.policy, "one policy per list");
    assert_eq!(second.owner, BOB);
    assert_eq!(listed.name, "adopter room (as before)");
    let again = vec![item("n/mine", BOB, Rule::Open)];
    send(&chain, &Adopting::Adopt(again)).unwrap();
    assert_eq!(adopted(&chain, "n/mine").0, mine);
    send(&chain, &Adopting::Write("n/written".into())).unwrap();
    assert_eq!(
        adopted(&chain, "n/written").0.owner,
        Party::Account(1),
        "ada's frame wrote it"
    );
    assert_eq!(
        adopted(&chain, "n/written").0.policy,
        only.id,
        "one Only me per owner, adopted or new"
    );
    let ours = chain.submit(
        Origin::Program("adopter".into()),
        "adopter",
        &Adopting::Write("n/ours".into()),
    );
    ours.unwrap();
    assert_eq!(adopted(&chain, "n/ours").0.owner, Party::Account(102));
    let reassign = vec![
        item("n/ours", BOB, Rule::OwnerOnly),
        item("n/written", BOB, Rule::OwnerOnly),
    ];
    send(&chain, &Adopting::Adopt(reassign)).unwrap();
    assert_eq!(adopted(&chain, "n/ours").0.owner, BOB);
    assert_eq!(
        adopted(&chain, "n/written").0.owner,
        ADA,
        "ada's stays hers"
    );
    let direct = Op::Adopt {
        items: vec![item("n/else", ADA, Rule::Open)],
    };
    assert_eq!(
        act(&chain, "ada", &direct).unwrap_err().code,
        code::UNAUTHORIZED
    );
}

#[test]
fn an_accounts_op_the_program_relays_in_its_own_name_pages_and_records_nothing_as_it() {
    use abi::role::identity::{Category, Kind, Profile, Script, Standing};
    let chain = adopting();
    chain.profile(Profile {
        number: 103,
        name: "relayed agent".into(),
        kind: Kind::Program {
            script: Some(Script {
                calls: "adopter".into(),
                config: String::new(),
            }),
            manager: 1,
            category: Category::Agent,
            standing: Standing::Active,
        },
    });
    send(&chain, &Adopting::Relay(103, "n/relayed".into())).unwrap();
    assert_eq!(
        adopted(&chain, "n/relayed").0.owner,
        Party::Account(102),
        "the program's own message wrote it, not the agent's relayed op"
    );
    send(&chain, &Adopting::Page("n/paged".into())).unwrap();
    assert_eq!(adopted(&chain, "n/paged").0.owner, Party::Account(102));
}

#[test]
fn todays_access_answer_reads_as_a_rule() {
    use abi::role::access::{Ownership as Listing, Party as Lister, Writers as Today};
    let listed = |owner: Option<Lister>, list| Today::Listed {
        at: "room".into(),
        owner,
        list,
    };
    assert_eq!(rule_of(&ADA, &Today::Anyone), Rule::Open);
    assert_eq!(
        rule_of(&ADA, &listed(Some(Lister::Account(1)), Listing::Anyone)),
        Rule::Open
    );
    assert_eq!(
        rule_of(&ADA, &listed(Some(Lister::Account(1)), Listing::Owner)),
        Rule::OwnerOnly
    );
    assert_eq!(
        rule_of(&ADA, &listed(None, Listing::Owner)),
        Rule::OwnerOnly
    );
    let board = Listing::Listed(vec![Lister::Program("board".into())]);
    assert_eq!(
        rule_of(&ADA, &listed(Some(Lister::Account(1)), board)),
        Rule::List {
            at: "room".into(),
            write: Writers::Listed(vec![Party::Program("board".into())]),
            by: ADA,
        }
    );
    assert_eq!(
        rule_of(&ADA, &listed(Some(Lister::Account(2)), Listing::Owner)),
        Rule::List {
            at: "room".into(),
            write: Writers::Listed(vec![BOB]),
            by: BOB,
        }
    );
}
