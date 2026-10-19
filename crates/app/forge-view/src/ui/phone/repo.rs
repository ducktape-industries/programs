//! A repository (Expo `app/(tabs)/forge/[repo]/index.tsx`, board 61): its
//! name over its owner, refs and activity, Copy and About in the title
//! row, the ref button beside the address, the six desktop tabs in one
//! sideways strip, then the open tab's body ([`super::tabs`]); and the
//! About sheet (`repo.tsx` `AboutSheet`).
//!
//! Reads `Query::Repo`, `Query::Refs` and the open changes' count; writes
//! the clipboard (Copy) and the browsed ref (through the ref sheet).
use view::design::{Ladder, Nav};
use view::prelude::*;
use view::{Div, Stateful};

use super::parts;
use super::tabs;
use crate::Forge;
use crate::id;
use crate::select::Writer;
use crate::state::{Menu, RepoTab, Sheet};
use crate::words::{self, ref_label};

pub(super) fn render(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let name = forge.repo_name();
    let mut page = design::page("forge-page-repo", name.clone(), nav, ladder, theme)
        .trailing(copy_button(forge, &name, cx, ladder, theme))
        .trailing(about_button(cx, ladder, theme));
    if let Some((info, _)) = forge.repo() {
        page = page.caption(words::repo_facts(
            &forge.principal_name(&info.repo.owner),
            info.repo.refs_count,
            info.repo.last_activity,
        ));
    }
    let archived = forge.archived().then(|| archived_row(ladder, theme));
    page.children(archived)
        .children(parts::notices(forge, ladder, theme))
        .child(ref_row(forge, &name, cx, ladder, theme))
        .child(strip(forge, cx, ladder, theme))
        .child(tabs::render(forge, cx, ladder, theme))
        .into_any_element()
}

// The badge an archived repository wears under its title row.
fn archived_row(ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id("forge-repo-archived-row"))
        .flex()
        .flex_none()
        .px(design::space::BLOCK)
        .pt(design::space::LG)
        .child(parts::badge(
            id("forge-repo-archived"),
            words::ARCHIVED,
            parts::Tint::Warn,
            ladder,
            theme,
        ))
}

/// Copy the address: its check says so once it is on the clipboard.
fn copy_button(
    forge: &Forge,
    name: &str,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let url = forge.repo_link(name);
    let said = match forge.copied.as_deref() == Some(url.as_str()) {
        true => "Address copied",
        false => "Copy the address",
    };
    parts::copy_button(
        "forge-repo-copy",
        ("copy", said),
        Some(url),
        forge,
        cx,
        (ladder, theme),
    )
}

fn about_button(cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> impl IntoElement {
    let about = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_sheet(Sheet::About, cx));
    design::icon_button(
        id("forge-dock-about"),
        design::icon("info", ladder.icon),
        "About",
        ladder,
        theme,
        about,
    )
}

/// The ref button (small, `git-branch`, `forge-ref-picker`) and the
/// address, one line, its middle ellipsized; 16 in, 12 over, 6 under.
fn ref_row(
    forge: &Forge,
    name: &str,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let label = match forge.refs() {
        Some([]) => words::NO_REFS_YET.to_owned(),
        _ => ref_label(&forge.head_name()),
    };
    let open = cx.listener(|forge, event: &ClickEvent, window, cx| {
        forge.open_dropdown(Menu::Ref, event, window, cx)
    });
    let address = parts::caption(forge.repo_link(name), ladder, theme)
        .font_family(design::fonts::FAMILY_MONO)
        .flex_1()
        .min_w(px(0.))
        .text_ellipsis_middle();
    div()
        .id(id("forge-repo-picker-row"))
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::LG)
        .px(design::space::BLOCK)
        .pt(design::space::LG)
        .pb(design::space::XS)
        .child(parts::small_button(
            id(Menu::Ref.key()),
            label,
            "git-branch",
            ladder,
            theme,
            open,
        ))
        .child(address)
}

/// The six tabs (`TABS`), one Tab stop whose arrows walk them; Changes
/// carries how many are open.
fn strip(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let tabs = RepoTab::ALL;
    let active = forge.tab_cursor.unwrap_or(forge.nav().tab);
    let at = |tab: RepoTab| tabs.iter().position(|it| *it == tab).unwrap_or(0);
    let bar = design::composite(id("forge-tab-list"), Role::TabList, "Repository")
        .orientation(design::Orientation::Horizontal)
        .wrap()
        .active(at(active), tabs.len())
        .on_move(cx.processor(move |forge, index: usize, _, cx| {
            forge.tab_cursor = tabs.get(index).copied();
            cx.notify();
        }))
        .on_press(cx.processor(move |forge, index: usize, _, cx| {
            if let Some(tab) = tabs.get(index) {
                forge.open_tab(*tab, cx);
            }
        }))
        .build();
    let mut bar = design::strip(bar, ladder, theme);
    for tab in tabs {
        let open = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_tab(tab, cx));
        let count = (tab == RepoTab::Changes)
            .then(|| forge.open_changes())
            .flatten();
        bar = bar.child(parts::strip_tab(
            id(format!("forge-tab-{}", tab.slug())),
            tab.label(),
            count,
            (forge.nav().tab == tab, active == tab),
            ladder,
            theme,
            open,
        ));
    }
    bar
}

// ------------------------------------------------------------------ About

/// The About sheet: the default head, the hash, the owner, the bounds,
/// who writes.
pub(super) fn about_sheet(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let close = cx.listener(|forge, _: &(), _, cx| forge.close_sheet(cx));
    let body = div()
        .id(id("forge-about-sheet-body"))
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .pb(design::space::BLOCK)
        .child(design::sheet_title(
            id("forge-about-head"),
            "About",
            ladder,
            theme,
        ))
        .child(about(forge, ladder, theme));
    design::sheet(id("forge-about-sheet"), "About", ladder, theme)
        .on_dismiss(close)
        .child(body)
        .into_any_element()
}

fn about(forge: &Forge, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let Some((info, bounds)) = forge.repo() else {
        return parts::quiet(
            id("forge-about-loading"),
            words::READING_REPOSITORY,
            ladder,
            theme,
        )
        .into_any_element();
    };
    let writers = forge.writers().map(Writer::rows);
    let access = match writers.as_deref() {
        None => words::READING_WRITERS.to_owned(),
        Some([]) => words::ONLY_THE_OWNER.to_owned(),
        Some(writers) => writers
            .iter()
            .map(|writer| forge.writer_name(writer))
            .collect::<Vec<_>>()
            .join(", "),
    };
    let hash = words::hash_kind(info.repo.hash);
    let fact = |label: &str, value: Div| parts::key_value(label, value, ladder, theme);
    div()
        .id(id("forge-about"))
        .flex()
        .flex_col()
        .child(fact(
            words::DEFAULT_HEAD,
            parts::data(ref_label(&info.repo.settings.head)),
        ))
        .child(fact("Hash", parts::data(hash)))
        .child(fact(
            "Owner",
            div().child(forge.principal_name(&info.repo.owner)),
        ))
        .child(fact(
            words::PAGE_SIZE,
            parts::data(bounds.page_size.to_string()),
        ))
        .child(fact(
            words::INLINE_BLOB_BOUND,
            parts::data(words::bytes(bounds.blob_bytes)),
        ))
        .child(fact("Access", div().child(access)))
        .into_any_element()
}
