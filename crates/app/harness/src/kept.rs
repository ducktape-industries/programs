//! The model a conversation keeps (`state::MODEL_KEPT`): the first configuration harness routed
//! for it, written as it opens, or for one already open as new harness code first runs; a
//! front-matter edit reaches only conversations opened after it.
use std::collections::BTreeMap;

use ::program::{Error, ExecCtx, QueryCtx};
use abi::role::identity::AccountNumber;
use store::{PageRequest, PageResponse};

use crate::agents::{Agent, card, configured_file, configured_pinned, harnessed_agent, routed};
use crate::config::Pin;
use crate::state::{BY_AGENT, MODEL_KEPT, THREADS};
use crate::{AgentSetup, Config, Configured, ConversationModel, ModelKept, ThreadRow, Window};

fn of(configured: &Configured) -> Option<ModelKept> {
    let Configured::Routed {
        config, provider, ..
    } = configured
    else {
        return None;
    };
    Some(ModelKept {
        model: config.model.clone(),
        max_tokens: config.max_tokens,
        thinking: config.thinking,
        compact_at: config.compact_at,
        provider: provider.clone(),
        service: None,
    })
}

fn front(ctx: &QueryCtx, path: &str) -> Result<Option<ModelKept>, Error> {
    let (configured, _) = configured_pinned(ctx, path)?;
    Ok(of(&configured))
}

pub(crate) fn opened(ctx: &ExecCtx, agent: &Agent, id: u64) -> Result<(), Error> {
    if let Some(kept) = front(ctx, &agent.path)? {
        MODEL_KEPT.put(ctx, &id, &kept);
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct Fronts(BTreeMap<AccountNumber, Option<ModelKept>>);

pub(crate) fn switched(ctx: &ExecCtx, fronts: &mut Fronts, row: &ThreadRow) -> Result<(), Error> {
    let settled = row.closed || MODEL_KEPT.has(ctx, &row.id);
    if settled {
        return Ok(());
    }
    let kept = match fronts.0.get(&row.agent) {
        Some(kept) => kept.clone(),
        None => {
            let read = match harnessed_agent(ctx, row.agent)? {
                Some(agent) => front(ctx, &agent.path)?,
                None => None,
            };
            fronts.0.insert(row.agent, read.clone());
            read
        }
    };
    if let Some(kept) = kept {
        MODEL_KEPT.put(ctx, &row.id, &kept);
    }
    Ok(())
}

pub(crate) fn configured_for(
    ctx: &ExecCtx,
    thread: &ThreadRow,
    path: &str,
) -> Result<
    (
        Configured,
        Pin,
        Option<String>,
        crate::config::ToolContinuation,
    ),
    Error,
> {
    let (configured, pin, compaction_instruction, tool_continuation) = configured_file(ctx, path)?;
    let now = of(&configured);
    let kept = match MODEL_KEPT.get(ctx, &thread.id)? {
        Some(kept) => kept,
        None => {
            if let Some(now) = &now {
                MODEL_KEPT.put(ctx, &thread.id, now);
            }
            return Ok((configured, pin, compaction_instruction, tool_continuation));
        }
    };
    if now.as_ref() == Some(&kept) {
        return Ok((configured, pin, compaction_instruction, tool_continuation));
    }
    let pin = Pin {
        service: None,
        provider: Some(kept.provider.clone()),
    };
    Ok((
        laid(ctx, configured, kept)?,
        pin,
        compaction_instruction,
        tool_continuation,
    ))
}

fn laid(ctx: &QueryCtx, configured: Configured, kept: ModelKept) -> Result<Configured, Error> {
    let read = configured
        .config()
        .map(|(snapshot, config)| (snapshot, config.clone()));
    let Some((snapshot, config)) = read else {
        return Ok(configured);
    };
    let config = Config {
        model: kept.model,
        max_tokens: kept.max_tokens,
        thinking: kept.thinking,
        compact_at: kept.compact_at,
        ..config
    };
    routed(ctx, snapshot, config, kept.provider)
}

pub(crate) fn thread_card(ctx: &QueryCtx, id: u64) -> Result<Option<AgentSetup>, Error> {
    let Some(row) = THREADS.get(ctx, &id)? else {
        return Ok(None);
    };
    let Some(mut setup) = card(ctx, row.agent)? else {
        return Ok(None);
    };
    if let Some(kept) = MODEL_KEPT.get(ctx, &id)? {
        setup.configured = laid(ctx, setup.configured, kept)?;
    }
    Ok(Some(setup))
}

pub(crate) fn models(
    ctx: &QueryCtx,
    agent: AccountNumber,
    page: &PageRequest,
) -> Result<PageResponse<ConversationModel>, Error> {
    BY_AGENT.range_of(ctx, &agent, page)?.try_map(|(_, key)| {
        let thread = u64::MAX - key;
        let kept = MODEL_KEPT.get(ctx, &thread)?;
        let window = match &kept {
            Some(kept) => crate::providers::window(ctx, &kept.provider, &kept.model)?,
            None => Window::Unasked,
        };
        Ok(ConversationModel {
            thread,
            kept,
            window,
        })
    })
}
