//! An egress service called as the agent: the write that asks it and waits for its whole answer,
//! the code a refusal is read as from its status, and one event of a text/event-stream answer.
use ::program::{Error, ExecCtx, code};
use abi::role::egress;

use super::Write;

pub(crate) fn whole(ctx: &ExecCtx, request: egress::Request) -> Result<Vec<Write>, Error> {
    let target = ctx.env().role(abi::role::EGRESS)?.clone();
    let op = egress::Op::Request {
        request,
        delivery: egress::Delivery::Whole,
        wait: None,
    };
    let payload = abi::encode(&op);
    Ok(vec![Write { target, payload }])
}

pub(crate) fn refused_as(status: u16) -> &'static str {
    match status {
        400 | 422 => code::INVALID_INPUT,
        403 => code::UNAUTHORIZED,
        404 => code::NOT_FOUND,
        503 => code::UNSUPPORTED,
        _ => code::WRONG_STATE,
    }
}

pub(crate) fn event(data: &[u8]) -> (String, String) {
    let text = String::from_utf8_lossy(data);
    let mut name = "message".to_owned();
    let mut lines = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            name = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("data:") {
            lines.push(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    (name, lines.join("\n"))
}
