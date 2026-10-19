//! Expo `src/network/explorer/programs.tsx` (mobile dev 266633c): the
//! Programs page under the strip. What the programs role will change
//! (Scheduled), runs (Running) and lists (Views), and who changes each name
//! (Access); a program's sheet, and Add. Reads the programs role, the
//! accounts, the status and the reader's account and voices (`acting`);
//! writes a Cancel (`press`), which sheet is open (`sheet`) and the form a
//! sheet opens (`open_form`).
use ::programs::{NOTICE, Pending};
use view::design::{mono, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::acting::{self, Edit, Offer, Path};
use crate::decode::{minutes, plural};
use crate::state::Sheet;
use crate::words;
use crate::{Cx, Explorer, Network, Note, Route};

use super::Look;
use super::parts::{
    Press, aside, caption, chevron, column, list_row, module_face, note, quiet, refused, scroller,
    section, small, title_and_caption,
};

const UNREAD_BLOCK_MS: u64 = 1000;

pub(super) fn programs(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let network = match &*view.network {
        Loadable::Ready(network) | Loadable::Reloading(network, _) => network,
        Loadable::Failed(refusal) => return refused(&design::sentence(refusal), cx, k),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-programs-loading", words::READING_ROLE, k);
        }
    };
    let listed = if network.is_empty() {
        vec![quiet("explorer-empty", words::RUNS_NOTHING, k)]
    } else {
        [
            scheduled(view, network, cx, k),
            running(network, cx, k),
            views(network, cx, k),
        ]
        .into_iter()
        .flatten()
        .collect()
    };
    // what a write came to, at the top of the page's own scroll (Expo
    // `Outcomes`, `programs.tsx:150`)
    let outcome = view.note.as_ref().is_some_and(Note::of_a_write);
    scroller("explorer-page")
        .children(note(view, cx, k).filter(|_| outcome))
        .children(listed)
        .children(access(view, k))
        // Expo's own foot past the page's (`programs.tsx:243`)
        .child(div().flex_none().h(space::BLOCK))
        .into_any_element()
}

pub(super) fn add(view: &Explorer, page: design::Page, cx: Cx, k: &Look) -> design::Page {
    if !view.adds() {
        return page;
    }
    let open = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| {
        view.sheet = Some(Sheet::Add);
        cx.notify();
    });
    page.trailing(design::icon_button(
        "explorer-add-menu",
        design::icon("plus", k.ladder.icon),
        words::ADD,
        &k.ladder,
        &k.theme,
        open,
    ))
}

// ---------- the words ----------

fn by_words(view: &Explorer, pending: &Pending) -> String {
    let Some(by) = pending.by else {
        return words::BY_VOTE.into();
    };
    let tip = view.status.ready().map_or(0, |status| status.height);
    format!(
        "by {} · {} to look",
        view.name_of(by),
        plural(pending.height.saturating_sub(tip), "block", "blocks")
    )
}

// ---------- the sections ----------

/// What the programs role will fold in, under its head with their count.
fn scheduled(view: &Explorer, network: &Network, cx: Cx, k: &Look) -> Vec<AnyElement> {
    let count = (!network.changes.is_empty()).then(|| aside(network.changes.len().to_string()));
    let head = section("explorer-scheduled-header", "Scheduled", count, k);
    let none = network
        .changes
        .is_empty()
        .then(|| quiet("explorer-no-changes", words::NOTHING_SCHEDULED, k));
    let rows = network.changes.iter().map(|pending| {
        scheduled_row(view, network, pending, k)
            .children(cancel(view, pending, cx, k))
            .into_any_element()
    });
    std::iter::once(head.into_any_element())
        .chain(none)
        .chain(rows)
        .collect()
}

/// One change the programs role will fold in: what it does, and whose it
/// is. The program page draws the same row.
pub(super) fn scheduled_row(
    view: &Explorer,
    network: &Network,
    pending: &Pending,
    k: &Look,
) -> Stateful<Div> {
    list_row(
        SharedString::from(format!("explorer-change-{}", change_key(pending))),
        k,
    )
    .child(design::lead(
        design::icon("clock", k.ladder.icon_row).text_color(k.theme.muted),
    ))
    .child(title_and_caption(
        div()
            .truncate()
            .child(words::scheduled_words(pending, network)),
        caption(by_words(view, pending), k),
    ))
}

fn cancel(view: &Explorer, pending: &Pending, cx: Cx, k: &Look) -> Option<AnyElement> {
    let path = view.cancels(pending)?;
    let label = path.cancel_word();
    let op = acting::cancel(pending);
    let press = (!view.acting.busy).then(|| -> Press {
        Box::new(
            cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
                view.press(path, op.clone(), cx)
            }),
        )
    });
    let id = format!("explorer-cancel-{}", change_key(pending));
    Some(small(id, label, design::Kind::Quiet, press, k))
}

/// Each running program, under its head with their count (Expo's Running).
fn running(network: &Network, cx: Cx, k: &Look) -> Vec<AnyElement> {
    let head = section(
        "explorer-running-header",
        "Running",
        Some(aside(network.programs.len().to_string())),
        k,
    );
    let rows = network.programs.iter().map(|entry| {
        let caption = format!(
            "{} · {}",
            words::code_words(&entry.code),
            words::changed_by(network.fills_a_role(&entry.program))
        );
        entry_row(network, &entry.program, caption, cx, k)
    });
    vec![
        head.into_any_element(),
        column("explorer-programs-list", rows.collect()).into_any_element(),
    ]
}

/// Each view-only entry, under its head; none, no section (Expo's Views).
fn views(network: &Network, cx: Cx, k: &Look) -> Vec<AnyElement> {
    if network.views.is_empty() {
        return Vec::new();
    }
    let head = section(
        "explorer-views-header",
        "Views",
        Some(aside(plural(network.views.len() as u64, "view", "views"))),
        k,
    );
    let rows = network.views.iter().map(|listed| {
        let caption = format!(
            "{} · {} · {}",
            words::VIEW_ONLY,
            words::code_words(&listed.view),
            words::changed_by(network.fills_a_role(&listed.name))
        );
        entry_row(network, &listed.name, caption, cx, k)
    });
    vec![
        head.into_any_element(),
        column("explorer-views-list", rows.collect()).into_any_element(),
    ]
}

/// A program's or a view's row: its square face, its name in the data
/// face, the caption, a hook badge where it is one; the press opens its
/// sheet.
fn entry_row(
    network: &Network,
    name: &str,
    caption_words: String,
    cx: Cx,
    k: &Look,
) -> Stateful<Div> {
    let open = {
        let name = name.to_owned();
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
            view.sheet = Some(Sheet::Program(name.clone()));
            cx.notify();
        })
    };
    let hook = network.is_hook(name).then(|| {
        design::badge(
            SharedString::from(format!("explorer-hook-{name}")),
            "hook",
            &k.ladder,
            k.theme.muted,
            k.theme.surface_raised,
        )
    });
    list_row(SharedString::from(format!("explorer-program-{name}")), k)
        .role(Role::Button)
        .aria_label(name.to_owned())
        .focusable()
        .on_click(open)
        .child(design::lead(module_face(name, k.ladder.avatar, k)))
        .child(title_and_caption(
            mono(name.to_owned())
                .truncate()
                .text_type(k.ladder.secondary),
            caption(caption_words, k),
        ))
        .children(hook)
        .child(chevron(k))
}

fn access(view: &Explorer, k: &Look) -> Vec<AnyElement> {
    let block_time = view
        .status
        .ready()
        .map_or(UNREAD_BLOCK_MS, |status| status.block_time_ms);
    let wait = words::account_wait(Some(minutes(NOTICE, block_time)));
    let head = section("explorer-access-header", "Access", Some(aside(wait)), k);
    vec![
        head.into_any_element(),
        quiet("explorer-access-rule", words::OPEN_TO_ACCOUNTS, k),
    ]
}

/// A scheduled change's key: its height and its program, as the chain's
/// Cancel names it.
fn change_key(pending: &Pending) -> String {
    format!("{}-{}", pending.height, pending.change.program())
}

// ---------- the sheets ----------

/// The press of a sheet row that leaves the sheet for `route`.
fn leaves_for(route: Route, cx: Cx) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx))
}

/// The press of a sheet row that leaves the sheet for a form.
fn opens_form(
    edit: Edit,
    path: Path,
    cx: Cx,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
        view.sheet = None;
        view.open_form(edit.clone(), path, cx)
    })
}

/// What closes a sheet: its scrim, its handle, Back.
fn closes(cx: Cx) -> impl Fn(&(), &mut Window, &mut App) + 'static {
    cx.listener(|view: &mut Explorer, _: &(), _, cx| {
        view.sheet = None;
        cx.notify();
    })
}

pub(super) fn add_sheet(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let add = view.adds().then(|| {
        let press = opens_form(Edit::Add, Path::Account, cx);
        design::sheet_row(
            Edit::Add.press_id(),
            "Add a program",
            &k.ladder,
            &k.theme,
            press,
        )
        .hint("by your account")
    });
    design::sheet("explorer-add-menu-sheet", words::ADD, &k.ladder, &k.theme)
        .on_dismiss(closes(cx))
        .child(
            div()
                .id("explorer-add-menu-rows")
                .flex()
                .flex_col()
                .children(add),
        )
        .into_any_element()
}

/// One program's or view-only entry's sheet (Expo `ProgramSheet`): its
/// face, name and kind; About, its transactions, and the reader's Upgrade
/// and Remove.
pub(super) fn program_sheet(view: &Explorer, name: &str, cx: Cx, k: &Look) -> Option<AnyElement> {
    let network = view.network.ready()?;
    let listed = view.runs(name).is_none();
    let hook = network.is_hook(name);
    let about = design::sheet_row(
        "explorer-program-about",
        "About",
        &k.ladder,
        &k.theme,
        leaves_for(Route::Program(name.to_owned()), cx),
    )
    .hint("capabilities, code, scheduled");
    let transactions = (!listed).then(|| {
        let press = leaves_for(Route::Transactions(Some(name.to_owned())), cx);
        design::sheet_row(
            "explorer-program-txs",
            "Transactions",
            &k.ladder,
            &k.theme,
            press,
        )
        .hint(format!("to {name}"))
    });
    let edits: Vec<AnyElement> = view
        .edits(network, name, listed)
        .into_iter()
        .map(|offer| edit_row(offer, cx, k))
        .collect();
    Some(
        design::sheet(
            "explorer-program-sheet",
            name.to_owned(),
            &k.ladder,
            &k.theme,
        )
        .on_dismiss(closes(cx))
        .child(
            div()
                .id("explorer-program-rows")
                .flex()
                .flex_col()
                .child(sheet_head(name, listed, hook, k))
                .child(about)
                .children(transactions)
                .children(edits),
        )
        .into_any_element(),
    )
}

// SDK gap: the sheet head with a face, the SDK builds it later
/// What a program's sheet is about: its square face, its name in the data
/// face, program or view only, and hook.
fn sheet_head(name: &str, listed: bool, hook: bool, k: &Look) -> Stateful<Div> {
    let kind = words::entry_kind(listed);
    let kind = if hook {
        format!("{kind} · hook")
    } else {
        kind.to_owned()
    };
    design::sheet_head("explorer-program-head", &k.theme).child(
        div()
            .flex()
            .items_center()
            .gap(space::LG)
            .child(module_face(name, k.ladder.avatar_sheet, k))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .child(
                        mono(name.to_owned())
                            .text_type(k.ladder.body)
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(caption(kind, k)),
            ),
    )
}

/// Upgrade or Remove on the reader's path, opening its form; Remove in the
/// danger tone.
fn edit_row(offer: Offer, cx: Cx, k: &Look) -> AnyElement {
    let id = SharedString::from(offer.edit.press_id());
    let danger = matches!(offer.edit, Edit::Remove { .. });
    let press = opens_form(offer.edit, offer.path, cx);
    design::sheet_row(id, offer.word, &k.ladder, &k.theme, press)
        .when(danger, |row| row.danger())
        .into_any_element()
}
