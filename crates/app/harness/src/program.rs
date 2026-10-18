//! The program: every op and query handed to its function. An attribution may start a thread's
//! loop, which awaits the model and each tool call. At its init harness registers with
//! attribution, which then hands it every change about an account whose script calls harness;
//! the egress role hands it each event of a model's streamed reply.
use ::program::schema::{Return, returns};
use ::program::{Awaiting, Ctx, Error, ExecCtx, Program, QueryCtx};

use crate::agents::{attributed, close, setup};
use crate::bookings::{book, run_booked, unbook};
use crate::conversations::{fork_thread, new_thread, open_thread};
use crate::models::{remove_model, set_model};
use crate::streams::streamed;
use crate::waiting::due;
use crate::{Op, PROGRAM, Query, Reply, attributed_as, bookings, queries, resume, tallies, tools};

pub struct Harness;

impl Program for Harness {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = &[
        returns::<u64>("NewThread"),
        returns::<u64>("ForkThread"),
        returns::<u64>("OpenThread"),
    ];

    fn init(ctx: &ExecCtx, _params: &[u8]) -> Result<(), Error> {
        let register = attribution::Op::Register {
            deliver: attributed_as(),
        };
        ctx.emit(attribution::PROGRAM, &register, ::program::Reply::None);
        Ok(())
    }

    fn access(ctx: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        crate::gate::needs(ctx, op)
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        crate::gate::resource(ctx, key)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Agent { agent } => Ok(Reply::Agent(setup(ctx, agent)?)),
            Query::Thread { thread } => Ok(Reply::Thread(queries::thread(ctx, thread)?)),
            Query::Threads { agent, page } => {
                Ok(Reply::Threads(queries::threads(ctx, agent, &page)?))
            }
            Query::Frames { thread, page } => {
                Ok(Reply::Frames(queries::frames(ctx, thread, &page)?))
            }
            Query::Held { thread } => Ok(Reply::Held(queries::held(ctx, thread)?)),
            Query::Calls { thread, page } => Ok(Reply::Calls(queries::calls(ctx, thread, &page)?)),
            Query::Tools => Ok(Reply::Tools(tools::infos())),
            Query::ThreadAt { agent, place } => {
                Ok(Reply::Thread(queries::thread_at(ctx, agent, place)?))
            }
            Query::Model { id } => Ok(Reply::Model(queries::model(ctx, id)?)),
            Query::Models { page } => Ok(Reply::Models(queries::models(ctx, &page)?)),
            Query::Withheld { thread } => Ok(Reply::Withheld(queries::withheld(ctx, thread)?)),
            Query::Partial { thread } => Ok(Reply::Partial(queries::partial(ctx, thread)?)),
            Query::Topic { thread } => Ok(Reply::Topic(queries::topic(ctx, thread)?)),
            Query::Bound { agent, page } => Ok(Reply::Threads(queries::bound(ctx, agent, &page)?)),
            Query::Tally { thread } => Ok(Reply::Tally(tallies::tally(ctx, thread)?)),
            Query::FramesFrom { thread, from, page } => Ok(Reply::Frames(queries::frames_from(
                ctx, thread, from, &page,
            )?)),
            Query::Call { thread, call } => Ok(Reply::Call(queries::call(ctx, thread, call)?)),
            Query::Bookings { agent, page } => {
                Ok(Reply::Bookings(bookings::bookings(ctx, agent, &page)?))
            }
            Query::Booking { entry } => Ok(Reply::Booking(bookings::booking(ctx, entry)?)),
            Query::UsageWindow { service } => {
                Ok(Reply::UsageWindow(crate::state::USAGE.get(ctx, &service)?))
            }
            Query::Settling { thread } => Ok(Reply::Settling(queries::settling(ctx, thread)?)),
        }
    }
}

impl Awaiting for Harness {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Close { thread } => close(&ctx, thread),
            Op::Attributed(change) => attributed(ctx, *change).await,
            Op::SetModel {
                id,
                wire,
                service,
                name,
            } => set_model(&ctx, id, wire, service, name),
            Op::RemoveModel { id } => remove_model(&ctx, id),
            Op::Streamed { thread, event } => streamed(&ctx, thread, event).await,
            Op::NewThread { agent } => new_thread(&ctx, agent),
            Op::ForkThread { agent } => fork_thread(&ctx, agent),
            Op::OpenThread { agent, place } => open_thread(&ctx, agent, place),
            Op::Book {
                height,
                target,
                payload,
                thread,
            } => book(&ctx, height, target, payload, thread).await,
            Op::Unbook { entry } => unbook(&ctx, entry).await,
            Op::RunBooked => run_booked(&ctx).await,
            Op::Resume { thread } => resume::resume(ctx, thread).await,
            Op::Report { manager } => resume::report(ctx, manager).await,
            Op::Settle { thread } => due(ctx, thread).await,
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(Harness);
