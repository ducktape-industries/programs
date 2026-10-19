//! The repositories: the full list when nothing is open, the rail that
//! switches between them when something is.
use view::prelude::*;
use view::{Div, Stateful, wire};

use crate::Forge;
use crate::queries::PAGE;
use crate::ui::components::{PRESS_TARGET, button, empty_state, heading, id, ref_label};
use crate::ui::{pending, staged};
use crate::words;
use forge::{Query, Reply, RepoInfo};
use view::design::Ladder;

fn query() -> Query {
    Query::Repos { page: PAGE }
}

fn listed<'a>(forge: &'a Forge, reply: &'a Reply) -> Vec<&'a RepoInfo> {
    let Reply::Repos { page, .. } = reply else {
        return Vec::new();
    };
    let needle = forge.search.text().trim().to_lowercase();
    page.items
        .iter()
        .filter(|info| needle.is_empty() || info.name.to_lowercase().contains(&needle))
        .collect()
}

/// The table's column widths: owner, default head, refs, last activity.
/// The name takes the rest. Last active holds "block 12,345,678" (16 mono
/// characters at 0.6 em of the caption size, and the cell's insets).
const OWNER_W: Pixels = px(150.);
const HEAD_W: Pixels = px(120.);
const REFS_W: Pixels = px(64.);
const ACTIVITY_W: Pixels = px(130.);
/// The overview's filter field.
const SEARCH_W: Pixels = px(260.);
/// The name keeps this much of a table row.
const NAME_MIN_W: Pixels = px(120.);
/// A table row's and its header's heights.
const ROW_H: Pixels = px(48.);
const HEADER_H: Pixels = px(30.);
/// A table cell's side inset.
const CELL_X: Pixels = px(12.);

/// Whether the list, `width` wide, holds the table: the name's floor and
/// every column, inside the rows' side margins and the scroll bar's gutter.
/// That is 616 px, under the view's 640 minimum; a narrower list drops the
/// header and puts each row's facts on one line under its name.
fn table_fits(width: f32) -> bool {
    let columns = NAME_MIN_W + OWNER_W + HEAD_W + REFS_W + ACTIVITY_W;
    width >= f32::from(columns + design::space::SM * 2. + design::size::SCROLLBAR)
}

/// The table's head: what each column holds, quiet and mono.
fn table_header(theme: &Theme) -> Stateful<Div> {
    let cell = |label: &'static str| {
        div()
            .id(id(format!("forge-repos-column-{label}")))
            .px(CELL_X)
            .font_family(design::fonts::FAMILY_MONO)
            .text_size(design::text::CAPTION)
            .text_color(theme.muted)
            .role(Role::ColumnHeader)
            .child(label)
    };
    div()
        .id(id("forge-repos-columns"))
        .mx(design::space::SM)
        .h(HEADER_H)
        .flex()
        .items_center()
        .border_b_1()
        .border_color(theme.border)
        .role(Role::Row)
        .child(cell("Name").flex_1().min_w(NAME_MIN_W))
        .child(cell("Owner").w(OWNER_W))
        .child(cell("Default").w(HEAD_W))
        .child(cell("Refs").w(REFS_W).flex().justify_end())
        .child(cell("Last active").w(ACTIVITY_W).flex().justify_end())
}

/// The cells of a repository's row, in the order ← → walk them: the
/// press that opens it, Copy, and the last-activity link.
const CELLS: usize = 3;

/// One repository: its name over the address it clones from, then its
/// owner, default branch, refs and last activity, in their columns when
/// `table`, else on one line under the name.
#[allow(
    clippy::too_many_arguments,
    reason = "the ladder rides beside the theme into every sized helper; the phone rework of this view reshapes the row"
)]
fn repo_row(
    forge: &Forge,
    info: &RepoInfo,
    owner: String,
    table: bool,
    active: Option<usize>,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let name = info.name.clone();
    let group = format!("forge-repo-{name}-row");
    let picture = forge.picture(&info.repo.owner, cx);
    let open = cx.listener({
        let name = name.clone();
        move |forge, _: &ClickEvent, _, cx| forge.open_repo(name.clone(), cx)
    });
    // the row's press lies under the whole row; Copy and the activity link
    // sit over it, beside it rather than inside it, as a button may not
    // hold them. Each is a cell of the grid, and the active cell's control
    // is the one assistive technology is told is active.
    let press = div()
        .id(id(format!("forge-repo-{name}-open")))
        .size_full()
        .aria_label(format!("Open {name}"))
        .on_click(open);
    let press = div()
        .id(id(format!("forge-repo-{name}-open-cell")))
        .absolute()
        .inset_0()
        .role(Role::GridCell)
        .child(design::item(press, Role::Button, active == Some(0)));
    div()
        .id(id(format!("forge-repo-{name}")))
        .group(group.clone())
        .relative()
        .mx(design::space::SM)
        .min_h(ROW_H)
        // a wrapped (narrow) row grows; the list's column must not squeeze it
        .flex_none()
        .flex()
        .flex_wrap()
        .items_center()
        .border_b_1()
        .border_color(theme.border)
        .hover(|style| style.bg(theme.surface))
        // the active row is drawn once the arrows have walked the list
        .when(active.is_some() && forge.repos_cursor.is_some(), |row| {
            row.bg(theme.surface)
        })
        .role(Role::Row)
        .child(press)
        .child(repo_title(
            forge,
            &name,
            &group,
            active == Some(1),
            cx,
            theme,
        ))
        .child(repo_facts(
            info,
            owner,
            picture,
            table,
            active == Some(2),
            theme,
            ladder,
        ))
        .into_any_element()
}

/// A repository's name over its clone address, and Copy on hover.
fn repo_title(
    forge: &Forge,
    name: &str,
    group: &str,
    active: bool,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Div {
    let url = forge.repo_link(name);
    div()
        .flex_1()
        .min_w(NAME_MIN_W)
        .px(CELL_X)
        .py(design::space::XS)
        .flex()
        .flex_col()
        .child(
            div()
                .font_weight(view::FontWeight::SEMIBOLD)
                .truncate()
                .child(name.to_owned()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.faint)
                        .child(url.clone()),
                )
                .child(
                    div()
                        .id(id(format!("forge-repo-{name}-copy-cell")))
                        .role(Role::GridCell)
                        .child(copy_button(forge, name, url, group, active, cx, theme)),
                ),
        )
}

/// Copies the clone address; shown on hover, while the arrows are on it,
/// and while it says Copied.
fn copy_button(
    forge: &Forge,
    name: &str,
    url: String,
    group: &str,
    active: bool,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
    let copy = cx.listener({
        let name = name.to_owned();
        move |forge, _: &ClickEvent, _, cx| {
            cx.host()
                .notify::<view::methods::ClipboardWrite>(url.clone());
            forge.copied = Some(name.clone());
            cx.notify();
        }
    });
    let copied = forge.copied.as_deref() == Some(name);
    let button = div()
        .id(id(format!("forge-repo-{name}-copy")))
        .aria_label(format!("Copy the address of {name}"))
        // a click here is not also a click on the row that opens the repo
        .occlude()
        .min_w(PRESS_TARGET)
        .min_h(PRESS_TARGET)
        .flex()
        .items_center()
        .px_1p5()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .hover(|style| style.text_color(theme.foreground))
        .on_click(copy)
        .child(if copied { "Copied" } else { "Copy" });
    let button = design::item(button, Role::Button, active);
    match copied || active {
        true => button,
        false => button
            .invisible()
            .group_hover(group.to_owned(), |style| style.visible()),
    }
}

/// What a repository is: owner, default branch, refs, last activity. In the
/// table each holds its column, the refs a bare count under their header;
/// wrapped, they share one line under the name, each as wide as it reads,
/// the refs with their unit.
fn repo_facts(
    info: &RepoInfo,
    owner: String,
    picture: crate::select::Picture,
    table: bool,
    active: bool,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let column = |cell: Div, width: Pixels| cell.when(table, |cell| cell.w(width));
    let refs = info.repo.refs_count;
    div()
        .flex()
        .items_center()
        .when(!table, |facts| {
            facts.w_full().flex_wrap().pb(design::space::XS)
        })
        .child(
            column(div(), OWNER_W)
                .px(CELL_X)
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    design::avatar_with(&owner, picture, design::size::AVATAR, theme)
                        .border_1()
                        .border_color(theme.border)
                        .font_weight(view::FontWeight::SEMIBOLD),
                )
                .child(div().min_w(px(0.)).truncate().child(owner)),
        )
        .child(
            column(div(), HEAD_W)
                .px(CELL_X)
                .truncate()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .child(ref_label(&info.repo.settings.head)),
        )
        .child(
            column(div(), REFS_W)
                .px(CELL_X)
                .flex()
                .justify_end()
                .child(match table {
                    true => refs.to_string(),
                    false => design::plural(refs, "ref", "refs"),
                }),
        )
        .child(
            column(div(), ACTIVITY_W)
                .id(id(format!("forge-repo-{}-activity-cell", info.name)))
                .px(CELL_X)
                .flex()
                .justify_end()
                .role(Role::GridCell)
                .child(design::item(
                    crate::ui::block_link(
                        id(format!("forge-repo-{}-activity", info.name)),
                        info.repo.last_activity,
                        theme,
                        ladder,
                    ),
                    Role::Link,
                    active,
                )),
        )
}

/// The screen: every repository of this network, newest activity first.
pub(crate) fn overview(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let body = staged(
        forge,
        &query(),
        "forge-repos-list",
        words::READING_REPOSITORIES,
        cx,
        theme,
        ladder,
    );
    let count = body.as_ref().ok().map(|reply| listed(forge, reply).len());
    let mut column = div()
        .id(id("forge-repos"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header(
            forge,
            count,
            cx,
            theme,
            ladder,
            "forge-repos-search",
        ));
    if let Some(form) = &forge.new_repo {
        column = column.child(dialog(form, cx, theme, ladder));
    }
    column = column.child(pending(forge, "repos", theme));
    let body = match body {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let rows = listed(forge, body);
    if rows.is_empty() {
        // the Expo app's (`app/(tabs)/forge/index.tsx:77-79`)
        let (glyph, title, detail) = match forge.search.is_blank() {
            true => ("hammer", words::NO_REPOSITORIES_YET, words::CREATE_ONE),
            false => (
                "search",
                words::NOTHING_MATCHES,
                words::NO_REPOSITORY_MATCHES,
            ),
        };
        return column
            .child(empty_state(
                id("forge-repos-empty"),
                glyph,
                title,
                detail,
                ladder,
                theme,
            ))
            .into_any_element();
    }
    // one Tab stop: ↑ ↓ walk the repositories, ← → a row's cells (Open,
    // Copy, activity), Enter or Space presses the active cell
    let names: Vec<String> = rows.iter().map(|info| info.name.clone()).collect();
    let heights: Vec<u64> = rows.iter().map(|info| info.repo.last_activity).collect();
    let (at, cell) = forge
        .repos_cursor
        .as_ref()
        .and_then(|(name, cell)| Some((names.iter().position(|it| it == name)?, *cell)))
        .unwrap_or((0, 0));
    let moved = names.clone();
    let stepped = names.clone();
    let pressed = names.clone();
    let mut list = design::composite(id("forge-repos-list"), Role::Grid, "Repositories")
        .active(at, names.len())
        .cells(cell, CELLS)
        .on_move(cx.processor(move |forge, index: usize, _, cx| {
            forge.repos_cursor = Some((moved[index].clone(), 0));
            cx.notify();
        }))
        .on_move_cell(cx.processor(move |forge, cell: usize, _, cx| {
            forge.repos_cursor = Some((stepped[at].clone(), cell));
            cx.notify();
        }))
        .on_press(cx.processor(move |forge, index: usize, _, cx| {
            let name = pressed[index].clone();
            match forge.repos_cursor.as_ref().map_or(0, |(_, cell)| *cell) {
                0 => forge.open_repo(name, cx),
                1 => {
                    let url = forge.repo_link(&name);
                    cx.host().notify::<view::methods::ClipboardWrite>(url);
                    forge.copied = Some(name);
                    cx.notify();
                }
                _ => {
                    cx.host().open_link(&crate::ui::block_href(heights[index]));
                }
            }
        }))
        .build()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    let table = table_fits(forge.layout.width);
    if table {
        list = list.child(table_header(theme));
    }
    for (index, info) in rows.into_iter().enumerate() {
        let owner = forge.principal_name(&info.repo.owner);
        let active = (index == at).then_some(cell);
        list = list.child(repo_row(
            forge, info, owner, table, active, cx, theme, ladder,
        ));
    }
    column.child(list).into_any_element()
}

/// The rail: the same repositories, compact, while one of them is open.
pub(crate) fn rail(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let column = div()
        .id(id("forge-rail"))
        .w(px(forge.layout.tree))
        .flex_none()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .bg(theme.background)
        .child(rail_home(cx, theme))
        .child(rail_search(forge, theme));
    let reply = match staged(
        forge,
        &query(),
        "forge-rail-list",
        words::READING_REPOSITORIES,
        cx,
        theme,
        ladder,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    // one Tab stop: ↑ ↓ walk the repositories, Enter opens the active one;
    // the arrows start on the open repository
    let names: Vec<String> = listed(forge, reply)
        .iter()
        .map(|info| info.name.clone())
        .collect();
    let at = forge
        .rail_cursor
        .as_ref()
        .or(forge.nav().repo.as_ref())
        .and_then(|name| names.iter().position(|it| it == name))
        .unwrap_or(0);
    let moved = names.clone();
    let pressed = names.clone();
    let mut list = design::composite(id("forge-rail-list"), Role::ListBox, "Repositories")
        .active(at, names.len())
        .on_move(cx.processor(move |forge, index: usize, _, cx| {
            forge.rail_cursor = Some(moved[index].clone());
            cx.notify();
        }))
        .on_press(cx.processor(move |forge, index: usize, _, cx| {
            forge.open_repo(pressed[index].clone(), cx)
        }))
        .build()
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (index, name) in names.into_iter().enumerate() {
        let open = cx.listener({
            let name = name.clone();
            move |forge, _: &ClickEvent, _, cx| forge.open_repo(name.clone(), cx)
        });
        let selected = forge.nav().repo.as_deref() == Some(name.as_str());
        let row = div()
            .id(id(format!("forge-rail-repo-{name}")))
            .h(design::size::CONTROL)
            .px(RAIL_X)
            .flex()
            .items_center()
            .when(selected, |item| {
                item.bg(theme.surface_raised)
                    .font_weight(view::FontWeight::MEDIUM)
            })
            .when(
                forge.rail_cursor.is_some() && index == at && !selected,
                |item| item.bg(theme.surface),
            )
            .hover(|style| style.bg(theme.surface))
            .aria_selected(selected)
            .on_click(open)
            .child(div().flex_1().truncate().child(name));
        list = list.child(design::item(row, Role::ListBoxOption, index == at));
    }
    column.child(list).into_any_element()
}

/// The rail's row inset.
const RAIL_X: Pixels = px(14.);

/// The window's title bar already says Forge: the rail's head is the way
/// back to every repository.
fn rail_home(cx: &mut Context<Forge>, theme: &Theme) -> Stateful<Div> {
    let home = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_repos(cx));
    div()
        .id(id("forge-rail-header"))
        .flex()
        .items_center()
        .px(RAIL_X)
        .pt(design::space::LG)
        .pb(design::space::SM)
        .child(
            div()
                .id(id("forge-rail-home"))
                .flex_1()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .hover(|style| style.text_color(theme.foreground))
                .role(Role::Button)
                .focusable()
                .on_click(home)
                .child("← All repositories"),
        )
}

/// The row pads, not the field: a full-width field with its own margins
/// ran past the rail's edge.
fn rail_search(forge: &Forge, theme: &Theme) -> Div {
    div().px(design::space::LG).pb(design::space::SM).child(
        Input::new(
            id("forge-rail-search"),
            &forge.search,
            "Filter repositories",
        )
        .h(design::size::CONTROL)
        .px_2()
        .py_1()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_color(theme.foreground)
        .placeholder("Search repositories"),
    )
}

fn header(
    forge: &Forge,
    count: Option<usize>,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
    search_id: &str,
) -> AnyElement {
    let new = cx.listener(|forge, _: &ClickEvent, window, cx| forge.start_repo(window, cx));
    div()
        .id(id("forge-repos-header"))
        .flex()
        .items_center()
        .gap(design::space::LG)
        .px(crate::ui::PAGE_X)
        .py(design::space::BLOCK)
        .child(heading(id("forge-repos-title"), "Repositories", 1, theme))
        .children(count.map(|count| {
            design::mono(count.to_string())
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
        }))
        .child(
            Input::new(
                id(search_id.to_owned()),
                &forge.search,
                "Filter repositories",
            )
            .ml(design::space::LG)
            .h(design::size::CONTROL)
            .w(SEARCH_W)
            .px_2()
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.background)
            .text_color(theme.foreground)
            .placeholder(words::FILTER_BY_NAME),
        )
        .child(div().flex_1())
        .child(
            button(
                id("forge-new-repo"),
                words::NEW_REPOSITORY,
                ladder,
                theme,
                new,
            )
            .kind(design::Kind::Primary)
            .enabled(forge.may_write()),
        )
        .into_any_element()
}

fn dialog(
    form: &crate::state::NewRepo,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    // a name typed again takes the last refusal with it
    let typed = cx.listener(|forge, _: &wire::TextChange, _, cx| {
        if let Some(form) = &mut forge.new_repo {
            form.error.clear();
        }
        cx.notify();
    });
    let formats = design::segmented(
        id("forge-new-repo-format"),
        words::OBJECT_FORMAT,
        ladder,
        theme,
        Some(usize::from(form.sha1)),
        [
            (id("forge-new-repo-sha256"), "SHA-256".into()),
            (id("forge-new-repo-sha1"), "SHA-1".into()),
        ],
        cx.processor(|forge, picked: usize, _, cx| {
            if let Some(form) = &mut forge.new_repo {
                form.sha1 = picked == 1;
            }
            cx.notify();
        }),
    );
    let create = cx.listener(|forge, _: &ClickEvent, _, cx| forge.create_repo(cx));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_repo(cx));
    let caption = |text: &'static str| {
        div()
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(text)
    };
    let mut card = div()
        .id(id("forge-new-repo-card"))
        .flex()
        .flex_col()
        .gap(design::space::LG)
        .mx(crate::ui::PAGE_X)
        .mb(design::space::LG)
        .px(design::space::BLOCK)
        .py(design::space::BLOCK)
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(heading(
            id("forge-new-repo-title"),
            words::NEW_REPOSITORY,
            2,
            theme,
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(design::space::XL)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(200.))
                        .flex()
                        .flex_col()
                        .gap(design::space::XS)
                        .child(caption("Name"))
                        .child(
                            Input::new(
                                id("forge-new-repo-name"),
                                &form.name,
                                words::REPOSITORY_NAME,
                            )
                            .h(design::size::CONTROL)
                            .w_full()
                            .px_2()
                            .border_1()
                            .border_color(theme.border_strong)
                            .bg(theme.background)
                            .text_color(theme.foreground)
                            .placeholder(words::NAME_RULE)
                            .on_change(typed)
                            .on_submit(cx.listener(|forge, _: &(), _, cx| forge.create_repo(cx))),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(design::space::XS)
                        .child(caption(words::OBJECT_FORMAT))
                        .child(formats),
                ),
        )
        .child(caption(words::FORMAT_RULE));
    if !form.error.is_empty() {
        card = card.child(
            div()
                .id(id("forge-new-repo-error"))
                .text_size(design::text::SECONDARY)
                .text_color(theme.danger)
                .child(form.error.clone()),
        );
    }
    card.child(
        div()
            .flex()
            .justify_end()
            .gap_2()
            .child(
                button(id("forge-new-repo-cancel"), "Cancel", ladder, theme, cancel)
                    .kind(design::Kind::Quiet),
            )
            .child(
                button(id("forge-new-repo-submit"), "Create", ladder, theme, create)
                    .kind(design::Kind::Primary),
            ),
    )
    .into_any_element()
}
