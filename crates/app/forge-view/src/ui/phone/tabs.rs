//! A repository's tab bodies on the phone (Expo `src/forge/repo.tsx`):
//! README, Code, Commits, Changes, Refs and Settings, each a list in the
//! kit's rows under the strip [`super::repo`] draws.
//!
//! Reads the browsed ref's tree, README, history, the changes under the
//! picked filter, the refs and their comparisons, and the repository's
//! settings and writers; writes the settings (`configure`), revokes and
//! grants, and opens a change from a ref or from the Changes tab
//! (`start_change`).
use std::rc::Rc;

use view::design::{Kind, Ladder, TextType};
use view::prelude::*;
use view::{Div, Paged, Stateful};

use super::parts::{self, Chip, Row, Tone};
use crate::id;
use crate::list;
use crate::select::Writer;
use crate::state::{Filter, Menu, RepoTab, Sheet};
use crate::tree::{self, Slot};
use crate::words::{self, ref_label};
use crate::{Forge, Stage};
use forge::{BlobView, ChangeSummary, CommitInfo, Content, Judgment, Query, Reply};
use markdown::phone as document;

pub(super) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    match forge.nav().tab {
        RepoTab::Readme => readme(forge, cx, ladder, theme),
        RepoTab::Code => code(forge, cx, ladder, theme),
        RepoTab::Commits => commits(forge, cx, ladder, theme),
        RepoTab::Changes => changes(forge, cx, ladder, theme),
        RepoTab::Refs => refs(forge, cx, ladder, theme),
        RepoTab::Settings => settings(forge, cx, ladder, theme),
    }
}

fn body(key: &str) -> Stateful<Div> {
    div()
        .id(id(key.to_owned()))
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
}

fn no_commits(forge: &Forge, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let address = forge.repo_link(&forge.repo_name());
    design::empty_state(
        id("forge-tree-no-commits"),
        "git-commit-horizontal",
        words::NO_COMMITS_YET,
        format!("Push code to this ref to browse it here: git push {address} main"),
        ladder,
        theme,
    )
    .into_any_element()
}

fn unresolved(forge: &Forge, ladder: &Ladder, theme: &Theme) -> Option<AnyElement> {
    if forge.head_oid().is_some() {
        return None;
    }
    Some(match forge.refs() {
        Some(_) => no_commits(forge, ladder, theme),
        None => parts::quiet(
            id("forge-ref-resolving"),
            words::RESOLVING_REF,
            ladder,
            theme,
        )
        .into_any_element(),
    })
}

fn readme(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let page = body("forge-readme").overflow_y_scroll();
    if let Some(state) = unresolved(forge, ladder, theme) {
        return page.child(state).into_any_element();
    }
    let Some(root) = forge.tree_query(Vec::new()) else {
        return page.into_any_element();
    };
    let tree = parts::stage(
        forge,
        &root,
        "forge-readme-tree",
        words::READING_TREE,
        cx,
        (ladder, theme),
    );
    if let Err(state) = tree {
        return page.child(state).into_any_element();
    }
    let Some((name, oid)) = forge.readme() else {
        let ref_ = ref_label(&forge.head_name());
        return page
            .child(design::empty_state(
                id("forge-readme-none"),
                "file-text",
                words::NO_README,
                format!("Add a README.md at the root of {ref_}."),
                ladder,
                theme,
            ))
            .into_any_element();
    };
    page.children(readme_document(forge, &name, &oid, cx, ladder, theme))
        .into_any_element()
}

fn readme_document(
    forge: &Forge,
    name: &[u8],
    oid: &str,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> [AnyElement; 2] {
    let ref_ = ref_label(&forge.head_name());
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid: oid.to_owned(),
        range: None,
    };
    let blob = match parts::stage(
        forge,
        &query,
        "forge-readme-blob",
        words::READING_README,
        cx,
        (ladder, theme),
    ) {
        Ok(Reply::Blob { blob, .. }) => Ok(blob),
        Ok(_) => Err(div().into_any_element()),
        Err(state) => Err(state),
    };
    let right = match &blob {
        Ok(blob) => format!("{ref_} · {}", parts::size_of(blob.size)),
        Err(_) => ref_,
    };
    let bar = parts::file_bar(
        "forge-readme-title",
        crate::words::path_text(name),
        right,
        ladder,
        theme,
    );
    let content = match blob {
        Ok(blob) => readme_body(forge, oid, blob, cx, ladder, theme),
        Err(state) => state,
    };
    [
        bar.into_any_element(),
        div()
            .p(design::space::BLOCK)
            .child(content)
            .into_any_element(),
    ]
}

fn readme_body(
    forge: &Forge,
    oid: &str,
    blob: &BlobView,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    if blob.content != Content::Text {
        return parts::quiet(
            id("forge-readme-binary"),
            words::README_NOT_TEXT,
            ladder,
            theme,
        )
        .p_0()
        .into_any_element();
    }
    let blocks = forge.blob_cache.doc(oid, &blob.bytes);
    let links = crate::markdown::links(Vec::new(), cx);
    document::markdown("forge-readme-body", &blocks, &links, ladder, theme)
}

fn code(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let bar = parts::file_bar(
        "forge-code-bar",
        forge.repo_name(),
        ref_label(&forge.head_name()),
        ladder,
        theme,
    );
    folder_body(forge, Vec::new(), bar, cx, ladder, theme)
}

pub(super) fn folder_body(
    forge: &Forge,
    dir: Vec<u8>,
    bar: Stateful<Div>,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let filter = Input::new(
        id("forge-tree-search"),
        &forge.tree_search,
        words::FILTER_FILES,
    )
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None)
    .virtual_keyboard(VirtualKeyboard::Search)
    .placeholder(words::FILTER_FILES);
    let page = body("forge-code").child(bar).child(
        div()
            .p(design::space::BLOCK)
            .pb(design::space::SM)
            .child(design::field(filter, ladder, theme).lead("search")),
    );
    if let Some(state) = unresolved(forge, ladder, theme) {
        return page.child(state).into_any_element();
    }
    let Some(tree) = forge.tree_query(dir.clone()) else {
        return page.into_any_element();
    };
    if let Err(state) = parts::stage(
        forge,
        &tree,
        "forge-tree",
        words::READING_TREE,
        cx,
        (ladder, theme),
    ) {
        return page.child(state).into_any_element();
    }
    let rows = forge.tree_rows_at(dir);
    if rows.is_empty() {
        return page
            .child(design::empty_state(
                id("forge-tree-empty"),
                "folder-open",
                words::NOTHING_HERE,
                words::NO_FILE_KEPT,
                ladder,
                theme,
            ))
            .into_any_element();
    }
    page.child(tree_list(forge, rows, cx, ladder, theme))
        .into_any_element()
}

fn press_entry(forge: &mut Forge, row: &tree::Row, cx: &mut Context<Forge>) {
    match &row.slot {
        Slot::Entry { .. } if row.is_dir() => forge.open_folder(Some(row.path.clone()), cx),
        Slot::Entry { oid, .. } => forge.open_file(row.path.clone(), oid.clone(), cx),
        Slot::Loading | Slot::Failed(_) => {}
    }
}

fn tree_list(
    forge: &Forge,
    rows: Vec<tree::Row>,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let rows = Rc::new(rows);
    let count = rows.len();
    let pressed = rows.clone();
    let press: parts::Route<usize> = Rc::new(
        cx.processor(move |forge, index: usize, _, cx| press_entry(forge, &pressed[index], cx)),
    );
    let opened = rows.clone();
    let (ladder, theme) = (*ladder, *theme);
    let paint = move |index: usize, active: bool| {
        let press = press.clone();
        tree_row(&rows[index], index, active, press, &ladder, &theme)
    };
    parts::virtual_list(
        ("forge-tree-list", "Files"),
        count,
        &forge.tree_scroll,
        move |forge, index, _, cx| press_entry(forge, &opened[index], cx),
        paint,
        forge,
        cx,
    )
}

fn tree_row(
    row: &tree::Row,
    index: usize,
    active: bool,
    press: parts::Route<usize>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let key = id(format!("forge-tree-{}", crate::words::path_text(&row.path)));
    let line = match &row.slot {
        Slot::Loading => {
            return parts::quiet(key, words::READING, ladder, theme).into_any_element();
        }
        Slot::Failed(sentence) => {
            return parts::notice(key, sentence.clone(), Tone::Danger, ladder, theme)
                .into_any_element();
        }
        Slot::Entry { .. } if row.is_dir() => Row::titled(key, format!("{}/", row.name))
            .lead(parts::lead_icon("folder", ladder, theme))
            .chevron(),
        Slot::Entry { .. } => {
            Row::data(key, row.name.clone()).lead(parts::lead_icon("file-text", ladder, theme))
        }
    };
    let inset = design::space::BLOCK + design::space::LG * row.depth as f32;
    let line = line.press(ladder, theme).pl(inset).on_click(
        move |_: &ClickEvent, window: &mut Window, app: &mut App| press(index, window, app),
    );
    parts::option(line, active).into_any_element()
}

fn commits(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    if let Some(state) = unresolved(forge, ladder, theme) {
        return body("forge-commits").child(state).into_any_element();
    }
    let ids = ("forge-log", "forge-log-list");
    log(forge, &forge.log_query(), ids, cx, ladder, theme)
}

pub(super) fn log(
    forge: &Forge,
    query: &Query,
    (element_id, list_id): (&str, &'static str),
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let log = match landed(forge, query, element_id, cx, ladder, theme) {
        Ok(log) => log.clone(),
        Err(state) => return state,
    };
    let (held, count) = log.read(|log| (log.rows().len(), log.count()));
    if held == 0 {
        return design::empty_state(
            id(format!("{element_id}-empty")),
            "git-commit-horizontal",
            words::NO_COMMITS,
            words::NO_HISTORY,
            ladder,
            theme,
        )
        .into_any_element();
    }
    let pressed = log.clone();
    let list = list::list(
        list_id,
        "Commits",
        held,
        Some(&forge.log_scroll),
        forge,
        cx,
        move |forge, index, _, cx| {
            let oid = pressed.read(|log| log.rows().get(index).map(|commit| commit.oid.clone()));
            forge.open_commit(oid, cx)
        },
    );
    let paint = log_rows(log, list_id, forge, cx, ladder, theme);
    list.child(list::window(
        element_id,
        count,
        None,
        Some(&forge.log_scroll),
        paint,
    ))
    .into_any_element()
}

fn landed<'a>(
    forge: &'a Forge,
    query: &Query,
    element_id: &str,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Result<&'a Entity<Paged<CommitInfo>>, AnyElement> {
    let log = forge.logs.get(query).map(|(log, _)| log);
    let refusal = log.and_then(|log| log.read(|log| log.failed().cloned()));
    match (log, refusal) {
        (_, Some(refusal)) => {
            let query = query.clone();
            let retry =
                cx.listener(move |forge, _: &ClickEvent, _, cx| forge.retry(query.clone(), cx));
            Err(
                design::refused(element_id, design::sentence(&refusal), ladder, theme, retry)
                    .into_any_element(),
            )
        }
        (Some(log), None) if !log.read(Paged::is_loading) => Ok(log),
        _ => Err(parts::quiet(
            id(format!("{element_id}-loading")),
            words::READING_HISTORY,
            ladder,
            theme,
        )
        .into_any_element()),
    }
}

fn log_rows(
    log: Entity<Paged<CommitInfo>>,
    list_id: &'static str,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl Fn(std::ops::Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static {
    let (at, _) = list::cursor(forge, list_id);
    let walked = list::walked(forge, list_id);
    let open: parts::Route<String> =
        Rc::new(cx.processor(|forge, oid: String, _, cx| forge.open_commit(Some(oid), cx)));
    let (ladder, theme) = (*ladder, *theme);
    let more = format!("{list_id}-more");
    move |range, _, app| {
        log.update(app, |log, cx| log.show(range.clone(), cx));
        log.read(|log| {
            range
                .map(|index| match log.rows().get(index) {
                    Some(commit) => {
                        let open = open.clone();
                        let oid = commit.oid.clone();
                        let row = parts::commit_row(commit, &ladder, &theme)
                            .press(&ladder, &theme)
                            .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
                                open(oid.clone(), window, app)
                            });
                        parts::option(row, walked && index == at).into_any_element()
                    }
                    None => parts::quiet(id(more.clone()), words::READING_HISTORY, &ladder, &theme)
                        .min_h(ladder.row_tall)
                        .into_any_element(),
                })
                .collect()
        })
    }
}

const FILTERS: [Filter; 6] = [
    Filter::Open,
    Filter::Merged,
    Filter::Closed,
    Filter::Judgment,
    Filter::Authored,
    Filter::Involves,
];

fn changes(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let search = Input::new(
        id("forge-changes-search"),
        &forge.change_search,
        words::SEARCH_TITLES,
    )
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None)
    .virtual_keyboard(VirtualKeyboard::Search)
    .placeholder(words::SEARCH_TITLES);
    let start =
        cx.listener(|forge, _: &ClickEvent, window, cx| forge.start_change(Vec::new(), window, cx));
    let new_change = parts::small_button(
        id("forge-new-change"),
        words::NEW_CHANGE,
        "git-pull-request",
        ladder,
        theme,
        start,
    )
    .enabled(forge.me_principal().is_some());
    let page = body("forge-changes")
        .child(filter_chips(forge, cx, ladder, theme))
        .child(
            div()
                .flex()
                .items_center()
                .gap(design::space::SM)
                .px(design::space::BLOCK)
                .pb(design::space::SM)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(design::field(search, ladder, theme).lead("search")),
                )
                .child(new_change),
        )
        .children(parts::pending(forge, "changes", cx, ladder, theme));
    let repo = forge.repo_name();
    let Some(query) = forge.changes_query(&repo) else {
        return page
            .child(design::empty_state(
                id("forge-changes-no-account"),
                "user-round",
                words::NO_ACCOUNT,
                words::CREATE_AN_ACCOUNT,
                ladder,
                theme,
            ))
            .into_any_element();
    };
    let listed = match parts::stage(
        forge,
        &query,
        "forge-changes-list",
        words::READING_CHANGES,
        cx,
        (ladder, theme),
    ) {
        Ok(reply) => listed(reply, &repo),
        Err(state) => return page.child(state).into_any_element(),
    };
    let needle = forge.change_search.text().trim().to_lowercase();
    let shown: Vec<(&ChangeSummary, Option<&Judgment>)> = listed
        .into_iter()
        .filter(|(summary, _)| needle.is_empty() || summary.title.to_lowercase().contains(&needle))
        .collect();
    if shown.is_empty() {
        return page
            .child(no_changes(forge.filter, ladder, theme))
            .into_any_element();
    }
    page.child(change_list(&shown, forge, cx, ladder, theme))
        .into_any_element()
}

fn filter_chips(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let open = forge.open_changes().map(|count| count as usize);
    let chips = FILTERS
        .iter()
        .map(|filter| Chip {
            id: id(format!("forge-filter-{}", filter.slug())),
            label: filter.label(),
            count: (*filter == Filter::Open).then_some(open).flatten(),
        })
        .collect();
    let chosen = FILTERS
        .iter()
        .position(|filter| *filter == forge.filter)
        .unwrap_or(0);
    let pick = cx.processor(|forge, index: usize, _, cx| forge.set_filter(FILTERS[index], cx));
    parts::chips("forge-filters", chips, chosen, ladder, theme, pick)
}

fn listed<'a>(reply: &'a Reply, repo: &str) -> Vec<(&'a ChangeSummary, Option<&'a Judgment>)> {
    match reply {
        Reply::Changes { page, .. } => page.items.iter().map(|summary| (summary, None)).collect(),
        Reply::Judgment { page, .. } => page
            .items
            .iter()
            .filter(|item| item.change.repo == repo)
            .map(|item| (&item.change, Some(item)))
            .collect(),
        _ => Vec::new(),
    }
}

fn no_changes(filter: Filter, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let (title, said) = match filter {
        Filter::Judgment => (words::NOTHING_WAITS, words::NOTHING_ASKED),
        _ => (words::NO_CHANGES_HERE, words::COMPARE_TO_OPEN),
    };
    design::empty_state(
        id("forge-changes-empty"),
        "git-pull-request",
        title,
        said,
        ladder,
        theme,
    )
    .into_any_element()
}

fn change_list(
    shown: &[(&ChangeSummary, Option<&Judgment>)],
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let numbers: Vec<u64> = shown.iter().map(|(summary, _)| summary.n).collect();
    let rows = shown
        .iter()
        .map(|(summary, judgment)| {
            let n = summary.n;
            let open: parts::Press = Box::new(
                cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_change(Some(n), cx)),
            );
            (
                parts::change_row(
                    summary,
                    *judgment,
                    parts::Listed::Repo,
                    forge,
                    ladder,
                    theme,
                ),
                open,
            )
        })
        .collect();
    parts::rows_list(
        "forge-change-list",
        "Changes",
        rows,
        move |forge, index, _, cx| forge.open_change(Some(numbers[index]), cx),
        forge,
        cx,
        (ladder, theme),
    )
    .flex_1()
    .min_h(px(0.))
    .overflow_y_scroll()
}

fn refs(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let all = forge.refs().unwrap_or_default();
    let count = |prefix: &[u8]| {
        all.iter()
            .filter(|info| info.name.starts_with(prefix))
            .count()
    };
    let chips = vec![
        Chip {
            id: id("forge-refs-branches"),
            label: "Branches",
            count: Some(count(b"refs/heads/")),
        },
        Chip {
            id: id("forge-refs-tags"),
            label: "Tags",
            count: Some(count(b"refs/tags/")),
        },
    ];
    let pick = cx.processor(|forge, index: usize, _, cx| {
        forge.refs_tags = index == 1;
        cx.notify();
    });
    let page = body("forge-refs")
        .child(parts::chips(
            "forge-refs-kind",
            chips,
            usize::from(forge.refs_tags),
            ladder,
            theme,
            pick,
        ))
        .children(parts::pending(forge, "changes", cx, ladder, theme))
        .children(forge.forbids().map(|said| {
            parts::quiet(id("forge-refs-forbidden"), said, ladder, theme).into_any_element()
        }));
    if let Err(state) = parts::stage(
        forge,
        &forge.refs_query(),
        "forge-refs-list",
        words::READING_REFS,
        cx,
        (ladder, theme),
    ) {
        return page.child(state).into_any_element();
    }
    if all.is_empty() {
        return page
            .child(design::empty_state(
                id("forge-refs-empty"),
                "git-branch",
                words::NO_REFS,
                words::UNBORN,
                ladder,
                theme,
            ))
            .into_any_element();
    }
    let prefix: &[u8] = if forge.refs_tags {
        b"refs/tags/"
    } else {
        b"refs/heads/"
    };
    let names: Vec<Vec<u8>> = all
        .iter()
        .filter(|info| info.name.starts_with(prefix))
        .map(|info| info.name.clone())
        .collect();
    page.child(ref_list(forge, names, cx, ladder, theme))
        .into_any_element()
}

fn ref_list(
    forge: &Forge,
    names: Vec<Vec<u8>>,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let lines: Rc<Vec<RefLine>> =
        Rc::new(names.iter().map(|name| RefLine::of(forge, name)).collect());
    let routes = RefRoutes {
        browse: Rc::new(cx.processor(|forge, name: Vec<u8>, _, cx| forge.pick_ref(name, cx))),
        compare: Rc::new(
            cx.processor(|forge, name: Vec<u8>, window, cx| forge.start_change(name, window, cx)),
        ),
        may_compare: forge.writes_repo(),
    };
    let (ladder, theme) = (*ladder, *theme);
    let painted = lines.clone();
    let paint = move |index: usize, active: bool| {
        ref_row(&painted[index], &routes, active, &ladder, &theme)
    };
    parts::virtual_list(
        ("forge-refs-list", "Refs"),
        lines.len(),
        &forge.refs_scroll,
        move |forge, index, _, cx| forge.pick_ref(lines[index].name.clone(), cx),
        paint,
        forge,
        cx,
    )
}

struct RefLine {
    name: Vec<u8>,
    label: String,
    facts: String,
    browsing: bool,
    default: bool,
    branch: bool,
}

impl RefLine {
    fn of(forge: &Forge, name: &[u8]) -> RefLine {
        let target = forge
            .refs()
            .and_then(|refs| refs.iter().find(|info| info.name == name))
            .map(|info| design::short_hex(&info.target));
        let facts = target
            .into_iter()
            .chain(forge.standing(name))
            .collect::<Vec<_>>()
            .join(" · ");
        RefLine {
            name: name.to_vec(),
            label: ref_label(name),
            facts,
            browsing: name == forge.head_name(),
            default: name == forge.default_head(),
            branch: name.starts_with(b"refs/heads/"),
        }
    }
}

struct RefRoutes {
    browse: parts::Route<Vec<u8>>,
    compare: parts::Route<Vec<u8>>,
    may_compare: bool,
}

fn ref_row(
    line: &RefLine,
    routes: &RefRoutes,
    active: bool,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let label = &line.label;
    let mut row = Row::titled(id(format!("forge-ref-row-{label}")), label.clone())
        .lead(parts::lead_icon("git-branch", ladder, theme))
        .caption(parts::caption(line.facts.clone(), ladder, theme))
        .chosen(line.browsing);
    if line.default {
        row = row.tail(design::badge(
            id(format!("forge-ref-default-{label}")),
            "default",
            ladder,
            theme.muted,
            theme.surface_raised,
        ));
    } else if line.branch {
        let (compare, from) = (routes.compare.clone(), line.name.clone());
        let start = move |_: &ClickEvent, window: &mut Window, app: &mut App| {
            compare(from.clone(), window, app)
        };
        row = row.tail(
            design::button(
                id(format!("forge-compare-{label}")),
                "Compare",
                ladder,
                theme,
                start,
            )
            .small()
            .enabled(routes.may_compare),
        );
    }
    let (browse, name) = (routes.browse.clone(), line.name.clone());
    let row = row.press(ladder, theme).on_click(
        move |_: &ClickEvent, window: &mut Window, app: &mut App| browse(name.clone(), window, app),
    );
    parts::option(row, active).into_any_element()
}

fn settings(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let page = body("forge-settings")
        .overflow_y_scroll()
        .children(parts::pending(forge, "settings", cx, ladder, theme));
    let reply = parts::stage(
        forge,
        &forge.repo_query(),
        "forge-settings-repo",
        words::READING_REPOSITORY,
        cx,
        (ladder, theme),
    );
    let repo = match reply {
        Ok(Reply::Repo { repo, .. }) => repo,
        Ok(_) => return page.into_any_element(),
        Err(state) => return page.child(state).into_any_element(),
    };
    let owner = forge.owns_repo();
    let configures = forge.may_configure();
    let section = |key: &str, label: &str| {
        design::section_head(id(key.to_owned()), label.to_owned(), ladder, theme)
    };
    page.child(section("forge-settings-refs", "Refs"))
        .child(default_head(
            forge,
            &repo.repo.settings.head,
            configures,
            cx,
            ladder,
            theme,
        ))
        .child(section("forge-settings-access", "Access"))
        .children(crate::policies::repo_row(forge, cx, ladder, theme))
        .child(
            Row::titled(id("forge-writer-owner"), "Owner")
                .tail(
                    div()
                        .text_type(ladder.secondary)
                        .child(forge.principal_name(&repo.repo.owner)),
                )
                .draw(ladder, theme),
        )
        .children(switches(
            forge,
            &repo.repo.settings,
            configures,
            cx,
            ladder,
            theme,
        ))
        .child(section("forge-settings-writers", words::CAN_WRITE))
        .children(writers(forge, owner, cx, ladder, theme))
        .into_any_element()
}

fn default_head(
    forge: &Forge,
    head: &[u8],
    owner: bool,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let mut row = Row::titled(id(Menu::Head.key()), words::DEFAULT_HEAD)
        .caption(parts::caption(words::DEFAULT_HEAD_RULE, ladder, theme))
        .tail(parts::data(ref_label(head)).text_type(ladder.secondary));
    if owner {
        row = row.chevron();
    }
    let row = row.draw(ladder, theme);
    match owner && !forge.busy("settings") {
        true => design::pressed(row, ladder)
            .role(Role::Button)
            .focusable()
            .on_click(cx.listener(|forge, event: &ClickEvent, window, cx| {
                forge.open_dropdown(Menu::Head, event, window, cx)
            }))
            .into_any_element(),
        false => row.into_any_element(),
    }
}

fn switches(
    forge: &Forge,
    settings: &forge::Settings,
    owner: bool,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> [AnyElement; 2] {
    let may = owner && !forge.busy("settings");
    let (force, delete) = (!settings.allow_force, !settings.allow_delete);
    let force = cx.listener(move |forge, _: &ClickEvent, _, cx| {
        forge.configure_with(|settings| settings.allow_force = force, cx)
    });
    let delete = cx.listener(move |forge, _: &ClickEvent, _, cx| {
        forge.configure_with(|settings| settings.allow_delete = delete, cx)
    });
    let flag =
        |key: &str, title: &'static str, said: &'static str, on: bool, flip: parts::Press| {
            Row::titled(id(format!("{key}-row")), title)
                .caption(parts::caption(said, ladder, theme))
                .tail(design::switch(
                    id(key.to_owned()),
                    title,
                    on,
                    may,
                    ladder,
                    theme,
                    flip,
                ))
                .draw(ladder, theme)
                .into_any_element()
        };
    [
        flag(
            "forge-settings-force",
            words::ALLOW_FORCE_PUSHES,
            words::ALLOW_FORCE_PUSHES_RULE,
            settings.allow_force,
            Box::new(force),
        ),
        flag(
            "forge-settings-delete",
            words::ALLOW_REF_DELETION,
            words::ALLOW_REF_DELETION_RULE,
            settings.allow_delete,
            Box::new(delete),
        ),
    ]
}

fn writers(
    forge: &Forge,
    owner: bool,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    match forge.writers_stage() {
        Stage::Ready(list) => writer_rows(forge, &Writer::rows(list), owner, cx, ladder, theme),
        Stage::Loading => vec![
            parts::quiet(
                id("forge-settings-writers-loading"),
                words::READING_WRITERS,
                ladder,
                theme,
            )
            .into_any_element(),
        ],
        Stage::Failed(refusal) => {
            let retry = cx.listener(|forge, _: &ClickEvent, _, cx| forge.retry_writers(cx));
            vec![
                design::refused(
                    "forge-settings-writers",
                    design::sentence(refusal),
                    ladder,
                    theme,
                    retry,
                )
                .into_any_element(),
            ]
        }
    }
}

fn writer_rows(
    forge: &Forge,
    writers: &[Writer],
    owner: bool,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    let busy = forge.busy("settings");
    let mut rows: Vec<AnyElement> = writers
        .iter()
        .map(|writer| {
            let name = forge.writer_name(writer);
            let mut row = Row::titled(id(format!("forge-writer-{name}")), name.clone());
            if owner {
                let who = writer.clone();
                let revoke =
                    cx.listener(move |forge, _: &ClickEvent, _, cx| forge.revoke(who.clone(), cx));
                row = row.tail(
                    design::button(
                        id(format!("forge-revoke-{name}")),
                        "Revoke",
                        ladder,
                        theme,
                        revoke,
                    )
                    .small()
                    .kind(Kind::Quiet)
                    .enabled(!busy),
                );
            }
            row.draw(ladder, theme).into_any_element()
        })
        .collect();
    if writers.is_empty() {
        rows.push(
            parts::quiet(
                id("forge-writers-none"),
                words::ONLY_THE_OWNER,
                ladder,
                theme,
            )
            .into_any_element(),
        );
    }
    if owner {
        let grant = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_sheet(Sheet::Grant, cx));
        let row = Row::titled(id("forge-grant"), "Grant write access…")
            .caption(parts::caption(words::PICK_A_WRITER, ladder, theme))
            .chevron()
            .draw(ladder, theme)
            .role(Role::Button)
            .focusable()
            .when(!busy, |row| design::pressed(row, ladder).on_click(grant));
        rows.push(row.into_any_element());
    }
    rows
}

pub(super) fn grant_sheet(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let listed = forge
        .writers()
        .into_iter()
        .flat_map(|list| list.accounts())
        .map(forge::Principal::Account);
    let skip: Vec<forge::Principal> = forge
        .repo()
        .map(|(info, _)| info.repo.owner.clone())
        .into_iter()
        .chain(listed)
        .collect();
    let ask = parts::PeopleAsk {
        key: "forge-grant-sheet",
        title: words::GRANT_WRITE_ACCESS,
        caption: Some(words::PICK_A_WRITER),
        skip: &skip,
    };
    parts::people_sheet(ask, Forge::grant_to, forge, cx, ladder, theme)
}
