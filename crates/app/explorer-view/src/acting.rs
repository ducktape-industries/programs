//! What the reader changes on the Programs page, and how: by its account on
//! a program that fills no role (the op goes to the programs role), else by
//! the validators' vote (the op goes to valset wrapped in a `Role` motion for
//! the programs role), else not at all. The
//! app's confirm card is the yes for every op; nothing here asks again.
//! The forms' ops are built by pure functions, each a unit test.
use programs::{Change, NOTICE, Pending, Programs, Scheduled};
use valset::{Motion, Valset, Voted};
use view::methods::Submitted;
use view::prelude::*;

use crate::Explorer;
use crate::state::{Network, Note};

const SLACK: u64 = 30;
const LEAD_MS: u64 = 10 * 60 * 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Path {
    Account,
    Vote,
}

impl Path {
    pub(crate) fn upgrade_word(self) -> &'static str {
        match self {
            Path::Account => "Upgrade",
            Path::Vote => "Propose upgrade",
        }
    }

    pub(crate) fn remove_word(self) -> &'static str {
        match self {
            Path::Account => "Remove",
            Path::Vote => "Propose remove",
        }
    }

    pub(crate) fn cancel_word(self) -> &'static str {
        match self {
            Path::Account => "Cancel",
            Path::Vote => "Propose cancel",
        }
    }
}

#[derive(Default)]
pub(crate) struct Acting {
    pub(crate) account: Option<u64>,
    pub(crate) signer: Option<Vec<u8>>,
    pub(crate) voices: Vec<Vec<u8>>,
    pub(crate) form: Option<Form>,
    pub(crate) busy: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Upgrade { name: String, view: bool },
    Remove { name: String, view: bool },
    Add,
}

impl Edit {
    pub(crate) fn press_id(&self) -> String {
        match self {
            Edit::Upgrade { name, .. } => format!("explorer-upgrade-{name}"),
            Edit::Remove { name, .. } => format!("explorer-remove-{name}"),
            Edit::Add => "explorer-add".into(),
        }
    }
}

pub(crate) struct Offer {
    pub(crate) edit: Edit,
    pub(crate) path: Path,
    pub(crate) word: &'static str,
}

pub(crate) struct Form {
    pub(crate) edit: Edit,
    pub(crate) path: Path,
    pub(crate) code: TextField,
    pub(crate) height: TextField,
    pub(crate) name: TextField,
    pub(crate) params: TextField,
    pub(crate) view: bool,
    pub(crate) problem: Option<Problem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Problem {
    pub(crate) sentence: String,
    pub(crate) at: Option<FormField>,
}

impl Problem {
    fn at(field: FormField, sentence: impl Into<String>) -> Problem {
        Problem {
            sentence: sentence.into(),
            at: Some(field),
        }
    }

    fn whole(sentence: impl Into<String>) -> Problem {
        Problem {
            sentence: sentence.into(),
            at: None,
        }
    }

    pub(crate) fn of(problem: Option<&Problem>, field: FormField) -> Option<&str> {
        problem
            .filter(|problem| problem.at == Some(field))
            .map(|problem| problem.sentence.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    Field(FormField),
    Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FormField {
    Name,
    Code,
    Params,
    Height,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Takes {
    Number,
    Id,
    Hex,
}

impl FormField {
    pub(crate) fn id(self) -> &'static str {
        match self {
            FormField::Name => "explorer-form-name",
            FormField::Code => "explorer-form-code",
            FormField::Params => "explorer-form-params",
            FormField::Height => "explorer-form-height",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            FormField::Name => "Name",
            FormField::Code => "Code id",
            FormField::Params => "Params",
            FormField::Height => "At block",
        }
    }

    pub(crate) fn hint(self) -> &'static str {
        match self {
            FormField::Name => "program id",
            FormField::Code => "sha256:… of the published code",
            FormField::Params => "params hex, or empty",
            FormField::Height => "block height",
        }
    }

    pub(crate) fn takes(self) -> Takes {
        match self {
            FormField::Height => Takes::Number,
            FormField::Name | FormField::Code => Takes::Id,
            FormField::Params => Takes::Hex,
        }
    }
}

impl Form {
    pub(crate) fn asks(&self) -> Vec<Asked> {
        let field = Asked::Field;
        let mut asks = match self.edit {
            Edit::Add => vec![field(FormField::Name), Asked::Kind, field(FormField::Code)],
            Edit::Upgrade { .. } => vec![field(FormField::Code)],
            Edit::Remove { .. } => Vec::new(),
        };
        if self.edit == Edit::Add && !self.view {
            asks.push(field(FormField::Params));
        }
        asks.push(field(FormField::Height));
        asks
    }

    pub(crate) fn text(&self, field: FormField) -> &TextField {
        match field {
            FormField::Name => &self.name,
            FormField::Code => &self.code,
            FormField::Params => &self.params,
            FormField::Height => &self.height,
        }
    }

    pub(crate) fn title(&self) -> String {
        match (&self.edit, self.path) {
            (Edit::Upgrade { name, .. }, Path::Account) => format!("Upgrade {name}"),
            (Edit::Upgrade { name, .. }, Path::Vote) => format!("Propose upgrade · {name}"),
            (Edit::Remove { name, .. }, Path::Account) => format!("Remove {name}"),
            (Edit::Remove { name, .. }, Path::Vote) => format!("Propose remove · {name}"),
            (Edit::Add, _) => "Add a program".into(),
        }
    }

    pub(crate) fn verb(&self) -> &'static str {
        match self.path {
            Path::Account => "Schedule",
            Path::Vote => "Propose",
        }
    }

    pub(crate) fn notes(&self, network: &Network, tip: u64) -> Vec<String> {
        let lands = match self.path {
            Path::Account => format!(
                "Block {} or later: the validators get {NOTICE} blocks to look, and can cancel it by vote until it lands.",
                design::grouped(earliest(tip))
            ),
            Path::Vote => "Lands at this block once the validators carry the vote.".into(),
        };
        let kept = match &self.edit {
            Edit::Upgrade { name, view: false } => network
                .programs
                .iter()
                .find(|entry| &entry.program == name)
                .map(|entry| {
                    format!(
                        "Params kept: {}, as {name} runs now.",
                        design::plural(entry.params.len() as u64, "byte", "bytes")
                    )
                }),
            _ => None,
        };
        std::iter::once(lands).chain(kept).collect()
    }
}

impl Acting {
    pub(crate) fn votes(&self) -> bool {
        !self.voices.is_empty()
    }
}

impl Explorer {
    pub(crate) fn path(&self, program: &str) -> Option<Path> {
        if self.account_changes(program) {
            return Some(Path::Account);
        }
        self.acting.votes().then_some(Path::Vote)
    }

    pub(crate) fn edits(&self, network: &Network, name: &str, listed: bool) -> Vec<Offer> {
        let Some(path) = self.path(name) else {
            return Vec::new();
        };
        let bound = network
            .roles
            .as_ref()
            .is_none_or(|roles| programs::role_bound(roles, name));
        let mut out = vec![Offer {
            edit: Edit::Upgrade {
                name: name.to_owned(),
                view: listed,
            },
            path,
            word: path.upgrade_word(),
        }];
        let removes = match path {
            Path::Account if network.is_hook(name) => self.acting.votes().then_some(Path::Vote),
            path => Some(path),
        };
        if let Some(path) = removes.filter(|_| !bound) {
            out.push(Offer {
                edit: Edit::Remove {
                    name: name.to_owned(),
                    view: listed,
                },
                path,
                word: path.remove_word(),
            });
        }
        out
    }

    pub(crate) fn cancels(&self, pending: &Pending) -> Option<Path> {
        match pending.by {
            Some(_) if self.account_changes(pending.change.program()) => Some(Path::Account),
            _ => self.acting.votes().then_some(Path::Vote),
        }
    }

    pub(crate) fn account_changes(&self, program: &str) -> bool {
        let (Some(network), Some(_)) = (self.network.ready(), self.acting.account) else {
            return false;
        };
        network.roles.is_some() && !network.fills_a_role(program)
    }

    pub(crate) fn adds(&self) -> bool {
        self.network.ready().is_some() && self.acting.account.is_some()
    }

    pub(crate) fn session_changed(&mut self, session: Session, cx: &mut Context<Self>) {
        if self.session_chain != session.chain_id {
            self.session_chain = session.chain_id;
            cx.notify();
        }
        let signer = design::unhex(&session.signer).filter(|key| !key.is_empty());
        if signer != self.acting.signer || session.account != self.acting.account {
            self.acting = Acting {
                account: session.account,
                signer,
                ..Acting::default()
            };
            cx.notify();
        }
        self.read_voices(cx);
    }

    pub(crate) fn read_voices(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.acting.signer.clone() else {
            return;
        };
        let ask = cx.host().query(valset::ask::Voices { key: key.clone() });
        cx.land(ask, move |view, answer, cx| {
            if view.acting.signer.as_ref() != Some(&key) {
                return;
            }
            match answer {
                Ok(voices) if voices != view.acting.voices => {
                    view.acting.voices = voices;
                    cx.notify();
                }
                Ok(_) => {}
                Err(refusal) => cx.log_refused("whom this key votes for", &refusal),
            }
        })
        .detach();
    }

    pub(crate) fn open_form(&mut self, edit: Edit, path: Path, cx: &mut Context<Self>) {
        let height = match self.status.ready() {
            Some(status) => default_height(path, status.height, status.block_time_ms).to_string(),
            None => String::new(),
        };
        self.acting.form = Some(Form {
            edit,
            path,
            code: TextField::default(),
            height: TextField::new(height),
            name: TextField::default(),
            params: TextField::default(),
            view: false,
            problem: None,
        });
        cx.notify();
    }

    pub(crate) fn close_form(&mut self, cx: &mut Context<Self>) {
        self.acting.form = None;
        cx.notify();
    }

    pub(crate) fn add_view(&mut self, view: bool, cx: &mut Context<Self>) {
        if let Some(form) = &mut self.acting.form {
            form.view = view;
            cx.notify();
        }
    }

    pub(crate) fn submit_form(&mut self, cx: &mut Context<Self>) {
        let (Some(form), Some(network)) = (&mut self.acting.form, self.network.ready()) else {
            return;
        };
        let tip = self.status.ready().map_or(0, |status| status.height);
        match built(form, network, tip) {
            Ok(op) => {
                form.problem = None;
                let path = form.path;
                self.send(path, op, cx);
            }
            Err(problem) => {
                form.problem = Some(problem);
                cx.notify();
            }
        }
    }

    pub(crate) fn press(&mut self, path: Path, op: programs::Op, cx: &mut Context<Self>) {
        self.acting.form = None;
        self.send(path, op, cx);
    }

    fn send(&mut self, path: Path, op: programs::Op, cx: &mut Context<Self>) {
        if self.acting.busy {
            return;
        }
        self.acting.busy = true;
        self.note = None;
        let title = programs::describe(&op).title;
        let landed =
            move |view: &mut Self, answer: Result<Submitted, Error>, cx: &mut Context<Self>| {
                view.landed(path, &title, answer, cx)
            };
        match path {
            Path::Account => {
                let ask = cx.host().ask::<Submit<Programs>>(op);
                cx.land(ask, landed).detach();
            }
            Path::Vote => {
                let motion = Motion::Role {
                    role: abi::role::PROGRAMS.into(),
                    op: abi::encode(&op),
                };
                let ask = cx.host().ask::<Submit<Valset>>(valset::Op::Vote(motion));
                cx.land(ask, landed).detach();
            }
        }
        cx.notify();
    }

    fn landed(
        &mut self,
        path: Path,
        title: &str,
        answer: Result<Submitted, Error>,
        cx: &mut Context<Self>,
    ) {
        self.acting.busy = false;
        let Some(problem) = design::failure(&answer) else {
            if let Ok(Submitted::Applied { output, .. }) = answer {
                self.acting.form = None;
                self.note = Some(Note::Sent(applied_op_description(path, &output, title)));
            }
            cx.notify();
            return;
        };
        match &mut self.acting.form {
            Some(form) => form.problem = Some(Problem::whole(problem)),
            None => self.note = Some(Note::Refused(problem)),
        }
        cx.notify();
    }
}

impl Network {
    pub(crate) fn is_empty(&self) -> bool {
        self.programs.is_empty() && self.views.is_empty() && self.changes.is_empty()
    }

    pub(crate) fn is_hook(&self, name: &str) -> bool {
        self.hooks.iter().any(|hook| hook == name)
    }

    pub(crate) fn role_of(&self, name: &str) -> Option<&str> {
        self.roles
            .as_ref()?
            .iter()
            .find(|(_, program)| program.as_str() == name)
            .map(|(role, _)| role)
    }

    pub(crate) fn source_of(&self, name: &str) -> Option<&programs::Provenance> {
        let built = self.sources.iter().find(|built| built.program == name)?;
        Some(&built.source)
    }

    pub(crate) fn fills_a_role(&self, name: &str) -> bool {
        self.roles
            .as_ref()
            .is_some_and(|roles| programs::role_bound(roles, name))
    }
}

fn applied_op_description(path: Path, output: &[u8], title: &str) -> String {
    match (path, abi::decode::<Voted>(output)) {
        (Path::Account, _) => format!("Sent to the programs role: {title}."),
        (Path::Vote, Ok(Voted::Enacted)) => format!("Enacted: {title}."),
        (Path::Vote, Ok(Voted::Counted { votes, needed })) => {
            format!("Vote counted, {votes} of {needed}: {title}.")
        }
        (Path::Vote, Err(_)) => format!("Voted: {title}."),
    }
}

pub(crate) fn default_height(path: Path, tip: u64, block_time_ms: u64) -> u64 {
    match path {
        Path::Account => tip + NOTICE + SLACK,
        Path::Vote => tip + LEAD_MS.div_ceil(block_time_ms.max(1)),
    }
}

pub(crate) fn earliest(tip: u64) -> u64 {
    tip + 1 + NOTICE
}

pub(crate) fn built(form: &Form, network: &Network, tip: u64) -> Result<programs::Op, Problem> {
    let at = |height: u64, change: Change| programs::Op::Schedule(Scheduled { height, change });
    Ok(match &form.edit {
        Edit::Upgrade { name, view: false } => {
            let current = network
                .programs
                .iter()
                .find(|entry| &entry.program == name)
                .ok_or_else(|| Problem::whole(format!("{name} no longer runs here.")))?;
            let entry = programs::Entry {
                code: release_code_id(form)?,
                ..current.clone()
            };
            at(height_after_tip(form, tip)?, Change::Set(entry))
        }
        Edit::Upgrade { name, view: true } => {
            let view = programs::View {
                name: name.clone(),
                view: release_code_id(form)?,
            };
            at(height_after_tip(form, tip)?, Change::SetView(view))
        }
        Edit::Remove { name, view } => {
            let change = if *view {
                Change::RemoveView(name.clone())
            } else {
                Change::Remove(name.clone())
            };
            at(height_after_tip(form, tip)?, change)
        }
        Edit::Add => {
            let name = form.name.text().trim().to_owned();
            if name.is_empty() {
                return Err(Problem::at(FormField::Name, "Name the program."));
            }
            let code = release_code_id(form)?;
            let change = if form.view {
                Change::SetView(programs::View { name, view: code })
            } else {
                let params = form.params.text();
                let params = params.trim().trim_start_matches("0x");
                let params = design::unhex(params).ok_or_else(|| {
                    Problem::at(FormField::Params, "Params are hex digits, or nothing.")
                })?;
                Change::Set(programs::Entry {
                    program: name,
                    code,
                    params,
                })
            };
            at(height_after_tip(form, tip)?, change)
        }
    })
}

fn release_code_id(form: &Form) -> Result<abi::BlobId, Problem> {
    let text = form.code.text();
    let text = text.trim();
    let digits = text.strip_prefix("sha256:").unwrap_or(text);
    let digest: Option<[u8; 32]> = design::unhex(digits).and_then(|bytes| bytes.try_into().ok());
    digest
        .map(abi::BlobId::Sha256)
        .ok_or_else(|| Problem::at(FormField::Code, "A code id is sha256: and 64 hex digits."))
}

fn height_after_tip(form: &Form, tip: u64) -> Result<u64, Problem> {
    let height: u64 = form
        .height
        .text()
        .trim()
        .replace(',', "")
        .parse()
        .map_err(|_| Problem::at(FormField::Height, "The height is a block number."))?;
    if height <= tip {
        return Err(Problem::at(
            FormField::Height,
            format!(
                "Block {} has passed: pick a later one.",
                design::grouped(height)
            ),
        ));
    }
    Ok(height)
}

pub(crate) fn cancel(pending: &programs::Pending) -> programs::Op {
    programs::Op::Cancel {
        height: pending.height,
        program: pending.change.program().to_owned(),
    }
}
