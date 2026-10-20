//! The state Settings keeps: what it read, and each form as typed.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use view::prelude::*;

use crate::photo::Photo;
use crate::queries::Seat;

pub(crate) const TTL: [u64; 3] = [1, 7, 30];

#[derive(Default, Serialize, Deserialize)]
pub struct Settings {
    pub(crate) session: Session,
    pub(crate) section: Section,
    pub(crate) sheet: Option<Sheet>,
    pub(crate) account: Loadable<Option<Seat>>,
    pub(crate) invite: Loadable<Invite>,
    pub(crate) ttl: usize,
    pub(crate) copied: Loadable<()>,
    pub(crate) create_account: Form,
    pub(crate) create_agent: Form,
    pub(crate) agent_key: Form,
    pub(crate) rename_agent: BTreeMap<u64, Form>,
    pub(crate) agent_standing: Form,
    pub(crate) answering: BTreeMap<u64, Answering>,
    pub(crate) holdings: Loadable<policy_view::Holdings>,
    pub(crate) policy_sheet: Option<PolicySheet>,
    pub(crate) policy_form: PolicyForm,
    pub(crate) policy_items: Loadable<usize>,
    pub(crate) policy_write: Form,
    pub(crate) change_photo: Form,
    pub(crate) remove_photo: Form,
    pub(crate) photo_of: Option<u64>,
    #[serde(skip)]
    pub(crate) photo: Option<Photo>,
    #[serde(skip)]
    pub(crate) pictures: Option<Entity<view::pictures::Pictures>>,
    #[serde(skip)]
    pub(crate) reads_files: bool,
}

impl Settings {
    pub(crate) fn sections(&self) -> Vec<Section> {
        let has_account = matches!(self.account.ready(), Some(Some(Seat::Account(_))));
        [
            Section::Account,
            Section::Agents,
            Section::Policies,
            Section::Invites,
        ]
        .into_iter()
        .filter(|section| has_account || !matches!(section, Section::Agents | Section::Policies))
        .collect()
    }

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

    pub(crate) fn shown(&self) -> Section {
        match self.sections().contains(&self.section) {
            true => self.section,
            false => Section::Account,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Sheet {
    Actions(u64),
    Rename(u64),
    Answers(u64),
    CreateAgent,
    AddKey,
}

impl Sheet {
    pub(crate) fn after_landing(self) -> Option<Sheet> {
        match self {
            Self::Rename(number) => Some(Self::Actions(number)),
            Self::CreateAgent | Self::AddKey => None,
            Self::Actions(_) | Self::Answers(_) => Some(self),
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Answering {
    pub(crate) listing: bool,
    pub(crate) pick: Form,
    pub(crate) form: Form,
}

impl Answering {
    pub(crate) fn busy(&self) -> bool {
        self.pick.busy || self.form.busy
    }

    pub(crate) fn stopped(&self) -> &Form {
        match self.pick.problem {
            Some(_) => &self.pick,
            None => &self.form,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Choice {
    #[default]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Section {
    #[default]
    Account,
    Agents,
    Policies,
    Invites,
}

impl Section {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::Agents => "Agents",
            Self::Policies => "Policies",
            Self::Invites => "Invites",
        }
    }
    pub(crate) fn slug(self) -> &'static str {
        match self {
            Self::Account => "account",
            Self::Agents => "agents",
            Self::Policies => "policies",
            Self::Invites => "invites",
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Form {
    pub(crate) text: TextField,
    pub(crate) busy: bool,
    pub(crate) problem: Option<Problem>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) enum Problem {
    Empty,
    NotAKeyRequest,
    NoSuchAccount,
    SeveralAccounts,
    NotAPicture,
    Refused(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum PolicySheet {
    Actions(u64),
    Edit(Option<u64>),
    Remove(u64),
    Move(u64),
    Default(#[serde(with = "view::borsh_bytes")] ownership::Scope),
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct PolicyForm {
    pub(crate) name: Form,
    pub(crate) grant: Choice,
    #[serde(with = "view::borsh_bytes")]
    pub(crate) listed: Vec<ownership::Party>,
    pub(crate) names: BTreeMap<u64, String>,
    pub(crate) person: Form,
}
