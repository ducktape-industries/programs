//! Who can start an agent's conversations and who can edit a provider's route, as ownership says
//! (through `policy_view::item`), and what the reader may do by it. An item with no record keeps
//! the rule it had before policies (J-011 Q8).
use ownership::Party;
use policy_view::{Editing, Item, item};
use view::design::Ladder;
use view::methods::Change;
use view::prelude::*;
use view::{Div, Stateful};

use crate::read::Route;
use crate::{Agents, Screen};

const CONVERSATIONS: &str = "agent-conversations-policy";
const ROUTE: &str = "route-policy";
const STARTS: &str = "Who can start its conversations";
pub(crate) const CLAIM: &str = "Claim who starts them";
pub(crate) const CLAIM_HINT: &str = "Your policies decide it from then on.";

pub(crate) fn conversations_policy(view: &mut Agents) -> &mut Editing {
    &mut view.conversations_policy
}

pub(crate) fn route_policy(view: &mut Agents) -> &mut Editing {
    &mut view.route_policy
}

pub(crate) fn route_item(provider: &str) -> Item {
    Item {
        program: harness::PROGRAM.to_owned(),
        key: harness::tables::PROVIDER_ROUTES.key(&provider.to_owned()),
        label: format!("provider {provider}'s route"),
    }
}

impl Agents {
    pub(crate) fn reader(&self) -> Option<Party> {
        self.me.map(Party::Account)
    }

    fn conversations_item(&self, agent: u64) -> Item {
        Item {
            program: harness::PROGRAM.to_owned(),
            key: harness::tables::CONVERSATIONS.key(&agent),
            label: format!("{}'s conversations", self.agent_name(agent)),
        }
    }

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
            Screen::Routes => self.route_draft.shown,
            Screen::SetRoute => true,
            _ => false,
        };
        let existing = self
            .route_draft
            .was
            .as_ref()
            .map(|_| self.route_draft.provider());
        match existing.filter(|_| form) {
            Some(id) => item::hold(self, route_policy, route_item(&id), reader, cx),
            None => item::let_go(self, route_policy),
        }
    }

    pub(crate) fn may_start(&self, agent: u64, before: bool) -> bool {
        let read = self
            .conversations_policy
            .admits(&self.conversations_item(agent));
        match read {
            Some(admits) => self.me.is_some() && admits,
            None => before,
        }
    }

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

    pub(crate) fn policy_sheet_open(&self) -> bool {
        self.route_policy.sheet.is_some() || self.conversations_policy.sheet.is_some()
    }

    pub(crate) fn close_policy_sheet(&mut self, cx: &mut Context<Self>) {
        item::close_sheet(self, route_policy, cx);
        item::close_sheet(self, conversations_policy, cx);
    }

    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        item::moved(self, conversations_policy, change.as_ref(), cx);
        item::moved(self, route_policy, change.as_ref(), cx);
        let listed = self.screen == Screen::Routes
            && self.routes.ready().is_some_and(|routes| {
                routes.iter().any(|route| {
                    let held = route_item(&route.row.provider);
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
        let touched = |program: &str, key: Vec<u8>| {
            let query = ownership::Query::Effective {
                program: program.to_owned(),
                key,
            };
            change
                .as_ref()
                .is_none_or(|change| change.touches::<ownership::Ownership, _>(&query))
        };
        let conversations = self
            .screen
            .agent()
            .is_some_and(|agent| touched(harness::PROGRAM, self.conversations_item(agent).key));
        let config = self
            .detail
            .ready()
            .and_then(|detail| detail.config_writers.as_ref())
            .is_some_and(|writers| match writers {
                program::Writers::Listed { at, .. } => {
                    touched(files::PROGRAM, files::guard_key(at))
                }
                _ => false,
            });
        if listed || conversations || config {
            self.read_screen(cx);
        }
    }
}

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

pub(crate) fn route_row(
    view: &Agents,
    cx: &mut Context<Agents>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    item::row(&view.route_policy, ROUTE, route_policy, cx, ladder, theme)
}

pub(crate) fn route_words(view: &Agents, route: &Route) -> String {
    let owned = route.standing.record.is_some();
    match owned {
        true => item::about(&route.standing, view.reader().as_ref()),
        false => {
            let named = view.roster.ready().map_or_else(
                || format!("account {}", route.row.owner),
                |roster| roster.name(route.row.owner),
            );
            match view.me == Some(route.row.owner) {
                true => format!("{named} (you)"),
                false => named,
            }
        }
    }
}

pub(crate) fn sheet(
    view: &Agents,
    cx: &mut Context<Agents>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    item::sheet(&view.route_policy, ROUTE, route_policy, cx, ladder, theme).or_else(|| {
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
