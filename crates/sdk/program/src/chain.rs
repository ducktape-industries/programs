//! A native chain of programs, run as the kernel runs them: several programs
//! each seated by name over its own [`MockHost`], querying each other
//! through the bytes, and a submission run as one frame, its messages after
//! it, depth first, in emit order, undone whole on a rejection it does not
//! absorb (the kernel's `run_frame`, `crates/kernel/host/src/lib.rs`).
//! [`MockChain`] is the kernel a native test lacks; a program alone still
//! runs over a bare [`MockHost`].
//!
//! Left out of the kernel's run, on purpose: fuel (a run never runs out),
//! blob rostering (a blob put stays put), state roots (`ctx.root` is
//! `None`), committed reads (they read the same map as a plain read), and
//! the query stack's cycle check (a program asked again while a query of it
//! is being answered is refused as unknown rather than as a cycle), and
//! receipts (a submission is its output or its refusal; what ran nested is
//! read off the hosts, and a rejected run's events go with its writes,
//! where the kernel's receipt would keep them; the receipt the scheduler
//! keeps for a scheduled run carries its program and outcome alone).
//!
//! A block ends when [`MockChain::at`] moves past it: the chain then runs the
//! entries the scheduler role holds due at that height, as the kernel's
//! `crates/kernel/host/src/scheduling.rs` does.

use std::cell::{Cell, Ref, RefCell, RefMut};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use abi::role::identity::{Kind, Profile, Query, Reply, Standing};
use abi::role::{self, awaits, ownership, reads, scheduler};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::awaiting::{self, Answer, Awaiting, Step, Suspended};
use crate::{
    AccountNumber, Blob, BlobId, Called, Cause, Emitted, EntryId, Env, Error, ExecCtx, Foreign,
    FrameId, HashKind, MessageId, MockHost, Need, Origin, Outcome, Principal, Program, ProgramId,
    QueryCtx, Resumed, Roles, Ticket, code, identity_role, invalid, unauthorized, unexpected_reply,
};

/// How deep messages nest in one frame: a submission runs at depth 0 and
/// each message, or reply, one deeper than the run that emitted it. The
/// kernel's `MAX_DEPTH` (`crates/kernel/host/src/lib.rs`).
pub const MAX_DEPTH: u32 = 11;

type Entry<Ctx> = Rc<dyn Fn(Ctx, &[u8]) -> Result<(), Error>>;
type Execute = Rc<dyn Fn(ExecCtx, &[u8]) -> Step>;
type Lookup = Rc<dyn Fn(QueryCtx, &str) -> Result<(), Error>>;
type Sender = Result<Option<Principal>, Error>;

struct Seat {
    host: MockHost,
    init: Entry<ExecCtx>,
    execute: Execute,
    query: Entry<QueryCtx>,
    access: Entry<QueryCtx>,
    resource: Lookup,
}

enum Call<'a> {
    Init(&'a [u8]),
    Execute(&'a [u8]),
    Resume { ticket: Ticket, answer: Answer },
}

#[derive(Clone)]
enum Waiter {
    Nobody,
    Reply { id: MessageId },
    Call { ticket: Ticket },
}

enum Status {
    Finished,
    Deferred,
    Parked,
}

struct Run {
    outcome: Result<Vec<u8>, Error>,
    status: Status,
}

impl Run {
    fn finished(outcome: Result<Vec<u8>, Error>) -> Run {
        Run {
            outcome,
            status: Status::Finished,
        }
    }
}

struct Segment {
    outcome: Result<Vec<u8>, Error>,
    calls: Vec<Called>,
    deferred: bool,
    parked: Option<Suspended>,
}

impl Segment {
    fn finished(outcome: Result<Vec<u8>, Error>) -> Segment {
        Segment {
            outcome,
            calls: Vec::new(),
            deferred: false,
            parked: None,
        }
    }
}

struct Start {
    frame: Option<FrameId>,
    waiter: Waiter,
    checkpoint: Checkpoint,
}

#[derive(Clone)]
struct Checkpoint {
    written: Vec<crate::mock::Written>,
    parked: BTreeSet<FrameId>,
    reads: BTreeMap<Ticket, OpenRead>,
}

struct Parked {
    suspended: Suspended,
    env: Env,
    waiter: Waiter,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenRead {
    pub requester: ProgramId,
    pub target: ProgramId,
    pub query: Vec<u8>,
    pub made: u64,
    pub answered: Option<reads::Read>,
}

/// Who holds what, for a chain with no identity program seated: each key
/// the account it holds, and each account's profile (a program's account is
/// the profile whose kind names the program). The identity role over it
/// ([`identity_role`]) answers programs and the chain alike, as identity
/// answers any asker.
///
/// ```ignore
/// let chain = MockChain::default();
/// chain.hold(b"ada-key", 1); // a user, unless a profile says otherwise
/// chain.register("forge", 900);
/// assert_eq!(chain.roster().keys[b"ada-key".as_slice()], 1);
/// ```
#[derive(Default)]
pub struct Roster {
    pub keys: BTreeMap<Vec<u8>, AccountNumber>,
    pub profiles: BTreeMap<AccountNumber, Profile>,
}

impl Roster {
    pub(crate) fn account(&self, asked: &Query) -> Result<Option<AccountNumber>, Error> {
        let number = match asked {
            Query::Account(key) => self.keys.get(key).copied(),
            Query::OfProgram(program) => self
                .profiles
                .values()
                .find(|profile| profile.kind == Kind::System(program.clone()))
                .map(|profile| profile.number),
            Query::Runs { program, account } => {
                let runs = self.profiles.get(account).is_some_and(|profile| {
                    matches!(&profile.kind, Kind::Program { script: Some(script), .. } if &script.calls == program)
                });
                if !runs {
                    return Err(unauthorized(format!(
                        "{program} does not run account {account}"
                    )));
                }
                Some(*account)
            }
            Query::Profile(_) | Query::Profiles { .. } => {
                unreachable!("only who acts is asked here")
            }
        };
        if let Some(number) = number
            && let Some(profile) = self.profiles.get(&number)
            && let Kind::Program { standing, .. } = profile.kind
            && standing != Standing::Active
        {
            return Err(unauthorized(format!("account {number} is suspended")));
        }
        Ok(number)
    }
}

/// Profiles alone: a roster that holds no key. Its `Account` answers
/// `None` for every key, as for a key on no account, so a test whose code
/// wrongly asks `Account` here passes silently: put the keys a test reads
/// in [`Roster::keys`].
impl From<Vec<Profile>> for Roster {
    fn from(profiles: Vec<Profile>) -> Roster {
        Roster {
            keys: BTreeMap::new(),
            profiles: profiles.into_iter().map(|p| (p.number, p)).collect(),
        }
    }
}

/// Several programs over one clock, run as the kernel runs them. The chain
/// is founded with the roles genesis binds ([`founded`](MockChain::founded);
/// [`MockHost::roles`] by default) and is at a height and time
/// ([`at`](MockChain::at) moves it); every program is seated
/// by name ([`seat`](MockChain::seat)) over a host of its own
/// ([`host`](MockChain::host)); a signed submission is one frame
/// ([`submit`](MockChain::submit)). Who a key or a program acts as: the
/// program seated at the identity role's binding, asked as the kernel
/// asks it, or, when none is, the [`Roster`].
///
/// ```ignore
/// let mut chain = MockChain::default();
/// chain.seat::<Forge>("forge");
/// chain.seat::<Chat>("chat");
/// chain.register("forge", 900);
/// chain.hold(b"ada-key", 1);
/// chain.init("forge", &bounds())?;
/// // forge's op, and what it emitted to chat, as one frame
/// let output = chain.submit(Origin::Signed(b"ada-key".to_vec()), "forge", &op)?;
/// let reply: chat::Reply = chain.query("chat", &chat::Query::Roots { .. })?;
/// ```
pub struct MockChain {
    roles: Roles,
    seats: BTreeMap<ProgramId, Seat>,
    roster: Rc<RefCell<Roster>>,
    next_message: Cell<u64>,
    next_ticket: Cell<Ticket>,
    next_frame: Cell<FrameId>,
    parked: RefCell<BTreeMap<FrameId, Parked>>,
    clock: Cell<(u64, u64)>,
    held: RefCell<BTreeMap<BlobId, Blob>>,
    reads: RefCell<BTreeMap<Ticket, OpenRead>>,
    unserved: Cell<bool>,
}

impl Default for MockChain {
    fn default() -> MockChain {
        MockChain::founded(MockHost::roles())
    }
}

impl MockChain {
    fn role(&self, role: &str) -> Result<ProgramId, Error> {
        self.roles
            .bound(role)
            .cloned()
            .map_err(crate::kernel::error_from)
    }

    pub fn founded(roles: Roles) -> MockChain {
        MockChain {
            roles,
            seats: BTreeMap::new(),
            roster: Rc::default(),
            next_message: Cell::new(0),
            next_ticket: Cell::new(0),
            next_frame: Cell::new(0),
            parked: RefCell::default(),
            clock: Cell::new((1, 0)),
            held: RefCell::default(),
            reads: RefCell::default(),
            unserved: Cell::new(false),
        }
    }

    /// Seats `M` as `program`, over a fresh host of its own, which every
    /// program seated may query and which is returned for the test to read
    /// and write (`chain.host(program)` finds it again). Seated at the
    /// identity role's binding, `M` is the identity the chain asks.
    pub fn seat<M: Program>(&mut self, program: impl Into<ProgramId>) -> MockHost {
        let execute = Rc::new(|ctx, payload: &[u8]| Step::Done(crate::execute::<M>(&ctx, payload)));
        self.seat_with::<M>(program.into(), execute)
    }

    pub fn seat_awaiting<A: Awaiting>(&mut self, program: impl Into<ProgramId>) -> MockHost {
        let execute = Rc::new(|ctx, payload: &[u8]| awaiting::start::<A>(ctx, payload));
        self.seat_with::<A>(program.into(), execute)
    }

    /// Puts `body` as a blob of `kind` the way the node's blob upload does,
    /// apart from any frame: the network holds it, so every program, seated
    /// now or later (at its next run or query), reads it.
    pub fn put_blob(&self, kind: &str, body: Vec<u8>) -> Result<BlobId, Error> {
        let id = crate::blob_id(HashKind::Sha256, kind, &body)?;
        let kind = kind.to_owned();
        self.held.borrow_mut().insert(id, Blob { kind, body });
        self.share_blobs();
        Ok(id)
    }

    pub fn drop_blob(&self, id: &BlobId) {
        self.held.borrow_mut().remove(id);
        for seat in self.seats.values() {
            seat.host.borrow_mut().blobs.remove(id);
        }
    }

    pub fn frames(&self) -> usize {
        self.parked.borrow().len()
    }

    pub fn serve_reads(&self, served: bool) {
        self.unserved.set(!served);
    }

    pub fn reads(&self) -> Vec<(Ticket, OpenRead)> {
        let open = self.reads.borrow();
        open.iter().map(|(id, read)| (*id, read.clone())).collect()
    }

    pub fn answer_read(&self, id: Ticket) -> Result<(), Error> {
        let open = self.open_read(id)?;
        if open.answered.is_some() {
            let answered = format!("read {id} is answered already");
            return Err(Error::new(code::WRONG_STATE, answered));
        }
        if !self.awaited(id) {
            self.reads.borrow_mut().remove(&id);
            return Ok(());
        }
        let (height, _) = self.clock.get();
        let env = Env {
            origin: Origin::Program(open.requester.clone()),
            sender: None,
            ..self.env(open.target.clone())
        };
        let reply = match self.query_raw(&env, &open.query) {
            Ok(reply) => reply,
            Err(refusal) => {
                self.reads.borrow_mut().remove(&id);
                return self.resolve_read(id, Err(refusal));
            }
        };
        let framed = blob::frame(reads::KIND, &reply).map_err(crate::kernel::error_from)?;
        let entry = reads::Entry {
            id: self.put_blob(reads::KIND, reply)?,
            len: framed.len() as u64,
            kind: reads::KIND.to_owned(),
        };
        if let Some(open) = self.reads.borrow_mut().get_mut(&id) {
            open.answered = Some(reads::Read { entry, at: height });
        }
        Ok(())
    }

    pub fn ready_read(&self, id: Ticket) -> Result<(), Error> {
        let open = self.open_read(id)?;
        let Some(read) = open.answered else {
            let early = format!("read {id} is not answered yet");
            return Err(Error::new(code::WRONG_STATE, early));
        };
        self.reads.borrow_mut().remove(&id);
        if !self.awaited(id) {
            return Ok(());
        }
        let line = [b"read/".as_slice(), &id.to_be_bytes()].concat();
        let requester = self.seated(&open.requester)?;
        let on = (open.requester.clone(), line);
        requester.host.borrow_mut().lines.insert(on, read.entry.id);
        self.resolve_read(id, Ok(abi::encode(&read)))
    }

    pub fn answer_reads(&self) -> Result<(), Error> {
        let asked: Vec<Ticket> = self
            .reads
            .borrow()
            .iter()
            .filter(|(_, open)| open.answered.is_none())
            .map(|(id, _)| *id)
            .collect();
        for id in asked {
            self.answer_read(id)?;
            let answered = self.reads.borrow().contains_key(&id);
            if answered {
                self.ready_read(id)?;
            }
        }
        Ok(())
    }

    // ends every frame `program` has parked as the kernel does when the program's code changes
    // (`crates/kernel/host/src/parking.rs`, `cancel_parked`): each fails as cancelled, and what
    // it waited on answers no one
    pub fn cancel_parked(&self, program: &str) {
        let frames: Vec<FrameId> = self
            .parked
            .borrow()
            .iter()
            .filter(|(_, parked)| parked.env.program == program)
            .map(|(id, _)| *id)
            .collect();
        let (height, time) = self.clock.get();
        for id in frames {
            let Some(parked) = self.parked.borrow_mut().remove(&id) else {
                continue;
            };
            let env = Env {
                height,
                time,
                ..parked.env
            };
            let start = Start {
                frame: Some(id),
                waiter: parked.waiter,
                checkpoint: self.checkpoint(),
            };
            let cancelled = crate::kernel::error_from(awaits::cancelled(program));
            self.finish(&env, start, Err(cancelled), false, 0);
        }
    }

    fn seat_with<M: Program>(&mut self, program: ProgramId, execute: Execute) -> MockHost {
        let host = MockHost::default();
        let seat = Seat {
            host: host.clone(),
            init: Rc::new(|ctx, params| M::init(&ctx, params)),
            execute,
            query: Rc::new(|ctx, request| crate::query::<M>(&ctx, request)),
            access: Rc::new(|ctx, payload| crate::access::<M>(&ctx, payload)),
            resource: Rc::new(|ctx, key| crate::resource::<M>(&ctx, key)),
        };
        let roster = self.roster.clone();
        if let Some(identity) = self.roles.get(role::IDENTITY) {
            host.borrow_mut().siblings.insert(
                identity.clone(),
                Box::new(move |_, request| identity_role(&roster.borrow(), request)),
            );
        }
        host.borrow_mut()
            .siblings
            .insert(program.clone(), seat.sibling());
        host.borrow_mut()
            .owners
            .insert(program.clone(), seat.owners());
        for (name, other) in &self.seats {
            let mut theirs = other.host.borrow_mut();
            theirs.siblings.insert(program.clone(), seat.sibling());
            theirs.owners.insert(program.clone(), seat.owners());
            drop(theirs);
            let mut mine = host.borrow_mut();
            mine.siblings.insert(name.clone(), other.sibling());
            mine.owners.insert(name.clone(), other.owners());
        }
        self.seats.insert(program, seat);
        host
    }

    /// The host `program` is seated over.
    #[track_caller]
    pub fn host(&self, program: &str) -> &MockHost {
        &self.seated(program).expect("a seated program").host
    }

    fn seated(&self, program: &str) -> Result<&Seat, Error> {
        self.seats
            .get(program)
            .ok_or_else(|| Error::new(code::UNKNOWN_PROGRAM, program))
    }

    /// The roster, to read or edit ([`hold`](MockChain::hold) and
    /// [`profile`](MockChain::profile) are the usual edits).
    pub fn roster(&self) -> Ref<'_, Roster> {
        self.roster.borrow()
    }

    fn roster_mut(&self) -> RefMut<'_, Roster> {
        self.roster.borrow_mut()
    }

    /// Seats `key` in `account`, as identity would; an account no profile
    /// names yet is a user's. The roster answers only while no program is
    /// seated at the identity role: then identity's own ops seat keys.
    pub fn hold(&self, key: impl Into<Vec<u8>>, account: AccountNumber) {
        let mut roster = self.roster_mut();
        roster.keys.insert(key.into(), account);
        roster.profiles.entry(account).or_insert_with(|| Profile {
            number: account,
            name: format!("account {account}"),
            kind: Kind::User,
        });
    }

    /// Adds or replaces an account's profile: a system account, a program
    /// account and its standing (the roster's, like [`hold`](MockChain::hold)).
    pub fn profile(&self, profile: Profile) {
        self.roster_mut().profiles.insert(profile.number, profile);
    }

    /// Moves the chain to `height` and `time`: the block every later
    /// submission, init and query runs in (height 1, time 0 at first).
    pub fn at(&self, height: u64, time: u64) {
        let (from, then) = self.clock.get();
        for ended in from..height {
            self.clock.set((ended, then));
            self.run_scheduled();
            self.expire_reads();
        }
        self.clock.set((height, time));
    }

    fn run_scheduled(&self) {
        let Ok(scheduler) = self.role(role::SCHEDULER) else {
            return;
        };
        let (height, _) = self.clock.get();
        let mut after = None;
        loop {
            let due = scheduler::Query::Due {
                height,
                after,
                limit: None,
            };
            let Ok(scheduler::Reply::Due { entries, next }) = self.query(&scheduler, &due) else {
                return;
            };
            for listed in entries {
                self.run_listed(&scheduler, listed.id);
            }
            let Some(next) = next else {
                return;
            };
            after = Some(next);
        }
    }

    fn run_listed(&self, scheduler: &ProgramId, id: EntryId) {
        let asked = scheduler::Query::Entry { id };
        let Ok(scheduler::Reply::Entry(Some(entry))) = self.query(scheduler, &asked) else {
            return;
        };
        let ran_already = entry.receipt.is_some();
        if ran_already {
            return;
        }
        let outcome = match self.run_entry(&entry) {
            Ok(output) => abi::Outcome::Applied { output },
            Err(refusal) => abi::Outcome::Rejected(crate::kernel::refusal_from(refusal)),
        };
        let receipt = abi::Receipt {
            program: entry.target,
            outcome,
            events: Vec::new(),
            nested: Vec::new(),
        };
        let ran = abi::encode(&scheduler::Op::Ran { entry: id, receipt });
        let env = self.env(scheduler.clone());
        let _ = self.run_frame(env, Call::Execute(&ran), Waiter::Nobody, 0);
    }

    fn run_entry(&self, entry: &scheduler::Entry) -> Result<Vec<u8>, Error> {
        let sender = self.account(Query::OfProgram(entry.scheduler.clone()))?;
        let env = Env {
            origin: Origin::Program(entry.scheduler.clone()),
            sender,
            cause: Cause::Scheduled { entry: entry.id },
            ..self.env(entry.target.clone())
        };
        self.run_frame(env, Call::Execute(&entry.payload), Waiter::Nobody, 0)
            .outcome
    }

    /// `program`'s env for a direct call by the chain itself at this height
    /// and time: what [`Program::init`] runs in.
    pub fn env(&self, program: impl Into<ProgramId>) -> Env {
        let (height, time) = self.clock.get();
        Env {
            height,
            time,
            roles: self.roles.clone(),
            ..MockHost::env(program).root()
        }
    }

    /// `program`'s [`init`](Program::init) with `params` (borsh), as the
    /// kernel admits a program: one frame run by the chain itself at this
    /// height, what init emits run after it, and a refusal undoing it all.
    pub fn init(&self, program: &str, params: &impl BorshSerialize) -> Result<(), Error> {
        let params = abi::encode(params);
        let env = self.env(program);
        self.run_frame(env, Call::Init(&params), Waiter::Nobody, 0)
            .outcome
            .map(drop)
    }

    /// Gives `program` the account `number`, as identity does when the
    /// kernel admits a program: what its messages act as (the roster's,
    /// like [`hold`](MockChain::hold)).
    pub fn register(&self, program: &str, number: AccountNumber) {
        self.profile(Profile {
            number,
            name: program.into(),
            kind: Kind::System(program.into()),
        });
    }

    /// Runs `op` (`program`'s own `Op`, borsh) as one frame, as the kernel
    /// runs a submission: `origin`'s sender resolved through identity (a
    /// key that holds no account still runs, as no one; a refusal there
    /// rejects the frame), then the program, then what it emitted, depth
    /// first. The program's output, or the refusal that rejected the frame,
    /// which left every host as it was. A `Signed` origin is a submission
    /// as the kernel admits one; `Program` and `Root` are the chain's
    /// convenience, a message or a genesis call sent by hand.
    pub fn submit<O: BorshSerialize>(
        &self,
        origin: Origin,
        program: impl Into<ProgramId>,
        op: &O,
    ) -> Result<Vec<u8>, Error> {
        self.submit_raw(origin, program, abi::encode(op))
    }

    pub fn submit_from<O: BorshSerialize>(
        &self,
        client: &str,
        origin: Origin,
        program: impl Into<ProgramId>,
        op: &O,
    ) -> Result<Vec<u8>, Error> {
        let named = |client: Option<String>| {
            for seat in self.seats.values() {
                seat.host.borrow_mut().client = client.clone();
            }
        };
        named(Some(client.to_owned()));
        let submitted = self.submit(origin, program, op);
        named(None);
        submitted
    }

    /// [`submit`](MockChain::submit) with the payload's bytes as they are.
    pub fn submit_raw(
        &self,
        origin: Origin,
        program: impl Into<ProgramId>,
        payload: impl AsRef<[u8]>,
    ) -> Result<Vec<u8>, Error> {
        let sender = match &origin {
            Origin::Signed(key) => self.account(Query::Account(key.clone()))?,
            Origin::Program(program) => self.account(Query::OfProgram(program.clone()))?,
            Origin::Root => Some(Principal::Root),
        };
        let env = Env {
            origin,
            sender,
            ..self.env(program)
        };
        let call = Call::Execute(payload.as_ref());
        self.run_frame(env, call, Waiter::Nobody, 0).outcome
    }

    pub fn submit_foreign<O: BorshSerialize>(
        &self,
        foreign: Foreign,
        program: impl Into<ProgramId>,
        op: &O,
    ) -> Result<Vec<u8>, Error> {
        let crosspond = self.role(role::CROSSPOND)?;
        let env = self.env(program).from_foreign(crosspond, foreign);
        let payload = abi::encode(op);
        self.run_frame(env, Call::Execute(&payload), Waiter::Nobody, 0)
            .outcome
    }

    /// `program`'s answer to `request`, asked by the chain itself (origin
    /// `Root`, no sender), borsh both ways.
    pub fn query<Q: BorshSerialize, R: BorshDeserialize>(
        &self,
        program: &str,
        request: &Q,
    ) -> Result<R, Error> {
        let env = Env {
            sender: None,
            ..self.env(program)
        };
        let answer = self.query_raw(&env, &abi::encode(request))?;
        abi::decode(&answer).map_err(crate::kernel::error_from)
    }

    /// [`query`](MockChain::query) with the request's bytes as they are,
    /// and the answer's: what a node relays for a view.
    pub fn query_bytes(&self, program: &str, request: &[u8]) -> Result<Vec<u8>, Error> {
        let env = Env {
            sender: None,
            ..self.env(program)
        };
        self.query_raw(&env, request)
    }

    fn query_raw(&self, env: &Env, request: &[u8]) -> Result<Vec<u8>, Error> {
        self.share_blobs();
        let seat = self.seated(&env.program)?;
        (seat.query)(seat.host.query(env.clone()), request)?;
        Ok(seat.host.take_response())
    }

    fn account(&self, asked: Query) -> Result<Option<Principal>, Error> {
        let identity = self.role(role::IDENTITY)?;
        let account = if self.seats.contains_key(&identity) {
            let env = Env {
                sender: None,
                ..self.env(identity.clone())
            };
            let answered = self.query_raw(&env, &abi::encode(&asked))?;
            match abi::decode(&answered) {
                Ok(Reply::Account(account)) => account,
                Ok(other) => {
                    return Err(unexpected_reply(&identity, &format!("{asked:?}"), &other));
                }
                Err(fault) => {
                    return Err(unexpected_reply(
                        &identity,
                        &format!("{asked:?}"),
                        &fault.sentence,
                    ));
                }
            }
        } else {
            self.roster().account(&asked)?
        };
        Ok(account.map(Principal::Account))
    }

    fn own_sender(&self, program: &ProgramId, asked: bool) -> Sender {
        match asked {
            true => self.account(Query::OfProgram(program.clone())),
            false => Ok(None),
        }
    }

    fn sender(&self, program: &ProgramId, acting: Option<AccountNumber>, own: &Sender) -> Sender {
        match acting {
            None => own.clone(),
            Some(account) => self.run_by(program, account),
        }
    }

    fn foreign_sender(
        &self,
        program: &ProgramId,
        acting: Option<AccountNumber>,
        foreign: Foreign,
    ) -> Sender {
        let bound_to_crosspond = self.roles.get(role::CROSSPOND) == Some(program);
        match (bound_to_crosspond, acting) {
            (false, _) => Err(unauthorized(
                "only the program bound to the crosspond role sends as a foreign user",
            )),
            (true, Some(account)) => Err(invalid(format!(
                "a message sent as a foreign user cannot also act as account {account}"
            ))),
            (true, None) => Ok(Some(Principal::Foreign(foreign))),
        }
    }

    fn run_by(&self, program: &ProgramId, account: AccountNumber) -> Sender {
        let asked = Query::Runs {
            program: program.clone(),
            account,
        };
        match self.account(asked)? {
            Some(Principal::Account(number)) if number == account => {
                Ok(Some(Principal::Account(account)))
            }
            _ => Err(unauthorized(format!(
                "{program} does not run account {account}"
            ))),
        }
    }

    fn run_frame(&self, env: Env, call: Call, waiter: Waiter, depth: u32) -> Run {
        if depth > MAX_DEPTH {
            let too_deep = format!("messages nest deeper than {MAX_DEPTH}");
            return Run::finished(Err(Error::new(code::CAPACITY, too_deep)));
        }
        let checkpoint = self.checkpoint();
        if let Call::Execute(payload) = &call
            && gated(&env)
            && let Err(refusal) = self.gate(&env, payload)
        {
            self.restore(checkpoint);
            return Run::finished(Err(refusal));
        }
        let segment = self.segment(&env, None, call, depth);
        let start = Start {
            frame: None,
            waiter,
            checkpoint,
        };
        self.follow(env, start, segment, depth)
    }

    fn gate(&self, env: &Env, payload: &[u8]) -> Result<(), Error> {
        let seat = self.seated(&env.program)?;
        (seat.access)(seat.host.query(env.clone()), payload)?;
        let needs: Vec<Need> =
            abi::decode(&seat.host.take_response()).map_err(crate::kernel::error_from)?;
        if needs.is_empty() {
            return Ok(());
        }
        let checker = self.role(role::ACCESS)?;
        let check = abi::role::access::Op::Check {
            program: env.program.clone(),
            actor: env.actor(),
            needs,
        };
        let seat = self.seated(&checker)?;
        let check_env = Env {
            program: checker.clone(),
            origin: Origin::Root,
            sender: Some(Principal::Root),
            cause: Cause::Direct,
            ..env.clone()
        };
        let step = (seat.execute)(seat.host.exec(check_env), &abi::encode(&check));
        let sent = !seat.host.take_emissions().is_empty();
        match (step, sent) {
            (Step::Done(Err(refusal)), _) => Err(refusal),
            (Step::Done(Ok(())), false) => Ok(()),
            (Step::Done(Ok(())), true) | (Step::Parked(_), _) => Err(Error::new(
                code::UNSUPPORTED,
                format!("{checker} sends no message while it checks access"),
            )),
        }
    }

    fn segment(&self, env: &Env, suspended: Option<Suspended>, call: Call, depth: u32) -> Segment {
        self.share_blobs();
        let checkpoint = self.checkpoint();
        let seat = match self.seated(&env.program) {
            Ok(seat) => seat,
            Err(unknown) => return Segment::finished(Err(unknown)),
        };
        let host = &seat.host;
        let first = self.next_message.get();
        host.borrow_mut().next_message = first;
        host.borrow_mut().next_ticket = self.next_ticket.get();
        host.borrow_mut().touched.clear();
        let runs_an_op = !matches!(call, Call::Init(_));
        let step = match (suspended, call) {
            (Some(suspended), Call::Resume { ticket, answer }) => {
                awaiting::resume(suspended, env.clone(), ticket, answer)
            }
            (_, Call::Init(params)) => Step::Done((seat.init)(host.exec(env.clone()), params)),
            (_, Call::Execute(payload)) => (seat.execute)(host.exec(env.clone()), payload),
            (None, Call::Resume { .. }) => {
                Step::Done(Err(Error::new(code::PROTOCOL, "nothing is parked")))
            }
        };
        let output = host.take_output();
        let emitted = host.take_emitted();
        let calls = host.take_calls();
        let deferred = host.take_deferred();
        let resumes = host.take_resumes();
        self.next_message.set(first + emitted.len() as u64);
        self.next_ticket.set(host.borrow().next_ticket);
        let (ran, parked) = match step {
            Step::Done(done) => (done, None),
            Step::Parked(suspended) => (Ok(()), Some(suspended)),
        };
        let guarded = ran.and_then(|()| self.guard(env, seat, runs_an_op));
        let delivered = guarded.and_then(|()| self.deliver(env, emitted, first, depth));
        if let Err(refusal) = delivered {
            self.restore(checkpoint);
            return Segment::finished(Err(refusal));
        }
        for resumed in resumes {
            self.resume_parked(resumed, env, depth + 1);
        }
        Segment {
            outcome: Ok(output),
            calls,
            deferred,
            parked,
        }
    }

    fn guard(&self, env: &Env, seat: &Seat, runs_an_op: bool) -> Result<(), Error> {
        let touched = std::mem::take(&mut seat.host.borrow_mut().touched);
        let Some(guardian) = self.roles.get(role::OWNERSHIP) else {
            return Ok(());
        };
        let Ok(guarding) = self.seated(guardian) else {
            return Ok(());
        };
        let voted = matches!(&env.origin, Origin::Program(sender) if self.roles.get(role::VALIDATORS) == Some(sender));
        let by_the_chain = env.origin == Origin::Root;
        if !runs_an_op || by_the_chain || voted || *guardian == env.program {
            return Ok(());
        }
        let writes: Vec<ownership::Write> = {
            let state = &seat.host.borrow().state;
            let records = &guarding.host.borrow().state;
            touched
                .into_iter()
                .filter_map(|(key, touch)| {
                    let kept = state.contains_key(&key);
                    let recorded = records.contains_key(&ownership::record_key(&env.program, &key));
                    let created = !touch.existed && kept;
                    let claims = created && touch.claimed.is_some();
                    let changed = touch.existed || kept;
                    let asks = claims || (changed && recorded);
                    asks.then_some(ownership::Write {
                        key,
                        claimed: touch.claimed,
                        existed: touch.existed,
                        kept,
                    })
                })
                .collect()
        };
        if writes.is_empty() {
            return Ok(());
        }
        let guard = ownership::Op::Guard(ownership::Guard {
            program: env.program.clone(),
            origin: env.origin.clone().into(),
            sender: env.sender.clone().map(Into::into),
            writes,
        });
        let asked = Env {
            program: guardian.clone(),
            origin: Origin::Root,
            sender: Some(Principal::Root),
            cause: Cause::Direct,
            ..env.clone()
        };
        let host = &guarding.host;
        let step = (guarding.execute)(host.exec(asked), &abi::encode(&guard));
        host.take_output();
        let emitted = !host.take_emissions().is_empty();
        let called = !host.take_calls().is_empty() || !host.take_resumes().is_empty();
        let sent = emitted || called || host.take_deferred();
        match (step, sent) {
            (Step::Done(Err(refusal)), _) => Err(refusal),
            (Step::Done(Ok(())), false) => Ok(()),
            (Step::Done(Ok(())), true) | (Step::Parked(_), _) => Err(Error::new(
                code::UNSUPPORTED,
                format!("{guardian} sends no message while it guards"),
            )),
        }
    }

    fn deliver(
        &self,
        env: &Env,
        emitted: Vec<Emitted>,
        first: u64,
        depth: u32,
    ) -> Result<(), Error> {
        let as_itself = emitted.iter().any(Emitted::as_its_emitter);
        let emitter = self.own_sender(&env.program, as_itself);
        for (seq, Emitted { message, foreign }) in emitted.into_iter().enumerate() {
            let id = MessageId {
                program: env.program.clone(),
                seq: first + seq as u64,
            };
            let sender = match foreign {
                Some(foreign) => self.foreign_sender(&env.program, message.acting, foreign),
                None => self.sender(&env.program, message.acting, &emitter),
            };
            let ran = match sender {
                Err(refusal) => Run::finished(Err(refusal)),
                Ok(sender) => {
                    let env = self.nested(
                        env,
                        &message.target,
                        &env.program,
                        sender,
                        Cause::Message(id.clone()),
                    );
                    let waiter = match message.reply {
                        true => Waiter::Reply { id: id.clone() },
                        false => Waiter::Nobody,
                    };
                    self.run_frame(env, Call::Execute(&message.payload), waiter, depth + 1)
                }
            };
            let waits = matches!(ran.status, Status::Parked);
            if waits {
                continue;
            }
            let refusal = match (ran.outcome, message.reply) {
                (Ok(_), false) => continue,
                (Err(refusal), false) => refusal,
                (ran, true) => {
                    let outcome = match ran {
                        Ok(output) => Outcome::Applied { output },
                        Err(refusal) => Outcome::Rejected(refusal),
                    };
                    match self.reply(env, &message.target, id, outcome, depth + 1) {
                        Ok(_) => continue,
                        Err(refusal) => refusal,
                    }
                }
            };
            return Err(refusal);
        }
        Ok(())
    }

    fn reply(
        &self,
        env: &Env,
        by: &ProgramId,
        id: MessageId,
        outcome: Outcome,
        depth: u32,
    ) -> Result<Vec<u8>, Error> {
        let sender = self.account(Query::OfProgram(by.clone()))?;
        let emitter = id.program.clone();
        let env = self.nested(env, &emitter, by, sender, Cause::Reply { id, outcome });
        self.run_frame(env, Call::Execute(&[]), Waiter::Nobody, depth)
            .outcome
    }

    fn nested(
        &self,
        env: &Env,
        me: &ProgramId,
        origin: &ProgramId,
        sender: Option<Principal>,
        cause: Cause,
    ) -> Env {
        Env {
            program: me.clone(),
            origin: Origin::Program(origin.clone()),
            sender,
            cause,
            ..env.clone()
        }
    }

    fn follow(&self, env: Env, start: Start, segment: Segment, depth: u32) -> Run {
        let Segment {
            mut outcome,
            mut calls,
            mut deferred,
            parked: mut suspended,
        } = segment;
        let mut ready = VecDeque::new();
        let mut open = Vec::new();
        loop {
            let Some(instance) = suspended.take() else {
                return self.finish(&env, start, outcome, deferred, depth);
            };
            self.place(&env, calls, &mut ready, &mut open, depth);
            let Some((ticket, answer)) = ready.pop_front() else {
                return self.park(env, start, outcome, instance, open, depth);
            };
            let resume = Call::Resume { ticket, answer };
            let next = self.segment(&env, Some(instance), resume, depth);
            outcome = next.outcome;
            calls = next.calls;
            deferred = next.deferred;
            suspended = next.parked;
            if outcome.is_err() {
                self.restore(start.checkpoint.clone());
            }
        }
    }

    fn place(
        &self,
        env: &Env,
        calls: Vec<Called>,
        ready: &mut VecDeque<(Ticket, Answer)>,
        open: &mut Vec<awaits::Call>,
        depth: u32,
    ) {
        if calls.is_empty() {
            return;
        }
        let as_itself = calls.iter().any(|call| call.acting.is_none());
        let caller = self.own_sender(&env.program, as_itself);
        for Called {
            ticket,
            target,
            payload,
            acting,
        } in calls
        {
            if target == reads::PROGRAM {
                match self.ask_read(env, ticket, &payload) {
                    Ok(()) => open.push(awaits::Call { ticket, target }),
                    Err(refusal) => ready.push_back((ticket, Err(refusal))),
                }
                continue;
            }
            let ran = match self.sender(&env.program, acting, &caller) {
                Err(refusal) => Run::finished(Err(refusal)),
                Ok(sender) => {
                    let env =
                        self.nested(env, &target, &env.program, sender, Cause::Call { ticket });
                    let waiter = Waiter::Call { ticket };
                    self.run_frame(env, Call::Execute(&payload), waiter, depth + 1)
                }
            };
            match ran.status {
                Status::Finished => ready.push_back((ticket, ran.outcome)),
                Status::Deferred | Status::Parked => open.push(awaits::Call { ticket, target }),
            }
        }
    }

    fn park(
        &self,
        env: Env,
        start: Start,
        outcome: Result<Vec<u8>, Error>,
        suspended: Suspended,
        calls: Vec<awaits::Call>,
        depth: u32,
    ) -> Run {
        let id = match start.frame {
            Some(id) => id,
            None => {
                let id = self.next_frame.get();
                self.next_frame.set(id + 1);
                id
            }
        };
        let park = awaits::Op::Park {
            frame: id,
            program: env.program.clone(),
            calls,
        };
        if let Err(refusal) = self.tell_awaits(Origin::Root, &park, &env, depth + 1) {
            return self.fail(&env, start, refusal, depth);
        }
        let parked = Parked {
            suspended,
            env,
            waiter: start.waiter,
        };
        self.parked.borrow_mut().insert(id, parked);
        Run {
            outcome,
            status: Status::Parked,
        }
    }

    fn fail(&self, env: &Env, start: Start, refusal: Error, depth: u32) -> Run {
        self.restore(start.checkpoint.clone());
        self.finish(env, start, Err(refusal), false, depth)
    }

    fn finish(
        &self,
        env: &Env,
        start: Start,
        outcome: Result<Vec<u8>, Error>,
        deferred: bool,
        depth: u32,
    ) -> Run {
        let Some(id) = start.frame else {
            let status = match deferred {
                true => Status::Deferred,
                false => Status::Finished,
            };
            return Run { outcome, status };
        };
        self.parked.borrow_mut().remove(&id);
        let close = awaits::Op::Close { frame: id };
        let _ = self.tell_awaits(Origin::Root, &close, env, depth + 1);
        if !deferred {
            self.answer(env, start.waiter, &outcome, depth);
        }
        Run::finished(outcome)
    }

    fn answer(&self, env: &Env, waiter: Waiter, outcome: &Result<Vec<u8>, Error>, depth: u32) {
        match waiter {
            Waiter::Nobody => {}
            Waiter::Reply { id } => {
                let outcome = match outcome.clone() {
                    Ok(output) => Outcome::Applied { output },
                    Err(refusal) => Outcome::Rejected(refusal),
                };
                let _ = self.reply(env, &env.program, id, outcome, depth + 1);
            }
            Waiter::Call { ticket } => {
                let answer = outcome.clone().map_err(crate::kernel::refusal_from);
                let resolve = awaits::Op::Resolve { ticket, answer };
                let origin = Origin::Program(env.program.clone());
                let _ = self.tell_awaits(origin, &resolve, env, depth + 1);
            }
        }
    }

    fn tell_awaits(
        &self,
        origin: Origin,
        op: &awaits::Op,
        env: &Env,
        depth: u32,
    ) -> Result<Vec<u8>, Error> {
        let sender = match &origin {
            Origin::Program(program) => self.account(Query::OfProgram(program.clone()))?,
            Origin::Root | Origin::Signed(_) => Some(Principal::Root),
        };
        let env = Env {
            program: self.role(role::AWAITS)?,
            origin,
            sender,
            cause: Cause::Direct,
            ..env.clone()
        };
        let payload = abi::encode(op);
        self.run_frame(env, Call::Execute(&payload), Waiter::Nobody, depth)
            .outcome
    }

    fn resume_parked(&self, resumed: Resumed, resolver: &Env, depth: u32) {
        let Resumed {
            frame,
            ticket,
            answer,
        } = resumed;
        let Some(parked) = self.parked.borrow_mut().remove(&frame) else {
            return;
        };
        let env = Env {
            height: resolver.height,
            time: resolver.time,
            ..parked.env
        };
        let start = Start {
            frame: Some(frame),
            waiter: parked.waiter,
            checkpoint: self.checkpoint(),
        };
        if depth > MAX_DEPTH {
            let too_deep = Error::new(
                code::CAPACITY,
                format!("messages nest deeper than {MAX_DEPTH}"),
            );
            self.fail(&env, start, too_deep, depth);
            return;
        }
        let resume = Call::Resume { ticket, answer };
        let segment = self.segment(&env, Some(parked.suspended), resume, depth);
        self.follow(env, start, segment, depth);
    }

    fn open_read(&self, id: Ticket) -> Result<OpenRead, Error> {
        let open = self.reads.borrow().get(&id).cloned();
        open.ok_or_else(|| Error::new(code::NOT_FOUND, format!("no read {id} is open")))
    }

    fn ask_read(&self, env: &Env, ticket: Ticket, payload: &[u8]) -> Result<(), Error> {
        let asked = abi::decode::<reads::Op>(payload).map_err(crate::kernel::error_from)?;
        let reads::Op::Read { target, query } = asked else {
            let only = "a program asks $reads to read; only a member answers a read";
            return Err(Error::new(code::UNSUPPORTED, only));
        };
        if self.unserved.get() {
            return Err(crate::kernel::error_from(reads::unserved()));
        }
        let open = OpenRead {
            requester: env.program.clone(),
            target,
            query,
            made: env.height,
            answered: None,
        };
        self.reads.borrow_mut().insert(ticket, open);
        Ok(())
    }

    fn awaited(&self, id: Ticket) -> bool {
        let Ok(awaits) = self.role(role::AWAITS) else {
            return false;
        };
        let asked = awaits::Query::Ticket { ticket: id };
        let row = self.query::<_, awaits::Reply>(&awaits, &asked);
        matches!(row, Ok(awaits::Reply::Ticket(Some(_))))
    }

    fn resolve_read(&self, id: Ticket, answer: Result<Vec<u8>, Error>) -> Result<(), Error> {
        let answer = answer.map_err(crate::kernel::refusal_from);
        let resolve = awaits::Op::Resolve { ticket: id, answer };
        let env = self.env(reads::PROGRAM);
        self.tell_awaits(Origin::Root, &resolve, &env, 0).map(drop)
    }

    fn expire_reads(&self) {
        let (height, _) = self.clock.get();
        let due: Vec<Ticket> = self
            .reads
            .borrow()
            .iter()
            .filter(|(_, open)| open.made.saturating_add(reads::WAIT) <= height)
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.reads.borrow_mut().remove(&id);
            if self.awaited(id) {
                let expired = crate::kernel::error_from(reads::expired(id));
                let _ = self.resolve_read(id, Err(expired));
            }
        }
    }

    fn share_blobs(&self) {
        let mut every = self.held.borrow().clone();
        for seat in self.seats.values() {
            every.extend(seat.host.borrow().blobs.clone());
        }
        for seat in self.seats.values() {
            seat.host.borrow_mut().blobs.extend(every.clone());
        }
    }

    fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            written: self
                .seats
                .values()
                .map(|seat| seat.host.written())
                .collect(),
            parked: self.parked.borrow().keys().copied().collect(),
            reads: self.reads.borrow().clone(),
        }
    }

    fn restore(&self, checkpoint: Checkpoint) {
        for (seat, written) in self.seats.values().zip(checkpoint.written) {
            seat.host.restore(written);
        }
        self.parked
            .borrow_mut()
            .retain(|frame, _| checkpoint.parked.contains(frame));
        *self.reads.borrow_mut() = checkpoint.reads;
    }
}

fn gated(env: &Env) -> bool {
    let completes = matches!(env.cause, Cause::Reply { .. });
    let by_the_chain = env.origin == Origin::Root;
    !(completes || by_the_chain)
}

impl Seat {
    fn owners(&self) -> crate::mock::Owners {
        let (host, resource) = (self.host.clone(), self.resource.clone());
        Box::new(move |env, key| {
            resource(host.query(env), key)?;
            Ok(host.take_response())
        })
    }

    fn sibling(&self) -> crate::Sibling {
        let (host, query) = (self.host.clone(), self.query.clone());
        Box::new(move |env, request| {
            query(host.query(env), request)?;
            Ok(host.take_response())
        })
    }
}

#[cfg(test)]
mod foreign;
#[cfg(test)]
mod guarded;
#[cfg(test)]
mod tests;
