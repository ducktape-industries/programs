// resources on a MockChain: each liveness claim of the contract as a walk that runs (J-460
// tests-1b.md): placement order, walks booked for the next block that end when the kind is full or
// the queue ends, counts and their standing, losses placed again, and asks made through a program.
use std::cell::Cell;

use ::program::{Error, ExecCtx, MockChain, Origin, Program, QueryCtx, Reply as Wanted, code};
use abi::role::identity::{
    Category, Kind as AccountKind, Profile, Script, Standing as AccountStanding,
};
use abi::role::scheduler;
use borsh::{BorshDeserialize, BorshSerialize};

use crate::*;

const OWNER: &[u8] = b"owner-key";
const OWNER_ACCOUNT: u64 = 11;
const ADA: &[u8] = b"ada-key";
const ADA_ACCOUNT: u64 = 12;
const BO: &[u8] = b"bo-key";
const BO_ACCOUNT: u64 = 13;
const CY: &[u8] = b"cy-key";
const CY_ACCOUNT: u64 = 14;
const P: &[u8] = b"p-key";
const P_ACCOUNT: u64 = 21;
const Q: &[u8] = b"q-key";
const Q_ACCOUNT: u64 = 22;
const N: &[u8] = b"n-key";
const N_ACCOUNT: u64 = 23;
const CLIENT: &str = "client";
const CLIENT_ACCOUNT: u64 = 904;
const ROBOT_ACCOUNT: u64 = 40;
const RESOURCES_ACCOUNT: u64 = 903;
const KIND: &str = "build";
const NOTARY: &str = "notary.tls";
const LONG: u64 = 1_000;

#[derive(Debug, BorshSerialize, BorshDeserialize)]
enum Through {
    AskAs { account: u64, spec: Spec },
    Send(Op),
}

struct Client;

impl Program for Client {
    const NAME: &'static str = CLIENT;
    type Op = Through;
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, op: Through) -> Result<(), Error> {
        match op {
            Through::AskAs { account, spec } => {
                ctx.emit_as(account, PROGRAM, &Op::Request { spec }, Wanted::None);
            }
            Through::Send(op) => {
                ctx.emit(PROGRAM, &op, Wanted::None);
            }
        }
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

struct World {
    chain: MockChain,
    height: Cell<u64>,
}

fn world() -> World {
    let mut chain = MockChain::default();
    chain.seat::<Resources>(PROGRAM);
    chain.seat::<Client>(CLIENT);
    chain.seat::<::scheduler::Scheduler>(::scheduler::PROGRAM);
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    for (key, account) in [
        (OWNER, OWNER_ACCOUNT),
        (ADA, ADA_ACCOUNT),
        (BO, BO_ACCOUNT),
        (CY, CY_ACCOUNT),
        (P, P_ACCOUNT),
        (Q, Q_ACCOUNT),
        (N, N_ACCOUNT),
    ] {
        chain.hold(key, account);
    }
    chain.register(PROGRAM, RESOURCES_ACCOUNT);
    chain.register(CLIENT, CLIENT_ACCOUNT);
    chain.profile(Profile {
        number: ROBOT_ACCOUNT,
        name: "robot".into(),
        kind: AccountKind::Program {
            manager: ADA_ACCOUNT,
            category: Category::Agent,
            standing: AccountStanding::Active,
            script: Some(Script {
                calls: CLIENT.into(),
                config: String::new(),
            }),
        },
    });
    let world = World {
        chain,
        height: Cell::new(1),
    };
    world.to(2);
    world
}

fn meter(name: &str, reported_by: Counter, checked_by: Option<Counter>) -> Meter {
    Meter {
        name: name.into(),
        unit: name.into(),
        reported_by,
        checked_by,
    }
}

fn kind_spec(walk: u64, lapse: u64, meters: Vec<Meter>) -> KindSpec {
    KindSpec {
        meters,
        slot: "one build".into(),
        lapse: Some(lapse),
        notary: None,
        walk,
    }
}

fn spec() -> Spec {
    Spec {
        kind: KIND.into(),
        needs: Needs {
            units: 1,
            labels: Vec::new(),
            pin: None,
            isolation: Isolation::None,
            notarized: false,
        },
        class: String::new(),
        on_loss: OnLoss::Retry,
        notify: None,
    }
}

fn needing(label: &str) -> Spec {
    let mut spec = spec();
    spec.needs.labels = vec![Selector::Has(label.into())];
    spec
}

fn offer_of(name: &str, kind: &str, capacity: u64) -> Offer {
    Offer {
        name: name.into(),
        kind: kind.into(),
        labels: Vec::new(),
        capacity,
        reach: Reach::Direct,
        terms: Terms {
            serves: None,
            notarizes: false,
        },
        isolation: Isolation::None,
    }
}

fn id(provider: u64, name: &str) -> OfferId {
    OfferId {
        provider,
        name: name.into(),
    }
}

impl World {
    fn act(&self, key: &[u8], op: &Op) -> Result<Vec<u8>, Error> {
        self.chain.submit(Origin::Signed(key.to_vec()), PROGRAM, op)
    }

    fn through(&self, op: &Through) -> Result<Vec<u8>, Error> {
        self.chain
            .submit(Origin::Program(CLIENT.into()), CLIENT, op)
    }

    fn ask(&self, query: Query) -> Reply {
        self.chain.query(PROGRAM, &query).unwrap()
    }

    fn to(&self, height: u64) {
        self.height.set(height);
        self.chain.at(height, height * 1000);
    }

    fn next(&self) {
        self.to(self.height.get() + 1);
    }

    fn now(&self) -> u64 {
        self.height.get()
    }

    fn register(&self, name: &str, spec: KindSpec) {
        let op = Op::RegisterKind {
            name: name.into(),
            spec,
        };
        self.act(OWNER, &op).unwrap();
    }

    fn admit(&self, kind: &str, provider: u64) {
        let op = Op::Admit {
            kind: kind.into(),
            provider,
        };
        self.act(OWNER, &op).unwrap();
    }

    fn offer(&self, key: &[u8], offer: Offer) {
        self.act(key, &Op::Offer { offer }).unwrap();
    }

    fn build(&self, walk: u64, capacity: u64) {
        self.register(KIND, kind_spec(walk, LONG, Vec::new()));
        self.admit(KIND, P_ACCOUNT);
        self.offer(P, offer_of("p", KIND, capacity));
        self.settle();
    }

    fn settle(&self) {
        self.to(self.now() + 2);
    }

    fn request(&self, key: &[u8], spec: Spec) -> u64 {
        let made = self.act(key, &Op::Request { spec }).unwrap();
        abi::decode(&made).unwrap()
    }

    fn view(&self, request: u64) -> RequestView {
        let Reply::Request(Some(view)) = self.ask(Query::Request { id: request }) else {
            panic!("no request {request}");
        };
        view
    }

    fn state(&self, request: u64) -> RequestState {
        self.view(request).state
    }

    fn assignment(&self, request: u64, attempt: u32) -> Option<Assignment> {
        let Reply::Assignment(row) = self.ask(Query::Assignment { request, attempt }) else {
            panic!("Assignment answers an assignment");
        };
        row
    }

    fn placed_on(&self, request: u64) -> Option<OfferId> {
        let view = self.view(request);
        if view.state != RequestState::Placed {
            return None;
        }
        self.assignment(request, view.attempt)
            .map(|assignment| assignment.offer)
    }

    fn why(&self, request: u64) -> WhyView {
        let Reply::Why(Some(why)) = self.ask(Query::Why { request }) else {
            panic!("no why for {request}");
        };
        why
    }

    fn offer_view(&self, offer: OfferId) -> OfferView {
        let Reply::Offer(Some(view)) = self.ask(Query::Offer { offer }) else {
            panic!("no such offer");
        };
        view
    }

    fn holds(&self, offer: OfferId) -> Vec<u64> {
        let asked = Query::Holds {
            offer,
            page: PageRequest::default(),
        };
        let Reply::Assignments(page) = self.ask(asked) else {
            panic!("Holds answers assignments");
        };
        page.items.iter().map(|held| held.request).collect()
    }

    fn usage(&self, request: u64, attempt: u32) -> Usage {
        let Reply::Usage(Some(usage)) = self.ask(Query::Usage { request, attempt }) else {
            panic!("no usage for {request} at {attempt}");
        };
        usage
    }

    fn claim(&self, key: &[u8], request: u64) {
        let attempt = self.view(request).attempt;
        self.act(key, &Op::Claim { request, attempt }).unwrap();
    }

    fn end(&self, key: &[u8], request: u64) {
        let attempt = self.view(request).attempt;
        let report = Op::Report {
            request,
            attempt,
            outcome: Some(Outcome::Passed),
            readings: Vec::new(),
            evidence: None,
        };
        self.act(key, &report).unwrap();
    }

    fn steps_at(&self, kind: &str, height: u64) -> usize {
        let place = abi::encode(&Op::Place { kind: kind.into() });
        let mut steps = 0;
        let mut after = None;
        loop {
            let due = scheduler::Query::Due {
                height,
                after,
                limit: None,
            };
            let scheduler::Reply::Due { entries, next } =
                self.chain.query(::scheduler::PROGRAM, &due).unwrap()
            else {
                panic!("Due answers entries");
            };
            for listed in entries {
                let asked = scheduler::Query::Entry { id: listed.id };
                let scheduler::Reply::Entry(Some(entry)) =
                    self.chain.query(::scheduler::PROGRAM, &asked).unwrap()
                else {
                    panic!("a listed entry is there");
                };
                let ours = entry.target == PROGRAM && entry.payload == place;
                steps += usize::from(ours);
            }
            match next {
                Some(next) => after = Some(next),
                None => return steps,
            }
        }
    }

    fn steps_next(&self, kind: &str) -> usize {
        self.steps_at(kind, self.now() + 1)
    }

    fn steps_pending(&self, kind: &str) -> usize {
        self.steps_at(kind, self.now()) + self.steps_next(kind)
    }

    fn work(&self) -> (u64, u64) {
        let work = self.chain.host(PROGRAM).borrow().work;
        (work.gets, work.scanned)
    }

    fn reset_work(&self) {
        self.chain.host(PROGRAM).borrow_mut().work = Default::default();
    }

    fn policy_of(&self, accounts: &[u64]) -> PolicyId {
        let create = ownership::Op::Create {
            name: "members".into(),
            write: ownership::Writers::Listed(
                accounts
                    .iter()
                    .map(|account| ownership::Party::Account(*account))
                    .collect(),
            ),
        };
        let made = self
            .chain
            .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &create)
            .unwrap();
        abi::decode(&made).unwrap()
    }
}

#[test]
fn an_ending_report_books_a_walk_and_places_nothing_in_its_own_frame() {
    let world = world();
    world.build(10, 1);
    let first = world.request(ADA, spec());
    assert_eq!(world.placed_on(first), Some(id(P_ACCOUNT, "p")));
    let second = world.request(BO, spec());
    assert_eq!(world.state(second), RequestState::Queued);
    world.claim(P, first);
    world.to(5);
    world.end(P, first);
    assert_eq!(world.state(second), RequestState::Queued);
    assert_eq!(world.steps_next(KIND), 1);
    world.to(7);
    assert_eq!(world.placed_on(second), Some(id(P_ACCOUNT, "p")));
}

#[test]
fn a_queue_no_offer_takes_costs_the_provider_nothing() {
    let mut costs = Vec::new();
    for queued in [1, 40] {
        let world = world();
        world.build(10, 1);
        let held = world.request(ADA, spec());
        world.claim(P, held);
        for _ in 0..queued {
            world.request(BO, needing("gpu"));
        }
        world.next();
        world.reset_work();
        world.end(P, held);
        costs.push(world.work());
    }
    assert_eq!(
        costs[0], costs[1],
        "the ending report's reads grew with the queue"
    );
}

#[test]
fn a_step_out_of_free_slots_ends_the_walk_and_books_nothing() {
    let world = world();
    world.build(10, 1);
    let first = world.request(ADA, spec());
    let second = world.request(BO, spec());
    let third = world.request(CY, spec());
    world.claim(P, first);
    world.to(5);
    world.end(P, first);
    world.to(7);
    assert_eq!(world.placed_on(second), Some(id(P_ACCOUNT, "p")));
    assert_eq!(world.state(third), RequestState::Queued);
    for _ in 0..5 {
        assert_eq!(world.steps_pending(KIND), 0, "a full kind booked a step");
        world.next();
    }
    assert_eq!(world.state(third), RequestState::Queued);
    world.claim(P, second);
    world.end(P, second);
    assert_eq!(world.steps_next(KIND), 1);
    world.to(world.now() + 2);
    assert_eq!(world.placed_on(third), Some(id(P_ACCOUNT, "p")));
}

#[test]
fn a_step_examines_at_most_walk_and_goes_on_next_block() {
    let world = world();
    world.build(2, 1);
    let held = world.request(ADA, spec());
    for _ in 0..5 {
        world.request(BO, needing("gpu"));
    }
    let takeable = world.request(CY, spec());
    world.claim(P, held);
    world.to(5);
    world.end(P, held);
    assert_eq!(world.steps_at(KIND, 6), 1);
    world.to(7);
    assert_eq!(world.state(takeable), RequestState::Queued);
    assert_eq!(world.steps_at(KIND, 7), 1);
    world.to(8);
    assert_eq!(world.state(takeable), RequestState::Queued);
    assert_eq!(world.steps_at(KIND, 8), 1);
    world.to(9);
    assert_eq!(world.placed_on(takeable), Some(id(P_ACCOUNT, "p")));
    assert_eq!(world.steps_at(KIND, 9), 0);
}

#[test]
fn a_step_at_the_queues_end_books_nothing() {
    let world = world();
    world.build(10, 2);
    let held = world.request(ADA, spec());
    world.request(BO, needing("gpu"));
    world.claim(P, held);
    world.to(5);
    world.end(P, held);
    world.to(7);
    for _ in 0..3 {
        assert_eq!(
            world.steps_pending(KIND),
            0,
            "a walk at its queue's end booked a step"
        );
        world.next();
    }
}

#[test]
fn two_capacity_events_in_one_block_book_one_step() {
    let world = world();
    world.build(10, 2);
    let one = world.request(ADA, spec());
    let two = world.request(BO, spec());
    let three = world.request(CY, spec());
    let four = world.request(CY, spec());
    world.claim(P, one);
    world.claim(P, two);
    world.to(5);
    world.end(P, one);
    world.end(P, two);
    assert_eq!(world.steps_at(KIND, 6), 1);
    world.to(7);
    assert_eq!(world.holds(id(P_ACCOUNT, "p")), [three, four]);
}

#[test]
fn a_capacity_event_during_a_walk_joins_it() {
    let world = world();
    world.build(1, 3);
    let held: Vec<u64> = (0..3).map(|_| world.request(ADA, spec())).collect();
    for request in &held {
        world.claim(P, *request);
    }
    world.request(BO, needing("gpu"));
    world.request(BO, needing("gpu"));
    let first = world.request(CY, spec());
    let second = world.request(CY, spec());
    world.to(5);
    world.end(P, held[0]);
    world.to(7);
    world.end(P, held[1]);
    assert_eq!(world.steps_at(KIND, 7), 1);
    world.to(9);
    assert_eq!(world.state(first), RequestState::Placed);
    assert_eq!(world.state(second), RequestState::Queued);
    world.to(10);
    assert_eq!(world.state(second), RequestState::Placed);
}

#[test]
fn a_request_filed_behind_a_running_walk_is_placed_before_the_walk_ends() {
    let world = world();
    world.register(KIND, kind_spec(1, LONG, Vec::new()));
    world.admit(KIND, P_ACCOUNT);
    world.admit(KIND, Q_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 1));
    let mut gpu = offer_of("q", KIND, 1);
    gpu.labels = vec![("gpu".into(), "a100".into())];
    world.offer(Q, gpu);
    let classes = Policy {
        classes: vec![
            Class {
                name: "urgent".into(),
                may_use: None,
                notarized: false,
            },
            Class {
                name: "normal".into(),
                may_use: None,
                notarized: false,
            },
        ],
        ..Policy::default()
    };
    let set = Op::SetPolicy {
        kind: KIND.into(),
        policy: classes,
    };
    world.act(OWNER, &set).unwrap();
    world.settle();
    let class = |name: &str, spec: Spec| Spec {
        class: name.into(),
        ..spec
    };
    let on_p = world.request(CY, class("normal", spec()));
    let on_q = world.request(CY, class("normal", needing("gpu")));
    assert_eq!(world.placed_on(on_p), Some(id(P_ACCOUNT, "p")));
    assert_eq!(world.placed_on(on_q), Some(id(Q_ACCOUNT, "q")));
    world.request(BO, class("normal", needing("tpu")));
    world.request(BO, class("normal", needing("tpu")));
    world.claim(P, on_p);
    world.claim(Q, on_q);
    world.end(P, on_p);
    world.settle();
    assert_eq!(world.steps_at(KIND, world.now()), 1);
    let urgent = world.request(ADA, class("urgent", needing("gpu")));
    assert_eq!(world.state(urgent), RequestState::Queued);
    world.end(Q, on_q);
    world.to(world.now() + 3);
    assert_eq!(world.placed_on(urgent), Some(id(Q_ACCOUNT, "q")));
}

#[test]
fn a_capacity_event_holds_back_no_newcomer_unless_a_queued_request_takes_its_offer() {
    let world = world();
    world.build(10, 3);
    let held = world.request(ADA, spec());
    world.claim(P, held);
    world.to(5);
    world.end(P, held);
    let request = world.request(BO, spec());
    assert_eq!(world.state(request), RequestState::Placed);
    world.act(CY, &Op::Pick { spec: spec() }).unwrap();
    let untakable = world.request(BO, needing("gpu"));
    world.claim(P, request);
    world.end(P, request);
    assert_eq!(world.state(untakable), RequestState::Queued);
    world.act(CY, &Op::Pick { spec: spec() }).unwrap();
    let late = world.request(ADA, spec());
    assert_eq!(world.state(late), RequestState::Placed);
}

#[test]
fn a_lapsed_offer_takes_back_nothing_it_lost() {
    let world = world();
    world.register(KIND, kind_spec(10, 5, Vec::new()));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 2));
    world.settle();
    let request = world.request(ADA, spec());
    world.claim(P, request);
    world.to(9);
    assert_eq!(
        world.offer_view(id(P_ACCOUNT, "p")).offer.state,
        OfferState::Lapsed
    );
    let view = world.view(request);
    let placed = (view.attempt, view.state, world.placed_on(request));
    assert_eq!(placed, (1, RequestState::Queued, None));
}

#[test]
fn a_provider_is_disputed_once_per_assignment_however_often_its_requester_recounts() {
    let world = world();
    let tokens = meter("tokens", Counter::Provider, Some(Counter::Requester));
    world.register(KIND, kind_spec(10, LONG, vec![tokens]));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 1));
    world.settle();
    let request = world.request(ADA, spec());
    world.claim(P, request);
    let report = Op::Report {
        request,
        attempt: 1,
        outcome: Some(Outcome::Passed),
        readings: vec![("tokens".into(), 1000)],
        evidence: None,
    };
    world.act(P, &report).unwrap();
    for tokens in [2000, 1000, 2000, 1000, 2000, 1000] {
        let confirm = Op::Confirm {
            request,
            attempt: 1,
            readings: vec![("tokens".into(), tokens)],
        };
        world.act(ADA, &confirm).unwrap();
    }
    let asked = Query::Record {
        offer: id(P_ACCOUNT, "p"),
    };
    let Reply::Record(Some(record)) = world.ask(asked) else {
        panic!("p has a record");
    };
    assert_eq!(record.disputed, 1);
}

#[test]
fn a_report_that_goes_down_and_up_charges_its_group_once() {
    let world = world();
    let tokens = meter("tokens", Counter::Provider, None);
    world.register(KIND, kind_spec(10, LONG, vec![tokens]));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 1));
    let policy = Policy {
        share: Share {
            meter: Some("tokens".into()),
            window: None,
        },
        ..Policy::default()
    };
    let set = Op::SetPolicy {
        kind: KIND.into(),
        policy,
    };
    world.act(OWNER, &set).unwrap();
    world.settle();
    let request = world.request(ADA, spec());
    world.claim(P, request);
    for count in [100, 0, 100] {
        let report = Op::Report {
            request,
            attempt: 1,
            outcome: None,
            readings: vec![("tokens".into(), count)],
            evidence: None,
        };
        world.act(P, &report).unwrap();
    }
    let asked = Query::Ledger {
        kind: KIND.into(),
        group: String::new(),
        page: PageRequest::default(),
    };
    let Reply::Ledger(page) = world.ask(asked) else {
        panic!("Ledger answers rows");
    };
    let charged: u64 = page
        .items
        .iter()
        .filter(|row| row.meter == "tokens")
        .map(|row| row.used)
        .sum();
    assert_eq!(charged, 100);
}

#[test]
fn a_request_is_placed_in_its_own_frame_unless_a_queued_request_takes_its_offer() {
    let world = world();
    world.build(10, 1);
    let first = world.request(ADA, spec());
    assert_eq!(world.placed_on(first), Some(id(P_ACCOUNT, "p")));
    let queued = world.request(BO, spec());
    world.claim(P, first);
    world.to(5);
    world.end(P, first);
    let late = world.request(CY, spec());
    assert_eq!(world.state(late), RequestState::Queued);
    world.to(7);
    assert_eq!(world.placed_on(queued), Some(id(P_ACCOUNT, "p")));
    assert_eq!(world.state(late), RequestState::Queued);
}

#[test]
fn pick_places_in_its_frame_or_is_refused_saying_why() {
    let world = world();
    world.build(10, 1);
    let made = world.act(ADA, &Op::Pick { spec: spec() }).unwrap();
    let placement: Placement = abi::decode(&made).unwrap();
    assert_eq!(placement.offer, id(P_ACCOUNT, "p"));
    let picked = world
        .assignment(placement.request, placement.attempt)
        .unwrap();
    assert_eq!(picked.state, AssignState::Claimed);
    let refused = world.act(BO, &Op::Pick { spec: spec() }).unwrap_err();
    assert_eq!(refused.code, code::WRONG_STATE);
}

#[test]
fn a_walk_places_by_class_then_fair_share_then_age() {
    let world = world();
    world.build(10, 1);
    let ada = world.policy_of(&[ADA_ACCOUNT]);
    let bo = world.policy_of(&[BO_ACCOUNT]);
    let policy = Policy {
        classes: vec![
            Class {
                name: "urgent".into(),
                may_use: None,
                notarized: false,
            },
            Class {
                name: "normal".into(),
                may_use: None,
                notarized: false,
            },
        ],
        groups: vec![
            Group {
                name: "g1".into(),
                members: ada,
                weight: 1,
            },
            Group {
                name: "g2".into(),
                members: bo,
                weight: 2,
            },
        ],
        quotas: Vec::new(),
        share: Share::default(),
        tolerance_ppm: 0,
    };
    world
        .act(
            OWNER,
            &Op::SetPolicy {
                kind: KIND.into(),
                policy,
            },
        )
        .unwrap();
    world.settle();
    let class = |name: &str| Spec {
        class: name.into(),
        ..spec()
    };
    let filler = world.request(CY, class("normal"));
    let a1 = world.request(ADA, class("normal"));
    let b1 = world.request(BO, class("normal"));
    let a2 = world.request(ADA, class("normal"));
    let b2 = world.request(BO, class("normal"));
    let urgent = world.request(BO, class("urgent"));
    let mut running = filler;
    for (expected, group) in [
        (urgent, "g2"),
        (a1, "g1"),
        (b1, "g2"),
        (a2, "g1"),
        (b2, "g2"),
    ] {
        world.claim(P, running);
        world.end(P, running);
        world.to(world.now() + 2);
        assert_eq!(world.holds(id(P_ACCOUNT, "p")), [expected]);
        let reason = world.why(expected).assignment.unwrap().reason;
        assert_eq!(reason.group, group);
        running = expected;
    }
}

#[test]
fn a_group_over_quota_waits_for_its_window() {
    let world = world();
    world.build(10, 2);
    let ada = world.policy_of(&[ADA_ACCOUNT]);
    let policy = Policy {
        groups: vec![Group {
            name: "g1".into(),
            members: ada,
            weight: 1,
        }],
        quotas: vec![Quota {
            group: "g1".into(),
            meter: String::new(),
            window: 10,
            most: 1,
        }],
        ..Policy::default()
    };
    world
        .act(
            OWNER,
            &Op::SetPolicy {
                kind: KIND.into(),
                policy,
            },
        )
        .unwrap();
    world.settle();
    let first = world.request(ADA, spec());
    assert_eq!(world.state(first), RequestState::Placed);
    let over = world.request(ADA, spec());
    assert_eq!(world.state(over), RequestState::Queued);
    let waiting = world.why(over).waiting;
    assert!(
        waiting.contains(&Waiting::OverQuota {
            group: "g1".into(),
            meter: String::new(),
            until: 10,
        }),
        "{waiting:?}"
    );
    assert_eq!(world.steps_at(KIND, 10), 1);
    world.to(10);
    assert_eq!(world.state(over), RequestState::Queued);
    world.to(11);
    assert_eq!(world.state(over), RequestState::Placed);
}

#[test]
fn a_program_asks_as_the_account_it_runs() {
    let world = world();
    world.build(10, 1);
    let ask = Through::AskAs {
        account: ROBOT_ACCOUNT,
        spec: spec(),
    };
    world.through(&ask).unwrap();
    let asked = Query::Queued {
        kind: KIND.into(),
        page: PageRequest::default(),
    };
    let placed = world.offer_view(id(P_ACCOUNT, "p")).offer.busy;
    assert_eq!(placed, 1);
    let held = world.holds(id(P_ACCOUNT, "p"));
    let row = world.view(held[0]).request;
    assert_eq!(
        (row.by, row.via),
        (ROBOT_ACCOUNT, Some(ProgramId::from(CLIENT)))
    );
    let not_run = Through::AskAs {
        account: ADA_ACCOUNT,
        spec: spec(),
    };
    let refused = world.through(&not_run).unwrap_err();
    assert_eq!(refused.code, code::UNAUTHORIZED);
    let Reply::Requests(queued) = world.ask(asked) else {
        panic!("Queued answers requests");
    };
    assert!(queued.items.is_empty());
    let signed = world.request(BO, spec());
    let row = world.view(signed).request;
    assert_eq!((row.by, row.via), (BO_ACCOUNT, None));
}

#[test]
fn reports_without_an_outcome_replace_counts_and_only_an_outcome_ends() {
    let world = world();
    let tokens = meter("tokens", Counter::Provider, Some(Counter::Requester));
    world.register(KIND, kind_spec(10, LONG, vec![tokens]));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 1));
    world.settle();
    let request = world.request(ADA, spec());
    world.claim(P, request);
    let report = |outcome: Option<Outcome>, tokens: u64| Op::Report {
        request,
        attempt: 1,
        outcome,
        readings: vec![("tokens".into(), tokens)],
        evidence: None,
    };
    for count in [5, 9] {
        world.act(P, &report(None, count)).unwrap();
        assert_eq!(world.usage(request, 1).provider, [("tokens".into(), count)]);
        let held = world.assignment(request, 1).unwrap().state;
        assert_eq!(held, AssignState::Claimed);
        assert_eq!(world.offer_view(id(P_ACCOUNT, "p")).offer.busy, 1);
    }
    world.act(P, &report(Some(Outcome::Passed), 12)).unwrap();
    let ended = world.assignment(request, 1).unwrap().state;
    assert_eq!(ended, AssignState::Ended(Outcome::Passed));
    assert_eq!(world.usage(request, 1).provider, [("tokens".into(), 12)]);
    assert_eq!(world.offer_view(id(P_ACCOUNT, "p")).offer.busy, 0);
    assert_eq!(world.state(request), RequestState::Ended);
    let after = world.act(P, &report(None, 13)).unwrap_err();
    assert_eq!(after.code, code::WRONG_STATE);
    let other = world.act(Q, &report(None, 1)).unwrap_err();
    assert_eq!(other.code, code::WRONG_STATE);
}

#[test]
fn a_checked_meter_is_confirmed_within_tolerance_and_disputed_past_it() {
    let world = world();
    let tokens = meter("tokens", Counter::Provider, Some(Counter::Requester));
    world.register(KIND, kind_spec(10, LONG, vec![tokens]));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 2));
    let policy = Policy {
        tolerance_ppm: 10_000,
        ..Policy::default()
    };
    let set = Op::SetPolicy {
        kind: KIND.into(),
        policy,
    };
    world.act(OWNER, &set).unwrap();
    world.settle();
    let first = world.request(ADA, spec());
    let second = world.request(ADA, spec());
    world.claim(P, first);
    world.claim(P, second);
    let report = Op::Report {
        request: first,
        attempt: 1,
        outcome: Some(Outcome::Passed),
        readings: vec![("tokens".into(), 1000)],
        evidence: None,
    };
    world.act(P, &report).unwrap();
    assert_eq!(world.usage(first, 1).standing, Standing::Attested);
    let confirm = |request: u64, tokens: u64| Op::Confirm {
        request,
        attempt: 1,
        readings: vec![("tokens".into(), tokens)],
    };
    world.act(ADA, &confirm(first, 1005)).unwrap();
    assert_eq!(world.usage(first, 1).standing, Standing::Confirmed);
    world.act(ADA, &confirm(first, 1100)).unwrap();
    let disputed = Standing::Disputed {
        meters: vec!["tokens".into()],
    };
    assert_eq!(world.usage(first, 1).standing, disputed);
    assert_eq!(world.usage(first, 1).requester, [("tokens".into(), 1100)]);
    let stranger = world.act(BO, &confirm(first, 1000)).unwrap_err();
    assert_eq!(stranger.code, code::UNAUTHORIZED);
    world.act(ADA, &confirm(second, 40)).unwrap();
    assert_eq!(world.usage(second, 1).standing, Standing::Claimed);

    let calls = meter("calls", Counter::Chain, None);
    world.register("service", kind_spec(10, LONG, vec![calls]));
    world.admit("service", Q_ACCOUNT);
    world.offer(Q, offer_of("q", "service", 1));
    world.settle();
    let ask = Through::Send(Op::Request {
        spec: Spec {
            kind: "service".into(),
            ..spec()
        },
    });
    world.through(&ask).unwrap();
    let request = world.holds(id(Q_ACCOUNT, "q"))[0];
    world.claim(Q, request);
    let count = Op::Count {
        request,
        attempt: 1,
        outcome: Some(Outcome::Passed),
        readings: vec![("calls".into(), 1)],
    };
    let not_via = world.act(ADA, &count).unwrap_err();
    assert_eq!(not_via.code, code::UNAUTHORIZED);
    world.through(&Through::Send(count)).unwrap();
    let counted = world.usage(request, 1);
    assert_eq!(counted.standing, Standing::Counted);
    assert_eq!(counted.chain, [("calls".into(), 1)]);
}

fn full() -> (World, u64, u64) {
    let world = world();
    world.register(KIND, kind_spec(10, LONG, Vec::new()));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 1));
    world.settle();
    let held = world.request(ADA, spec());
    world.claim(P, held);
    let queued = world.request(BO, spec());
    world.to(5);
    assert_eq!(world.steps_next(KIND), 0);
    (world, held, queued)
}

#[test]
fn a_decline_books_a_step_and_never_goes_back_to_that_offer() {
    let (world, held, queued) = full();
    let decline = Op::Decline {
        request: held,
        attempt: 1,
        why: "no room".into(),
    };
    world.act(P, &decline).unwrap();
    assert_eq!(world.steps_next(KIND), 1);
    world.to(7);
    assert_eq!(world.placed_on(queued), Some(id(P_ACCOUNT, "p")));
    world.claim(P, queued);
    world.end(P, queued);
    world.to(10);
    assert_eq!(world.state(held), RequestState::Queued);
    let passed = Query::PassedOver {
        request: held,
        page: PageRequest::default(),
    };
    let Reply::PassedOver(page) = world.ask(passed) else {
        panic!("PassedOver answers rows");
    };
    let row = PassedOverRow {
        offer: id(P_ACCOUNT, "p"),
        passed: Passed::Declined,
    };
    assert_eq!(page.items, [row]);
}

#[test]
fn a_new_offer_and_an_admit_each_book_a_step() {
    let (world, _, queued) = full();
    world.offer(Q, offer_of("q", KIND, 1));
    assert_eq!(world.steps_next(KIND), 1);
    world.to(7);
    assert_eq!(world.state(queued), RequestState::Queued);
    world.admit(KIND, Q_ACCOUNT);
    assert_eq!(world.steps_next(KIND), 1);
    world.to(9);
    assert_eq!(world.placed_on(queued), Some(id(Q_ACCOUNT, "q")));
}

#[test]
fn a_set_policy_books_a_step() {
    let (world, _, _) = full();
    let set = Op::SetPolicy {
        kind: KIND.into(),
        policy: Policy::default(),
    };
    world.act(OWNER, &set).unwrap();
    assert_eq!(world.steps_next(KIND), 1);
}

#[test]
fn a_count_with_an_outcome_ends_the_assignment_and_books_a_step() {
    let world = world();
    world.build(10, 1);
    world
        .through(&Through::Send(Op::Request { spec: spec() }))
        .unwrap();
    let held = world.holds(id(P_ACCOUNT, "p"))[0];
    world.claim(P, held);
    let queued = world.request(BO, spec());
    world.to(5);
    let count = Op::Count {
        request: held,
        attempt: 1,
        outcome: Some(Outcome::Passed),
        readings: Vec::new(),
    };
    world.through(&Through::Send(count)).unwrap();
    assert_eq!(world.state(held), RequestState::Ended);
    assert_eq!(world.steps_next(KIND), 1);
    world.to(7);
    assert_eq!(world.placed_on(queued), Some(id(P_ACCOUNT, "p")));
}

#[test]
fn a_missed_offer_waits_for_its_beat_and_the_request_goes_by_on_loss() {
    let world = world();
    world.build(10, 1);
    let parked = Spec {
        on_loss: OnLoss::Park,
        ..spec()
    };
    world
        .through(&Through::Send(Op::Request { spec: parked }))
        .unwrap();
    let held = world.holds(id(P_ACCOUNT, "p"))[0];
    world.claim(P, held);
    let queued = world.request(BO, spec());
    world.to(5);
    let missed = Op::Missed {
        request: held,
        attempt: 1,
    };
    let not_via = world.act(ADA, &missed).unwrap_err();
    assert_eq!(not_via.code, code::UNAUTHORIZED);
    world.through(&Through::Send(missed)).unwrap();
    assert_eq!(world.state(held), RequestState::Lost);
    let offer = world.offer_view(id(P_ACCOUNT, "p")).offer;
    assert_eq!((offer.state, offer.busy), (OfferState::Missed, 0));
    assert_eq!(world.steps_next(KIND), 1);
    world.to(7);
    assert_eq!(world.state(queued), RequestState::Queued);
    let beat = Op::Beat {
        name: "p".into(),
        free: 1,
        readings: Vec::new(),
    };
    world.act(P, &beat).unwrap();
    assert_eq!(
        world.offer_view(id(P_ACCOUNT, "p")).offer.state,
        OfferState::Ready
    );
    assert_eq!(world.steps_next(KIND), 1);
    world.to(9);
    assert_eq!(world.placed_on(queued), Some(id(P_ACCOUNT, "p")));
}

#[test]
fn an_unclaimed_assignment_misses_its_offer_and_moves() {
    let world = world();
    world.register(KIND, kind_spec(10, 5, Vec::new()));
    world.admit(KIND, P_ACCOUNT);
    world.admit(KIND, Q_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 1));
    world.settle();
    let parked = Spec {
        on_loss: OnLoss::Park,
        ..spec()
    };
    let request = world.request(ADA, parked);
    assert_eq!(world.placed_on(request), Some(id(P_ACCOUNT, "p")));
    world.offer(Q, offer_of("q", KIND, 1));
    for height in [6, 8] {
        world.to(height);
        for (key, name) in [(P, "p"), (Q, "q")] {
            let beat = Op::Beat {
                name: name.into(),
                free: 1,
                readings: Vec::new(),
            };
            world.act(key, &beat).unwrap();
        }
    }
    world.to(9);
    assert_eq!(world.state(request), RequestState::Placed);
    assert_eq!(world.view(request).attempt, 1);
    world.to(10);
    assert_eq!(world.view(request).attempt, 2);
    assert_eq!(world.placed_on(request), Some(id(Q_ACCOUNT, "q")));
    let first = world.assignment(request, 1).unwrap().state;
    assert_eq!(first, AssignState::Missed);
    assert_eq!(
        world.offer_view(id(P_ACCOUNT, "p")).offer.state,
        OfferState::Missed
    );
}

#[test]
fn a_silent_offer_lapses_and_its_requests_go_by_on_loss() {
    let world = world();
    world.register(KIND, kind_spec(10, 5, Vec::new()));
    world.admit(KIND, P_ACCOUNT);
    world.admit(KIND, Q_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 2));
    world.settle();
    let retried = world.request(ADA, spec());
    let parked = world.request(
        BO,
        Spec {
            on_loss: OnLoss::Park,
            ..spec()
        },
    );
    world.claim(P, retried);
    world.claim(P, parked);
    world.offer(Q, offer_of("q", KIND, 1));
    for height in [4, 6] {
        world.to(height);
        let beat = Op::Beat {
            name: "q".into(),
            free: 1,
            readings: Vec::new(),
        };
        world.act(Q, &beat).unwrap();
    }
    world.to(8);
    assert_eq!(
        world.offer_view(id(P_ACCOUNT, "p")).offer.state,
        OfferState::Lapsed
    );
    assert_eq!(world.state(parked), RequestState::Lost);
    assert_eq!(world.view(retried).attempt, 2);
    assert_eq!(world.placed_on(retried), Some(id(Q_ACCOUNT, "q")));
}

#[test]
fn an_evicted_provider_takes_nothing_and_loses_what_it_held() {
    let world = world();
    world.register(KIND, kind_spec(10, LONG, Vec::new()));
    world.admit(KIND, P_ACCOUNT);
    world.offer(P, offer_of("p", KIND, 2));
    world.settle();
    let held = world.request(ADA, spec());
    world.claim(P, held);
    let evict = Op::Evict {
        kind: KIND.into(),
        provider: P_ACCOUNT,
    };
    world.act(OWNER, &evict).unwrap();
    let after = world.request(BO, spec());
    assert_eq!(world.state(after), RequestState::Queued);
    assert_eq!(world.state(held), RequestState::Placed);
    world.admit(KIND, Q_ACCOUNT);
    world.offer(Q, offer_of("q", KIND, 2));
    world.to(world.now() + 2);
    assert_eq!(world.placed_on(held), Some(id(Q_ACCOUNT, "q")));
    assert_eq!(world.view(held).attempt, 2);
    assert_eq!(world.placed_on(after), Some(id(Q_ACCOUNT, "q")));
}

#[test]
fn an_evicted_provider_admitted_again_takes_work_without_a_beat() {
    let world = world();
    world.build(10, 1);
    let evict = Op::Evict {
        kind: KIND.into(),
        provider: P_ACCOUNT,
    };
    world.act(OWNER, &evict).unwrap();
    world.settle();
    let offer = world.offer_view(id(P_ACCOUNT, "p"));
    assert_eq!(
        (offer.offer.state, offer.admitted),
        (OfferState::Ready, false)
    );
    world.admit(KIND, P_ACCOUNT);
    let request = world.request(ADA, spec());
    assert_eq!(world.placed_on(request), Some(id(P_ACCOUNT, "p")));
}

#[test]
fn a_notarized_request_waits_for_a_notary_of_another_provider() {
    let world = world();
    world.register(NOTARY, kind_spec(10, LONG, Vec::new()));
    let mut notarized_kind = kind_spec(10, LONG, Vec::new());
    notarized_kind.notary = Some(NOTARY.into());
    world.register(KIND, notarized_kind);
    world.admit(KIND, P_ACCOUNT);
    world.admit(NOTARY, P_ACCOUNT);
    world.admit(NOTARY, N_ACCOUNT);
    let mut notarizing = offer_of("p", KIND, 1);
    notarizing.terms.notarizes = true;
    world.offer(P, notarizing);
    let mut asked = spec();
    asked.needs.notarized = true;
    let request = world.request(ADA, asked);
    assert_eq!(world.state(request), RequestState::Queued);
    let waiting = world.why(request).waiting;
    let no_notary = Waiting::NoNotary {
        kind: NOTARY.into(),
    };
    assert!(waiting.contains(&no_notary), "{waiting:?}");
    world.offer(P, offer_of("own", NOTARY, 1));
    world.to(world.now() + 2);
    assert_eq!(world.state(request), RequestState::Queued);
    world.offer(N, offer_of("n", NOTARY, 1));
    assert_eq!(world.steps_next(KIND), 1);
    world.to(world.now() + 2);
    let placed = world.assignment(request, 1).unwrap();
    assert_eq!(placed.offer, id(P_ACCOUNT, "p"));
    assert_eq!(placed.notary, Some(id(N_ACCOUNT, "n")));
}

#[test]
fn notarizing_a_kind_needs_the_notary_kinds_consent() {
    let world = world();
    let ada = world.policy_of(&[ADA_ACCOUNT]);
    let default = ownership::Op::SetDefault {
        scope: ownership::Scope::Kind("resources.kind".into()),
        policy: Some(ada),
    };
    world
        .chain
        .submit(Origin::Signed(OWNER.to_vec()), ownership::PROGRAM, &default)
        .unwrap();
    world.register(NOTARY, kind_spec(10, LONG, Vec::new()));
    let mut notarized = kind_spec(10, LONG, Vec::new());
    notarized.notary = Some(NOTARY.into());
    let stranger = Op::RegisterKind {
        name: "bo.kind".into(),
        spec: notarized.clone(),
    };
    assert_eq!(
        world.act(BO, &stranger).unwrap_err().code,
        code::UNAUTHORIZED
    );
    let Reply::Kind(none) = world.ask(Query::Kind {
        name: "bo.kind".into(),
    }) else {
        panic!("Kind answers a kind");
    };
    assert_eq!(none, None);
    let consented = Op::RegisterKind {
        name: "ada.kind".into(),
        spec: notarized,
    };
    world.act(ADA, &consented).unwrap();
    let Reply::Kind(Some(row)) = world.ask(Query::Kind {
        name: "ada.kind".into(),
    }) else {
        panic!("the kind is there");
    };
    assert_eq!(row.owner, ADA_ACCOUNT);
    let plain = Op::RegisterKind {
        name: "bo.plain".into(),
        spec: kind_spec(10, LONG, Vec::new()),
    };
    world.act(BO, &plain).unwrap();
}

#[test]
fn one_notary_beat_books_one_step_for_each_kind_it_notarizes() {
    let world = world();
    world.register(NOTARY, kind_spec(10, 5, Vec::new()));
    let notarized: Vec<String> = (0..3).map(|at| format!("kind-{at}")).collect();
    for name in &notarized {
        let mut spec = kind_spec(10, LONG, Vec::new());
        spec.notary = Some(NOTARY.into());
        world.register(name, spec);
    }
    world.register("other", kind_spec(10, LONG, Vec::new()));
    world.admit(NOTARY, N_ACCOUNT);
    world.offer(N, offer_of("n", NOTARY, 1));
    world.to(8);
    assert_eq!(
        world.offer_view(id(N_ACCOUNT, "n")).offer.state,
        OfferState::Lapsed
    );
    let beat = Op::Beat {
        name: "n".into(),
        free: 1,
        readings: Vec::new(),
    };
    world.act(N, &beat).unwrap();
    let booked: usize = notarized.iter().map(|name| world.steps_at(name, 9)).sum();
    assert_eq!(booked, notarized.len());
    assert_eq!(world.steps_at("other", 9), 0);
    assert_eq!(world.steps_at(NOTARY, 9), 1);
}

#[test]
fn a_kind_is_refused_a_zero_walk_a_self_checked_meter_and_a_stranger() {
    let world = world();
    let zero = Op::RegisterKind {
        name: KIND.into(),
        spec: kind_spec(0, LONG, Vec::new()),
    };
    assert_eq!(
        world.act(OWNER, &zero).unwrap_err().code,
        code::INVALID_INPUT
    );
    let looped = meter("tokens", Counter::Provider, Some(Counter::Provider));
    let looped = Op::RegisterKind {
        name: KIND.into(),
        spec: kind_spec(10, LONG, vec![looped]),
    };
    assert_eq!(
        world.act(OWNER, &looped).unwrap_err().code,
        code::INVALID_INPUT
    );
    world.register(KIND, kind_spec(10, LONG, Vec::new()));
    let taken = Op::RegisterKind {
        name: KIND.into(),
        spec: kind_spec(5, LONG, Vec::new()),
    };
    assert_eq!(world.act(ADA, &taken).unwrap_err().code, code::UNAUTHORIZED);
    let admit = Op::Admit {
        kind: KIND.into(),
        provider: P_ACCOUNT,
    };
    assert_eq!(world.act(ADA, &admit).unwrap_err().code, code::UNAUTHORIZED);
    let Reply::Kind(Some(row)) = world.ask(Query::Kind { name: KIND.into() }) else {
        panic!("the kind is there");
    };
    assert_eq!((row.owner, row.spec.walk), (OWNER_ACCOUNT, 10));
}
