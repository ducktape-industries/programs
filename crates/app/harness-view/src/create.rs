//! The create form: a new agent is identity's `CreateAgent`, a `files` commit of its
//! configuration file (its bytes put first as `chunk` blobs, `op.put`, never in the commit;
//! proposed in its manager's home, `/home/<manager's name>/agents/`), access's `Set` of the
//! file's write list to `Owner`, so only the manager's account number changes it whatever names
//! change, identity's `SetScript` with a script that calls harness and names that file, then
//! access's `Set` of the agent's harness key when it answers more than its manager. A step that
//! is refused keeps the steps before it, so pressing on resumes where it stopped. Editing an
//! agent is the same form at its commit: a new version of its file, which keeps its list, and a
//! new list and a new script when its path moves; who may change the file and whom the agent
//! answers are the same two `Set`s, sent only when the form changes them.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use view::methods::{BlobBody, NewBlob, PutBlobs};
use view::prelude::*;

use harness::{AgentSetup, BlobId, Config, Configured, ServerTool, Thinking};
use identity::{Identity, Ownership, Party, Script};
use program::Writers;

use crate::read::Detail;
use crate::{Agents, Screen, applied};

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Draft {
    pub name: TextField,
    pub path: TextField,
    pub model: Option<String>,
    pub max_tokens: TextField,
    pub wait: TextField,
    pub thinking: Option<Thinking>,
    // what the configuration lists; the form shows no control for it and writes it back
    #[serde(default)]
    pub server_tools: Vec<ServerTool>,
    pub system: TextField,
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
    pub rethink: bool,
    pub base: Option<[u8; 32]>,
    pub script_path: Option<String>,
    pub problem: Option<String>,
    #[serde(skip)]
    pub sending: Option<Task<()>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Answers {
    #[default]
    Manager,
    Anyone,
    Listed,
}

/// Who may change the configuration file besides its manager, as an edit picks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Changers {
    Manager,
    Listed,
}

/// Whom the agent answers and who may change its file, in the form's own words.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Lists {
    pub answers: Answers,
    pub listed: String,
    pub changers: Option<Changers>,
    pub changers_listed: String,
}

pub(crate) struct Form {
    pub path: String,
    pub config: Config,
    pub ownership: Ownership,
    /// the file's own list, when an edit picks one
    pub changers: Option<Ownership>,
}

#[derive(Clone, Copy)]
enum Listing {
    Config,
    Answers,
}

enum Next {
    Create(String),
    Commit(u64),
    Keep(String, Ownership),
    Script(u64),
    Answer(u64, Ownership),
    Done(u64),
}

/// What the form says it does, under its title.
pub(crate) const CREATE_INTRO: &str = "A new account you manage, handed to harness.";
pub(crate) const EDIT_INTRO: &str =
    "Saving commits a new version of the configuration file; the next model call reads it.";

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
        let Some(model) = self.model.clone() else {
            return Err("Pick the model it asks.".into());
        };
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
        let ownership = match self.answers {
            Answers::Manager => Ownership::Owner,
            Answers::Anyone => Ownership::Anyone,
            Answers::Listed => match parties(&self.listed.text()) {
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
            Some(Changers::Listed) => match parties(&self.changers_listed.text()) {
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
            config: Config {
                model,
                max_tokens,
                wait,
                thinking: self.thinking,
                server_tools: self.server_tools.clone(),
                system: self.system.text().trim().to_owned(),
            },
            ownership,
            changers,
        })
    }

    /// The two lists as the form holds them now.
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
        // an edit sends a list only where it changes one; a new file is its manager's alone
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
            false => form.ownership != Ownership::Owner,
        };
        if answers_more && !self.answered {
            return Next::Answer(agent, form.ownership.clone());
        }
        Next::Done(agent)
    }
}

impl Draft {
    /// The form at an agent's configuration, whom it answers and who may change its file.
    fn editing(detail: &Detail, setup: &AgentSetup, name: &str) -> Draft {
        let config = setup.configured.config().map(|(_, config)| config.clone());
        let field = |text: Option<String>| TextField::new(text.unwrap_or_default());
        let (answers, listed) = match &detail.ownership {
            Ownership::Owner => (Answers::Manager, String::new()),
            Ownership::Anyone => (Answers::Anyone, String::new()),
            Ownership::Listed(parties) => (Answers::Listed, words(parties)),
        };
        // the file's own list; one it inherits from a folder's is not the form's to show
        let (changers, changers_listed) = match &detail.config_writers {
            Some(Writers::Listed { at, list, .. }) if *at == setup.path => match list {
                Ownership::Owner => (Some(Changers::Manager), String::new()),
                Ownership::Listed(parties) => (Some(Changers::Listed), words(parties)),
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
            model: config.as_ref().map(|config| config.model.clone()),
            max_tokens: field(config.as_ref().map(|config| config.max_tokens.to_string())),
            wait: field(
                config
                    .as_ref()
                    .and_then(|config| config.wait)
                    .map(|wait| wait.to_string()),
            ),
            thinking: config.as_ref().and_then(|config| config.thinking),
            server_tools: config
                .as_ref()
                .map(|config| config.server_tools.clone())
                .unwrap_or_default(),
            system: field(config.as_ref().map(|config| config.system.clone())),
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

fn party(word: &str) -> Party {
    match word.parse::<u64>() {
        Ok(number) => Party::Account(number),
        Err(_) => Party::Program(word.to_owned()),
    }
}

/// The parties a list field names, none when it names none.
fn parties(listed: &str) -> Option<Vec<Party>> {
    let parties: Vec<Party> = listed
        .split([',', ' '])
        .filter(|word| !word.is_empty())
        .map(party)
        .collect();
    (!parties.is_empty()).then_some(parties)
}

/// A list's parties as a field writes them: `7, forge`.
fn words(parties: &[Party]) -> String {
    let words: Vec<String> = parties
        .iter()
        .map(|party| match party {
            Party::Account(number) => number.to_string(),
            Party::Program(program) => program.clone(),
        })
        .collect();
    words.join(", ")
}

fn base_of(configured: &Configured) -> Option<[u8; 32]> {
    let snapshot = match configured {
        Configured::Unreadable { .. } => None,
        Configured::Malformed { snapshot, .. }
        | Configured::UnknownModel { snapshot, .. }
        | Configured::Ready { snapshot, .. } => Some(*snapshot),
    };
    match snapshot {
        Some(BlobId::Sha256(digest)) => Some(digest),
        Some(BlobId::Sha1(_)) | None => None,
    }
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

    /// The form at the agent on screen, read in full.
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

    pub(crate) fn rethink(
        &mut self,
        name: &str,
        thinking: Option<Thinking>,
        cx: &mut Context<Self>,
    ) {
        if self.draft.sending.is_some() {
            return;
        }
        let Some(draft) = self.editing(name) else {
            return;
        };
        self.draft = Draft {
            thinking,
            rethink: true,
            ..draft
        };
        cx.notify();
        self.advance(cx);
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
            Next::Keep(path, list) => {
                let op = access::Op::Set {
                    program: files::PROGRAM.to_owned(),
                    key: path,
                    list: Some(list),
                };
                self.set_list(op, Listing::Config, cx)
            }
            Next::Answer(agent, ownership) => {
                let op = access::Op::Set {
                    program: harness::PROGRAM.to_owned(),
                    key: harness::agent_key(agent),
                    list: Some(ownership),
                };
                self.set_list(op, Listing::Answers, cx)
            }
            Next::Done(agent) => {
                let rethought = self.draft.rethink;
                self.draft = Draft::default();
                match rethought {
                    // the thinking picker writes from the agent's page and stays there
                    true => {
                        self.read_screen(cx);
                        cx.notify();
                    }
                    false => self.go(Screen::Agent(agent), cx),
                }
            }
        }
    }

    fn send(&mut self, op: identity::Op, cx: &mut Context<Self>) {
        let submit = cx.host().ask::<Submit<Identity>>(op.clone());
        self.draft.sending = Some(cx.land(submit, move |view, answer, cx| {
            view.draft.sending = None;
            // a create in doubt may have made the account, and nothing here
            // reads the chain for it: say so before Next makes a second one
            // (follow-up: settle a doubt by its frame, as Expo does)
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
                    _ => Ok(()),
                });
            view.landed(landed, cx);
        }));
        cx.notify();
    }

    fn set_list(&mut self, op: access::Op, listing: Listing, cx: &mut Context<Self>) {
        let submit = cx.host().ask::<Submit<access::Access>>(op);
        self.draft.sending = Some(cx.land(submit, move |view, answer, cx| {
            view.draft.sending = None;
            let landed = applied(answer).map(|_| match listing {
                Listing::Config => view.draft.kept = true,
                Listing::Answers => view.draft.answered = true,
            });
            view.landed(landed, cx);
        }));
        cx.notify();
    }

    /// Puts the agent's config on the network as `chunk` blobs, then commits it.
    fn commit(&mut self, agent: u64, form: Form, cx: &mut Context<Self>) {
        let config = form.config.write().into_bytes();
        let size = config.len() as u64;
        let blobs: Vec<NewBlob> = config
            .chunks(files::CHUNK_SIZE as usize)
            .map(|chunk| NewBlob {
                kind: files::CHUNK.into(),
                body: BlobBody::Bytes(chunk.to_vec()),
            })
            .collect();
        // an empty config has no chunk to put
        if blobs.is_empty() {
            let content = files::Content::Chunks {
                size,
                chunks: Vec::new(),
            };
            return self.commit_put(agent, form, content, cx);
        }
        let put = cx.host().put(PutBlobs { blobs });
        self.draft.sending = Some(cx.land(put, move |view, answer, cx| {
            view.draft.sending = None;
            match answer {
                Ok(digests) => {
                    let chunks = digests.into_iter().map(BlobId::Sha256).collect();
                    let content = files::Content::Chunks { size, chunks };
                    view.commit_put(agent, form, content, cx);
                }
                Err(refusal) => view.landed(Err(design::sentence(&refusal)), cx),
            }
        }));
        cx.notify();
    }

    /// Commits the agent's config, its bytes on the network as `content`.
    fn commit_put(
        &mut self,
        agent: u64,
        form: Form,
        content: files::Content,
        cx: &mut Context<Self>,
    ) {
        let name = self.draft.name.text();
        let changes = vec![files::Change::Put {
            path: form.path,
            exec: false,
            meta: BTreeMap::new(),
            content,
        }];
        let op = files::Op::Commit {
            base: self.draft.base.map(BlobId::Sha256),
            message: format!("Configure agent {name} (account {agent})"),
            changes,
        };
        let submit = cx.host().ask::<Submit<files::Files>>(op);
        self.draft.sending = Some(cx.land(submit, |view, answer, cx| {
            view.draft.sending = None;
            let landed = applied(answer).map(|_| view.draft.committed = true);
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
