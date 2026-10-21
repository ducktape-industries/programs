//! A place's hold: Take grants it while no one else holds or drafts in that exact place, its
//! holder's post, Release, Break, an archive or an unseat ends it, Busy names who else is there, and
//! each end is told to the programs watching places; checked against a model from a fixed seed.
use std::collections::{BTreeMap, BTreeSet};

use proptest::collection::{SizeRange, vec};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRng, TestRunner};

use super::*;
use crate::state::{DRAFT_PIECES, DRAFTS, LEASES, PLACE_WATCHERS};
use crate::{Holder, Holding, PlaceEvent, dm_channel_id};

const DELIVER: u8 = 7;
const PEOPLE: [Principal; 3] = [ADA, BO, CY];
const TEAM: &str = "team";
const THREADS: [Option<u64>; 3] = [None, Some(1), Some(2)];

fn rooms() -> [String; 3] {
    ["general".into(), TEAM.into(), dm_channel_id(1, 2)]
}

fn take(channel: &str, thread: Option<u64>) -> Op {
    Op::Take {
        channel_id: channel.into(),
        thread,
    }
}

fn release(channel: &str, thread: Option<u64>) -> Op {
    Op::Release {
        channel_id: channel.into(),
        thread,
    }
}

fn clear(channel: &str, thread: Option<u64>) -> Op {
    Op::Break {
        channel_id: channel.into(),
        thread,
    }
}

fn drafting(channel: &str, thread: Option<u64>) -> Op {
    Op::Drafting {
        channel_id: channel.into(),
        thread,
        text: "x".into(),
        anew: false,
        doing: None,
    }
}

fn undraft(channel: &str) -> Op {
    Op::Undraft {
        channel_id: channel.into(),
    }
}

fn watch(deliver: Vec<u8>) -> Op {
    Op::WatchPlaces { deliver }
}

fn seat_in(channel: &str, who: &Principal, member: bool) -> Op {
    Op::SetMembership {
        channel_id: channel.into(),
        principal: who.clone(),
        member,
    }
}

fn seat(who: &Principal, member: bool) -> Op {
    seat_in(TEAM, who, member)
}

fn archive(archived: bool) -> Op {
    Op::SetChannelArchived {
        channel_id: "general".into(),
        archived,
    }
}

fn number(who: &Principal) -> u64 {
    who.account().expect("people hold accounts")
}

impl Chat {
    fn rooted() -> Chat {
        let mut chat = Chat::with_channel(PostPolicy::Open);
        chat.ok(&ADA, create(TEAM, PostPolicy::MembersOnly));
        chat.ok(&ADA, seat(&BO, true));
        let dm = Op::CreateDmChannel {
            counterpart: 2,
            name: "ada and bo".into(),
        };
        chat.ok(&ADA, dm);
        for room in rooms() {
            for root in 1..=2 {
                chat.ok(&ADA, post(&room, &format!("{room}-r{root}"), "root", None));
            }
        }
        chat
    }

    fn busy(
        &self,
        channel: &str,
        thread: Option<u64>,
        who: &Principal,
    ) -> Option<(Principal, Holding)> {
        let asked = Query::Busy {
            channel_id: channel.into(),
            thread,
            for_account: number(who),
        };
        let Reply::Busy(holder) = self.ask(asked) else {
            panic!("Busy answers a holder");
        };
        holder.map(
            |Holder {
                 principal, kind, ..
             }| (principal, kind),
        )
    }

    fn told(&self) -> Vec<(String, Option<u64>)> {
        let emitted = self.store.take_emissions().into_iter();
        let told = emitted.filter(|told| told.target == "forge");
        told.map(|told| {
            assert!(told.reply, "the watcher's answer comes back to chat");
            let (deliver, event) = told.payload.split_first().expect("deliver leads");
            assert_eq!(*deliver, DELIVER);
            let event: PlaceEvent = borsh::from_slice(event).expect("an event follows deliver");
            let PlaceEvent::Cleared { channel_id, thread } = event;
            (channel_id, thread)
        })
        .collect()
    }
}

fn at(channel: &str, thread: Option<u64>) -> (String, Option<u64>) {
    (channel.to_owned(), thread)
}

#[test]
fn holding_and_place_event_variants_only_append() {
    assert_eq!(describe::variants::<Holding>(), ["Lease", "Draft"]);
    assert_eq!(describe::variants::<PlaceEvent>(), ["Cleared"]);
}

#[test]
fn a_hold_is_taken_where_its_taker_may_post_under_a_threads_root() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    assert_eq!(chat.refused(&BO, take("general", None)), code::UNAUTHORIZED);
    chat.post(&ADA, "a1", "root", None);
    chat.post(&ADA, "a2", "reply", Some(1));
    chat.ok(&ADA, take("general", Some(2)));
    assert_eq!(
        chat.busy("general", Some(1), &BO),
        Some((ADA, Holding::Lease))
    );
    let own = chat.busy("general", Some(1), &ADA);
    assert_eq!(own, None, "the holder is not busy for itself");
    let foot = chat.busy("general", None, &BO);
    assert_eq!(foot, None, "a thread is not its channel's foot");
    chat.ok(&ADA, release("general", Some(2)));
    let let_go = chat.busy("general", Some(1), &BO);
    assert_eq!(let_go, None, "a release under a reply lets go of its root");
}

#[test]
fn holders_pages_the_hold_then_every_draft_in_that_exact_place() {
    let mut chat = Chat::rooted();
    let holders = |chat: &Chat, thread| {
        let mut named = Vec::new();
        let mut after = None;
        loop {
            let asked = Query::Holders {
                channel_id: "general".into(),
                thread,
                page: crate::PageRequest::resume(after, 1),
            };
            let Reply::Holders(page) = chat.ask(asked) else {
                panic!("Holders answers holders");
            };
            assert!(page.items.len() <= 1, "a page holds at most its limit");
            let holders = page.items.into_iter();
            named.extend(holders.map(|holder| (holder.principal, holder.kind)));
            match page.next {
                Some(next) => after = Some(next),
                None => return named,
            }
        }
    };
    assert!(holders(&chat, None).is_empty(), "no one is there");
    chat.ok(&BO, take("general", None));
    chat.ok(&ADA, drafting("general", None));
    chat.ok(&CY, drafting("general", Some(1)));
    let foot = holders(&chat, None);
    assert_eq!(foot[0], (BO, Holding::Lease), "the hold comes first");
    assert_eq!(foot.len(), 2, "{foot:?}");
    assert!(foot.contains(&(ADA, Holding::Draft)), "{foot:?}");
    assert_eq!(holders(&chat, Some(1)), [(CY, Holding::Draft)]);
    chat.ok(&BO, release("general", None));
    assert_eq!(holders(&chat, None), [(ADA, Holding::Draft)]);
}

#[test]
fn a_busy_refusal_names_who_is_there_and_how() {
    let mut chat = Chat::rooted();
    chat.ok(&BO, take("general", None));
    let refusal = format!("{:?}", chat.run(&CY, take("general", None)).unwrap_err());
    assert!(refusal.contains("busy: acct:2 holds #general"), "{refusal}");
    chat.ok(&ADA, drafting("general", Some(1)));
    let refusal = format!("{:?}", chat.run(&CY, take("general", Some(1))).unwrap_err());
    let expected = "busy: acct:1 is drafting in thread 1 of #general";
    assert!(refusal.contains(expected), "{refusal}");
    chat.ok(&ADA, take("general", Some(1)));
    assert_eq!(
        chat.busy("general", Some(1), &CY),
        Some((ADA, Holding::Lease))
    );
}

#[test]
fn a_holder_takes_its_own_hold_again_while_another_drafts_there() {
    let mut chat = Chat::rooted();
    chat.ok(&BO, take("general", None));
    chat.ok(&ADA, drafting("general", None));
    chat.ok(&BO, take("general", None));
    assert_eq!(chat.busy("general", None, &CY), Some((BO, Holding::Lease)));
    assert_eq!(chat.busy("general", None, &BO), Some((ADA, Holding::Draft)));
}

#[test]
fn a_hold_never_refuses_a_post_and_its_holders_post_there_lets_go() {
    let mut chat = Chat::rooted();
    chat.ok(&BO, take("general", Some(1)));
    chat.post(&CY, "c1", "meanwhile", Some(1));
    chat.post(&BO, "b1", "elsewhere", None);
    let held = chat.busy("general", Some(1), &CY);
    assert_eq!(
        held,
        Some((BO, Holding::Lease)),
        "a post elsewhere keeps it"
    );
    chat.post(&BO, "b2", "the answer", Some(1));
    assert_eq!(chat.busy("general", Some(1), &CY), None);
}

#[test]
fn release_lets_go_of_the_senders_own_hold_alone() {
    let mut chat = Chat::rooted();
    chat.ok(&BO, take("general", None));
    chat.ok(&CY, release("general", None));
    assert_eq!(chat.busy("general", None, &CY), Some((BO, Holding::Lease)));
    chat.ok(&BO, release("general", None));
    chat.ok(&BO, release("general", None));
    assert_eq!(chat.busy("general", None, &CY), None);
}

#[test]
fn an_unseat_ends_the_holds_and_draft_of_whom_it_takes_posting_from_and_tells_watchers() {
    let mut chat = Chat::rooted();
    chat.ok(&FORGE, watch(vec![DELIVER]));
    chat.ok(&BO, take(TEAM, None));
    chat.ok(&BO, drafting(TEAM, Some(1)));
    chat.ok(&ADA, take(TEAM, Some(2)));
    chat.told();
    chat.ok(&ADA, seat(&BO, false));
    assert_eq!(chat.told(), [at(TEAM, None), at(TEAM, Some(1))]);
    assert_eq!(chat.busy(TEAM, None, &ADA), None);
    assert_eq!(chat.busy(TEAM, Some(1), &ADA), None);
    let kept = chat.busy(TEAM, Some(2), &BO);
    assert_eq!(kept, Some((ADA, Holding::Lease)), "another's hold stays");
    chat.ok(&ADA, seat(&BO, true));
    assert_eq!(
        chat.busy(TEAM, None, &ADA),
        None,
        "a reseat brings nothing back"
    );
    assert_eq!(chat.busy(TEAM, Some(1), &ADA), None);
    chat.ok(&BO, take("general", None));
    chat.told();
    chat.ok(&ADA, seat_in("general", &BO, false));
    assert!(chat.told().is_empty(), "an open room still admits them");
    assert_eq!(chat.busy("general", None, &CY), Some((BO, Holding::Lease)));
}

#[test]
fn an_archive_ends_every_hold_and_draft_in_the_room_and_tells_watchers() {
    let mut chat = Chat::rooted();
    chat.ok(&FORGE, watch(vec![DELIVER]));
    chat.ok(&BO, take("general", None));
    chat.ok(&CY, drafting("general", Some(1)));
    chat.ok(&BO, take(TEAM, None));
    chat.told();
    chat.ok(&ADA, archive(true));
    assert_eq!(chat.told(), [at("general", None), at("general", Some(1))]);
    chat.ok(&ADA, archive(false));
    for who in &PEOPLE {
        for thread in THREADS {
            assert_eq!(
                chat.busy("general", thread, who),
                None,
                "an unarchive brings nothing back"
            );
        }
    }
    let other = chat.busy(TEAM, None, &ADA);
    assert_eq!(
        other,
        Some((BO, Holding::Lease)),
        "another room keeps its holds"
    );
}

#[test]
fn break_ends_a_places_hold_and_drafts_and_tells_watchers_once() {
    let mut chat = Chat::rooted();
    chat.ok(&FORGE, watch(vec![DELIVER]));
    chat.ok(&BO, take("general", Some(1)));
    chat.ok(&CY, drafting("general", Some(1)));
    chat.ok(&ADA, drafting("general", None));
    chat.told();
    chat.ok(&ADA, clear("general", Some(1)));
    assert_eq!(chat.told(), [at("general", Some(1))]);
    for who in &PEOPLE {
        assert_eq!(chat.busy("general", Some(1), who), None);
    }
    let foot = chat.busy("general", None, &BO);
    assert_eq!(
        foot,
        Some((ADA, Holding::Draft)),
        "another place keeps its draft"
    );
    chat.ok(&ADA, clear("general", Some(1)));
    assert!(chat.told().is_empty(), "an empty place ends nothing");
}

#[test]
fn only_a_program_watches_places_until_an_empty_deliver_stops_it() {
    let mut chat = Chat::rooted();
    chat.refused(&ADA, watch(vec![DELIVER]));
    chat.ok(&FORGE, watch(vec![DELIVER]));
    chat.ok(&BO, take("general", None));
    assert!(chat.told().is_empty(), "taking a hold ends none");
    chat.ok(&BO, release("general", None));
    assert_eq!(chat.told(), [at("general", None)]);
    chat.ok(&FORGE, watch(Vec::new()));
    chat.ok(&BO, take("general", None));
    chat.ok(&BO, release("general", None));
    assert!(chat.told().is_empty(), "a stopped watcher hears nothing");
}

#[derive(Clone, Copy, Debug)]
enum Act {
    Take,
    Release,
    Draft,
    Undraft,
    Post,
    Break,
    Archive(bool),
    Seat(bool),
}

#[derive(Clone, Copy, Debug)]
struct Step {
    who: usize,
    room: usize,
    thread: usize,
    act: Act,
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let act = prop_oneof![
        Just(Act::Take),
        Just(Act::Release),
        Just(Act::Draft),
        Just(Act::Undraft),
        Just(Act::Post),
        Just(Act::Break),
        any::<bool>().prop_map(Act::Archive),
        any::<bool>().prop_map(Act::Seat),
    ];
    let step = (0..PEOPLE.len(), 0..rooms().len(), 0..THREADS.len(), act);
    let step = step.prop_map(|(who, room, thread, act)| Step {
        who,
        room,
        thread,
        act,
    });
    vec(step, SizeRange::default())
}

fn runner() -> TestRunner {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    let rng = TestRng::deterministic_rng(config.rng_algorithm);
    TestRunner::new_with_rng(config, rng)
}

type Told = Vec<(usize, Option<u64>)>;

struct Model {
    leases: BTreeMap<(usize, Option<u64>), Principal>,
    drafts: BTreeMap<(usize, u64), Option<u64>>,
    archived: bool,
    seated: BTreeSet<u64>,
}

impl Model {
    fn new() -> Model {
        Model {
            leases: BTreeMap::new(),
            drafts: BTreeMap::new(),
            archived: false,
            seated: BTreeSet::from([number(&BO)]),
        }
    }

    fn may_post(&self, room: usize, who: &Principal) -> Result<(), &'static str> {
        let me = number(who);
        let admitted = match room {
            0 if self.archived => return Err(code::WRONG_STATE),
            0 => true,
            1 => me == number(&ADA) || self.seated.contains(&me),
            _ => me == number(&ADA) || me == number(&BO),
        };
        match admitted {
            true => Ok(()),
            false => Err(code::UNAUTHORIZED),
        }
    }

    fn busy(
        &self,
        room: usize,
        thread: Option<u64>,
        me: &Principal,
    ) -> Option<(Principal, Holding)> {
        let lease = self.leases.get(&(room, thread));
        if let Some(holder) = lease.filter(|holder| *holder != me) {
            return Some((holder.clone(), Holding::Lease));
        }
        let mut drafted = self.drafts.iter();
        let other = drafted.find(|((at, author), under)| {
            let here = *at == room && **under == thread;
            here && *author != number(me)
        });
        other.map(|((_, author), _)| (Principal::Account(*author), Holding::Draft))
    }

    fn release(&mut self, room: usize, thread: Option<u64>, who: &Principal) -> Told {
        if self.leases.get(&(room, thread)) != Some(who) {
            return Vec::new();
        }
        self.leases.remove(&(room, thread));
        vec![(room, thread)]
    }

    fn vacate(&mut self, room: usize, who: Option<u64>) -> Told {
        let theirs = |holder: u64| who.is_none_or(|who| who == holder);
        let mut places = BTreeSet::new();
        self.leases.retain(|(at, thread), holder| {
            let ends = *at == room && theirs(number(holder));
            if ends {
                places.insert(*thread);
            }
            !ends
        });
        self.drafts.retain(|(at, author), thread| {
            let ends = *at == room && theirs(*author);
            if ends {
                places.insert(*thread);
            }
            !ends
        });
        places.into_iter().map(|thread| (room, thread)).collect()
    }

    fn step(&mut self, step: &Step) -> Result<Told, &'static str> {
        let who = &PEOPLE[step.who];
        let (me, room, thread) = (number(who), step.room, THREADS[step.thread]);
        match step.act {
            Act::Take => {
                self.may_post(room, who)?;
                if self.leases.get(&(room, thread)) == Some(who) {
                    return Ok(Vec::new());
                }
                if self.busy(room, thread, who).is_some() {
                    return Err(code::WRONG_STATE);
                }
                self.leases.insert((room, thread), who.clone());
                Ok(Vec::new())
            }
            Act::Release => Ok(self.release(room, thread, who)),
            Act::Draft => {
                self.may_post(room, who)?;
                let left = self.drafts.insert((room, me), thread);
                let moved = left.filter(|left| *left != thread);
                Ok(moved.map(|left| (room, left)).into_iter().collect())
            }
            Act::Undraft => {
                let left = self.drafts.remove(&(room, me));
                Ok(left.map(|left| (room, left)).into_iter().collect())
            }
            Act::Post => {
                self.may_post(room, who)?;
                let left = self.drafts.remove(&(room, me));
                let mut told: Told = left.map(|left| (room, left)).into_iter().collect();
                told.extend(self.release(room, thread, who));
                Ok(told)
            }
            Act::Break => {
                let held = self.leases.remove(&(room, thread)).is_some();
                let before = self.drafts.len();
                self.drafts
                    .retain(|(at, _), under| !(*at == room && *under == thread));
                let ended = held || self.drafts.len() < before;
                Ok(ended.then_some((room, thread)).into_iter().collect())
            }
            Act::Archive(archived) => {
                self.archived = archived;
                match archived {
                    true => Ok(self.vacate(0, None)),
                    false => Ok(Vec::new()),
                }
            }
            Act::Seat(true) => {
                self.seated.insert(me);
                Ok(Vec::new())
            }
            Act::Seat(false) => {
                self.seated.remove(&me);
                match self.may_post(1, who) {
                    Ok(()) => Ok(Vec::new()),
                    Err(_) => Ok(self.vacate(1, Some(me))),
                }
            }
        }
    }
}

fn sender(step: &Step) -> Principal {
    match step.act {
        Act::Break | Act::Archive(_) | Act::Seat(_) => ADA,
        _ => PEOPLE[step.who].clone(),
    }
}

fn op(step: &Step, n: usize) -> Op {
    let (room, thread) = (&rooms()[step.room], THREADS[step.thread]);
    match step.act {
        Act::Take => take(room, thread),
        Act::Release => release(room, thread),
        Act::Draft => drafting(room, thread),
        Act::Undraft => undraft(room),
        Act::Post => post(room, &format!("m{n}"), "hi", thread),
        Act::Break => clear(room, thread),
        Act::Archive(archived) => archive(archived),
        Act::Seat(member) => seat(&PEOPLE[step.who], member),
    }
}

fn leased(steps: &[Step]) -> Result<Chat, TestCaseError> {
    let mut chat = Chat::rooted();
    chat.ok(&FORGE, watch(vec![DELIVER]));
    chat.store.take_emissions();
    let mut model = Model::new();
    let names = rooms();
    for (n, step) in steps.iter().enumerate() {
        match model.step(step) {
            Err(refusal) => {
                let answer = chat.refused(&sender(step), op(step, n));
                prop_assert_eq!(answer, refusal, "step {} {:?}", n, step);
                prop_assert!(
                    chat.told().is_empty(),
                    "step {} {:?} refused yet told",
                    n,
                    step
                );
            }
            Ok(expected) => {
                if let Err(refusal) = chat.run(&sender(step), op(step, n)) {
                    let refused = format!("step {n} {step:?} refused: {refusal:?}");
                    return Err(TestCaseError::fail(refused));
                }
                let expected: Vec<_> = (expected.into_iter())
                    .map(|(room, thread)| (names[room].clone(), thread))
                    .collect();
                prop_assert_eq!(chat.told(), expected, "step {} {:?} told", n, step);
            }
        }
        let reads = chat.reads();
        for ((channel, author, piece), _) in DRAFT_PIECES.all(&reads).unwrap() {
            let draft = DRAFTS.get(&reads, &(channel, author)).unwrap();
            let kept = draft.is_some_and(|draft| piece < draft.pieces);
            prop_assert!(kept, "step {} {:?}: a piece outlived its draft", n, step);
        }
        for (room, name) in names.iter().enumerate() {
            for thread in THREADS {
                for other in &PEOPLE {
                    let asked = chat.busy(name, thread, other);
                    let modelled = model.busy(room, thread, other);
                    let place = (name, thread, other);
                    prop_assert_eq!(
                        asked,
                        modelled,
                        "step {} {:?}: busy at {:?}",
                        n,
                        step,
                        place
                    );
                }
            }
        }
    }
    Ok(chat)
}

fn plain(steps: &[Step]) -> Chat {
    let mut chat = Chat::rooted();
    chat.height += 1;
    for (n, step) in steps.iter().enumerate() {
        match step.act {
            Act::Take | Act::Release => chat.height += 1,
            _ => {
                let _ = chat.run(&sender(step), op(step, n));
            }
        }
    }
    chat
}

fn kept(chat: &Chat) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let held = [LEASES.key(&()), PLACE_WATCHERS.key(&())];
    let state = chat.store.borrow().state.clone();
    let others = state.into_iter();
    (others.filter(|(key, _)| !held.iter().any(|prefix| key.starts_with(prefix)))).collect()
}

#[test]
fn holds_follow_the_model_in_every_place_and_change_nothing_any_other_op_writes() {
    let checked = runner().run(&steps(), |steps| {
        let leased = leased(&steps)?;
        let (with, without) = (kept(&leased), kept(&plain(&steps)));
        prop_assert!(with == without, "a hold changed what another op wrote");
        Ok(())
    });
    checked.unwrap();
}
