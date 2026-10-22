//! The program: every op and every query, each handed to its rule.
use ::program::schema::{Return, returns};
use ::program::tools::Tools;
#[cfg(feature = "program")]
use ::program::{Awaiting, Ctx, ExecCtx, MessageId, Outcome};
use ::program::{Error, Program, QueryCtx};

use crate::queries::{home, link_row, linked, name, named, outbound, ports, receipts, status};
#[cfg(feature = "program")]
use crate::rules::{
    Sending, close, deliver, init, open, port, pump, repin, replied, send, send_for, take_header,
};
use crate::{Op, PROGRAM, Query, Reply};

pub struct Crosspond;

impl Program for Crosspond {
    const NAME: &'static str = PROGRAM;
    type Op = Op;
    type Query = Query;
    type Reply = Reply;
    const RETURNS: &'static [Return] = &[returns::<u64>("Send"), returns::<u64>("SendFor")];

    fn tools() -> Tools {
        Tools::of::<crate::tools::Ops, crate::tools::Queries>()
    }

    #[cfg(feature = "program")]
    fn init(ctx: &ExecCtx, params: &[u8]) -> Result<(), Error> {
        init(ctx, params)
    }

    #[cfg(feature = "program")]
    fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
        replied(ctx, id, outcome)
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        match query {
            Query::Link { network } => Ok(Reply::Link(link_row(ctx, &network)?)),
            Query::Outbound { to, after } => Ok(Reply::Outbound(outbound(ctx, &to, after)?)),
            Query::Receipts { from, after } => Ok(Reply::Receipts(receipts(ctx, &from, after)?)),
            Query::Status { to, seq } => Ok(Reply::Status(status(ctx, &to, seq)?)),
            Query::Ports => Ok(Reply::Ports(ports(ctx)?)),
            Query::Name(foreign) => Ok(Reply::Name(name(ctx, foreign)?)),
            Query::Named {
                network,
                name: held,
            } => Ok(Reply::Named(named(ctx, network, held)?)),
            Query::Home => Ok(Reply::Home(home(ctx)?)),
            Query::Linked { name } => Ok(Reply::Linked(linked(ctx, &name)?)),
        }
    }
}

#[cfg(feature = "program")]
impl Awaiting for Crosspond {
    async fn execute(ctx: Ctx, op: Op) -> Result<(), Error> {
        match op {
            Op::Open {
                network,
                name,
                epoch_length,
                epoch,
                kind,
                validators,
            } => open(&ctx, network, name, epoch_length, epoch, kind, validators),
            Op::Repin {
                network,
                epoch,
                kind,
                validators,
            } => repin(&ctx, network, epoch, kind, validators),
            Op::Close { network } => close(&ctx, network),
            Op::Port { role, from } => port(&ctx, role, from),
            Op::Send {
                to,
                port,
                op,
                deadline,
            } => {
                let sending = Sending {
                    to,
                    port,
                    op,
                    deadline,
                };
                send(&ctx, sending).await
            }
            Op::SendFor {
                to,
                port,
                op,
                deadline,
                account,
            } => {
                let sending = Sending {
                    to,
                    port,
                    op,
                    deadline,
                };
                send_for(&ctx, sending, account).await
            }
            Op::Pump { to } => pump(&ctx, to).await,
            Op::Header { from, header } => take_header(&ctx, from, header),
            Op::DeliverProven {
                from,
                epoch,
                low,
                packets,
            } => deliver(&ctx, from, epoch, low, packets),
        }
    }
}

#[cfg(feature = "program")]
::program::export_awaiting!(Crosspond);
