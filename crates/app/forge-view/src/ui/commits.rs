//! Commits: a virtualized log of the picked ref, read a page at a time as
//! it is scrolled, and one commit's own diff against its first parent.
use std::rc::Rc;
use view::prelude::*;
use view::{Div, Paged, Stateful};

use crate::Forge;
use crate::queries::PAGE;
use crate::select::summary_of;
use crate::ui::components::{badge, button, empty_state, heading, id, quiet, short_hex};
use crate::ui::{diff, fact, refused};
use crate::words;
use forge::{CommitInfo, Query};
use view::design::Ladder;

pub(crate) fn query(
    forge: &Forge,
    from: forge::Revision,
    exclude: Option<forge::Revision>,
) -> Query {
    Query::Log {
        repo: forge.repo_name(),
        from,
        exclude,
        page: PAGE,
    }
}

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    if let Some(oid) = forge.nav().commit.clone() {
        return detail(forge, &oid, cx, theme, ladder);
    }
    if forge.tree_query(Vec::new()).is_none() {
        return match forge.refs() {
            Some(_) => crate::ui::code::no_commits(forge, theme, ladder),
            None => quiet(words::RESOLVING_REF, theme),
        };
    }
    log(forge, &forge.log_query(), "forge-log", cx, theme, ladder)
}

fn landed<'a>(
    forge: &'a Forge,
    query: &Query,
    element_id: &str,
    reading: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Result<&'a Entity<Paged<CommitInfo>>, AnyElement> {
    let log = forge.logs.get(query).map(|(log, _)| log);
    let refusal = log.and_then(|log| log.read(|log| log.failed().cloned()));
    match (log, refusal) {
        (_, Some(refusal)) => Err(refused(query, element_id, &refusal, cx, theme, ladder)),
        (Some(log), None) if !log.read(Paged::is_loading) => Ok(log),
        _ => Err(crate::ui::reading(element_id, reading, theme)),
    }
}

pub(crate) fn log(
    forge: &Forge,
    query: &Query,
    element_id: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let ladder = *ladder;
    let log = match landed(forge, query, element_id, READING, cx, theme, &ladder) {
        Ok(log) => log.clone(),
        Err(state) => return state,
    };
    let (held, count) = log.read(|log| (log.rows().len(), log.count()));
    if held == 0 {
        return empty_state(
            id(format!("{element_id}-empty")),
            "git-commit-horizontal",
            words::NO_COMMITS,
            words::NO_HISTORY,
            &ladder,
            theme,
        )
        .into_any_element();
    }
    let theme = *theme;
    let open: crate::ui::diff::Route<String> =
        Rc::new(cx.listener(|forge, oid: &String, _, cx| forge.open_commit(Some(oid.clone()), cx)));
    let handle = forge.log_scroll.clone();
    let list_id: &'static str = match element_id {
        "forge-change-log" => "forge-change-log-list",
        _ => "forge-log-list",
    };
    let (at, _) = crate::ui::components::cursor(forge, list_id);
    let at = at.min(held - 1);
    let walked = crate::ui::components::walked(forge, list_id);
    let pressed = log.clone();
    let list = crate::ui::components::list(
        list_id,
        "Commits",
        held,
        Some(&handle),
        forge,
        cx,
        move |forge, index, _, cx| {
            let oid = pressed.read(|log| log.rows().get(index).map(|commit| commit.oid.clone()));
            forge.open_commit(oid, cx)
        },
    );
    let more = format!("{element_id}-more");
    let rows = crate::ui::components::window(
        element_id,
        count,
        None,
        Some(&handle),
        move |range, _, app| {
            log.update(app, |log, cx| log.show(range.clone(), cx));
            log.read(|log| {
                range
                    .map(|index| match log.rows().get(index) {
                        Some(commit) => row(commit, index == at, walked, &open, &theme, &ladder),
                        None => reading_on(&more, &theme),
                    })
                    .collect()
            })
        },
    );
    list.child(rows).into_any_element()
}

const READING: &str = words::READING_HISTORY;

fn reading_on(key: &str, theme: &Theme) -> AnyElement {
    div()
        .id(id(key.to_owned()))
        .w_full()
        .flex()
        .items_center()
        .min_h(design::size::CONTROL)
        .px_2()
        .child(quiet(READING, theme))
        .into_any_element()
}

fn row(
    commit: &CommitInfo,
    active: bool,
    walked: bool,
    open: &crate::ui::diff::Route<String>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let oid = commit.oid.clone();
    let open = open.clone();
    let clicked = oid.clone();
    let mut row = crate::ui::components::row(format!("forge-commit-{oid}"), theme)
        .active(active.then_some(0))
        .walked(walked)
        .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
            open(&clicked, window, app)
        })
        .cell(
            div()
                .w(OID_W)
                .whitespace_nowrap()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(short_hex(&oid)),
        )
        .cell(div().flex_1().truncate().child(summary_of(commit)))
        .cell(quiet(
            String::from_utf8_lossy(&commit.author.name).into_owned(),
            theme,
        ))
        .cell(quiet(authored(commit), theme));
    if commit.parents.len() > 1 {
        row = row.cell(badge(
            id(format!("forge-commit-merge-{oid}")),
            format!("{} parents", commit.parents.len()),
            ladder,
            theme.accent_foreground,
            theme.accent_soft,
        ));
    }
    row.into_any_element()
}

fn facts(commit: &CommitInfo, theme: &Theme) -> Stateful<Div> {
    let parents = if commit.parents.is_empty() {
        words::ROOT_COMMIT.to_owned()
    } else {
        commit
            .parents
            .iter()
            .map(|parent| short_hex(parent))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let author = format!(
        "{} <{}> · {}",
        String::from_utf8_lossy(&commit.author.name),
        String::from_utf8_lossy(&commit.author.email),
        authored(commit)
    );
    div()
        .id(id("forge-commit-facts"))
        .flex()
        .flex_col()
        .gap_1()
        .px_3()
        .py_2()
        .child(fact("Commit", commit.oid.clone(), theme))
        .child(fact("Tree", commit.tree.clone(), theme))
        .child(fact("Parents", parents, theme))
        .child(fact("Author", author, theme))
        .child(quiet(
            String::from_utf8_lossy(&commit.message).into_owned(),
            theme,
        ))
}

fn authored(commit: &CommitInfo) -> String {
    design::date(
        u64::try_from(commit.author.time)
            .unwrap_or(0)
            .saturating_mul(1000),
    )
}

const OID_W: Pixels = px(100.);

pub(crate) fn detail(
    forge: &Forge,
    oid: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_commit(None, cx));
    let all = cx.listener(|forge, _: &ClickEvent, _, cx| forge.single_file(None, cx));
    let commit = forge.commit(oid);
    let mut column = div()
        .id(id("forge-commit-detail"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(
            div()
                .id(id("forge-commit-header"))
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .child(heading(
                    id("forge-commit-title"),
                    commit.as_ref().map_or_else(|| short_hex(oid), summary_of),
                    2,
                    theme,
                ))
                .child(div().flex_1())
                .when(forge.nav().diff_path.is_some(), |header| {
                    header.child(
                        button(
                            id("forge-commit-files-all"),
                            words::ALL_FILES,
                            ladder,
                            theme,
                            all,
                        )
                        .kind(design::Kind::Quiet),
                    )
                })
                .child(button(
                    id("forge-commit-close"),
                    "Back",
                    ladder,
                    theme,
                    close,
                )),
        );
    if let Some(commit) = &commit {
        column = column.child(facts(commit, theme));
    }
    let diff = match forge.commit_diff_query(oid) {
        Some(query) => diff::render(forge, &query, "forge-commit-diff", false, cx, theme, ladder),
        None => {
            let own = forge.commit_query(oid);
            landed(
                forge,
                &own,
                "forge-commit-diff",
                words::READING_DIFF,
                cx,
                theme,
                ladder,
            )
            .err()
            .unwrap_or_else(|| div().into_any_element())
        }
    };
    column.child(diff).into_any_element()
}
