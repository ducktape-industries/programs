//! Who can edit the open repository and the open change, as the ownership program says (through
//! `policy_view::item`), and what the reader may do by it. An item with no record keeps the rule
//! it had before policies (J-011 Q8).
use ownership::Party;
use policy_view::{Editing, Item, item};
use view::design::Ladder;
use view::methods::Change;
use view::prelude::*;
use view::{Div, Stateful};

use crate::Forge;

const REPO: &str = "forge-repo-policy";
const CHANGE: &str = "forge-change-policy";

pub(crate) fn repo_policy(forge: &mut Forge) -> &mut Editing {
    &mut forge.repo_policy
}

pub(crate) fn change_policy(forge: &mut Forge) -> &mut Editing {
    &mut forge.change_policy
}

impl Forge {
    fn reader(&self) -> Option<Party> {
        self.my_account().map(Party::Account)
    }

    fn repo_item(&self) -> Option<Item> {
        let name = self.nav.repo.clone()?;
        Some(Item {
            program: forge::PROGRAM.to_owned(),
            key: forge::tables::CUSTODY.key(&name),
            label: name,
        })
    }

    fn change_item(&self) -> Option<Item> {
        let (repo, n) = (self.nav.repo.clone()?, self.nav.change?);
        Some(Item {
            program: forge::PROGRAM.to_owned(),
            key: forge::tables::AUTHORSHIP.key(&(repo, n)),
            label: format!("change #{n}"),
        })
    }

    pub(crate) fn settle_policies(&mut self, cx: &mut Context<Self>) {
        let reader = self.reader();
        match self.repo_item() {
            Some(repo) => item::hold(self, repo_policy, repo, reader.clone(), cx),
            None => item::let_go(self, repo_policy),
        }
        match self.change_item() {
            Some(change) => item::hold(self, change_policy, change, reader, cx),
            None => item::let_go(self, change_policy),
        }
    }

    fn by_policy(&self, read: Option<bool>, before: bool) -> bool {
        match read {
            Some(admits) => self.may_write() && admits,
            None => before,
        }
    }

    pub(crate) fn may_change_repo(&self) -> bool {
        let read = self
            .repo_item()
            .and_then(|repo| self.repo_policy.admits(&repo));
        self.by_policy(read, self.writes_repo())
    }

    pub(crate) fn may_configure(&self) -> bool {
        let read = self
            .repo_item()
            .and_then(|repo| self.repo_policy.admits(&repo));
        self.by_policy(read, self.owns_repo())
    }

    pub(crate) fn may_edit_change(&self) -> bool {
        let read = self
            .change_item()
            .and_then(|change| self.change_policy.admits(&change));
        self.by_policy(read, self.authored())
    }

    pub(crate) fn may_close_change(&self) -> bool {
        (self.may_write() && self.authored()) || self.may_change_repo()
    }

    fn authored(&self) -> bool {
        let me = self.me_principal();
        self.change()
            .is_some_and(|(change, ..)| me.as_ref() == Some(&change.author))
    }

    pub(crate) fn policy_sheet_open(&self) -> bool {
        self.change_policy.sheet.is_some() || self.repo_policy.sheet.is_some()
    }

    pub(crate) fn close_policy_sheet(&mut self, cx: &mut Context<Self>) {
        item::close_sheet(self, change_policy, cx);
        item::close_sheet(self, repo_policy, cx);
    }

    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        item::moved(self, repo_policy, change.as_ref(), cx);
        item::moved(self, change_policy, change.as_ref(), cx);
    }
}

pub(crate) fn repo_row(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    item::row(&forge.repo_policy, REPO, repo_policy, cx, ladder, theme)
}

pub(crate) fn change_row(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    item::row(
        &forge.change_policy,
        CHANGE,
        change_policy,
        cx,
        ladder,
        theme,
    )
}

pub(crate) fn sheet(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    item::sheet(
        &forge.change_policy,
        CHANGE,
        change_policy,
        cx,
        ladder,
        theme,
    )
    .or_else(|| item::sheet(&forge.repo_policy, REPO, repo_policy, cx, ladder, theme))
}
