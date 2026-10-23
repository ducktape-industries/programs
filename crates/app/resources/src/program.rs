//! The program: one match over every op and one over every query, each handed to its rule.
use ::program::schema::Return;
use ::program::tools::{Described, Tools};
use ::program::{Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{
    admission, admit, beat, cancel, claim, confirm, count, decline, evict, holds, in_force, lapse,
    ledger, missed, offer, offer_view, offers, passed_over, pick, place, queued, rank,
    register_kind, replied, report, request, request_view, set_policy, unclaimed, why, withdraw,
};
use crate::state::{ADMITTED, ASSIGNED, KINDS, POLICIES, RECORDS, USED};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Resources;

impl Program for Resources {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;

    fn tools() -> Tools {
        Tools::of::<Op, Query>()
    }

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        match op {
            Op::RegisterKind { name, spec } => register_kind(ctx, name, spec),
            Op::SetPolicy { kind, policy } => set_policy(ctx, kind, policy),
            Op::Admit { kind, provider } => admit(ctx, kind, provider),
            Op::Evict { kind, provider } => evict(ctx, kind, provider),
            Op::Offer { offer: offered } => offer(ctx, offered),
            Op::Withdraw { name } => withdraw(ctx, name),
            Op::Beat {
                name,
                free,
                readings,
            } => beat(ctx, name, free, readings),
            Op::Request { spec } => request(ctx, spec),
            Op::Pick { spec } => pick(ctx, spec),
            Op::Cancel { request: id } => cancel(ctx, id),
            Op::Claim {
                request: id,
                attempt,
            } => claim(ctx, id, attempt),
            Op::Decline {
                request: id,
                attempt,
                why: said,
            } => decline(ctx, id, attempt, said),
            Op::Report {
                request: id,
                attempt,
                outcome,
                readings,
                evidence,
            } => report(ctx, id, attempt, outcome, readings, evidence),
            Op::Count {
                request: id,
                attempt,
                outcome,
                readings,
            } => count(ctx, id, attempt, outcome, readings),
            Op::Missed {
                request: id,
                attempt,
            } => missed(ctx, id, attempt),
            Op::Confirm {
                request: id,
                attempt,
                readings,
            } => confirm(ctx, id, attempt, readings),
            Op::Lapse { offer: id } => lapse(ctx, id),
            Op::Unclaimed {
                request: id,
                attempt,
            } => unclaimed(ctx, id, attempt),
            Op::Place { kind } => place(ctx, kind),
            Op::Rank {
                kind,
                provider,
                tier,
            } => rank(ctx, kind, provider, tier),
        }
    }

    fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        replied(ctx, id, outcome)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Kind { name } => Reply::Kind(KINDS.get(ctx, &name)?),
            Query::Kinds { page } => Reply::Kinds(KINDS.range(ctx, &page)?.map(|(_, row)| row)),
            Query::Policy { kind } => {
                Reply::Policy(POLICIES.get(ctx, &kind)?.map(|row| row.policy))
            }
            Query::Admitted { kind, page } => {
                Reply::Admitted(ADMITTED.range_of(ctx, &kind, &page)?.map(|(_, row)| row))
            }
            Query::Offer { offer: id } => Reply::Offer(offer_view(ctx, &id)?),
            Query::Offers { kind, page } => Reply::Offers(offers(ctx, kind, &page)?),
            Query::Request { id } => Reply::Request(request_view(ctx, id)?),
            Query::Queued { kind, page } => Reply::Requests(queued(ctx, kind, &page)?),
            Query::Assignment {
                request: id,
                attempt,
            } => Reply::Assignment(ASSIGNED.get(ctx, &(id, attempt))?),
            Query::Holds { offer: id, page } => Reply::Assignments(holds(ctx, id, &page)?),
            Query::Usage {
                request: id,
                attempt,
            } => Reply::Usage(USED.get(ctx, &(id, attempt))?),
            Query::Ledger { kind, group, page } => Reply::Ledger(ledger(ctx, kind, group, &page)?),
            Query::Record { offer: id } => {
                Reply::Record(RECORDS.get(ctx, &(id.provider, id.name))?)
            }
            Query::Why { request: id } => Reply::Why(why(ctx, id)?),
            Query::PassedOver { request: id, page } => {
                Reply::PassedOver(passed_over(ctx, id, &page)?)
            }
            Query::InForce { kind } => Reply::InForce(in_force(ctx, &kind)?),
            Query::Admission { kind, provider } => {
                Reply::Admission(admission(ctx, kind, provider)?)
            }
        })
    }
}

#[cfg(feature = "program")]
::program::export!(Resources);
