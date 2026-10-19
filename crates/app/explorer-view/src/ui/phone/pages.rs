//! Expo `src/network/explorer/pages.tsx`: the root's pages under the strip,
//! Overview, Blocks, Transactions and Accounts (Programs is
//! [`programs`]), and the Transactions page one program's page opens
//! ([`txs`]). Reads the window (`chain`), the status, the validators, the
//! accounts and what activity recorded for each; writes nothing but the
//! route a row opens.
use std::ops::Range;

use view::design::space;
use view::prelude::*;
use view::{Div, Stateful};

use crate::decode::{grouped, plural};
use crate::words::{self, unindexed};
use crate::{Cx, Explorer, Route, TxRow};

use super::Look;
use super::parts::{
    Fact, Who, aside, block_lines, caption, chevron, column, face, facts, list_row, more_link,
    opens, quiet, refused, scroller, section, tail, title_and_caption, tx_line,
};

/// The rows each Overview section draws (Expo `pages.tsx:20` `LATEST`).
const LATEST: usize = 12;
/// The most lines a list page draws (Expo `src/network/window.ts`
/// `LIST_ROWS`).
const LIST_ROWS: usize = 50;

/// The root page's list under the strip.
pub(super) fn body(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    match view.pages.first() {
        Route::Blocks => chain_gate(view, cx, k).unwrap_or_else(|| blocks(view, cx, k)),
        Route::Transactions(_) => {
            chain_gate(view, cx, k).unwrap_or_else(|| transactions(view, None, cx, k))
        }
        Route::Accounts => accounts(view, cx, k),
        Route::Programs => super::programs::programs(view, cx, k),
        // the root is always a page's own list (`Explorer::go`)
        Route::Overview
        | Route::Block(_)
        | Route::Tx { .. }
        | Route::Account(_)
        | Route::Program(_) => chain_gate(view, cx, k).unwrap_or_else(|| overview(view, cx, k)),
    }
}

/// A window page before the window's first page lands (Expo `ChainGate`):
/// words::READING_CHAIN, or its refusal with Retry; `None` once it holds
/// a block.
pub(super) fn chain_gate(view: &Explorer, cx: Cx, k: &Look) -> Option<AnyElement> {
    if !view.chain.blocks.is_empty() {
        return None;
    }
    Some(match &view.chain.failed {
        Some(sentence) => refused(sentence, cx, k),
        None => quiet("explorer-loading", words::READING_CHAIN, k),
    })
}

/// How many blocks the window holds.
fn held(view: &Explorer) -> u64 {
    view.chain.blocks.len() as u64
}

/// The figures, the latest blocks and the latest transactions (Expo
/// `Overview`).
fn overview(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let txs = if view.chain.txs.is_empty() {
        quiet("explorer-no-txs", words::no_transactions_in(held(view)), k)
    } else {
        column(
            "explorer-latest-txs-list",
            view.chain
                .txs
                .iter()
                .take(LATEST)
                .map(|tx| tx_line(view, tx, Who::Signer, cx, k))
                .collect(),
        )
        .into_any_element()
    };
    let all_blocks = more_link(
        "explorer-all-blocks",
        words::ALL_BLOCKS,
        Route::Blocks,
        cx,
        k,
    );
    let all_txs = more_link(
        "explorer-all-txs",
        words::ALL_TRANSACTIONS,
        Route::Transactions(None),
        cx,
        k,
    );
    scroller("explorer-page")
        .child(facts("explorer-stats", figures(view), k))
        .child(section(
            "explorer-latest-blocks-heading",
            "Latest activity",
            Some(all_blocks),
            k,
        ))
        .child(column(
            "explorer-latest-blocks-list",
            block_lines(view, LATEST, cx, k),
        ))
        .child(section(
            "explorer-latest-txs-heading",
            "Latest transactions",
            Some(all_txs),
            k,
        ))
        .child(txs)
        .into_any_element()
}

/// The Overview's four figures: the height and the block time, the epoch
/// and the blocks to the next, the validators, the accounts; a dash for
/// one not read yet.
fn figures(view: &Explorer) -> Vec<Fact> {
    let status = view.status.ready();
    let dash = || words::NO_VALUE.to_owned();
    let left = view.epoch_left();
    vec![
        Fact {
            key: "height",
            label: "Height",
            value: status.map_or_else(dash, |status| grouped(status.height)),
            caption: status.map(|status| words::block_every(status.block_time_ms)),
        },
        Fact {
            key: "epoch",
            label: "Epoch",
            value: status
                .filter(|_| left.is_some())
                .map_or_else(dash, |status| grouped(status.epoch)),
            caption: left.map(words::next_in),
        },
        Fact {
            key: "validators",
            label: "Validators",
            value: view
                .validators
                .ready()
                .map_or_else(dash, |keys| grouped(keys.len() as u64)),
            caption: None,
        },
        Fact {
            key: "accounts",
            label: "Accounts",
            value: view
                .accounts
                .ready()
                .map_or_else(dash, |accounts| grouped(accounts.list.len() as u64)),
            caption: None,
        },
    ]
}

/// The window's blocks, newest first, each run of empty ones folded (Expo
/// `Blocks`), and where the node's archive stops when it stops inside the
/// window.
fn blocks(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let archive = view
        .chain
        .unlisted_below()
        .filter(|_| view.chain.complete)
        .map(|from| quiet("explorer-blocks-unindexed", unindexed(from), k));
    scroller("explorer-page")
        .child(section(
            "explorer-blocks-heading",
            "Blocks",
            Some(aside(words::last_blocks(held(view)))),
            k,
        ))
        .child(column(
            "explorer-blocks-list",
            block_lines(view, LIST_ROWS, cx, k),
        ))
        .children(archive)
        .into_any_element()
}

/// The window's transactions, newest first, or those sent to `program`
/// (Expo `Transactions`).
pub(super) fn transactions(view: &Explorer, program: Option<&str>, cx: Cx, k: &Look) -> AnyElement {
    let matching: Vec<&TxRow> = view
        .chain
        .txs
        .iter()
        .filter(|tx| program.is_none_or(|program| tx.target == program))
        .collect();
    let title = match program {
        Some(program) => words::transactions_to(program),
        None => "Transactions".to_owned(),
    };
    let count = words::transactions_in(matching.len() as u64, held(view));
    let list = if matching.is_empty() {
        quiet("explorer-no-txs", words::no_transactions_in(held(view)), k)
    } else {
        column(
            "explorer-transactions-list",
            matching
                .into_iter()
                .take(LIST_ROWS)
                .map(|tx| tx_line(view, tx, Who::Signer, cx, k))
                .collect(),
        )
        .into_any_element()
    };
    scroller("explorer-page")
        .child(section(
            "explorer-transactions-heading",
            title,
            Some(aside(count)),
            k,
        ))
        .child(list)
        .into_any_element()
}

/// Every account identity lists (Expo `Accounts`, a `FlatList`): each with
/// its face, what it is and its devices, the transactions activity
/// recorded for it; only the rows on screen are drawn.
fn accounts(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let listed = match &view.accounts {
        Loadable::Ready(listed) | Loadable::Reloading(listed, _) => listed,
        Loadable::Failed(refusal) => return refused(&design::sentence(refusal), cx, k),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-accounts-loading", words::READING_ACCOUNTS, k);
        }
    };
    let count = words::tx_recorded(listed.list.len() as u64);
    let look = *k;
    let rows = uniform_list(
        "explorer-accounts-list",
        listed.list.len(),
        cx.processor(move |view: &mut Explorer, range: Range<usize>, _, cx| {
            let Some(listed) = view.accounts.ready() else {
                return Vec::new();
            };
            range
                .filter_map(|index| listed.list.get(index))
                .map(|account| account_line(view, account, cx, &look))
                .collect()
        }),
    )
    .flex_1()
    .min_h_0()
    // the list's foot (Expo `pages.tsx:134` `ListFooterComponent`)
    .pb(space::BLOCK);
    div()
        .id("explorer-page")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(section(
            "explorer-accounts-heading",
            "Accounts",
            Some(aside(count)),
            k,
        ))
        .child(rows)
        .into_any_element()
}

/// An account (Expo `Accounts`' row): its face, its name, `#n · what it is
/// · its devices`, the transactions recorded for it.
fn account_line(view: &Explorer, account: &identity::Account, cx: Cx, k: &Look) -> Stateful<Div> {
    let number = account.number;
    let sent = view.sent_by(number);
    let about = format!(
        "#{number} · {} · {}",
        view.kind_of(account),
        plural(account.keys().len() as u64, "device", "devices")
    );
    let row = list_row(SharedString::from(format!("explorer-account-{number}")), k);
    opens(
        row,
        account.card.name.clone(),
        Route::Account(number),
        cx,
        k,
    )
    .child(design::lead(face(
        account,
        view.picture(account, cx),
        k.ladder.avatar,
        k,
    )))
    .child(title_and_caption(
        div().truncate().child(account.card.name.clone()),
        caption(about, k),
    ))
    .child(tail(
        sent.map_or_else(|| words::NO_COUNT.to_owned(), words::tx_count),
        k,
    ))
    .child(chevron(k))
}
