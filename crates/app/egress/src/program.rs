//! The program: every op and every query, each handed to its rule.
use ::program::{Awaiting, Ctx, Error, ExecCtx, MessageId, Outcome, Program, QueryCtx};

use crate::rules::{
    answer, assign, assigned, end_stream, events, expire, node, nodes, register, request,
    request_row, serve, service, services, serving,
};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Egress;

impl Program for Egress {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const TABLES: &'static [::program::Table] = crate::state::TABLES;

    fn access(_: &QueryCtx, op: &Op) -> Result<Vec<::program::Need>, Error> {
        Ok(crate::rules::needs(op))
    }

    fn resource(ctx: &QueryCtx, key: &str) -> Result<::program::Resource, Error> {
        crate::rules::resource(ctx, key)
    }

    fn reply(_: &ExecCtx, _: &MessageId, _: &Outcome) -> Result<(), Error> {
        Ok(())
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Assigned { key } => Ok(Reply::Requests(assigned(ctx, &key)?)),
            Query::Node { key } => Ok(Reply::Node(node(ctx, &key)?)),
            Query::Request { id } => Ok(Reply::Request(request_row(ctx, id)?)),
            Query::Nodes { page } => Ok(Reply::Nodes(nodes(ctx, &page)?)),
            Query::Service { name } => Ok(Reply::Service(service(ctx, &name)?)),
            Query::Services { page } => Ok(Reply::Services(services(ctx, &page)?)),
            Query::Serving { service: name } => Ok(Reply::Serving(serving(ctx, &name)?)),
        }
    }
}

impl Awaiting for Egress {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Register { service: name } => register(&ctx, name),
            Op::Assign {
                service: name,
                nodes: keys,
            } => assign(&ctx, name, keys),
            Op::Serve { services: names } => serve(&ctx, names),
            Op::Request {
                request: asked,
                delivery,
                wait,
            } => request(&ctx, asked, delivery, wait).await,
            Op::Answer { id, answer: given } => answer(&ctx, id, given).await,
            Op::Events(given) => events(&ctx, given).await,
            Op::EndStream { id, events, end } => end_stream(&ctx, id, events, end).await,
            Op::Expire { id } => expire(&ctx, id),
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(Egress);
