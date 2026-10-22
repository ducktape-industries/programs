//! What harness reads of chat for an agent goes inside chat's `As`, naming the agent: chat judges
//! the agent's seat, never harness's own, and refuses an agent's own `As` once it sits inside one.
use ::program::{AccountNumber, Error, QueryCtx};

pub(crate) fn chat_for(
    ctx: &QueryCtx,
    agent: AccountNumber,
    query: &chat::Query,
) -> Result<chat::Reply, Error> {
    ctx.query(chat::PROGRAM, &as_agent(agent, abi::encode(query)))
}

pub(crate) fn raw_for(
    ctx: &QueryCtx,
    agent: AccountNumber,
    program: String,
    payload: Vec<u8>,
) -> Result<Vec<u8>, Error> {
    let asks_chat = program == chat::PROGRAM;
    match asks_chat {
        true => ctx.query_raw(program, abi::encode(&as_agent(agent, payload))),
        false => ctx.query_raw(program, payload),
    }
}

fn as_agent(agent: AccountNumber, query: Vec<u8>) -> chat::Query {
    chat::Query::As {
        reader: chat::Principal::Account(agent),
        query,
    }
}
