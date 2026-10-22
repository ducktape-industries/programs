//! The create form: identity's `CreateAgent`, a files commit of its configuration (chunk blobs put
//! first), the file's own guard and policy, identity's `SetScript`, identity's `SetOwnership` of
//! whom it answers; a refused step resumes there. An edit writes the file's body alone, and moved lists.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use view::methods::{BlobBody, NewBlob, PutBlobs};
use view::prelude::*;

use harness::{AgentSetup, BlobId, Config, Configured, Pin, Thinking};
use identity::{Identity, Ownership, Party, Script};
use policy_view::link;
use program::Writers;

use crate::read::{self, Detail};
use crate::{Agents, Screen, applied};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Draft {
    pub name: TextField,
    pub path: TextField,
    // the model as its vendor names it, and the provider whose route reaches it
    #[serde(default)]
    pub vendor: TextField,
    #[serde(default)]
    pub provider: Option<String>,
    pub max_tokens: TextField,
    pub wait: TextField,
    pub thinking: Option<Thinking>,
    pub system: TextField,
    // an edit's prompt as it opened: the body is written only once the field moves from it
    #[serde(default)]
    pub prompt: String,
    pub answers: Answers,
    pub listed: TextField,
    /// who may change the configuration file, as an edit picks it; none keeps its list
    pub changers: Option<Changers>,
    pub changers_listed: TextField,
    /// the two lists as an edit opened with them: what it sends only when they change
    pub was: Option<Lists>,
    pub created: Option<u64>,
    pub committed: bool,
    pub kept: bool,
    pub scripted: bool,
    pub answered: bool,
    pub again: bool,
    pub base: Option<[u8; 32]>,
    pub script_path: Option<String>,
    pub problem: Option<String>,
    #[serde(skip)]
    pub sending: Option<Task<()>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Answers {
    Manager,
    #[default]
    Anyone,
    Listed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Changers {
    Manager,
    Listed,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Lists {
    pub answers: Answers,
    pub listed: String,
    pub changers: Option<Changers>,
    pub changers_listed: String,
}

pub(crate) struct Form {
    pub path: String,
    pub writes: Writes,
    pub ownership: Ownership,
    /// the file's own list, when an edit picks one
    pub changers: Option<Ownership>,
}

pub(crate) enum Writes {
    Whole { config: Config, provider: String },
    Edit { body: Option<String> },
}

enum Next {
    Create(String),
    Commit(u64),
    Keep(String, Ownership),
    Script(u64),
    Answer(u64, Ownership),
    Done(u64),
}

pub(crate) const VENDOR_HINT: &str = "The model as its vendor names it, like claude-opus-5-5.";
pub(crate) const PROVIDER_PICK_HINT: &str =
    "The provider whose route reaches it: the Routes page lists each one's services.";
pub(crate) const CREATE_INTRO: &str = "A new account you manage, handed to harness.";
pub(crate) const EDIT_INTRO: &str = "Saving writes the configuration file's system prompt and keeps its front matter, which the agent's Setup edits; the next model call reads it.";

pub(crate) fn proposed_path(home: Option<&str>, name: &str) -> Option<String> {
    let home = home?;
    let slug: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c.is_alphanumeric() || c == '-' || c == '_' {
            true => c,
            false => '-',
        })
        .collect();
    Some(format!("/home/{home}/agents/{slug}.md"))
}

impl Draft {
    /// What the form's one press does next.
    pub fn verb(&self) -> &'static str {
        match (self.again, self.created) {
            (true, _) => "Save",
            (false, None) => "Create",
            (false, Some(_)) => "Continue",
        }
    }

    /// What a create sends, each step with whether it landed.
    pub fn steps(&self) -> [(&'static str, bool); 5] {
        [
            ("Create the account", self.created.is_some()),
            ("Write its configuration", self.committed),
            ("Make it yours alone to change", self.kept),
            ("Hand it to harness", self.scripted),
            ("Who it answers, if more than you", self.answered),
        ]
    }

    pub fn path(&self, home: Option<&str>) -> Option<String> {
        let typed = self.path.text().trim().to_owned();
        match typed.is_empty() {
            true => proposed_path(home, &self.name.text()),
            false => Some(typed),
        }
    }

    pub fn checked(&self, home: Option<&str>) -> Result<Form, String> {
        if self.name.text().trim().is_empty() {
            return Err("Name the agent.".into());
        }
        let Some(path) = self.path(home) else {
            return Err(
                "Your account's name is not read yet, so no home is proposed: type the configuration file's path.".into(),
            );
        };
        let system = self.system.text().trim().to_owned();
        let writes = match self.again {
            true => Writes::Edit {
                body: (system != self.prompt).then_some(system),
            },
            false => {
                let config = self.config(system)?;
                let Some(provider) = self.provider.clone() else {
                    return Err(crate::setup::PICK_PROVIDER.into());
                };
                Writes::Whole { config, provider }
            }
        };
        let ownership = match self.answers {
            Answers::Manager => Ownership::Owner,
            Answers::Anyone => Ownership::Anyone,
            Answers::Listed => match parse_listed_parties(&self.listed.text()) {
                Some(parties) => Ownership::Listed(parties),
                None => {
                    return Err(
                        "List the account numbers and programs it answers, like 7, 12, forge."
                            .into(),
                    );
                }
            },
        };
        let changers = match self.changers {
            None => None,
            Some(Changers::Manager) => Some(Ownership::Owner),
            Some(Changers::Listed) => match parse_listed_parties(&self.changers_listed.text()) {
                Some(parties) => Some(Ownership::Listed(parties)),
                None => {
                    return Err(
                        "List the account numbers and programs that may change the configuration, like 7, 12."
                            .into(),
                    );
                }
            },
        };
        Ok(Form {
            path,
            writes,
            ownership,
            changers,
        })
    }

    fn config(&self, system: String) -> Result<Config, String> {
        let model = self.vendor.text().trim().to_owned();
        if model.is_empty() {
            return Err(crate::setup::NAME_MODEL.into());
        }
        let max_tokens = match self.max_tokens.text().trim().parse::<u32>() {
            Ok(max) if max > 0 => max,
            _ => return Err("Max output tokens is a whole number, at least 1.".into()),
        };
        let wait = self.wait.text().trim().to_owned();
        let wait = match wait.is_empty() {
            true => None,
            false => match wait.parse::<u64>() {
                Ok(blocks) if blocks > 0 => Some(blocks),
                _ => return Err("Wait is a whole number of blocks, at least 1, or empty.".into()),
            },
        };
        Ok(Config {
            model,
            max_tokens,
            wait,
            thinking: self.thinking,
            server_tools: Vec::new(),
            listen: Vec::new(),
            settle: None,
            compact_at: None,
            system,
        })
    }

    fn lists(&self) -> Lists {
        Lists {
            answers: self.answers,
            listed: self.listed.text().trim().to_owned(),
            changers: self.changers,
            changers_listed: self.changers_listed.text().trim().to_owned(),
        }
    }

    fn next(&self, form: &Form) -> Next {
        let Some(agent) = self.created else {
            return Next::Create(self.name.text().trim().to_owned());
        };
        if !self.committed {
            return Next::Commit(agent);
        }
        let moved = self.script_path.as_deref() != Some(form.path.as_str());
        let (now, was) = (self.lists(), self.was.clone().unwrap_or_default());
        let relisted = self.again
            && now.changers.is_some()
            && (now.changers, &now.changers_listed) != (was.changers, &was.changers_listed);
        if (moved || relisted) && !self.kept {
            let list = form.changers.clone().unwrap_or(Ownership::Owner);
            return Next::Keep(form.path.clone(), list);
        }
        if moved && !self.scripted {
            return Next::Script(agent);
        }
        let answers_more = match self.again {
            true => (now.answers, &now.listed) != (was.answers, &was.listed),
            false => form.ownership != Ownership::Anyone,
        };
        if answers_more && !self.answered {
            return Next::Answer(agent, form.ownership.clone());
        }
        Next::Done(agent)
    }
}

impl Draft {
    fn editing(detail: &Detail, setup: &AgentSetup, name: &str) -> Draft {
        let prompt = setup
            .configured
            .config()
            .map(|(_, config)| config.system.clone());
        let prompt = prompt.unwrap_or_default();
        let (answers, listed) = match &detail.ownership {
            Ownership::Owner => (Answers::Manager, String::new()),
            Ownership::Anyone => (Answers::Anyone, String::new()),
            Ownership::Listed(parties) => (Answers::Listed, parties_as_field_text(parties)),
        };
        let (changers, changers_listed) = match &detail.config_writers {
            Some(Writers::Listed { at, list, .. }) if *at == setup.path => match list {
                Ownership::Owner => (Some(Changers::Manager), String::new()),
                Ownership::Listed(parties) => {
                    (Some(Changers::Listed), parties_as_field_text(parties))
                }
                Ownership::Anyone => (None, String::new()),
            },
            Some(_) | None => (None, String::new()),
        };
        let was = Lists {
            answers,
            listed: listed.clone(),
            changers,
            changers_listed: changers_listed.clone(),
        };
        Draft {
            name: TextField::new(name),
            path: TextField::new(setup.path.clone()),
            system: TextField::new(prompt.clone()),
            prompt,
            answers,
            listed: TextField::new(listed),
            changers,
            changers_listed: TextField::new(changers_listed),
            was: Some(was),
            created: Some(setup.agent),
            again: true,
            base: base_of(&setup.configured),
            script_path: Some(setup.path.clone()),
            ..Draft::default()
        }
    }
}

fn foreign(word: &str) -> Option<program::abi::Foreign> {
    let (account, network) = word.split_once('@')?;
    Some(program::abi::Foreign {
        network: view::design::unhex(network)?.try_into().ok()?,
        account: account.parse().ok()?,
    })
}

fn party(word: &str) -> Party {
    if let Some(foreign) = foreign(word) {
        return Party::Foreign(foreign);
    }
    match word.parse::<u64>() {
        Ok(number) => Party::Account(number),
        Err(_) => Party::Program(word.to_owned()),
    }
}

fn parse_listed_parties(listed: &str) -> Option<Vec<Party>> {
    let parties: Vec<Party> = listed
        .split([',', ' '])
        .filter(|word| !word.is_empty())
        .map(party)
        .collect();
    (!parties.is_empty()).then_some(parties)
}

fn parties_as_field_text(parties: &[Party]) -> String {
    let words: Vec<String> = parties
        .iter()
        .map(|party| match party {
            Party::Account(number) => number.to_string(),
            Party::Program(program) => program.clone(),
            Party::Foreign(foreign) => {
                format!(
                    "{}@{}",
                    foreign.account,
                    program::abi::hex(&foreign.network)
                )
            }
        })
        .collect();
    words.join(", ")
}

pub(crate) fn base_of(configured: &Configured) -> Option<[u8; 32]> {
    let snapshot = match configured {
        Configured::Unreadable { .. } => None,
        Configured::Malformed { snapshot, .. }
        | Configured::Routed { snapshot, .. }
        | Configured::NoRoute { snapshot, .. }
        | Configured::Unprovided { snapshot, .. } => Some(*snapshot),
    };
    match snapshot {
        Some(BlobId::Sha256(digest)) => Some(digest),
        Some(BlobId::Sha1(_)) | None => None,
    }
}

pub(crate) async fn write_file(
    host: &Host,
    path: String,
    base: Option<[u8; 32]>,
    bytes: Vec<u8>,
    message: String,
) -> Result<(), String> {
    let size = bytes.len() as u64;
    let blobs: Vec<NewBlob> = bytes
        .chunks(files::CHUNK_SIZE as usize)
        .map(|chunk| NewBlob {
            kind: files::CHUNK.into(),
            body: BlobBody::Bytes(chunk.to_vec()),
        })
        .collect();
    let digests = match blobs.is_empty() {
        true => Vec::new(),
        false => host
            .put(PutBlobs { blobs })
            .await
            .map_err(|refusal| design::sentence(&refusal))?,
    };
    let chunks = digests.into_iter().map(BlobId::Sha256).collect();
    let op = files::Op::Commit {
        base: base.map(BlobId::Sha256),
        message,
        changes: vec![files::Change::Put {
            path,
            exec: false,
            meta: BTreeMap::new(),
            content: files::Content::Chunks { size, chunks },
        }],
    };
    applied(host.ask::<Submit<files::Files>>(op).await).map(|_| ())
}

impl Agents {
    pub(crate) fn start_create(&mut self, cx: &mut Context<Self>) {
        self.draft = Draft::default();
        self.go(Screen::Create, cx);
    }

    pub(crate) fn resume(&mut self, agent: u64, name: &str, cx: &mut Context<Self>) {
        self.draft = Draft {
            name: TextField::new(name),
            created: Some(agent),
            ..Draft::default()
        };
        self.go(Screen::Create, cx);
    }

    fn editing(&self, name: &str) -> Option<Draft> {
        let detail = self.detail.ready()?;
        Some(Draft::editing(detail, detail.setup.as_ref()?, name))
    }

    pub(crate) fn edit(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(draft) = self.editing(name) else {
            return;
        };
        self.draft = draft;
        self.go(Screen::Create, cx);
    }

    pub(crate) fn advance(&mut self, cx: &mut Context<Self>) {
        if self.draft.sending.is_some() {
            return;
        }
        let form = match self.draft.checked(self.home()) {
            Ok(form) => form,
            Err(problem) => {
                self.draft.problem = Some(problem);
                return cx.notify();
            }
        };
        self.draft.problem = None;
        match self.draft.next(&form) {
            Next::Create(name) => self.send(identity::Op::CreateAgent { name }, cx),
            Next::Commit(agent) => self.commit(agent, form, cx),
            Next::Script(account) => {
                let script = Some(Script {
                    calls: harness::PROGRAM.to_owned(),
                    config: form.path,
                });
                self.send(identity::Op::SetScript { account, script }, cx)
            }
            Next::Keep(path, list) => self.keep(path, list, cx),
            Next::Answer(account, ownership) => {
                self.send(identity::Op::SetOwnership { account, ownership }, cx)
            }
            Next::Done(agent) => {
                self.draft = Draft::default();
                self.go(Screen::Agent(agent), cx);
            }
        }
    }

    fn send(&mut self, op: identity::Op, cx: &mut Context<Self>) {
        let submit = cx.host().ask::<Submit<Identity>>(op.clone());
        self.draft.sending = Some(cx.land(submit, move |view, answer, cx| {
            view.draft.sending = None;
            let in_doubt = matches!(answer, Ok(view::methods::Submitted::InDoubt { .. }));
            let landed = applied(answer)
                .map_err(|said| match &op {
                    identity::Op::CreateAgent { .. } if in_doubt => {
                        format!("{said} Look for the account before you create another.")
                    }
                    _ => said,
                })
                .and_then(|output| match op {
                    identity::Op::CreateAgent { .. } => {
                        let number = view::methods::decode::<u64>(&output)
                            .map_err(|error| format!("No account number came back: {error}"))?;
                        view.draft.created = Some(number);
                        Ok(())
                    }
                    identity::Op::SetScript { .. } => {
                        view.draft.scripted = true;
                        Ok(())
                    }
                    identity::Op::SetOwnership { .. } => {
                        view.draft.answered = true;
                        Ok(())
                    }
                    _ => Ok(()),
                });
            view.landed(landed, cx);
        }));
        cx.notify();
    }

    fn keep(&mut self, path: String, list: Ownership, cx: &mut Context<Self>) {
        let Some(me) = self.me else {
            return;
        };
        let host = cx.host();
        let write = link::writers(&list);
        let work = async move {
            if let Some(listed) = link::access_list(&host, files::PROGRAM, &path, &write).await?
                && !link::landed(&listed)
            {
                return Ok(listed);
            }
            let guarded = host
                .query(files::ask::Guarded { path: path.clone() })
                .await?;
            if guarded.as_deref() != Some(path.as_str()) {
                let guard = files::Op::Guard { path: path.clone() };
                let made = host.ask::<Submit<files::Files>>(guard).await?;
                if !link::landed(&made) {
                    return Ok(made);
                }
            }
            let item = policy_view::Item {
                program: files::PROGRAM.to_owned(),
                key: files::guard_key(&path),
                label: path,
            };
            link::link(&host, item, ownership::Party::Account(me), write).await
        };
        self.draft.sending = Some(cx.land(work, |view, answer, cx| {
            view.draft.sending = None;
            let landed = applied(answer).map(|_| view.draft.kept = true);
            view.landed(landed, cx);
        }));
        cx.notify();
    }

    fn commit(&mut self, agent: u64, form: Form, cx: &mut Context<Self>) {
        let message = format!(
            "Configure agent {} (account {agent})",
            self.draft.name.text()
        );
        let (host, base) = (cx.host(), self.draft.base);
        let from = self
            .draft
            .script_path
            .clone()
            .unwrap_or_else(|| form.path.clone());
        let writing = async move {
            let bytes = match form.writes {
                Writes::Whole { config, provider } => {
                    let pin = Pin {
                        service: None,
                        provider: Some(provider),
                    };
                    config.write(&pin).into_bytes()
                }
                Writes::Edit { body } => {
                    let file = read::file(&host, &from, base.map(BlobId::Sha256)).await?;
                    match body {
                        Some(system) => harness::config::with_body(&file, &system)
                            .map_err(|malformed| {
                                format!("The file keeps no front matter to write the prompt under: {malformed}.")
                            })?
                            .into_bytes(),
                        None => file,
                    }
                }
            };
            write_file(&host, form.path, base, bytes, message).await
        };
        let sent = async move { Ok::<_, Error>(writing.await) };
        self.draft.sending = Some(cx.land(sent, |view, written, cx| {
            view.draft.sending = None;
            let written = written.unwrap_or_else(|refusal| Err(design::sentence(&refusal)));
            let landed = written.map(|_| view.draft.committed = true);
            view.landed(landed, cx);
        }));
        cx.notify();
    }

    fn landed(&mut self, landed: Result<(), String>, cx: &mut Context<Self>) {
        match landed {
            Ok(()) => self.advance(cx),
            Err(problem) => {
                self.draft.problem = Some(problem);
                cx.notify();
            }
        }
    }
}
