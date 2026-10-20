//! The wires harness speaks to a model, one module each, reached through one `match` on
//! [`Wire`] per step: the turn request's parts, the reply read back as a turn, each streamed
//! event folded into a [`Partial`], the model's context window, and a thread's summary. What
//! they read back is harness's own: a [`Reply`] of [`Said`] blocks, a [`Summary`] the wire
//! alone reads again.
pub(crate) mod anthropic;
pub(crate) mod harness;

use abi::role::egress::http::{Head, Response};

pub(crate) use self::harness::Lanes;
use crate::pictures::Looks;
use crate::{Awaited, Flow, Frame, Partial, Role, Said, ServerTool, Stop, Thinking, Usage, Wire};

pub(crate) struct Header<'a> {
    pub name: &'a str,
    pub max_tokens: u32,
    pub thinking: Option<Thinking>,
    pub server_tools: &'a [ServerTool],
    pub system: &'a [String],
    pub file_tools: &'a [crate::tools::file::FileTool],
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
    Pictures(String),
    Failed(String),
}

pub(crate) struct Summary {
    pub block: String,
    pub tokens: u64,
}

pub(crate) enum Summarized {
    Summary(Summary),
    Overflow,
    Pictures(String),
    Failed(String),
}

pub(crate) struct Learned {
    pub window: Option<u64>,
    pub provider: String,
}

pub(crate) fn header(wire: Wire, header: &Header) -> String {
    match wire {
        Wire::AnthropicMessages => anthropic::header(header),
        Wire::Harness => harness::header(header),
    }
}

// the frames in `foreign` hold server-tool blocks another account minted, which the wire sends as
// plain text
pub(crate) fn message(
    wire: Wire,
    role: Role,
    frames: &[Frame],
    (looks, foreign): (&Looks, &Foreign),
    lanes: &Lanes,
    first: bool,
    last: bool,
) -> String {
    match wire {
        Wire::AnthropicMessages => anthropic::message(role, frames, (looks, foreign), first, last),
        Wire::Harness => harness::message(role, frames, (looks, foreign), lanes, first),
    }
}

pub(crate) type Foreign = std::collections::BTreeSet<u64>;

pub(crate) fn answers(block: &str) -> Option<String> {
    Wire::ALL.iter().find_map(|wire| match wire {
        Wire::AnthropicMessages | Wire::Harness => anthropic::answers(block),
    })
}

// whether the model takes one image of `size` bytes, else why not
pub(crate) fn image_size(wire: Wire, size: u64) -> Result<(), String> {
    match wire {
        Wire::AnthropicMessages | Wire::Harness => anthropic::image_size(size),
    }
}

pub(crate) fn image_side(wire: Wire, width: u32, height: u32) -> Result<(), String> {
    match wire {
        Wire::AnthropicMessages | Wire::Harness => anthropic::image_side(width, height),
    }
}

pub(crate) fn close(wire: Wire) -> &'static str {
    match wire {
        Wire::AnthropicMessages => anthropic::CLOSE,
        Wire::Harness => harness::CLOSE,
    }
}

pub(crate) fn summarize(wire: Wire) -> &'static str {
    match wire {
        Wire::AnthropicMessages => anthropic::SUMMARIZE,
        Wire::Harness => harness::CLOSE,
    }
}

pub(crate) fn summary(wire: Wire, written: Wire, block: &str, text: &str) -> String {
    match (wire, written) {
        (Wire::AnthropicMessages, Wire::AnthropicMessages) => anthropic::summary_message(block),
        (Wire::AnthropicMessages, Wire::Harness) => anthropic::summary_text(text),
        (Wire::Harness, Wire::AnthropicMessages) => {
            let lane = model_wire::Lane::of(model_wire::anthropic::PROVIDER, "");
            harness::summary_message(lane, text, Some(block.to_owned()))
        }
        (Wire::Harness, Wire::Harness) => harness::own_summary(block),
    }
}

pub(crate) fn summary_words(written: Wire, block: &str) -> String {
    match written {
        Wire::AnthropicMessages => anthropic::summary_words(block),
        Wire::Harness => harness::summary_words(block),
    }
}

pub(crate) fn turn_head(wire: Wire) -> Head {
    match wire {
        Wire::AnthropicMessages => anthropic::head(),
        Wire::Harness => harness::turn_head(),
    }
}

pub(crate) fn summary_head(wire: Wire) -> Head {
    match wire {
        Wire::AnthropicMessages => anthropic::head(),
        Wire::Harness => harness::summary_head(),
    }
}

pub(crate) fn model_head(wire: Wire, name: &str) -> Head {
    match wire {
        Wire::AnthropicMessages => anthropic::model(name),
        Wire::Harness => harness::model(name),
    }
}

pub(crate) fn awaited(head: &Head) -> Option<Awaited> {
    Wire::ALL.iter().find_map(|wire| match wire {
        Wire::AnthropicMessages => anthropic::awaited(head),
        Wire::Harness => harness::awaited(head),
    })
}

pub(crate) fn answered(wire: Wire, response: &Response) -> Answered {
    match wire {
        Wire::AnthropicMessages => anthropic::answered(response),
        Wire::Harness => harness::answered(response),
    }
}

pub(crate) fn summarized(wire: Wire, response: &Response) -> Summarized {
    match wire {
        Wire::AnthropicMessages => anthropic::summarized(response),
        Wire::Harness => harness::summarized(response),
    }
}

pub(crate) fn learned(wire: Wire, response: &Response) -> Result<Learned, String> {
    match wire {
        Wire::AnthropicMessages => anthropic::window(response).map(|window| Learned {
            window,
            provider: model_wire::anthropic::PROVIDER.into(),
        }),
        Wire::Harness => {
            harness::learned(response).map(|(window, provider)| Learned { window, provider })
        }
    }
}

pub(crate) fn fold(partial: &mut Partial, data: &[u8]) {
    let Some(data) = model_wire::event_data(data) else {
        return;
    };
    match partial.wire {
        Wire::AnthropicMessages => anthropic::fold(partial, &data),
        Wire::Harness => harness::fold(partial, &data),
    }
}

pub(crate) fn last_pass(wire: Wire, data: &[u8]) -> Option<u64> {
    let data = model_wire::event_data(data)?;
    match wire {
        Wire::AnthropicMessages => anthropic::last_pass(&data),
        Wire::Harness => harness::last_pass(&data),
    }
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
