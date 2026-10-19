//! The host a native test runs a program over: maps for state and blobs, and
//! what the program sent (`output`, `response`, `emissions`, `events`) kept
//! for the test to read. Sibling programs answer through `siblings`
//! ([`MockHost::sibling`] seats a program there); signatures verify through
//! `verifier` (none set: verification is refused). A `MockHost` is a shared
//! handle: the contexts made over it and the test see one host.
//! [`MockHost::env`] is the env a test starts from.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;
use std::rc::Rc;

use abi::role::{self, identity::Profile};
use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, HostOp, HostReply, Message,
    Scheme,
};
use sha2::Digest as _;

use crate::{
    AccountNumber, Cause, Env, Error, ExecCtx, FrameId, Order, Origin, Principal, Program,
    ProgramId, QueryCtx, Range, Roles, Roster, Ticket, code,
};

/// A program another one queries: the env the host hands the answering
/// program (the asker's chain, height, time and roles; `program` the one
/// asked, `origin` the asker, no sender) and the request's bytes.
pub type Sibling = Box<dyn Fn(Env, &[u8]) -> Result<Vec<u8>, Error>>;

pub type Owners = Box<dyn Fn(Env, &str) -> Result<Vec<u8>, Error>>;

/// The identity role over a fixed roster, for a native test of a program
/// that names accounts, answered as identity answers any asker: `Account`
/// and `OfProgram` from the roster's keys and programs ([`Roster`]; refused
/// `unauthorized` while a program account does not act), `Profile` and
/// `Profiles` (paged by number) from its profiles.
pub fn identity_role(roster: &Roster, request: &[u8]) -> Result<Vec<u8>, Error> {
    use abi::role::identity::{Query, Reply};
    let reply = match abi::decode(request).map_err(crate::kernel::error_from)? {
        asked @ (Query::Account(_) | Query::OfProgram(_) | Query::Runs { .. }) => {
            Reply::Account(roster.account(&asked)?)
        }
        Query::Profile(number) => Reply::Profile(roster.profiles.get(&number).cloned()),
        Query::Profiles { after, limit } => {
            let mut page: Vec<Profile> = roster
                .profiles
                .values()
                .filter(|p| after.is_none_or(|after| p.number > after))
                .cloned()
                .collect();
            let limit = limit.max(1) as usize;
            let more = page.len() > limit;
            page.truncate(limit);
            let next = page.last().filter(|_| more).map(|p| p.number);
            Reply::Profiles {
                profiles: page,
                next,
            }
        }
    };
    Ok(abi::encode(&reply))
}
pub type Verifier = Box<dyn Fn(Scheme, &[u8], &[u8], &[u8], &[u8]) -> bool>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Touch {
    pub existed: bool,
    pub claimed: Option<String>,
}

#[derive(Default)]
pub struct MockState {
    pub state: BTreeMap<Vec<u8>, Vec<u8>>,
    // each key a run set or deleted, with whether it held a value the first time and the kind a
    // write claimed it as: what the kernel tells the ownership role (a `MockChain` clears it
    // before each run)
    pub touched: BTreeMap<Vec<u8>, Touch>,
    pub blobs: BTreeMap<BlobId, Blob>,
    /// The last `set_return_data`.
    pub output: Vec<u8>,
    /// The last query's response.
    pub response: Vec<u8>,
    pub emissions: Vec<Message>,
    /// The number the next emit gets: 0 after a [`MockHost::take_emissions`];
    /// a [`MockChain`](crate::MockChain) carries it across a frame.
    pub next_message: u64,
    pub events: Vec<Vec<u8>>,
    pub calls: Vec<Called>,
    pub next_ticket: Ticket,
    pub deferred: bool,
    pub resumes: Vec<Resumed>,
    pub siblings: BTreeMap<ProgramId, Sibling>,
    pub owners: BTreeMap<ProgramId, Owners>,
    pub verifier: Option<Verifier>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Called {
    pub ticket: Ticket,
    pub target: ProgramId,
    pub payload: Vec<u8>,
    pub acting: Option<AccountNumber>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resumed {
    pub frame: FrameId,
    pub ticket: Ticket,
    pub answer: Result<Vec<u8>, Error>,
}

#[derive(Clone, Default)]
pub struct MockHost(Rc<RefCell<MockState>>);

#[derive(Clone)]
pub(crate) struct Written {
    state: BTreeMap<Vec<u8>, Vec<u8>>,
    blobs: BTreeMap<BlobId, Blob>,
    sent: (Vec<u8>, Vec<Message>, Vec<Vec<u8>>),
    waits: (Vec<Called>, bool, Vec<Resumed>),
}

impl MockHost {
    /// The roles as the suite's genesis binds them, for a native test's
    /// env: a sibling registered under `identity` answers the identity role.
    pub fn roles() -> Roles {
        Roles::from_iter([
            (role::PROGRAMS, "programs"),
            (role::VALIDATORS, "valset"),
            (role::IDENTITY, "identity"),
            (role::BLOBS, "blobs"),
            (role::AWAITS, "awaits"),
            (role::EGRESS, "egress"),
            (role::HALO, "halo"),
            (role::SCHEDULER, "scheduler"),
            (role::ACCESS, "access"),
            (role::OWNERSHIP, "ownership"),
        ])
    }

    pub fn roles_with(role: &str, program: impl Into<ProgramId>) -> Roles {
        let program = program.into();
        MockHost::roles()
            .iter()
            .filter(|(filled, _)| *filled != role)
            .map(|(filled, by)| (filled.to_owned(), by.clone()))
            .chain([(role.to_owned(), program)])
            .collect()
    }

    #[track_caller]
    pub fn role(role: &str) -> ProgramId {
        match MockHost::roles().get(role) {
            Some(program) => program.clone(),
            None => panic!("the suite binds no program to the {role} role"),
        }
    }

    /// A frame to `program` as a person sends one: signed by key `[1; 32]`,
    /// which holds account 1, on chain `net` at height 1, time 0, under
    /// the suite's [`roles`](MockHost::roles). The kernel sends a program
    /// no other frame outside genesis; the chain itself is
    /// [`Env::root`]. Change what a test cares about with [`Env::signed`],
    /// [`Env::from_program`] or struct update:
    /// `Env { height: 7, ..MockHost::env("chat") }`.
    pub fn env(program: impl Into<ProgramId>) -> Env {
        Env {
            chain_id: b"net".to_vec(),
            height: 1,
            time: 0,
            program: program.into(),
            origin: Origin::Signed(vec![1; 32]),
            sender: Some(Principal::Account(1)),
            roles: MockHost::roles(),
            cause: Cause::Direct,
        }
    }

    pub fn identity(&self, roster: impl Into<Roster>) {
        let roster = roster.into();
        let answer: Sibling = Box::new(move |_, request| identity_role(&roster, request));
        self.borrow_mut()
            .siblings
            .insert(MockHost::role(role::IDENTITY), answer);
    }

    /// Seats `M`, over `host` (its own state), as the program `program`
    /// another one queries here: the request goes through `M`'s own
    /// decoding and the answer through its encoding, as on the host.
    pub fn sibling<M: Program>(&self, program: impl Into<ProgramId>, host: &MockHost) {
        let host = host.clone();
        let program = program.into();
        let asked = host.clone();
        let answer: Sibling = Box::new(move |env, request| {
            crate::query::<M>(&asked.query(env), request)?;
            Ok(asked.take_response())
        });
        let owners: Owners = Box::new(move |env, key| {
            crate::resource::<M>(&host.query(env), key)?;
            Ok(host.take_response())
        });
        let mut mock = self.borrow_mut();
        mock.siblings.insert(program.clone(), answer);
        mock.owners.insert(program, owners);
    }

    /// An execute's context over this host.
    pub fn exec(&self, env: Env) -> ExecCtx {
        ExecCtx::over(self.clone(), env)
    }

    /// A query's context over this host.
    pub fn query(&self, env: Env) -> QueryCtx {
        QueryCtx::over(self.clone(), env)
    }

    pub fn borrow(&self) -> Ref<'_, MockState> {
        self.0.borrow()
    }

    pub fn borrow_mut(&self) -> RefMut<'_, MockState> {
        self.0.borrow_mut()
    }

    pub fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut self.borrow_mut().output)
    }

    pub fn take_emissions(&self) -> Vec<Message> {
        let mut mock = self.borrow_mut();
        mock.next_message = 0;
        std::mem::take(&mut mock.emissions)
    }

    /// The last query's response, as [`crate::query`] handed it over.
    pub fn take_response(&self) -> Vec<u8> {
        std::mem::take(&mut self.borrow_mut().response)
    }

    /// Runs `write` and, when it refuses, checks it left this host as it
    /// found it: no state, blob, emission, event or output. The attack
    /// check every program's harness shares: a rule checks before it
    /// writes, so a refusal needs no rollback.
    #[track_caller]
    pub fn attempt<T>(&self, write: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
        let before = self.written();
        let result = write();
        if result.is_err() {
            let after = self.written();
            assert_eq!(after.state, before.state, "a refused write changed state");
            assert_eq!(after.blobs, before.blobs, "a refused write stored a blob");
            assert_eq!(after.sent, before.sent, "a refused write sent something");
            assert_eq!(
                after.waits, before.waits,
                "a refused write called or resumed"
            );
        }
        result
    }

    /// [`MockHost::attempt`] a write that must refuse: its refusal.
    #[track_caller]
    pub fn refused<T: std::fmt::Debug>(&self, write: impl FnOnce() -> Result<T, Error>) -> Error {
        self.attempt(write).expect_err("the write was refused")
    }

    /// Everything a write leaves on this host, copied.
    pub(crate) fn written(&self) -> Written {
        let mock = self.borrow();
        Written {
            state: mock.state.clone(),
            blobs: mock.blobs.clone(),
            sent: (
                mock.output.clone(),
                mock.emissions.clone(),
                mock.events.clone(),
            ),
            waits: (mock.calls.clone(), mock.deferred, mock.resumes.clone()),
        }
    }

    /// Puts back what [`written`](MockHost::written) copied: the undo.
    pub(crate) fn restore(&self, written: Written) {
        let mut mock = self.borrow_mut();
        mock.state = written.state;
        mock.blobs = written.blobs;
        (mock.output, mock.emissions, mock.events) = written.sent;
        (mock.calls, mock.deferred, mock.resumes) = written.waits;
    }

    /// One host call from a context whose env is `env`, as the real host
    /// answers it. An emitted message is kept, numbered from 0, for the
    /// test to run.
    pub(crate) fn serve(&self, env: &Env, op: HostOp) -> HostReply {
        if let HostOp::Resource { program, key } = op {
            let owners = self.borrow_mut().owners.remove(&program);
            return HostReply::Query(match owners {
                Some(owners) => {
                    let asked = Env {
                        program: program.clone(),
                        origin: Origin::Program(env.program.clone()),
                        sender: None,
                        cause: Cause::Direct,
                        ..env.clone()
                    };
                    let answer = owners(asked, &key).map_err(crate::kernel::refusal_from);
                    self.borrow_mut().owners.insert(program, owners);
                    answer
                }
                None => Err(abi::Refusal::new(code::UNKNOWN_PROGRAM, program)),
            });
        }
        if let HostOp::Query { program, request } = op {
            // The sibling leaves the map while it answers, so it may use
            // this host itself.
            let sibling = self.borrow_mut().siblings.remove(&program);
            return HostReply::Query(match sibling {
                Some(sibling) => {
                    let asked = Env {
                        program: program.clone(),
                        origin: Origin::Program(env.program.clone()),
                        sender: None,
                        cause: Cause::Direct,
                        ..env.clone()
                    };
                    let answer = sibling(asked, &request).map_err(crate::kernel::refusal_from);
                    self.borrow_mut().siblings.insert(program, sibling);
                    answer
                }
                None => Err(abi::Refusal::new(code::UNKNOWN_PROGRAM, program)),
            });
        }
        let mut mock = self.borrow_mut();
        match op {
            HostOp::Get(key) | HostOp::CommittedGet(key) => {
                HostReply::Value(mock.state.get(&key).cloned())
            }
            HostOp::Scan(scan) | HostOp::CommittedScan(scan) => {
                HostReply::Entries(mock.scan_state(&scan.into()))
            }
            HostOp::BlobGet(id) => HostReply::Blob(mock.blobs.get(&id).cloned()),
            HostOp::BlobStat(id) => {
                HostReply::BlobHeader(mock.blobs.get(&id).map(|blob| BlobHeader {
                    kind: blob.kind.clone(),
                    len: blob.body.len() as u64,
                }))
            }
            HostOp::BlobRead { id, offset, len } => {
                HostReply::Value(mock.blobs.get(&id).map(|b| {
                    let start = (offset as usize).min(b.body.len());
                    let end = start.saturating_add(len as usize).min(b.body.len());
                    b.body[start..end].to_vec()
                }))
            }
            HostOp::Root(_) => HostReply::Root(None),
            HostOp::Crypto(CryptoOp::Sha256(bytes)) => {
                HostReply::Crypto(CryptoReply::Digest(sha2::Sha256::digest(bytes).into()))
            }
            HostOp::Crypto(CryptoOp::Verify {
                scheme,
                key,
                namespace,
                message,
                signature,
            }) => match &mock.verifier {
                Some(verify) => HostReply::Crypto(CryptoReply::Verified(verify(
                    scheme, &key, &namespace, &message, &signature,
                ))),
                None => HostReply::Refused(abi::Refusal::new(
                    code::UNSUPPORTED,
                    "MockHost verifies nothing until a verifier is set",
                )),
            },
            HostOp::Set { key, value } => {
                mock.touch(&key, None);
                mock.state.insert(key, value);
                HostReply::Done
            }
            HostOp::SetOwned { key, value, kind } => {
                mock.touch(&key, Some(kind));
                mock.state.insert(key, value);
                HostReply::Done
            }
            HostOp::Delete(key) => {
                mock.touch(&key, None);
                mock.state.remove(&key);
                HostReply::Done
            }
            HostOp::BlobPut { hash, kind, body } => match blob_id(hash, &kind, &body) {
                Ok(id) => {
                    mock.blobs.insert(id, Blob { kind, body });
                    HostReply::BlobId(id)
                }
                Err(error) => HostReply::Refused(crate::kernel::refusal_from(error)),
            },
            HostOp::Emit(message) => {
                let item = mock.next_message;
                mock.next_message += 1;
                mock.emissions.push(message);
                HostReply::Item(abi::ItemRef {
                    source: env.program.clone(),
                    item,
                })
            }
            HostOp::Event(payload) => {
                mock.events.push(payload);
                HostReply::Done
            }
            HostOp::Output(bytes) => {
                mock.output = bytes;
                HostReply::Done
            }
            HostOp::Respond(bytes) => {
                mock.response = bytes;
                HostReply::Done
            }
            HostOp::Await {
                target,
                payload,
                acting,
            } => {
                let ticket = mock.next_ticket;
                mock.next_ticket += 1;
                mock.calls.push(Called {
                    ticket,
                    target,
                    payload,
                    acting,
                });
                HostReply::Ticket(ticket)
            }
            HostOp::Defer => match env.cause {
                Cause::Call { ticket } => {
                    mock.deferred = true;
                    HostReply::Ticket(ticket)
                }
                _ => HostReply::Refused(abi::Refusal::new(
                    code::WRONG_STATE,
                    "this run answers no call",
                )),
            },
            HostOp::Resume {
                frame,
                ticket,
                answer,
            } => {
                let from_the_role = env.roles.get(role::AWAITS) == Some(&env.program);
                if !from_the_role {
                    return HostReply::Refused(abi::Refusal::new(
                        code::UNAUTHORIZED,
                        "only the awaits role resumes a frame",
                    ));
                }
                let answer = answer.map_err(crate::kernel::error_from);
                mock.resumes.push(Resumed {
                    frame,
                    ticket,
                    answer,
                });
                HostReply::Done
            }
            HostOp::Query { .. } | HostOp::Resource { .. } => unreachable!("answered above"),
        }
    }

    pub fn take_calls(&self) -> Vec<Called> {
        std::mem::take(&mut self.borrow_mut().calls)
    }

    pub fn take_deferred(&self) -> bool {
        std::mem::take(&mut self.borrow_mut().deferred)
    }

    pub fn take_resumes(&self) -> Vec<Resumed> {
        std::mem::take(&mut self.borrow_mut().resumes)
    }
}

impl MockState {
    fn touch(&mut self, key: &[u8], claim: Option<String>) {
        let existed = self.state.contains_key(key);
        let touch = self.touched.entry(key.to_vec()).or_insert(Touch {
            existed,
            claimed: None,
        });
        if claim.is_some() {
            touch.claimed = claim;
        }
    }

    fn scan_state(&self, range: &Range) -> Vec<Entry> {
        let admitted =
            self.state
                .iter()
                .filter(|(key, _)| range.admits(key))
                .map(|(key, value)| Entry {
                    key: key.clone(),
                    value: value.clone(),
                });
        let ordered: Vec<Entry> = if range.order == Order::Descending {
            admitted.rev().collect()
        } else {
            admitted.collect()
        };
        match range.limit {
            Some(limit) => ordered.into_iter().take(limit as usize).collect(),
            None => ordered,
        }
    }
}

/// The origins a test sends from, each with the sender the host would
/// resolve for it (an account, or `None` when the key or program holds none).
impl Env {
    /// Sent by the chain itself, as genesis and the kernel's own calls are:
    /// the most trusted frame, which no person's write ever is.
    pub fn root(self) -> Env {
        Env {
            origin: Origin::Root,
            sender: Some(Principal::Root),
            ..self
        }
    }

    /// Signed by `key`, acting as `account`: the account identity says the
    /// key holds, or `None` for a key that holds none (a write refuses it).
    pub fn signed(self, key: impl Into<Vec<u8>>, account: Option<AccountNumber>) -> Env {
        Env {
            origin: Origin::Signed(key.into()),
            sender: account.map(Principal::Account),
            ..self
        }
    }

    /// Sent by `program` (a message it emitted), acting as `account`, the
    /// account identity registered for it.
    pub fn from_program(
        self,
        program: impl Into<ProgramId>,
        account: Option<AccountNumber>,
    ) -> Env {
        Env {
            origin: Origin::Program(program.into()),
            sender: account.map(Principal::Account),
            ..self
        }
    }
}

/// The host's framing: `<kind> <len>\0<body>`, hashed whole.
pub fn blob_id(hash: HashKind, kind: &str, body: &[u8]) -> Result<BlobId, Error> {
    let framed = blob::frame(kind, body).map_err(crate::kernel::error_from)?;
    id_of(hash, &framed)
}

fn id_of(hash: HashKind, framed: &[u8]) -> Result<BlobId, Error> {
    blob::id_of(hash, framed).map_err(|collision| crate::kernel::error_from(collision.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Program;

    /// Answers who asked it, and at what height.
    struct Echo;

    impl Program for Echo {
        const NAME: &'static str = "echo";
        type Op = ();
        type Query = ();
        type Reply = (Origin, u64);

        fn execute(_: &ExecCtx, (): ()) -> Result<(), Error> {
            Ok(())
        }

        fn query(ctx: &QueryCtx, (): ()) -> Result<(Origin, u64), Error> {
            Ok((ctx.env().origin.clone(), ctx.env().height))
        }
    }

    /// A test's frame acts as a person, as every frame the chain sends a
    /// program outside genesis does; the chain itself is asked for by name.
    #[test]
    fn a_test_frame_acts_as_a_person_unless_it_says_root() {
        let host = MockHost::default();
        assert_eq!(host.exec(MockHost::env("m")).sender_account().unwrap(), 1);
        let chain = host.exec(MockHost::env("m").root());
        assert_eq!(chain.sender_account().unwrap_err().code, code::UNAUTHORIZED);
    }

    #[test]
    fn a_sibling_answers_as_the_host_asks_it() {
        let (asker, echo) = (MockHost::default(), MockHost::default());
        asker.sibling::<Echo>("echo", &echo);
        let ctx = asker.query(Env {
            height: 5,
            ..MockHost::env("chat")
        });
        let answer: (Origin, u64) = ctx.query("echo", &()).unwrap();
        assert_eq!(answer, (Origin::Program("chat".into()), 5));
        let unknown = ctx.query_raw("nobody", Vec::new()).unwrap_err();
        assert_eq!(unknown.code, code::UNKNOWN_PROGRAM);
    }
    #[test]
    fn a_sha1_blob_gets_git_s_id_and_a_shattered_half_is_refused() {
        let hello = blob_id(HashKind::Sha1, "blob", b"hello\n").unwrap();
        assert_eq!(
            abi::hex(hello.digest()),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        for half in [
            include_bytes!("../fixtures/shattered-1.bin"),
            include_bytes!("../fixtures/shattered-2.bin"),
        ] {
            let refused = id_of(HashKind::Sha1, half).unwrap_err();
            assert_eq!(refused.code, code::INVALID_INPUT);
            assert!(
                refused
                    .message
                    .contains("f92d74e3874587aaf443d1db961d4e26dde13e9c"),
                "{}",
                refused.message
            );
        }
    }
}
