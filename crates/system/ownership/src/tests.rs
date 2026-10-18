//! The guard over a native chain: `notes` writes the keys each op names (an owned `n/` table of
//! `test.note`, a shared `c/` counter), and `bot` sends `notes` the same op as its own account.

use ::program::{
    Error, ExecCtx, MockChain, Origin, Program, QueryCtx, Reply as Wanted, Table, code,
};
use abi::role::ownership::Write;

use crate::*;

type Writes = Vec<(String, Option<String>)>;

struct Notes;

impl Program for Notes {
    const NAME: &'static str = "notes";
    type Op = Writes;
    type Query = ();
    type Reply = ();
    const TABLES: &'static [Table] = &[Table::owned("n/", "test.note"), Table::shared("c/")];

    fn execute(ctx: &ExecCtx, writes: Writes) -> Result<(), Error> {
        for (key, value) in writes {
            match value {
                Some(value) => ctx.set(key, value),
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
fn a_new_key_is_its_writers_and_links_to_their_open_policy() {
    let chain = chain();
    note(&chain, "ada", "n/a", Some("hers")).unwrap();
    let Reply::Open(Some(open)) = ask(&chain, Query::Open { owner: ADA }) else {
        panic!("ada's open policy was made")
    };
    let kind = Some("test.note".to_owned());
    assert_eq!(
        record(&chain, "n/a"),
        Some(Record {
            owner: ADA,
            policy: open,
            kind
        })
    );
    let policy = effective(&chain, "n/a").unwrap();
    assert_eq!(
        (policy.name.as_str(), &policy.write, &policy.owner),
        (OPEN_POLICY, &Writers::Anyone, &ADA)
    );
    // open: anyone changes it, and its record stays ada's
    note(&chain, "bob", "n/a", Some("his")).unwrap();
    assert_eq!(record(&chain, "n/a").unwrap().owner, ADA);
    assert_eq!(record(&chain, "c/count"), None, "a shared key has no owner");
    let page = PageRequest::default();
    let Reply::Linked(linked) = ask(&chain, Query::Linked { policy: open, page }) else {
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
    // bob's own new key is his, on his open policy
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
    // ada's first item makes her open policy
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
    // the open policy never changes
    let Reply::Open(Some(open)) = ask(&chain, Query::Open { owner: ADA }) else {
        panic!("made when ada's first item was")
    };
    for op in [
        Op::Retire { policy: open },
        Op::Switch {
            from: open,
            to: loose,
        },
        Op::Edit {
            policy: open,
            name: "closed".into(),
            write: Writers::Owner,
        },
    ] {
        assert_eq!(act(&chain, "ada", &op).unwrap_err().code, code::WRONG_STATE);
    }
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
    // bob owns it now, under ada's policy, which admits its owner alone
    assert_eq!(record(&chain, "n/f").unwrap().owner, BOB);
    note(&chain, "bob", "n/f", Some("his")).unwrap();
    assert_eq!(
        note(&chain, "ada", "n/f", Some("hers")).unwrap_err().code,
        code::UNAUTHORIZED
    );
    // and links it only to a policy of his own
    assert_eq!(
        act(&chain, "bob", &relink).unwrap_err().code,
        code::INVALID_INPUT
    );
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
            kind: None,
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
