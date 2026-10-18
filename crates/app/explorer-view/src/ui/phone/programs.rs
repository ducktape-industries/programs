//! Expo `src/network/explorer/programs.tsx` (mobile dev 266633c): the
//! Programs page under the strip. What the programs role will change
//! (Scheduled), runs (Running) and lists (Views), and who changes each name
//! (Access); a program's sheet, and Add. Reads the programs role, the
//! accounts, the status and the reader's grants and voices (`acting`);
//! writes a Cancel, a Step down or a revoke (`press`), which sheet is open
//! (`sheet`) and the form a sheet opens (`open_form`).
use ::programs::{Grant, NOTICE, Pending, Scope};
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

/// The block time Expo words a maintainer's wait in before the node's
/// status is read (`programs.tsx:94`).
const UNREAD_BLOCK_MS: u64 = 1000;

pub(super) fn programs(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let network = match &view.network {
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
            running(view, network, cx, k),
            views(view, network, cx, k),
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
        .children(access(view, network, cx, k))
        // Expo's own foot past the page's (`programs.tsx:243`)
        .child(div().flex_none().h(space::BLOCK))
        .into_any_element()
}

/// Add on the root's title row while this page shows (Expo's
/// `ProgramsMenuHit`, there an `Ellipsis` named More): there while the
/// reader may add a program by grant or propose a grant by vote. Both add,
/// so it draws `plus` and is named Add, not the capsule's `ellipsis` and
/// More beside it ([`design::Page::trailing`]).
pub(super) fn add(view: &Explorer, page: design::Page, cx: Cx, k: &Look) -> design::Page {
    if !(view.adds() || view.acting.votes()) {
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

/// An account by its name, the reader's marked "(you)" (Expo
/// `holderName`).
fn holder_name(view: &Explorer, number: u64) -> String {
    words::holder(
        &view.name_of(number),
        view.acting.account == Some(number),
        false,
    )
}

/// Who changes `name` (Expo `changedBy`): the validators' vote for a
/// role's program, else its maintainers and the every-program ones.
fn changed_by(view: &Explorer, network: &Network, name: &str) -> String {
    let Some(maintainers) = network.maintainers(name) else {
        return words::changed_by(&[]);
    };
    let mut named: Vec<String> = maintainers
        .named
        .iter()
        .map(|&number| holder_name(view, number))
        .collect();
    if maintainers.every {
        named.push(words::EVERY_PROGRAM.into());
    }
    words::changed_by(&named)
}

/// Whose a scheduled change is (Expo `byWords`): the vote's, or a
/// maintainer's grant and the blocks the validators have left to look.
fn by_words(view: &Explorer, pending: &Pending) -> String {
    let Some(by) = pending.by else {
        return words::BY_VOTE.into();
    };
    let tip = view.status.ready().map_or(0, |status| status.height);
    format!(
        "by {}'s grant · {} to look",
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

/// Cancel by the grant that made it, or Propose cancel by vote; nothing
/// where the reader has neither. Off while a write is out.
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
fn running(view: &Explorer, network: &Network, cx: Cx, k: &Look) -> Vec<AnyElement> {
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
            changed_by(view, network, &entry.program)
        );
        entry_row(network, &entry.program, caption, cx, k)
    });
    vec![
        head.into_any_element(),
        column("explorer-programs-list", rows.collect()).into_any_element(),
    ]
}

/// Each view-only entry, under its head; none, no section (Expo's Views).
fn views(view: &Explorer, network: &Network, cx: Cx, k: &Look) -> Vec<AnyElement> {
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
            changed_by(view, network, &listed.name)
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

/// Every grant (Expo's Access), under how long a maintainer's change waits.
fn access(view: &Explorer, network: &Network, cx: Cx, k: &Look) -> Vec<AnyElement> {
    let block_time = view
        .status
        .ready()
        .map_or(UNREAD_BLOCK_MS, |status| status.block_time_ms);
    let wait = words::maintainer_wait(Some(minutes(NOTICE, block_time)));
    let head = section("explorer-access-header", "Access", Some(aside(wait)), k);
    let none = network
        .grants
        .is_empty()
        .then(|| quiet("explorer-no-grants", words::NO_MAINTAINER, k));
    let rows = network
        .grants
        .iter()
        .map(|grant| grant_row(view, network, grant, cx, k));
    std::iter::once(head.into_any_element())
        .chain(none)
        .chain(rows)
        .collect()
}

/// One grant: what it covers, who holds it (an agent marked so, a program
/// not run yet said so); the holder steps down, a reader with voices
/// proposes the revoke.
fn grant_row(view: &Explorer, network: &Network, grant: &Grant, cx: Cx, k: &Look) -> AnyElement {
    let holder = words::holder(
        &view.name_of(grant.account),
        view.acting.account == Some(grant.account),
        view.is_agent(grant.account),
    );
    let mut about = words::changed_by(&[holder]);
    if matches!(&grant.scope, Scope::Program(id) if !network.names(id)) {
        about = format!("{about} · {}", words::may_add(&view.name_of(grant.account)));
    }
    list_row(
        SharedString::from(format!("explorer-grant-{}", grant_key(grant))),
        k,
    )
    .child(title_and_caption(
        mono(grant.scope.to_string())
            .truncate()
            .text_type(k.ladder.secondary),
        caption(about, k),
    ))
    .children(revoke(view, grant, cx, k))
    .into_any_element()
}

/// Step down from one's own grant, or Propose revoke by vote; nothing where
/// the reader has neither. Off while a write is out.
fn revoke(view: &Explorer, grant: &Grant, cx: Cx, k: &Look) -> Option<AnyElement> {
    let path = view.revokes(grant)?;
    let label = path.revoke_word();
    let op = ::programs::Op::Revoke(grant.clone());
    let press = (!view.acting.busy).then(|| -> Press {
        Box::new(
            cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
                view.press(path, op.clone(), cx)
            }),
        )
    });
    let id = format!("explorer-revoke-{}", grant_key(grant));
    Some(small(id, label, design::Kind::Quiet, press, k))
}

/// A scheduled change's key: its height and its program, as the chain's
/// Cancel names it.
fn change_key(pending: &Pending) -> String {
    format!("{}-{}", pending.height, pending.change.program())
}

/// A grant's key: its holder and what it covers, `all` for every program.
fn grant_key(grant: &Grant) -> String {
    let scope = match &grant.scope {
        Scope::Program(program) => program.as_str(),
        Scope::All => "all",
    };
    format!("{}-{scope}", grant.account)
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

/// Add (Expo's programs `Sheet` of `SheetRow`s): Add a program by grant,
/// Propose a grant by vote.
pub(super) fn add_sheet(view: &Explorer, cx: Cx, k: &Look) -> AnyElement {
    let add = view.adds().then(|| {
        let press = opens_form(Edit::Add, Path::Grant, cx);
        design::sheet_row(
            Edit::Add.press_id(),
            "Add a program",
            &k.ladder,
            &k.theme,
            press,
        )
        .hint("by your grant")
    });
    let grant = view.acting.votes().then(|| {
        let press = opens_form(Edit::Grant, Path::Vote, cx);
        design::sheet_row(
            Edit::Grant.press_id(),
            "Propose a grant",
            &k.ladder,
            &k.theme,
            press,
        )
        .hint(words::BY_VOTE)
    });
    design::sheet("explorer-add-menu-sheet", words::ADD, &k.ladder, &k.theme)
        .on_dismiss(closes(cx))
        .child(
            div()
                .id("explorer-add-menu-rows")
                .flex()
                .flex_col()
                .children(add)
                .children(grant),
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
