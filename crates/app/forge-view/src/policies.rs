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

// The repository's row and sheet, and the change's: every id in them starts so.
const REPO: &str = "forge-repo-policy";
const CHANGE: &str = "forge-change-policy";

pub(crate) fn repo_policy(forge: &mut Forge) -> &mut Editing {
    &mut forge.repo_policy
}

pub(crate) fn change_policy(forge: &mut Forge) -> &mut Editing {
    &mut forge.change_policy
}

impl Forge {
    // The reader as ownership knows a party: their account.
    fn reader(&self) -> Option<Party> {
        self.my_account().map(Party::Account)
    }

    // The open repository as ownership records it: its custody row.
    fn repo_item(&self) -> Option<Item> {
        let name = self.nav.repo.clone()?;
        Some(Item {
            program: forge::PROGRAM.to_owned(),
            key: forge::tables::CUSTODY.key(&name),
            label: name,
        })
    }

    // The open change as ownership records it: its authorship row.
    fn change_item(&self) -> Option<Item> {
        let (repo, n) = (self.nav.repo.clone()?, self.nav.change?);
        Some(Item {
            program: forge::PROGRAM.to_owned(),
            key: forge::tables::AUTHORSHIP.key(&(repo, n)),
            label: format!("change #{n}"),
        })
    }

    // Each frame: the open repository's standing is held while it is open, and the open change's
    // while it is (both decide what its screens offer).
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

    // A policy's answer once it is read (for a reader who writes at all), else `before`: the rule
    // from before policies.
    fn by_policy(&self, read: Option<bool>, before: bool) -> bool {
        match read {
            Some(admits) => self.may_write() && admits,
            None => before,
        }
    }

    // Whether the reader may do what the open repository's policy decides (push, merge, close
    // another's change): its answer, else as before policies, its owner and the writers access
    // lists.
    pub(crate) fn may_change_repo(&self) -> bool {
        let read = self
            .repo_item()
            .and_then(|repo| self.repo_policy.admits(&repo));
        self.by_policy(read, self.writes_repo())
    }

    // Whether the reader may change the open repository's settings (default head, force, delete):
    // its policy's answer, else as before policies, its owner.
    pub(crate) fn may_configure(&self) -> bool {
        let read = self
            .repo_item()
            .and_then(|repo| self.repo_policy.admits(&repo));
        self.by_policy(read, self.owns_repo())
    }

    // Whether the reader may edit the open change: its policy's answer, else as before policies,
    // its author.
    pub(crate) fn may_edit_change(&self) -> bool {
        let read = self
            .change_item()
            .and_then(|change| self.change_policy.admits(&change));
        self.by_policy(read, self.authored())
    }

    // Whether the reader may close the open change: its author closes their own, as forge lets
    // them; anyone else's close is the repository's policy's to decide.
    pub(crate) fn may_close_change(&self) -> bool {
        (self.may_write() && self.authored()) || self.may_change_repo()
    }

    fn authored(&self) -> bool {
        let me = self.me_principal();
        self.change()
            .is_some_and(|(change, ..)| me.as_ref() == Some(&change.author))
    }

    // Whether a policy sheet is open over the view (the phone's Back closes it first).
    pub(crate) fn policy_sheet_open(&self) -> bool {
        self.change_policy.sheet.is_some() || self.repo_policy.sheet.is_some()
    }

    pub(crate) fn close_policy_sheet(&mut self, cx: &mut Context<Self>) {
        item::close_sheet(self, change_policy, cx);
        item::close_sheet(self, repo_policy, cx);
    }

    // Ownership wrote: what is held is read again where the block touched it.
    pub(crate) fn ownership_moved(&mut self, change: Option<Change>, cx: &mut Context<Self>) {
        if self.hidden {
            return;
        }
        item::moved(self, repo_policy, change.as_ref(), cx);
        item::moved(self, change_policy, change.as_ref(), cx);
    }
}

// The open repository's row, for its settings.
pub(crate) fn repo_row(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Stateful<Div>> {
    item::row(&forge.repo_policy, REPO, repo_policy, cx, ladder, theme)
}

// The open change's row, for its details.
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

// The policy sheet open over forge, if any: the change's, else the repository's.
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
