//! What an assistant's earlier blocks become on their way to a model: a seal goes back only to what
//! reads it (its model, or its account), reasoning is otherwise left out, and a server tool's blocks
//! and citations otherwise go as their words. A tool call's id fits the target's pattern.
use crate::{Block, Lane, Message, Request, Target, UserBlock, anthropic, codex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Model,
    Account,
}

fn reasoning_scope(provider: &str) -> Option<Scope> {
    match provider {
        anthropic::PROVIDER => Some(Scope::Model),
        codex::PROVIDER => Some(Scope::Account),
        _ => None,
    }
}

struct Reader<'a> {
    provider: &'a str,
    model: &'a str,
    target: &'a Target,
}

impl Reader<'_> {
    fn same_model(&self, lane: &Lane) -> bool {
        lane.provider == self.provider && lane.model == self.model
    }

    fn pinned_to(&self, lane: &Lane) -> bool {
        let Target::Pinned { service, account } = self.target else {
            return false;
        };
        let pinned =
            lane.service.as_ref() == Some(service) && lane.account.as_ref() == Some(account);
        lane.provider == self.provider && pinned
    }

    fn same_account(&self, lane: &Lane) -> bool {
        let judged = self.target == &Target::Unnamed && lane.provider == self.provider;
        judged || self.pinned_to(lane)
    }

    fn reads_reasoning(&self, lane: &Lane) -> bool {
        match reasoning_scope(&lane.provider) {
            Some(Scope::Model) => self.same_model(lane),
            Some(Scope::Account) => self.same_model(lane) && self.pinned_to(lane),
            None => false,
        }
    }
}

pub fn tool_id(provider: &str, id: &str) -> String {
    match provider {
        anthropic::PROVIDER => anthropic::tool_id(id),
        _ => id.to_owned(),
    }
}

fn words(lane: &Lane, block: &Block) -> Option<Block> {
    let text = match lane.provider.as_str() {
        anthropic::PROVIDER => anthropic::plain(block),
        _ => match block {
            Block::Text { text, .. } => Some(text.clone()),
            _ => None,
        },
    };
    let text = text.filter(|text| !text.is_empty())?;
    Some(Block::Text {
        text,
        citations: Vec::new(),
    })
}

fn fitted(block: &Block, lane: &Lane, reader: &Reader) -> Option<Block> {
    match block {
        Block::Text { citations, .. } if citations.is_empty() || reader.same_account(lane) => {
            Some(block.clone())
        }
        Block::Text { .. } => words(lane, block),
        Block::Reasoning {
            sealed: Some(_), ..
        }
        | Block::Redacted { .. }
            if reader.reads_reasoning(lane) =>
        {
            Some(block.clone())
        }
        Block::Reasoning { .. } | Block::Redacted { .. } => None,
        Block::ToolUse { id, name, input } => Some(Block::ToolUse {
            id: tool_id(reader.provider, id),
            name: name.clone(),
            input: input.clone(),
        }),
        Block::ServerToolUse { .. } | Block::ServerToolResult { .. }
            if reader.same_account(lane) =>
        {
            Some(block.clone())
        }
        Block::ServerToolUse { .. } | Block::ServerToolResult { .. } => words(lane, block),
        Block::Summary {
            sealed: Some(_), ..
        } if lane.provider == reader.provider => Some(block.clone()),
        Block::Summary { text, .. } => Some(Block::Summary {
            text: text.clone(),
            sealed: None,
        }),
    }
}

fn user(block: &UserBlock, provider: &str) -> UserBlock {
    match block {
        UserBlock::Result {
            call,
            failed,
            content,
        } => UserBlock::Result {
            call: tool_id(provider, call),
            failed: *failed,
            content: content.clone(),
        },
        other => other.clone(),
    }
}

pub fn fit(request: &Request, provider: &str) -> Request {
    let reader = Reader {
        provider,
        model: &request.model,
        target: &request.target,
    };
    let messages = request
        .messages
        .iter()
        .map(|message| match message {
            Message::User { blocks } => Message::User {
                blocks: blocks.iter().map(|block| user(block, provider)).collect(),
            },
            Message::Assistant { lane, blocks } => Message::Assistant {
                lane: lane.clone(),
                blocks: blocks
                    .iter()
                    .filter_map(|block| fitted(block, lane, &reader))
                    .collect(),
            },
        })
        .collect();
    Request {
        model: request.model.clone(),
        max_tokens: request.max_tokens,
        thinking: request.thinking,
        system: request.system.clone(),
        tools: request.tools.clone(),
        server_tools: request.server_tools.clone(),
        messages,
        target: request.target.clone(),
        cache_key: request.cache_key.clone(),
    }
}
