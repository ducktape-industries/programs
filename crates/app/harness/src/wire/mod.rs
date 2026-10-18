//! The wires harness speaks to a model, one module each, reached through one `match` on
//! [`Wire`] per step: the turn request's parts, the reply read back as a turn, each streamed
//! event folded into a [`Partial`], the model's context window, and a thread's summary. What
//! they read back is harness's own: a [`Reply`] of [`Said`] blocks, a [`Summary`] the wire
//! alone reads again.
pub(crate) mod anthropic;

use abi::role::egress::http::{Head, Response};

use crate::{Flow, Frame, Partial, Role, Said, ServerTool, Stop, Thinking, Usage, Wire};

pub(crate) struct Header<'a> {
    pub name: &'a str,
    pub max_tokens: u32,
    pub thinking: Option<Thinking>,
    pub server_tools: &'a [ServerTool],
    pub system: &'a [String],
}

#[derive(Debug, PartialEq)]
pub(crate) struct Reply {
    pub model: String,
    pub said: Vec<Said>,
    pub stop: Stop,
    pub usage: Usage,
}

pub(crate) enum Answered {
    Reply(Reply),
    Overflow,
    Failed(String),
}

pub(crate) struct Summary {
    pub block: String,
    pub tokens: u64,
}

pub(crate) enum Summarized {
    Summary(Summary),
    Overflow,
    Failed(String),
}

pub(crate) fn header(wire: Wire, header: &Header) -> String {
    match wire {
        Wire::AnthropicMessages => anthropic::header(header),
    }
}

pub(crate) fn message(wire: Wire, role: Role, frames: &[Frame], first: bool, last: bool) -> String {
    match wire {
        Wire::AnthropicMessages => anthropic::message(role, frames, first, last),
    }
}

pub(crate) fn close(wire: Wire) -> &'static str {
    match wire {
        Wire::AnthropicMessages => anthropic::CLOSE,
    }
}

pub(crate) fn summarize(wire: Wire) -> &'static str {
    match wire {
        Wire::AnthropicMessages => anthropic::SUMMARIZE,
    }
}

pub(crate) fn summary(wire: Wire, written: Wire, block: &str, text: &str) -> String {
    let own = written == wire;
    match (wire, own) {
        (Wire::AnthropicMessages, true) => anthropic::summary_message(block),
        (Wire::AnthropicMessages, false) => anthropic::summary_text(text),
    }
}

pub(crate) fn summary_words(written: Wire, block: &str) -> String {
    match written {
        Wire::AnthropicMessages => anthropic::summary_words(block),
    }
}

pub(crate) fn turn_head(wire: Wire, summarized: bool) -> Head {
    match wire {
        Wire::AnthropicMessages => anthropic::head(summarized),
    }
}

pub(crate) fn summary_head(wire: Wire) -> Head {
    match wire {
        Wire::AnthropicMessages => anthropic::head(true),
    }
}

pub(crate) fn model_head(wire: Wire, name: &str) -> Head {
    match wire {
        Wire::AnthropicMessages => anthropic::model(name),
    }
}

pub(crate) fn answered(wire: Wire, response: &Response) -> Answered {
    match wire {
        Wire::AnthropicMessages => anthropic::answered(response),
    }
}

pub(crate) fn summarized(wire: Wire, response: &Response) -> Summarized {
    match wire {
        Wire::AnthropicMessages => anthropic::summarized(response),
    }
}

pub(crate) fn window(wire: Wire, response: &Response) -> Result<Option<u64>, String> {
    match wire {
        Wire::AnthropicMessages => anthropic::window(response),
    }
}

pub(crate) fn fold(partial: &mut Partial, data: &[u8]) {
    let Some(data) = event_data(data) else {
        return;
    };
    match partial.wire {
        Wire::AnthropicMessages => anthropic::fold(partial, &data),
    }
}

fn event_data(event: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(event);
    let data: Vec<&str> = text
        .split(['\n', '\r'])
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|value| value.strip_prefix(' ').unwrap_or(value))
        .collect();
    let has_data = !data.is_empty();
    has_data.then(|| data.join("\n"))
}

pub(crate) fn folded(partial: Partial) -> Answered {
    match partial.flow {
        Flow::Ended => Answered::Reply(Reply {
            model: partial.model,
            said: partial.said,
            stop: partial.stop.unwrap_or(Stop::EndTurn),
            usage: partial.usage,
        }),
        Flow::Overflowed => Answered::Overflow,
        Flow::Open => Answered::Failed(format!(
            "the model's reply ended after {} events, before the model finished it",
            partial.next_seq
        )),
        Flow::Broken { reason } => {
            Answered::Failed(format!("the model's streamed reply broke: {reason}"))
        }
        Flow::Failed { text } => Answered::Failed(text),
    }
}

#[cfg(test)]
pub(crate) mod tests;
