//! The vote, for a reader whose seated key votes for a validator (valset's
//! `Voices`): the motions that wait, the programs role's scheduled changes and
//! who made each, and the presses that vote on them. A program is changed
//! in Explorer's Programs; this list stays here, in valset's view, which
//! only the vote changes, so an account's change is seen where no
//! account can hide it. A press the reader's uncast voices would carry
//! to the quorum enacts, so it waits for a yes first
//! ([`Voting::confirming`]). The words for a motion are here, pure, so
//! each is a unit test.
use abi::hex;
use abi::role::halo;
use programs::{Change, Entry, Pending};
use valset::{Motion, Op, Valset, Voted};
use view::methods::Submitted;
use view::prelude::*;

use crate::Nodes;
use crate::queries::{self, Motions, Names, Node, Schedule};

#[derive(Default)]
pub(crate) struct Voting {
    pub(crate) signer: Option<Vec<u8>>,
    pub(crate) voices: Vec<Vec<u8>>,
    pub(crate) motions: Reread<Motions>,
    pub(crate) roster: Reread<Vec<Entry>>,
    pub(crate) scheduled: Reread<Schedule>,
    pub(crate) confirming: Option<Motion>,
    pub(crate) busy: bool,
    pub(crate) outcome: Option<Result<String, String>>,
    pub(crate) pressed_on: Option<Vec<u8>>,
}

impl Voting {
    pub(crate) fn votes(&self) -> bool {
        !self.voices.is_empty()
    }

    pub(crate) fn pressable(&self) -> bool {
        !self.busy && self.motions.ready().is_some()
    }

    pub(crate) fn voters(&self, motion: &Motion) -> &[Vec<u8>] {
        self.motions
            .ready()
            .and_then(|motions| motions.pending.iter().find(|p| &p.motion == motion))
            .map_or(&[], |pending| pending.voters.as_slice())
    }

    pub(crate) fn enacts(&self, motion: &Motion) -> bool {
        let Some(motions) = self.motions.ready() else {
            return true;
        };
        let voters = self.voters(motion);
        let uncast = self.voices.iter().filter(|v| !voters.contains(v)).count();
        (voters.len() + uncast) as u64 >= motions.needed
    }

    pub(crate) fn uncast(&self, motion: &Motion) -> usize {
        let voters = self.voters(motion);
        self.voices.iter().filter(|v| !voters.contains(v)).count()
    }

    pub(crate) fn entries(&self) -> &[Entry] {
        self.roster.ready().map_or(&[], Vec::as_slice)
    }

    pub(crate) fn words(&self, motion: &Motion) -> String {
        words(motion, self.entries())
    }
}

impl Nodes {
    pub(crate) fn session_changed(&mut self, session: Session, cx: &mut Context<Self>) {
        self.read_reader(session.account, cx);
        self.chain = Some(session.chain_id).filter(|chain| !chain.is_empty());
        let signer = design::unhex(&session.signer).filter(|key| !key.is_empty());
        if signer != self.voting.signer {
            self.voting.voices.clear();
            self.voting.confirming = None;
            self.voting.signer = signer;
        }
        self.read_voices(cx);
    }

    fn read_reader(&mut self, account: Option<u64>, cx: &mut Context<Self>) {
        if account == self.reader_account {
            return;
        }
        self.reader_account = account;
        self.reader.clear();
        self.reading_reader = account.map(|number| {
            let ask = queries::name(cx.host(), number);
            cx.land(ask, |view, answer, cx| {
                match answer {
                    Ok(name) => view.reader = name.unwrap_or_default(),
                    Err(refusal) => cx.log_refused("the reader's name", &refusal),
                }
                cx.notify();
            })
        });
    }

    pub(crate) fn read_voices(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.voting.signer.clone() else {
            return;
        };
        let ask = queries::voices(cx.host(), key.clone());
        cx.land(ask, move |view, answer, cx| {
            if view.voting.signer.as_ref() != Some(&key) {
                return;
            }
            match answer {
                Ok(voices) => {
                    view.voting.voices = voices;
                    if view.voting.votes() {
                        view.read_motions(cx);
                        view.read_programs(cx);
                    }
                }
                Err(refusal) => cx.log_refused("whom this key votes for", &refusal),
            }
            cx.notify();
        })
        .detach();
    }

    pub(crate) fn read_motions(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.voting.motions,
            |_, cx| queries::motions(cx.host()),
            Self::read_motions,
        );
    }

    pub(crate) fn read_programs(&mut self, cx: &mut Context<Self>) {
        if !self.voting.votes() {
            return;
        }
        self.read_roster(cx);
        self.read_scheduled(cx);
    }

    fn read_roster(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.voting.roster,
            |_, cx| queries::roster(cx.host()),
            Self::read_roster,
        );
    }

    fn read_scheduled(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.voting.scheduled,
            |_, cx| queries::scheduled(cx.host()),
            Self::read_scheduled,
        );
    }

    pub(crate) fn press(&mut self, motion: Motion, cx: &mut Context<Self>) {
        if !self.voting.pressable() {
            return;
        }
        match self.voting.enacts(&motion) {
            true => self.voting.confirming = Some(motion),
            false => self.vote(motion, cx),
        }
        cx.notify();
    }

    pub(crate) fn confirm(&mut self, cx: &mut Context<Self>) {
        if let Some(motion) = self.voting.confirming.take() {
            self.vote(motion, cx);
        }
        cx.notify();
    }

    pub(crate) fn hold(&mut self, cx: &mut Context<Self>) {
        self.voting.confirming = None;
        cx.notify();
    }

    fn vote(&mut self, motion: Motion, cx: &mut Context<Self>) {
        self.voting.busy = true;
        self.voting.outcome = None;
        self.voting.pressed_on = self.open.clone();
        let ask = cx.host().ask::<Submit<Valset>>(Op::Vote(motion));
        cx.land(ask, |view, answer, cx| {
            view.voting.busy = false;
            let said = said(answer);
            let closed = view.voting.pressed_on.is_some() && view.voting.pressed_on != view.open;
            view.voting.outcome = (said.is_err() || !closed).then_some(said);
            view.read_voices(cx);
            cx.notify();
        })
        .detach();
    }

    pub(crate) fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        self.voting.outcome = None;
        cx.notify();
    }
}

pub(crate) fn said(answer: Result<Submitted, Error>) -> Result<String, String> {
    match answer {
        Ok(Submitted::Applied { output, .. }) => Ok(match abi::decode::<Voted>(&output) {
            Ok(Voted::Enacted) => "Enacted".to_owned(),
            Ok(Voted::Counted { votes, needed }) => format!("Vote counted: {votes} of {needed}"),
            Err(_) => "Voted".to_owned(),
        }),
        answer => Err(design::failure(&answer).expect("only a landed write has no failure")),
    }
}

pub(crate) fn motions_on(node: &Node) -> Vec<(&'static str, &'static str, Motion)> {
    let key = node.key.clone();
    match node.validator {
        true => vec![
            ("demote", "Demote", Motion::Demote { key: key.clone() }),
            ("remove", "Remove", Motion::Remove { key }),
        ],
        false => vec![("promote", "Promote", Motion::Promote { key })],
    }
}

pub(crate) fn cancel(pending: &Pending) -> Motion {
    let op = programs::Op::Cancel {
        height: pending.height,
        program: pending.change.program().to_owned(),
    };
    Motion::Role {
        role: abi::role::PROGRAMS.into(),
        op: abi::encode(&op),
    }
}

pub(crate) fn code_words(code: &abi::BlobId) -> String {
    let kind = match code.kind() {
        abi::HashKind::Sha256 => "sha256",
        abi::HashKind::Sha1 => "sha1",
    };
    format!("{kind}:{}", design::short_hex(&hex(code.digest())))
}

fn key_words(key: &[u8]) -> String {
    design::short_hex(&hex(key))
}

pub(crate) fn named(names: &Names, account: u64) -> String {
    names
        .get(&account)
        .cloned()
        .unwrap_or_else(|| format!("account {account}"))
}

pub(crate) fn words(motion: &Motion, roster: &[Entry]) -> String {
    match motion {
        Motion::Promote { key } => format!("Promote {} to validator", key_words(key)),
        Motion::Demote { key } => format!("Demote {} to resident", key_words(key)),
        Motion::Remove { key } => format!("Remove {} from the network", key_words(key)),
        Motion::Door { open } => {
            let door = crate::words::door(*open);
            format!("{}: {}", door.label, door.meaning)
        }
        Motion::Role { role, op } if role == abi::role::PROGRAMS => match abi::decode(op) {
            Ok(programs::Op::ScheduleOver { scheduled, .. }) => {
                scheduled_words(scheduled.height, &scheduled.change, roster)
            }
            Ok(programs::Op::ScheduleOverIn { blocks, change, .. }) => {
                let when = format!("{} blocks after the vote passes", design::grouped(blocks));
                change_words(&when, &change, roster)
            }
            Ok(programs::Op::Cancel { height, program }) => format!(
                "Cancel {program}'s change at block {}",
                design::grouped(height)
            ),
            Ok(programs::Op::Own { program, holder }) => match holder {
                programs::Holder::Vote => format!("Hand {program} to the validators' vote"),
                programs::Holder::Account(account) => {
                    format!("Hand {program} to account {account}")
                }
                programs::Holder::Accounts(accounts) => {
                    let listed: Vec<String> = accounts.iter().map(u64::to_string).collect();
                    format!("Hand {program} to accounts {}", listed.join(", "))
                }
            },
            Err(_) if op.first() == Some(&0) => {
                "A plain Schedule the programs role no longer takes".into()
            }
            Err(_) => "A program change this screen can't read".into(),
        },
        Motion::Role { role, op } if role == abi::role::HALO => match abi::decode(op) {
            Ok(halo::Op::Open { name, cap }) => format!("Open lane {name} at class {cap:?}"),
            Ok(halo::Op::Set {
                name,
                cap,
                enabled: true,
            }) => format!("Serve lane {name} at class {cap:?}"),
            Ok(halo::Op::Set {
                name,
                enabled: false,
                ..
            }) => format!("Pause lane {name}"),
            Ok(halo::Op::Close { name }) => format!("Close lane {name}"),
            Err(_) => "A lane change this screen can't read".into(),
        },
        Motion::Role { role, .. } => format!("An op for the {role} role"),
        Motion::Queries { policy } => queries_words(*policy).into(),
    }
}

fn queries_words(policy: valset::QueryPolicy) -> &'static str {
    match policy {
        valset::QueryPolicy::Signed => "Answer signed queries only",
        valset::QueryPolicy::Open => "Answer unsigned queries too",
    }
}

fn params_words(entry: &Entry, roster: &[Entry]) -> &'static str {
    match roster.iter().find(|now| now.program == entry.program) {
        Some(now) if now.params == entry.params => "params unchanged",
        Some(_) => "params changed",
        None => "a new program",
    }
}

pub(crate) fn scheduled_words(height: u64, change: &Change, roster: &[Entry]) -> String {
    let at = format!("at block {}", design::grouped(height));
    change_words(&at, change, roster)
}

fn change_words(when: &str, change: &Change, roster: &[Entry]) -> String {
    match change {
        Change::Set(entry) => format!(
            "Set {} to code {} {when} · {}",
            entry.program,
            code_words(&entry.code),
            params_words(entry, roster)
        ),
        Change::Publish { entry, roles } if roles.is_empty() => format!(
            "Publish {} at code {} {when} · {} · no role",
            entry.program,
            code_words(&entry.code),
            params_words(entry, roster)
        ),
        Change::Publish { entry, roles } => format!(
            "Publish {} at code {} {when} · {} · fills {}",
            entry.program,
            code_words(&entry.code),
            params_words(entry, roster),
            roles.join(", ")
        ),
        change => format!("{} {} {when}", change.verb(), change.program()),
    }
}
