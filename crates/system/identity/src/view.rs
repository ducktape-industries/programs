//! What a view shows of an account. Who the reader is comes with the
//! session (`Session.account`); no view asks identity for it.
use std::collections::BTreeMap;
use std::future::Future;

use crate::{Account, AccountNumber, PageRequest, PageResponse, ask};

// Each account's picture by its number (its card's avatar), over every page
// of identity's accounts; an account with none is left out. `ask` is
// `|ask| host.query(ask)` in a view, so this crate links no view runtime.
pub async fn avatars<F: Future<Output = Result<PageResponse<Account>, ::program::Error>>>(
    ask: impl Fn(ask::List) -> F,
) -> Result<BTreeMap<AccountNumber, ::program::BlobId>, ::program::Error> {
    let mut avatars = BTreeMap::new();
    let mut after = None;
    loop {
        let page = ask(ask::List {
            page: PageRequest { after, limit: None },
        })
        .await?;
        let pictured = page
            .items
            .into_iter()
            .filter_map(|account| Some((account.number, account.card.avatar?)));
        avatars.extend(pictured);
        after = page.next;
        if after.is_none() {
            return Ok(avatars);
        }
    }
}

// What an account is, as members and explorer label it: "User",
// "Agent · managed by eddy · suspended", "Program · chat". The badge and
// the note are `design::badge` and `design::note`; `name_of` names the
// manager, or its number stands in.
pub fn kind(
    kind: &crate::Kind,
    name_of: impl FnOnce(crate::AccountNumber) -> Option<String>,
) -> String {
    let what = design::badge(kind, |manager| {
        name_of(manager).unwrap_or_else(|| format!("#{manager}"))
    })
    .unwrap_or_else(|| "User".into());
    match design::note(kind) {
        Some(note) => format!("{what} · {note}"),
        None => what,
    }
}
