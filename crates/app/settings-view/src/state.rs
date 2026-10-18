//! The state Settings keeps: what it read, and each form as typed.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use view::prelude::*;

use crate::queries::Seat;

/// The invite lifetimes offered, in days.
pub(crate) const TTL: [u64; 3] = [1, 7, 30];

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub(crate) session: Session,
    /// the pane open: the desktop menu's pick, or the phone strip's
    pub(crate) section: Section,
    /// the sheet open over the phone's pane; Back closes it
    pub(crate) sheet: Option<Sheet>,
    pub(crate) account: Loadable<Option<Seat>>,
    pub(crate) invite: Loadable<Invite>,
    /// which of [`TTL`] the next invite lasts
    pub(crate) ttl: usize,
    /// the last copy of the invite: done, or the refusal it met
    pub(crate) copied: Loadable<()>,
    pub(crate) create_account: Form,
    pub(crate) create_agent: Form,
    pub(crate) agent_key: Form,
    /// each agent's rename, by its number
    pub(crate) rename_agent: BTreeMap<u64, Form>,
    /// the one suspend, resume or revoke in flight
    pub(crate) agent_standing: Form,
    pub(crate) answering: BTreeMap<u64, Answering>,
}

impl Settings {
    pub(crate) fn sections(&self) -> Vec<Section> {
        let has_account = matches!(self.account.ready(), Some(Some(Seat::Account(_))));
        [Section::Account, Section::Agents, Section::Invites]
            .into_iter()
            .filter(|section| *section != Section::Agents || has_account)
            .collect()
    }

    /// A write of Account's is in flight, any form's (Expo `useBusy`'s
    /// scope, `src/account/Agents.tsx:38-43`).
    pub(crate) fn writing(&self) -> bool {
        let forms = [
            &self.create_account,
            &self.create_agent,
            &self.agent_key,
            &self.agent_standing,
        ];
        forms
            .into_iter()
            .chain(self.rename_agent.values())
            .any(|form| form.busy)
            || self.answering.values().any(Answering::busy)
    }

    /// The pane on screen: the picked one, or Account once it is not listed.
    pub(crate) fn shown(&self) -> Section {
        match self.sections().contains(&self.section) {
            true => self.section,
            false => Section::Account,
        }
    }
}

/// A sheet over the phone's Agents pane; each form the desktop draws inline
/// is one. An agent's sheet is one sheet in three modes, by the agent's
/// number (the Expo app's `AgentSheet`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Sheet {
    /// an agent's actions
    Actions(u64),
    /// an agent's new name
    Rename(u64),
    /// whom an agent answers
    Answers(u64),
    /// a new agent's name
    CreateAgent,
    /// an agent's key request, pasted
    AddKey,
}

impl Sheet {
    /// The sheet once the op it sent landed: a rename goes back to the
    /// agent's actions, an add closes, the rest stay (Expo `Agents.tsx`).
    pub(crate) fn after_landing(self) -> Option<Sheet> {
        match self {
            Self::Rename(number) => Some(Self::Actions(number)),
            Self::CreateAgent | Self::AddKey => None,
            Self::Actions(_) | Self::Answers(_) => Some(self),
        }
    }
}

/// Whom one agent answers, as its manager changes it.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Answering {
    /// Specific picked with nobody added yet: the field shows before
    /// anything is sent
    pub(crate) listing: bool,
    /// Only me or Anyone, sent from the choice
    pub(crate) pick: Form,
    /// the person typed in the field; adding or removing one
    pub(crate) form: Form,
}

impl Answering {
    /// A change to whom it answers is in flight, the choice's or a person's.
    pub(crate) fn busy(&self) -> bool {
        self.pick.busy || self.form.busy
    }

    /// The form that says what stopped the last change; each change clears
    /// the other's, so at most one has a problem.
    pub(crate) fn stopped(&self) -> &Form {
        match self.pick.problem {
            Some(_) => &self.pick,
            None => &self.form,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Choice {
    Manager,
    Specific,
    Anyone,
}

impl Choice {
    pub(crate) const ALL: [Choice; 3] = [Self::Manager, Self::Specific, Self::Anyone];

    pub(crate) fn shown(ownership: &identity::Ownership, listing: bool) -> Self {
        match (ownership, listing) {
            (identity::Ownership::Listed(_), _) | (_, true) => Self::Specific,
            (identity::Ownership::Owner, false) => Self::Manager,
            (identity::Ownership::Anyone, false) => Self::Anyone,
        }
    }
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Manager => "Only me",
            Self::Specific => "Specific people",
            Self::Anyone => "Anyone",
        }
    }
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Specific => "specific",
            Self::Anyone => "anyone",
        }
    }
}

/// The panes of the left menu. Agents is listed only for an account that
/// manages agents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Section {
    #[default]
    Account,
    Agents,
    Invites,
}

impl Section {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::Agents => "Agents",
            Self::Invites => "Invites",
        }
    }
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Agents => "agents",
            Self::Invites => "invites",
        }
    }
}

/// A one-field form: what was typed, whether its submit is in flight, and
/// what stopped it.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Form {
    pub(crate) text: TextField,
    pub(crate) busy: bool,
    pub(crate) problem: Option<Problem>,
}

/// Why a form's submit did not land. The form that shows it words it.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum Problem {
    /// nothing was typed
    Empty,
    /// the text is not an `AddKey` for one of the reader's agents
    NotAKeyRequest,
    NoSuchAccount,
    SeveralAccounts,
    /// it did not land, worded as every view words it
    /// (`design::failure`)
    Refused(String),
}
