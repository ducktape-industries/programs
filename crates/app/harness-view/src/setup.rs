//! An agent's front matter, edited where its setup shows: every press goes to `Agents::press_setup`,
//! `SetupDraft::press` says what follows, and a save reads the file at the snapshot harness read,
//! rewrites its front matter alone (`with_front`) and commits over that snapshot.
use serde::{Deserialize, Serialize};
use view::prelude::*;

use harness::{AgentSetup, BlobId, Config, Configured, ServerTool, Thinking};

use crate::create::{base_of, write_file};
use crate::{Agents, read};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SetupDraft {
    // the agent whose front matter is open, its file and the snapshot harness read it at
    pub agent: Option<u64>,
    pub path: String,
    pub snapshot: Option<[u8; 32]>,
    // the model as its vendor names it, and the provider whose route reaches it
    pub vendor: TextField,
    pub provider: Option<String>,
    pub max_tokens: TextField,
    pub wait: TextField,
    pub thinking: Option<Thinking>,
    pub server_tools: Vec<ServerTool>,
    // the channel ids it hears every message of
    pub listen: Vec<String>,
    pub rooms: TextField,
    pub settle: TextField,
    pub compact_at: TextField,
    pub problem: Option<String>,
    #[serde(skip)]
    pub sending: Option<Task<()>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Press {
    Open(Box<AgentSetup>),
    Close,
    Provider(String),
    Thinking(Option<Thinking>),
    Tool(ServerTool),
    Listen(String),
    Save,
    Sent(Option<String>),
}

pub(crate) enum Then {
    Stay,
    Routes,
    Write(Config, String),
    Done,
}

impl SetupDraft {
    pub fn press(&mut self, press: Press) -> Then {
        if self.sending.is_some() && !matches!(press, Press::Sent(_)) {
            return Then::Stay;
        }
        match press {
            Press::Open(setup) => {
                *self = SetupDraft::opened(&setup);
                return Then::Routes;
            }
            Press::Close => *self = SetupDraft::default(),
            Press::Provider(provider) => self.provider = Some(provider),
            Press::Thinking(thinking) => self.thinking = thinking,
            Press::Tool(tool) => toggle(&mut self.server_tools, tool),
            Press::Listen(channel) => toggle(&mut self.listen, channel),
            Press::Save => match (self.config(), self.provider.clone()) {
                (Ok(config), Some(provider)) => {
                    self.problem = None;
                    return Then::Write(config, provider);
                }
                (Ok(_), None) => self.problem = Some(PICK_PROVIDER.into()),
                (Err(problem), _) => self.problem = Some(problem),
            },
            Press::Sent(None) => {
                *self = SetupDraft::default();
                return Then::Done;
            }
            Press::Sent(Some(problem)) => {
                self.sending = None;
                self.problem = Some(problem);
            }
        }
        Then::Stay
    }

    fn opened(setup: &AgentSetup) -> SetupDraft {
        let Some((_, config)) = setup.configured.config() else {
            return SetupDraft::default();
        };
        let field =
            |number: Option<u64>| TextField::new(number.map_or(String::new(), |n| n.to_string()));
        SetupDraft {
            agent: Some(setup.agent),
            path: setup.path.clone(),
            snapshot: base_of(&setup.configured),
            vendor: TextField::new(config.model.clone()),
            provider: provider_of(&setup.configured),
            max_tokens: TextField::new(config.max_tokens.to_string()),
            wait: field(config.wait),
            thinking: config.thinking,
            server_tools: config.server_tools.clone(),
            listen: config.listen.clone(),
            settle: field(config.settle),
            compact_at: field(config.compact_at),
            ..SetupDraft::default()
        }
    }

    // the front matter as the fields hold it; its `system` is empty, as the file keeps its body
    pub fn config(&self) -> Result<Config, String> {
        let model = self.vendor.text().trim().to_owned();
        if model.is_empty() {
            return Err(NAME_MODEL.into());
        }
        let max_tokens = match self.max_tokens.text().trim().parse::<u32>() {
            Ok(max) if max > 0 => max,
            _ => return Err("Max output tokens is a whole number, at least 1.".into()),
        };
        let wait = optional_positive_integer(
            &self.wait,
            "Wait is a whole number of blocks, at least 1, or empty.",
        )?;
        let settle = optional_positive_integer(
            &self.settle,
            "Settle is a whole number of milliseconds, at least 1, or empty.",
        )?;
        let compact_at = optional_positive_integer(
            &self.compact_at,
            "Compacts at is a whole number of tokens, at least 1, or empty.",
        )?;
        Ok(Config {
            model,
            max_tokens,
            wait,
            thinking: self.thinking,
            server_tools: self.server_tools.clone(),
            listen: self.listen.clone(),
            settle,
            compact_at,
            system: String::new(),
        })
    }
}

pub(crate) const NAME_MODEL: &str = "Write the model it asks, as its vendor names it.";
pub(crate) const PICK_PROVIDER: &str = "Pick the provider whose route reaches it.";

fn provider_of(configured: &Configured) -> Option<String> {
    match configured {
        Configured::Routed { provider, .. } | Configured::NoRoute { provider, .. } => {
            Some(provider.clone())
        }
        Configured::Unreadable { .. }
        | Configured::Malformed { .. }
        | Configured::Unprovided { .. } => None,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, item: T) {
    match list.iter().position(|held| *held == item) {
        Some(at) => {
            list.remove(at);
        }
        None => list.push(item),
    }
}

fn optional_positive_integer(field: &TextField, problem: &str) -> Result<Option<u64>, String> {
    let text = field.text();
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<u64>() {
        Ok(number) if number > 0 => Ok(Some(number)),
        _ => Err(problem.to_owned()),
    }
}

impl Agents {
    pub(crate) fn press_setup(&mut self, press: Press, cx: &mut Context<Self>) {
        match self.setup_draft.press(press) {
            Then::Stay => {}
            Then::Routes => {
                let (host, me) = (cx.host(), self.me);
                cx.load(self, read::routes(host.clone(), me), |view| {
                    &mut view.routes
                });
                let named = read::every_room(host, self.rooms.clone());
                cx.land(named, |_, named, cx| match named {
                    Ok(()) => cx.notify(),
                    Err(refusal) => cx.log_refused("chat's rooms", &refusal),
                })
                .detach();
            }
            Then::Write(config, provider) => {
                let draft = &self.setup_draft;
                let (path, at) = (draft.path.clone(), draft.snapshot);
                let agent = draft.agent.unwrap_or_default();
                let message = format!(
                    "Configure agent {} (account {agent})",
                    self.agent_name(agent)
                );
                let host = cx.host();
                let writing = async move {
                    let file = read::file(&host, &path, at.map(BlobId::Sha256)).await?;
                    let written = harness::config::with_front(&file, &config, &provider).map_err(
                        |malformed| format!("The file takes no new front matter: {malformed}."),
                    )?;
                    write_file(&host, path, at, written.into_bytes(), message).await
                };
                let sent = async move { Ok::<_, Error>(writing.await) };
                let task = cx.land(sent, |view, written, cx| {
                    let written = written.unwrap_or_else(|refusal| Err(design::sentence(&refusal)));
                    view.press_setup(Press::Sent(written.err()), cx)
                });
                self.setup_draft.sending = Some(task);
            }
            Then::Done => self.read_screen(cx),
        }
        cx.notify();
    }
}
