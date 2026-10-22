//! model-routing over a MockHost: a full account is learned once for every conversation, a pin
//! stays until its account goes full or leaves its pool, and nothing is written unless it changes.
use std::cell::Cell;

use ::program::{Env, Error, MockHost, Program, code};

use crate::*;

const HARNESS: u64 = 11;
const OTHER: u64 = 12;
const CLAUDE: &str = "claude";
const CODEX: &str = "codex";
const FABRICS: &str = "fabrics-claude";
const ZK: &str = "zk-claude";
const NOW: u64 = 1_800_000_000;
const HOUR: u64 = 3600;

struct World {
    host: MockHost,
    seconds: Cell<u64>,
}

impl World {
    fn env(&self) -> Env {
        Env {
            time: self.seconds.get() * 1000,
            ..MockHost::env(PROGRAM)
        }
    }

    fn ask(&self, query: Query) -> Reply {
        ModelRouting::query(&self.host.query(self.env()), query).unwrap()
    }
}

fn world() -> World {
    World {
        host: MockHost::default(),
        seconds: Cell::new(NOW),
    }
}

fn at(world: &World, seconds: u64) {
    world.seconds.set(seconds);
}

fn send(world: &World, account: u64, op: Op) -> Result<Vec<u8>, Error> {
    world.host.borrow_mut().touched.clear();
    let key = format!("key-{account}").into_bytes();
    let env = world.env().signed(key, Some(account));
    world
        .host
        .attempt(|| ModelRouting::execute(&world.host.exec(env), op))?;
    Ok(world.host.take_output())
}

fn written(world: &World) -> usize {
    world.host.borrow().touched.len()
}

fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn account(service: &str, account: &str) -> GatewayAccount {
    GatewayAccount {
        service: service.into(),
        account: account.into(),
    }
}

fn pool(world: &World, service: &str, accounts: &[&str]) -> usize {
    let op = Op::Pool {
        provider: CLAUDE.into(),
        service: service.into(),
        accounts: names(accounts),
    };
    send(world, HARNESS, op).unwrap();
    written(world)
}

fn full(world: &World, service: &str, label: &str, until: Until) -> usize {
    let op = Op::Full {
        provider: CLAUDE.into(),
        service: service.into(),
        account: label.into(),
        until,
    };
    send(world, HARNESS, op).unwrap();
    written(world)
}

fn available(world: &World, service: &str, label: &str, read_at: u64) -> usize {
    let op = Op::Available {
        provider: CLAUDE.into(),
        service: service.into(),
        account: label.into(),
        read_at,
    };
    send(world, HARNESS, op).unwrap();
    written(world)
}

fn pin_for(world: &World, conversation: u64, provider: &str, services: &[&str]) -> (Choice, usize) {
    let op = Op::Pin {
        conversation,
        provider: provider.into(),
        services: names(services),
    };
    let returned = send(world, HARNESS, op).unwrap();
    (abi::decode(&returned).unwrap(), written(world))
}

fn pin(world: &World, conversation: u64, services: &[&str]) -> (Choice, usize) {
    pin_for(world, conversation, CLAUDE, services)
}

fn resolve(world: &World, conversation: u64, services: &[&str]) -> Choice {
    let query = Query::Resolve {
        consumer: HARNESS,
        conversation,
        provider: CLAUDE.into(),
        services: names(services),
    };
    match world.ask(query) {
        Reply::Choice(choice) => choice,
        other => panic!("Resolve answered {other:?}"),
    }
}

fn fulls(world: &World, consumer: u64) -> Vec<FullRow> {
    let query = Query::Fulls {
        consumer,
        page: PageRequest::default(),
    };
    match world.ask(query) {
        Reply::Fulls(page) => page.items,
        other => panic!("Fulls answered {other:?}"),
    }
}

fn pins(world: &World) -> Vec<PinRow> {
    let query = Query::Pins {
        consumer: HARNESS,
        page: PageRequest::default(),
    };
    match world.ask(query) {
        Reply::Pins(page) => page.items,
        other => panic!("Pins answered {other:?}"),
    }
}

#[test]
fn a_quota_429_in_one_conversation_moves_every_other_off_the_account() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    assert_eq!(
        pin(&world, 1, &[FABRICS]).0,
        Choice::Chosen(account(FABRICS, "a"))
    );
    assert_eq!(
        pin(&world, 2, &[FABRICS]).0,
        Choice::Chosen(account(FABRICS, "a"))
    );
    assert_eq!(full(&world, FABRICS, "a", Until::Reset(NOW + HOUR)), 1);
    assert_eq!(
        resolve(&world, 2, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b")),
        "the second conversation learns of the 429 the first one met, with no call on a"
    );
    assert_eq!(
        pin(&world, 2, &[FABRICS]),
        (Choice::Chosen(account(FABRICS, "b")), 1)
    );
    assert_eq!(full(&world, FABRICS, "a", Until::Reset(NOW + HOUR)), 0);
    assert_eq!(full(&world, FABRICS, "a", Until::Unknown), 0);
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Chosen(account(FABRICS, "b")), 1)
    );
}

#[test]
fn a_pin_stays_through_successes_a_reorder_and_another_accounts_recovery() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Chosen(account(FABRICS, "a")), 1)
    );
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Pinned(account(FABRICS, "a")), 0)
    );
    assert_eq!(
        pool(&world, FABRICS, &["a", "b"]),
        0,
        "the same pool again writes nothing"
    );
    assert_eq!(pool(&world, FABRICS, &["b", "a"]), 1);
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Pinned(account(FABRICS, "a")), 0)
    );
    full(&world, FABRICS, "b", Until::Reset(NOW + 10));
    at(&world, NOW + 11);
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Pinned(account(FABRICS, "a")), 0)
    );
    pool(&world, ZK, &["z"]);
    assert_eq!(
        pin_for(&world, 1, CODEX, &[ZK]).0,
        Choice::Discover(ZK.into()),
        "a provider keeps pools and pins of its own"
    );
    assert_eq!(
        pin(&world, 1, &[ZK, FABRICS]).0,
        Choice::Pinned(account(FABRICS, "a"))
    );
}

#[test]
fn a_pool_never_recorded_is_discovered_and_an_empty_one_asks_no_account() {
    let world = world();
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Discover(FABRICS.into()), 0)
    );
    pool(&world, FABRICS, &[]);
    assert_eq!(pin(&world, 1, &[FABRICS]), (Choice::Wait(None), 0));
    pool(&world, FABRICS, &["a"]);
    assert_eq!(
        pin(&world, 1, &[FABRICS]).0,
        Choice::Chosen(account(FABRICS, "a"))
    );
    full(&world, FABRICS, "a", Until::Reset(NOW + HOUR));
    assert_eq!(
        pin(&world, 1, &[FABRICS, ZK]),
        (Choice::Discover(ZK.into()), 0),
        "a full pin on a known pool still asks for the next service's pool before waiting"
    );
    pool(&world, ZK, &["z"]);
    assert_eq!(
        pin(&world, 1, &[FABRICS, ZK]).0,
        Choice::Chosen(account(ZK, "z"))
    );
}

#[test]
fn an_account_its_pool_drops_moves_its_pin_and_keeps_its_full_episode_when_it_returns() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    pin(&world, 1, &[FABRICS]);
    pool(&world, FABRICS, &["b"]);
    assert_eq!(
        pin(&world, 1, &[FABRICS]).0,
        Choice::Chosen(account(FABRICS, "b")),
        "a dropped account is never called"
    );
    full(&world, FABRICS, "a", Until::Unknown);
    pool(&world, FABRICS, &["a", "b", "c"]);
    assert_eq!(
        pin(&world, 1, &[FABRICS]).0,
        Choice::Pinned(account(FABRICS, "b"))
    );
    assert_eq!(
        resolve(&world, 2, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b")),
        "a's episode outlived its leaving and coming back"
    );
    full(&world, FABRICS, "c", Until::Reset(NOW + 10));
    at(&world, NOW + 11);
    pool(&world, FABRICS, &["a", "b"]);
    let left: Vec<String> = fulls(&world, HARNESS)
        .into_iter()
        .map(|row| row.account)
        .collect();
    assert_eq!(
        left,
        ["a"],
        "a dropped account's episode is dropped once its reset passed"
    );
}

#[test]
fn a_spent_week_outlives_the_session_reset_and_a_passed_reset_needs_no_write() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    assert_eq!(full(&world, FABRICS, "a", Until::Reset(NOW + 5 * HOUR)), 1);
    assert_eq!(
        full(&world, FABRICS, "a", Until::Reset(NOW + 168 * HOUR)),
        1
    );
    assert_eq!(full(&world, FABRICS, "a", Until::Reset(NOW + 5 * HOUR)), 0);
    at(&world, NOW + 6 * HOUR);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b"))
    );
    at(&world, NOW + 168 * HOUR + 1);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "a"))
    );
    assert_eq!(
        fulls(&world, HARNESS)[0].until,
        Until::Reset(NOW + 168 * HOUR),
        "time passing wrote nothing"
    );
    assert_eq!(
        full(&world, FABRICS, "a", Until::Reset(NOW + 200 * HOUR)),
        1
    );
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b"))
    );
}

#[test]
fn with_every_account_full_it_waits_for_the_earliest_reset_and_an_unknown_one_for_a_report() {
    let world = world();
    pool(&world, FABRICS, &["a", "b", "c"]);
    full(&world, FABRICS, "a", Until::Reset(NOW + 2 * HOUR));
    full(&world, FABRICS, "b", Until::Reset(NOW + HOUR));
    full(&world, FABRICS, "c", Until::Unknown);
    assert_eq!(
        pin(&world, 1, &[FABRICS]),
        (Choice::Wait(Some(NOW + HOUR)), 0)
    );
    at(&world, NOW + HOUR);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b"))
    );
    assert_eq!(full(&world, FABRICS, "b", Until::Unknown), 1);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Wait(Some(NOW + 2 * HOUR))
    );
    at(&world, NOW + 2 * HOUR);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "a"))
    );
    assert_eq!(full(&world, FABRICS, "a", Until::Unknown), 1);
    assert_eq!(resolve(&world, 1, &[FABRICS]), Choice::Wait(None));
    assert_eq!(available(&world, FABRICS, "c", NOW + 2 * HOUR), 1);
    assert_eq!(available(&world, FABRICS, "c", NOW + 2 * HOUR), 0);
    assert_eq!(
        pin(&world, 1, &[FABRICS]).0,
        Choice::Chosen(account(FABRICS, "c"))
    );
}

#[test]
fn a_pin_off_a_full_account_takes_the_next_one_after_it_and_wraps() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    pool(&world, ZK, &["z"]);
    let route = [FABRICS, ZK];
    full(&world, FABRICS, "a", Until::Reset(NOW + HOUR));
    assert_eq!(
        pin(&world, 1, &route).0,
        Choice::Chosen(account(FABRICS, "b"))
    );
    available(&world, FABRICS, "a", NOW + 1);
    full(&world, FABRICS, "b", Until::Reset(NOW + HOUR));
    assert_eq!(pin(&world, 1, &route).0, Choice::Chosen(account(ZK, "z")));
    full(&world, ZK, "z", Until::Reset(NOW + HOUR));
    assert_eq!(
        pin(&world, 1, &route).0,
        Choice::Chosen(account(FABRICS, "a"))
    );
    assert_eq!(
        pin(&world, 1, &[ZK]),
        (Choice::Wait(Some(NOW + HOUR)), 0),
        "a pin on a service the route no longer lists is no pin"
    );
}

#[test]
fn each_caller_routes_in_a_namespace_of_its_own() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    let elsewhere = Op::Full {
        provider: CLAUDE.into(),
        service: FABRICS.into(),
        account: "a".into(),
        until: Until::Unknown,
    };
    send(&world, OTHER, elsewhere).unwrap();
    assert_eq!(fulls(&world, OTHER).len(), 1);
    assert!(fulls(&world, HARNESS).is_empty());
    assert_eq!(
        pin(&world, 1, &[FABRICS]).0,
        Choice::Chosen(account(FABRICS, "a"))
    );
}

#[test]
fn forget_drops_only_that_conversations_pins() {
    let world = world();
    pool(&world, FABRICS, &["a"]);
    let codex = Op::Pool {
        provider: CODEX.into(),
        service: "zk-codex".into(),
        accounts: names(&["x"]),
    };
    send(&world, HARNESS, codex).unwrap();
    pin(&world, 1, &[FABRICS]);
    pin_for(&world, 1, CODEX, &["zk-codex"]);
    pin(&world, 2, &[FABRICS]);
    assert_eq!(pins(&world).len(), 3);
    send(&world, HARNESS, Op::Forget { conversation: 1 }).unwrap();
    assert_eq!(written(&world), 2);
    let left: Vec<u64> = pins(&world).iter().map(|row| row.conversation).collect();
    assert_eq!(left, [2]);
}

#[test]
fn a_pool_naming_a_label_twice_or_a_blank_one_is_refused() {
    let world = world();
    for accounts in [names(&["a", "a"]), names(&[""])] {
        let op = Op::Pool {
            provider: CLAUDE.into(),
            service: FABRICS.into(),
            accounts,
        };
        let refused = send(&world, HARNESS, op).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused:?}");
    }
    pool(&world, FABRICS, &["a"]);
    let twice = Op::Pin {
        conversation: 1,
        provider: CLAUDE.into(),
        services: names(&[FABRICS, FABRICS]),
    };
    let refused = send(&world, HARNESS, twice).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused:?}");
}

#[test]
fn a_status_read_before_a_429_never_ends_the_episode_the_429_began() {
    let world = world();
    pool(&world, FABRICS, &["a", "b"]);
    at(&world, NOW + 10);
    full(&world, FABRICS, "a", Until::Unknown);
    assert_eq!(
        available(&world, FABRICS, "a", NOW + 5),
        0,
        "a status read before the 429 landed late"
    );
    assert_eq!(
        available(&world, FABRICS, "a", NOW + 10),
        0,
        "nor one read as the 429 was recorded"
    );
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b"))
    );
    assert_eq!(available(&world, FABRICS, "a", NOW + 11), 1);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "a"))
    );
    full(&world, FABRICS, "a", Until::Reset(NOW + HOUR));
    at(&world, NOW + 20);
    assert_eq!(full(&world, FABRICS, "a", Until::Reset(NOW + 2 * HOUR)), 1);
    assert_eq!(
        fulls(&world, HARNESS)[0].since,
        NOW + 20,
        "a later reset begins the episode again"
    );
    assert_eq!(available(&world, FABRICS, "a", NOW + 15), 0);
    assert_eq!(
        resolve(&world, 1, &[FABRICS]),
        Choice::Chosen(account(FABRICS, "b"))
    );
}
