//! Who can start an agent's conversations and who can edit a model, as the ownership program says
//! (through `policy_view::item`), and what the reader may do by it. An item with no record keeps
//! the rule it had before policies (J-011 Q8).
use ownership::Party;
use policy_view::{Editing, Item, item};
use view::design::Ladder;
use view::methods::Change;
use view::prelude::*;
use view::{Div, Stateful};

use crate::read::Model;
use crate::{Agents, Screen};

// The conversations' row and sheet, and the model's: every id in them starts so.
const CONVERSATIONS: &str = "agent-conversations-policy";
const MODEL: &str = "model-policy";
// The conversations' row title: their policy decides who starts one.
const STARTS: &str = "Who can start its conversations";
// Claim, for the agent's manager while nobody holds its conversations.
pub(crate) const CLAIM: &str = "Claim who starts them";
pub(crate) const CLAIM_HINT: &str = "Your policies decide it from then on.";

pub(crate) fn conversations_policy(view: &mut Agents) -> &mut Editing {
    &mut view.conversations_policy
}

pub(crate) fn model_policy(view: &mut Agents) -> &mut Editing {
    &mut view.model_policy
}

// A model as ownership records it: its record.
pub(crate) fn model_item(id: &str) -> Item {
    Item {
        program: harness::PROGRAM.to_owned(),
        key: harness::tables::MODELS.key(&id.to_owned()),
        label: format!("model {id}"),
    }
}

impl Agents {
    // The reader as ownership knows a party: their account.
    pub(crate) fn reader(&self) -> Option<Party> {
        self.me.map(Party::Account)
    }

    // Agent `agent`'s conversations as ownership records them: the row its manager claims.
    fn conversations_item(&self, agent: u64) -> Item {
        Item {
            program: harness::PROGRAM.to_owned(),
            key: harness::tables::CONVERSATIONS.key(&agent),
            label: format!("{}'s conversations", self.agent_name(agent)),
        }
    }

    // Each frame: the open agent's conversations are held while its page is open, and the model
    // whose form is open while it is.
    pub(crate) fn settle_policies(&mut self, cx: &mut Context<Self>) {
        let reader = self.reader();
        match self.screen.agent() {
            Some(agent) => {
                let held = self.conversations_item(agent);
                item::hold(self, conversations_policy, held, reader.clone(), cx)
            }
            None => item::let_go(self, conversations_policy),
        }
        let form = match self.screen {
            Screen::Models => self.model_draft.shown,
            Screen::SetModel => true,
            _ => false,
        };
        let existing = self.model_draft.was.as_ref().map(|_| self.model_draft.id());
        match existing.filter(|_| form) {
            Some(id) => item::hold(self, model_policy, model_item(&id), reader, cx),
            None => item::let_go(self, model_policy),
        }
    }

    // Whether the reader may start or fork agent `agent`'s conversations: their policy's answer
    // once read, else `before`, the rule from before policies.
    pub(crate) fn may_start(&self, agent: u64, before: bool) -> bool {
        let read = self
            .conversations_policy
            .admits(&self.conversations_item(agent));
        match read {
            Some(admits) => self.me.is_some() && admits,
            None => before,
        }
    }

    // Whether the reader, agent `agent`'s manager, may claim its conversations: once read, nobody
    // holds them.
    pub(crate) fn claims(&self, agent: u64, manager: u64) -> bool {
        let editing = &self.conversations_policy;
        let key = self.conversations_item(agent).key;
        let held = editing.item.as_ref().is_some_and(|held| held.key == key);
        let none = editing
            .standing
            .ready()
            .is_some_and(|standing| standing.record.is_none());
        self.me == Some(manager) && held && none
    }

    // Whether a policy sheet is open over the view (the phone's Back closes it first).
    pub(crate) fn policy_sheet_open(&self) -> bool {
        self.model_policy.sheet.is_some() || self.conversations_policy.sheet.is_some()
    }

    pub(crate) fn close_policy_sheet(&mut self, cx: &mut Context<Self>) {
        item::close_sheet(self, model_policy, cx);
        item::close_sheet(self, conversations_policy, cx);
    }

    // Ownership wrote: what is held is read again where the block touched it, and the models
    // where it touched one listed.
    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        item::moved(self, conversations_policy, change.as_ref(), cx);
        item::moved(self, model_policy, change.as_ref(), cx);
        let listed = self.screen == Screen::Models
            && self.models.ready().is_some_and(|models| {
                models.iter().any(|model| {
                    let held = model_item(&model.row.id);
                    let touched = |query: ownership::Query| {
                        change
                            .as_ref()
                            .is_none_or(|change| change.touches::<ownership::Ownership, _>(&query))
                    };
                    touched(ownership::Query::Record {
                        program: held.program.clone(),
                        key: held.key.clone(),
                    }) || touched(ownership::Query::Effective {
                        program: held.program,
                        key: held.key,
                    })
                })
            });
        if listed {
            self.read_screen(cx);
        }
    }
}

// The open agent's conversations row: whose they are and their policy.
pub(crate) fn conversations_row(
    view: &Agents,
    cx: &mut Context<Agents>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    let editing = &view.conversations_policy;
    item::row_titled(
        editing,
        CONVERSATIONS,
        STARTS,
        conversations_policy,
        cx,
        ladder,
        theme,
    )
}

// The model whose form is open: whose it is and its policy.
pub(crate) fn model_row(
    view: &Agents,
    cx: &mut Context<Agents>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    item::row(&view.model_policy, MODEL, model_policy, cx, ladder, theme)
}

// A listed model's words: whose it is and its policy once on record, else its owner's name.
pub(crate) fn model_words(view: &Agents, model: &Model) -> String {
    let owned = model.standing.record.is_some();
    match owned {
        true => item::about(&model.standing, view.reader().as_ref()),
        false => {
            let named = view.roster.ready().map_or_else(
                || format!("account {}", model.row.owner),
                |roster| roster.name(model.row.owner),
            );
            match view.me == Some(model.row.owner) {
                true => format!("{named} (you)"),
                false => named,
            }
        }
    }
}

// The policy sheet open over the view, if any: the model's, else the conversations'.
pub(crate) fn sheet(
    view: &Agents,
    cx: &mut Context<Agents>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    item::sheet(&view.model_policy, MODEL, model_policy, cx, ladder, theme).or_else(|| {
        let editing = &view.conversations_policy;
        item::sheet(
            editing,
            CONVERSATIONS,
            conversations_policy,
            cx,
            ladder,
            theme,
        )
    })
}
