//! Expo `app/(tabs)/explorer/_layout.tsx`: Explorer under a finger, the
//! Expo app's screens word for word (mobile `app/(tabs)/explorer/**`,
//! `src/network/explorer/*`, `src/kit/*`), one file per screen; the root
//! picks this tree once, from the device (`lib.rs`). The stack is the
//! model's [`Explorer::pages`]: the root ([`explorer`]) is the title row,
//! the page strip and the page's list; a block, a transaction, an account,
//! a program or a program's transactions goes over the page it was opened
//! from, Back popping one, and a sheet over that; a link that opened
//! nothing is a page of its own. Each file imports what it draws with.
//! Reads the stack, the open sheet and the note; writes Back. The
//! desktop's pages (`ui/*.rs`) are the other design: the reads, routes,
//! writes and words both draw from live outside `ui/`.
use view::design::Ladder;
use view::prelude::*;

use crate::state::{Linked, Sheet};
use crate::{Cx, Explorer, Note, Route};

mod account;
mod block;
mod explorer;
mod form;
mod pages;
mod parts;
mod program;
mod programs;
mod tx;
mod txs;
mod unlinked;

/// What every piece draws with: the theme, the touch ladder (the Expo
/// app's sizes, `design::TOUCH`), and the stack's depth, which a page's
/// `Nav` says.
#[derive(Clone, Copy)]
struct Look {
    theme: Theme,
    ladder: Ladder,
    depth: usize,
}

pub(crate) fn render(view: &Explorer, cx: Cx) -> AnyElement {
    let k = &Look {
        theme: *cx.global::<Theme>(),
        ladder: design::TOUCH,
        depth: view.pages.depth(),
    };
    // a link that opened nothing is a page of its own over the stack
    let gone = match &view.note {
        Some(note @ Note::Unlinked(_)) => Some(unlinked::unlinked(note, cx, k)),
        Some(
            note @ Note::NoSuchHash {
                link: Some(link), ..
            },
        ) => Some(match link {
            Linked::Block => block::by_hash(note, cx, k),
            Linked::Tx => tx::by_hash(note, cx, k),
        }),
        _ => None,
    };
    let lost = gone.is_some();
    let page = gone.unwrap_or_else(|| match view.pages.top().clone() {
        Route::Block(height) => block::block(view, height, cx, k),
        Route::Tx { height, position } => tx::tx(view, height, position, cx, k),
        Route::Account(number) => account::account(view, number, cx, k),
        Route::Program(name) => program::program(view, &name, cx, k),
        Route::Transactions(Some(name)) => txs::transactions(view, &name, cx, k),
        Route::Overview
        | Route::Blocks
        | Route::Transactions(None)
        | Route::Accounts
        | Route::Programs => explorer::root(view, cx, k),
    });
    let sheet = match &view.sheet {
        Some(Sheet::Add) => Some(programs::add_sheet(view, cx, k)),
        Some(Sheet::Program(name)) => programs::program_sheet(view, name, cx, k),
        None => form::sheet(view, cx, k),
    };
    // Back undoes what is drawn: the sheet on screen, else a link's page
    // that shows nothing, else the top page; with none, the host leaves the
    // program
    let drawn = sheet.is_some();
    let back = (drawn || lost || view.pages.depth() > 0).then(|| {
        cx.listener(move |view: &mut Explorer, _: &BackEvent, _, cx| {
            if drawn {
                view.close_sheet(cx)
            } else if lost {
                view.dismiss_note(cx)
            } else {
                view.leave_page(cx)
            }
        })
    });
    div()
        .id("explorer")
        .size_full()
        .flex()
        .flex_col()
        .bg(k.theme.background)
        .text_color(k.theme.foreground)
        .text_type(k.ladder.body)
        .child(page)
        .children(sheet)
        .when_some(back, |root, back| root.on_back(back))
        .into_any_element()
}

/// Back on a page's title row: the page goes.
fn back(cx: Cx) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.leave_page(cx))
}
