//! Programs whose execute awaits other programs: [`Awaiting`], the owned [`Ctx`] it runs with,
//! the [`Call`] future a call is, [`all`] for calls awaited together, and the executor that polls
//! a frame and parks it while it waits.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::ops::Deref;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use abi::{HostOp, HostReply};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::ctx::Frame;
use crate::{
    AccountNumber, Cause, EntryId, Env, Error, ExecCtx, Program, ProgramId, Ticket, decoded,
};

pub type Answer = Result<Vec<u8>, Error>;

pub trait Awaiting: Program<Op: 'static> {
    fn execute(ctx: Ctx, op: Self::Op) -> impl Future<Output = Result<(), Error>> + 'static;
}

type Answers = Rc<RefCell<BTreeMap<Ticket, Answer>>>;

pub struct Ctx {
    exec: ExecCtx,
    answers: Answers,
}

impl Deref for Ctx {
    type Target = ExecCtx;

    fn deref(&self) -> &ExecCtx {
        &self.exec
    }
}

impl Ctx {
    fn unsent(&self, target: ProgramId, payload: Vec<u8>, acting: Option<AccountNumber>) -> Call {
        Call {
            frame: self.exec.frame().clone(),
            answers: self.answers.clone(),
            state: State::Unsent {
                target,
                payload,
                acting,
            },
        }
    }

    pub fn call_raw(&self, target: impl Into<ProgramId>, payload: impl Into<Vec<u8>>) -> Call {
        self.unsent(target.into(), payload.into(), None)
    }

    pub fn call_raw_as(
        &self,
        account: AccountNumber,
        target: impl Into<ProgramId>,
        payload: impl Into<Vec<u8>>,
    ) -> Call {
        self.unsent(target.into(), payload.into(), Some(account))
    }

    pub async fn call<O: BorshSerialize, R: BorshDeserialize>(
        &self,
        target: impl Into<ProgramId>,
        op: &O,
    ) -> Result<R, Error> {
        let answer = self.call_raw(target, abi::encode(op)).await?;
        abi::decode(&answer).map_err(crate::kernel::error_from)
    }

    pub async fn call_as<O: BorshSerialize, R: BorshDeserialize>(
        &self,
        account: AccountNumber,
        target: impl Into<ProgramId>,
        op: &O,
    ) -> Result<R, Error> {
        let answer = self.call_raw_as(account, target, abi::encode(op)).await?;
        abi::decode(&answer).map_err(crate::kernel::error_from)
    }

    pub async fn schedule<O: BorshSerialize>(
        &self,
        height: u64,
        target: impl Into<ProgramId>,
        op: &O,
    ) -> Result<EntryId, Error> {
        let scheduler = self.env().role(abi::role::SCHEDULER)?.clone();
        let schedule = abi::role::scheduler::Op::Schedule {
            height,
            target: target.into(),
            payload: abi::encode(op),
        };
        self.call(scheduler, &schedule).await
    }

    pub async fn cancel(&self, entry: EntryId) -> Result<(), Error> {
        let scheduler = self.env().role(abi::role::SCHEDULER)?.clone();
        let cancel = abi::role::scheduler::Op::Cancel { entry };
        self.call(scheduler, &cancel).await
    }
}

type Pending<F> = (Pin<Box<F>>, Option<<F as Future>::Output>);

pub async fn all<F: Future>(futures: impl IntoIterator<Item = F>) -> Vec<F::Output> {
    let mut pending: Vec<Pending<F>> = futures
        .into_iter()
        .map(|future| (Box::pin(future), None))
        .collect();
    std::future::poll_fn(|context| {
        for (future, output) in pending.iter_mut() {
            let waiting = output.is_none();
            if waiting && let Poll::Ready(ready) = future.as_mut().poll(context) {
                *output = Some(ready);
            }
        }
        let every = pending.iter().all(|(_, output)| output.is_some());
        match every {
            true => Poll::Ready(()),
            false => Poll::Pending,
        }
    })
    .await;
    pending
        .into_iter()
        .filter_map(|(_, output)| output)
        .collect()
}

pub struct Call {
    frame: Rc<Frame>,
    answers: Answers,
    state: State,
}

enum State {
    Unsent {
        target: ProgramId,
        payload: Vec<u8>,
        acting: Option<AccountNumber>,
    },
    Waiting(Ticket),
}

impl Call {
    fn ticket(&mut self) -> Result<Ticket, Error> {
        let State::Unsent {
            target,
            payload,
            acting,
        } = &mut self.state
        else {
            let State::Waiting(ticket) = self.state else {
                unreachable!("a call is unsent or waiting")
            };
            return Ok(ticket);
        };
        let op = HostOp::Await {
            target: std::mem::take(target),
            payload: std::mem::take(payload),
            acting: *acting,
        };
        let ticket = match self.frame.host(op) {
            HostReply::Ticket(ticket) => ticket,
            HostReply::Refused(refusal) => return Err(crate::kernel::error_from(refusal)),
            other => panic!("host answered {other:?} where a ticket was expected"),
        };
        self.state = State::Waiting(ticket);
        Ok(ticket)
    }
}

impl Future for Call {
    type Output = Answer;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Answer> {
        let this = self.get_mut();
        let ticket = match this.ticket() {
            Ok(ticket) => ticket,
            Err(refused) => return Poll::Ready(Err(refused)),
        };
        match this.answers.borrow_mut().remove(&ticket) {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }
}

type Root = Pin<Box<dyn Future<Output = Result<(), Error>>>>;

pub struct Suspended {
    root: Root,
    frame: Rc<Frame>,
    answers: Answers,
}

pub enum Step {
    Done(Result<(), Error>),
    Parked(Suspended),
}

pub fn start<A: Awaiting>(exec: ExecCtx, payload: &[u8]) -> Step {
    let env = exec.env();
    if let Cause::Reply { id, outcome } = &env.cause {
        return Step::Done(A::reply(&exec, id, outcome));
    }
    let op = match decoded::<A::Op>(&env.program, "Op", payload) {
        Ok(op) => op,
        Err(refused) => return Step::Done(Err(refused)),
    };
    let frame = exec.frame().clone();
    let answers = Answers::default();
    let ctx = Ctx {
        exec,
        answers: answers.clone(),
    };
    let root = Box::pin(<A as Awaiting>::execute(ctx, op));
    poll(Suspended {
        root,
        frame,
        answers,
    })
}

pub fn resume(suspended: Suspended, env: Env, ticket: Ticket, answer: Answer) -> Step {
    suspended.frame.move_to(env);
    suspended.answers.borrow_mut().insert(ticket, answer);
    poll(suspended)
}

fn poll(mut suspended: Suspended) -> Step {
    let mut context = Context::from_waker(Waker::noop());
    match suspended.root.as_mut().poll(&mut context) {
        Poll::Ready(done) => Step::Done(done),
        Poll::Pending => Step::Parked(suspended),
    }
}

#[cfg(target_arch = "wasm32")]
pub mod exports {
    use std::cell::RefCell;

    use abi::{Invocation, ProgramCall, ProgramReply, Refusal, reason};

    use super::{Awaiting, Step, Suspended, resume, start};
    use crate::{Error, ExecCtx, QueryCtx, access, code, hook, query, resource};

    #[link(wasm_import_module = "ducktape")]
    unsafe extern "C" {
        fn host_park();
    }

    thread_local! {
        static PARKED: RefCell<Option<Suspended>> = const { RefCell::new(None) };
    }

    pub fn call<A: Awaiting>(ptr: u32, len: u32) -> u64 {
        let request = unsafe { Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize) };
        let reply = match abi::decode::<Invocation>(&request) {
            Ok(invocation) => dispatch::<A>(invocation),
            Err(fault) => Err(Refusal::new(reason::PROTOCOL, fault.sentence)),
        };
        crate::exports::leak(abi::encode(&reply))
    }

    fn dispatch<A: Awaiting>(Invocation { env, call }: Invocation) -> ProgramReply {
        let env = env.into();
        let reply = match call {
            ProgramCall::Init(params) => A::init(&ExecCtx::new(env), &params),
            ProgramCall::Execute(payload) => park(start::<A>(ExecCtx::new(env), &payload)),
            ProgramCall::Resume { ticket, answer } => {
                let answer = answer.map_err(crate::kernel::error_from);
                match PARKED.with(|parked| parked.borrow_mut().take()) {
                    Some(suspended) => park(resume(suspended, env, ticket, answer)),
                    None => Err(Error::new(code::PROTOCOL, "nothing is parked")),
                }
            }
            ProgramCall::Query(request) => query::<A>(&QueryCtx::new(env), &request),
            ProgramCall::Access(payload) => access::<A>(&QueryCtx::new(env), &payload),
            ProgramCall::Resource(key) => resource::<A>(&QueryCtx::new(env), &key),
            ProgramCall::Hook(record) => hook::<A>(&ExecCtx::new(env), record),
        };
        reply.map_err(crate::kernel::refusal_from)
    }

    fn park(step: Step) -> Result<(), Error> {
        match step {
            Step::Done(done) => done,
            Step::Parked(suspended) => {
                PARKED.with(|parked| *parked.borrow_mut() = Some(suspended));
                unsafe { host_park() };
                Ok(())
            }
        }
    }
}

#[macro_export]
macro_rules! export_awaiting {
    ($program:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(len: u32) -> u32 {
            $crate::exports::alloc(len)
        }

        #[cfg(target_arch = "wasm32")]
        #[unsafe(no_mangle)]
        pub extern "C" fn call(ptr: u32, len: u32) -> u64 {
            $crate::awaiting::exports::call::<$program>(ptr, len)
        }
    };
}
