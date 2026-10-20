//! The program: every op and query handed to its function. An attribution may start a thread's
//! loop, which awaits the model and each tool call. At its init harness registers with
//! attribution for every change about an account whose script calls harness.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Awaiting, Ctx, Error, ExecCtx, Program, QueryCtx};

use crate::agents::{attributed, card, close, setup};
use crate::bookings::{book, run_booked, unbook};
use crate::conversations::{fork_thread, new_thread, open_thread};
use crate::models::{remove_model, set_compact_at, set_model, set_services};
use crate::streams::streamed;
use crate::waiting::due;
use crate::waits::{pod_event, pod_wait, unwait, wait, wait_due};
use crate::{
    Op, PROGRAM, Query, Reply, attributed_as, bookings, budget, queries, resume, tallies, tools,
    waits,
};

pub struct Harness;

impl Program for Harness {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

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
            Query::Budget => Ok(Reply::Budget(budget::report(ctx)?)),
            Query::Waits { agent, page } => Ok(Reply::Waits(waits::waits(ctx, agent, &page)?)),
            Query::Wait { wait } => Ok(Reply::Wait(waits::wait_row(ctx, wait)?)),
            Query::Armed { thread, page } => Ok(Reply::Waits(waits::armed(ctx, thread, &page)?)),
            Query::Card { agent } => Ok(Reply::Agent(card(ctx, agent)?)),
            Query::Wakes { thread } => Ok(Reply::Wakes(crate::wakes::wakes(ctx, thread)?)),
            Query::AgentTools { agent } => Ok(Reply::AgentTools(tools::of_agent(ctx, agent)?)),
            Query::Cards { page } => Ok(Reply::Cards(crate::cards::cards(ctx, &page)?)),
            Query::Services { model } => Ok(Reply::Services(crate::routes::services(ctx, &model)?)),
            Query::Route { thread } => Ok(Reply::Route(crate::routes::route(ctx, thread)?)),
            Query::Routes { page } => Ok(Reply::Routes(crate::routes::routes(ctx, &page)?)),
            Query::Answerers { thread, page } => Ok(Reply::Answerers(crate::routes::answerers(
                ctx, thread, &page,
            )?)),
        }
    }
}

impl Awaiting for Harness {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Close { thread } => close(&ctx, thread).await,
            Op::Attributed(change) => attributed(ctx, *change).await,
            Op::SetModel {
                id,
                wire,
                service,
                name,
            } => set_model(&ctx, id, wire, service, name),
            Op::RemoveModel { id } => remove_model(&ctx, id),
            Op::SetCompactAt { id, compact_at } => set_compact_at(&ctx, id, compact_at),
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
            Op::Watch { watch } => crate::watches::watch(ctx, watch).await,
            Op::Heard { watch, event } => crate::watches::heard(&ctx, watch, event).await,
            Op::ReadBudget { service } => budget::read(ctx, service).await,
            Op::Wait { thread, event } => wait(&ctx, thread, event).await,
            Op::Unwait { wait } => unwait(&ctx, wait).await,
            Op::WaitDue => wait_due(&ctx).await,
            Op::PodWait { wait } => pod_wait(&ctx, wait).await,
            Op::PodEvent { wait, event } => pod_event(&ctx, wait, event).await,
            Op::Forged(event) => waits::forged(&ctx, event).await,
            Op::ClaimConversations { agent } => crate::conversations::claim(&ctx, agent),
            Op::AdoptOwners { kind, after, limit } => {
                crate::adopt::adopt_owners(&ctx, &kind, after, limit)
            }
            Op::Filed(row) => waits::filed(&ctx, row).await,
            Op::SetServices { id, services } => set_services(&ctx, id, services),
            Op::ReadBudgets => budget::read_all(ctx).await,
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(Harness);
