//! The models form: a record set, edited or removed. Every press goes to `Agents::press_model`,
//! which hands it to `ModelDraft::press` and does the one thing that answers: send the ops it
//! lists, in order, once the person said yes. Who may change a record is harness's to say.
use serde::{Deserialize, Serialize};
use view::prelude::*;

use harness::{ModelRow, Wire};

use crate::read::Roster;
use crate::{Agents, Screen, applied};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ModelDraft {
    pub id: TextField,
    pub wire: usize,
    // the services that reach it, in the order harness tries them: the first is the record's
    pub services: Vec<String>,
    pub name: TextField,
    pub compact_at: TextField,
    // the record as the form opened it; none for a new one
    pub was: Option<Record>,
    // the desktop's form is open under the list
    pub shown: bool,
    pub asking: Option<Yes>,
    pub problem: Option<String>,
    #[serde(skip)]
    pub sending: Option<Task<()>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Record {
    pub wire: usize,
    pub name: String,
    pub services: Vec<String>,
    pub compact_at: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Yes {
    Save,
    Remove,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Press {
    Toggle,
    New,
    Open(ModelRow, Vec<String>),
    Wire(usize),
    Service(String),
    Ask(Yes),
    Yes,
    Unask,
    Sent(Option<String>),
}

pub(crate) enum Then {
    Stay,
    Send(Vec<harness::Op>),
    Done,
}

impl ModelDraft {
    pub fn wire(&self) -> Wire {
        Wire::ALL
            .get(self.wire)
            .copied()
            .unwrap_or(Wire::AnthropicMessages)
    }

    pub fn id(&self) -> String {
        self.id.text().trim().to_owned()
    }

    pub fn press(&mut self, press: Press) -> Then {
        if self.sending.is_some() && !matches!(press, Press::Sent(_)) {
            return Then::Stay;
        }
        match press {
            Press::Toggle => {
                let shown = !self.shown;
                *self = ModelDraft {
                    shown,
                    ..ModelDraft::default()
                };
            }
            Press::New => {
                *self = ModelDraft {
                    shown: true,
                    ..ModelDraft::default()
                }
            }
            Press::Open(row, services) => *self = ModelDraft::opened(row, services),
            Press::Wire(index) => self.wire = index,
            Press::Service(name) => match self.services.iter().position(|picked| *picked == name) {
                Some(at) => {
                    self.services.remove(at);
                }
                None => self.services.push(name),
            },
            Press::Ask(yes) => {
                let checked = match yes {
                    Yes::Save => self.ops().map(|_| ()),
                    Yes::Remove => Ok(()),
                };
                match checked {
                    Ok(()) => {
                        self.problem = None;
                        self.asking = Some(yes);
                    }
                    Err(problem) => self.problem = Some(problem),
                }
            }
            Press::Unask => self.asking = None,
            Press::Yes => {
                let ops = match self.asking {
                    Some(Yes::Save) => self.ops(),
                    Some(Yes::Remove) => Ok(vec![harness::Op::RemoveModel { id: self.id() }]),
                    None => return Then::Stay,
                };
                match ops {
                    Ok(ops) => return Then::Send(ops),
                    Err(problem) => {
                        self.asking = None;
                        self.problem = Some(problem);
                    }
                }
            }
            Press::Sent(None) => {
                *self = ModelDraft::default();
                return Then::Done;
            }
            Press::Sent(Some(problem)) => {
                self.sending = None;
                self.asking = None;
                self.problem = Some(problem);
            }
        }
        Then::Stay
    }

    fn opened(row: ModelRow, services: Vec<String>) -> ModelDraft {
        let wire = Wire::ALL
            .iter()
            .position(|wire| *wire == row.wire)
            .unwrap_or_default();
        let compact_at = row.compact_at.map(|tokens| tokens.to_string());
        ModelDraft {
            id: TextField::new(row.id),
            wire,
            services: services.clone(),
            name: TextField::new(row.name.clone()),
            compact_at: TextField::new(compact_at.unwrap_or_default()),
            was: Some(Record {
                wire,
                name: row.name,
                services,
                compact_at: row.compact_at,
            }),
            shown: true,
            ..ModelDraft::default()
        }
    }

    pub fn ops(&self) -> Result<Vec<harness::Op>, String> {
        let id = self.id();
        if id.is_empty() {
            return Err("Write the model id agents name in their configuration.".into());
        }
        let Some(first) = self.services.first().cloned() else {
            return Err("Pick the service that reaches the model.".into());
        };
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            return Err("Write the vendor's name for the model.".into());
        }
        let compact_at = self.compact_at.text().trim().to_owned();
        let compact_at = match compact_at.is_empty() {
            true => None,
            false => match compact_at.parse::<u64>() {
                Ok(tokens) if tokens > 0 => Some(tokens),
                _ => {
                    return Err(
                        "Compacts at is a whole number of tokens, at least 1, or empty.".into(),
                    );
                }
            },
        };
        let was = self.was.as_ref();
        let mut ops = Vec::new();
        if was.is_none_or(|was| was.wire != self.wire || was.name != name) {
            ops.push(harness::Op::SetModel {
                id: id.clone(),
                wire: self.wire(),
                service: first,
                name,
            });
        }
        let services_moved = match was {
            Some(was) => was.services != self.services,
            None => self.services.len() > 1,
        };
        if services_moved {
            ops.push(harness::Op::SetServices {
                id: id.clone(),
                services: self.services.clone(),
            });
        }
        if was.map_or(compact_at.is_some(), |was| was.compact_at != compact_at) {
            ops.push(harness::Op::SetCompactAt { id, compact_at });
        }
        match ops.is_empty() {
            true => Err("Nothing changed.".into()),
            false => Ok(ops),
        }
    }

    // what the yes shows: its title, and a line for each thing it does
    pub fn asked(&self, roster: Option<&Roster>) -> Option<(String, Vec<String>)> {
        let id = self.id();
        Some(match self.asking? {
            Yes::Save => {
                let ops = self.ops().unwrap_or_default();
                (format!("Save {id}"), ops.iter().map(op_words).collect())
            }
            Yes::Remove => {
                let naming = agents_using_model(roster, &id);
                (format!("Remove {id}?"), vec![removes_words(naming)])
            }
        })
    }
}

fn op_words(op: &harness::Op) -> String {
    match op {
        harness::Op::SetModel {
            wire,
            service,
            name,
            ..
        } => format!(
            "Set the record: {} through {service}, the vendor's name {name}",
            wire.label()
        ),
        harness::Op::SetServices { services, .. } => {
            format!("Reach it through {}", services.join(", then "))
        }
        harness::Op::SetCompactAt {
            compact_at: Some(tokens),
            ..
        } => format!(
            "Compact its conversations at {} tokens",
            design::grouped(*tokens)
        ),
        harness::Op::SetCompactAt {
            compact_at: None, ..
        } => "Clear its compact_at: a conversation compacts when its window fills, unless its agent names one".into(),
        other => format!("{other:?}"),
    }
}

fn agents_using_model(roster: Option<&Roster>, id: &str) -> Option<Vec<String>> {
    let roster = roster?;
    let names = roster.agents.iter().filter(|card| {
        let config = card.setup.configured.config();
        config.is_some_and(|(_, config)| config.model == id)
    });
    Some(names.map(|card| card.name.clone()).collect())
}

fn removes_words(naming: Option<Vec<String>>) -> String {
    let Some(names) = naming else {
        return "Reading which agents name it…".into();
    };
    match names.as_slice() {
        [] => "No agent's configuration names it.".into(),
        [one] => format!(
            "{one}'s configuration still names it: {one} asks no model until it names another."
        ),
        [rest @ .., last] => format!(
            "{} and {last} still name it in their configuration: each asks no model until it names another.",
            rest.join(", ")
        ),
    }
}

impl Agents {
    pub(crate) fn press_model(&mut self, press: Press, cx: &mut Context<Self>) {
        match self.model_draft.press(press) {
            Then::Stay => {}
            Then::Send(ops) => {
                let host = cx.host();
                let sending = async move {
                    for op in ops {
                        let answer = host.ask::<Submit<harness::Harness>>(op).await;
                        if let Err(problem) = applied(answer) {
                            return Ok::<_, Error>(Some(problem));
                        }
                    }
                    Ok(None)
                };
                let task = cx.land(sending, |view, sent, cx| {
                    let sent = sent.unwrap_or_else(|refusal| Some(design::sentence(&refusal)));
                    view.press_model(Press::Sent(sent), cx)
                });
                self.model_draft.sending = Some(task);
            }
            Then::Done => match self.screen {
                Screen::SetModel => self.go(Screen::Models, cx),
                _ => self.read_screen(cx),
            },
        }
        cx.notify();
    }
}
