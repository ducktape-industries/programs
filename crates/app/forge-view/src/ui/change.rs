//! One Change: its header, the three tabs a reviewer lives in, and its
//! details beside them. Conversation is chat's hidden channel; Files is
//! the reviewer's home.
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use view::prelude::*;
use view::{Anchor, AnchoredPositionMode, Div, FontWeight, Point, Stateful};

use crate::Forge;
use crate::select::{path_of, revision_name};
use crate::state::{ChangeTab, verdict_label};
use crate::ui::changes::state_chip;
use crate::ui::components::{badge, button, heading, id, path_text, quiet, short_hex};
use crate::ui::{PAGE_X, TAB_BAR_H, commits, diff, dock, pending, staged};
use crate::words;
use forge::{Change, ChangeState, FileDiff, Query, Reply, Verdict};
use view::design::Ladder;

/// The Files tab's file list.
const FILE_LIST_W: Pixels = px(240.);
/// The finish-review panel.
const FINISH_W: Pixels = px(380.);

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(query) = forge.change_query() else {
        return div().into_any_element();
    };
    let reply = match staged(
        forge,
        &query,
        "forge-change",
        words::READING_CHANGE,
        cx,
        theme,
        ladder,
    ) {
        Ok(reply) => reply,
        Err(state) => return state,
    };
    let Reply::Change { change, .. } = reply else {
        return div().into_any_element();
    };
    let scope = crate::state::change_key(&forge.repo_name(), change.n);
    let mut column = div()
        .id(id("forge-change-detail"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(header(forge, cx, theme, ladder))
        .child(tabs(forge, cx, theme, ladder))
        .child(pending(forge, &scope, theme));
    if let Some(form) = forge.change_form() {
        column = column.child(crate::ui::changes::form(form, forge, cx, theme, ladder));
    }
    let tab = forge.nav().change_tab;
    let body: AnyElement = match tab {
        ChangeTab::Conversation => crate::ui::conversation::render(forge, cx, theme, ladder),
        ChangeTab::Commits => match (forge.nav().commit.clone(), forge.shown_log()) {
            (Some(oid), _) => commits::detail(forge, &oid, cx, theme, ladder),
            (None, Some(log)) => commits::log(forge, &log, "forge-change-log", cx, theme, ladder),
            (None, None) => div().into_any_element(),
        },
        ChangeTab::Files => files(forge, cx, theme, ladder),
    };
    // wide, the details stand beside the conversation and the commits (the
    // files keep their width for the diff); narrow, "Details" lays them
    // over the whole screen
    let mut row = div()
        .id(id("forge-change-body"))
        .relative()
        .flex()
        .flex_1()
        .min_h(px(0.))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .min_h(px(0.))
                .child(body),
        );
    if forge.layout.narrow() {
        if forge.layout.dock_open {
            row = row.child(design::over(
                id("forge-details-over"),
                dock::sidebar(forge, cx, theme, ladder).w_full(),
                theme,
            ));
        }
    } else if tab != ChangeTab::Files {
        row = row.child(dock::sidebar(forge, cx, theme, ladder));
    }
    column.child(row).into_any_element()
}

fn header(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let Some((change, source, _, _)) = forge.change() else {
        return div().into_any_element();
    };
    let back = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_change(None, cx));
    let author = forge.principal_name(&change.author);
    let mut meta = format!(
        "{author} wants {}",
        words::endpoints(&revision_name(&change.from), &change.into)
    );
    if let Some(head) = source {
        meta = format!("{meta} · head {}", short_hex(head));
    }
    let mut column = div()
        .id(id("forge-change-header"))
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .px(PAGE_X)
        .pt(design::space::LG)
        .pb(design::space::MD)
        .child(
            div()
                .id(id("forge-change-back"))
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .hover(|style| style.text_color(theme.foreground))
                .role(Role::Link)
                .focusable()
                .on_click(back)
                .child(format!("← {} · Changes", forge.repo_name())),
        )
        .child(title_line(forge, change, cx, theme, ladder))
        .child(
            design::mono(meta)
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        );
    if closing(forge, change) {
        column = column.child(
            div()
                .id(id("forge-close-warning"))
                .text_size(design::text::SECONDARY)
                .text_color(theme.danger)
                .child(words::CLOSING_IS_FINAL),
        );
    }
    // why Merge is off: a strip while the change is open, a quiet line
    // once it has ended
    match forge.merge_block() {
        Some(crate::actions::MergeBlock::NotOpen) => {
            column = column.child(
                div()
                    .id(id("forge-merge-refusal"))
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(crate::actions::MergeBlock::NotOpen.sentence()),
            );
        }
        Some(blocked) => {
            column = column.child(
                div()
                    .id(id("forge-merge-refusal"))
                    .mt(design::space::SM)
                    .px(design::space::LG)
                    .py(design::space::MD)
                    .bg(theme.warning_soft)
                    .text_color(theme.warning)
                    .text_size(design::text::SECONDARY)
                    .child(blocked.sentence()),
            );
        }
        None => {}
    }
    column.into_any_element()
}

/// Whether this change's Close waits for its second press.
fn closing(forge: &Forge, change: &Change) -> bool {
    forge.closing == Some(crate::state::change_key(&forge.repo_name(), change.n))
}

/// The change's title, number and state, and what an open change offers:
/// Edit, Close, and Merge (kept in place, off, while it cannot run).
fn title_line(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let edit = cx.listener(|forge, _: &ClickEvent, window, cx| forge.start_edit(window, cx));
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| forge.close_change(cx));
    let merge = cx.listener(|forge, _: &ClickEvent, _, cx| forge.merge(cx));
    let open = change.state == ChangeState::Open;
    let edits = forge.may_edit_change();
    let closing = closing(forge, change);
    let mergeable = forge.merge_block().is_none();
    let mut top = div()
        .id(id("forge-change-head"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::SM)
        .child(heading(
            id("forge-change-heading"),
            change.title.clone(),
            1,
            theme,
        ))
        .child(
            div()
                .text_size(design::text::TITLE)
                .text_color(theme.muted)
                .child(format!("#{}", change.n)),
        )
        .child(state_chip(change.state, change.n, theme, ladder))
        .child(div().flex_1());
    // an ended change has nothing left to edit, close or merge
    if open {
        top = top
            .child(
                button(id("forge-edit-change"), "Edit", ladder, theme, edit)
                    .kind(design::Kind::Quiet)
                    .enabled(edits),
            )
            .child(
                button(
                    id("forge-close-change"),
                    if closing {
                        words::CLOSE_FOR_GOOD
                    } else {
                        "Close"
                    },
                    ladder,
                    theme,
                    close,
                )
                .kind(design::Kind::Outline)
                .enabled(forge.may_close_change()),
            )
            .child(
                button(id("forge-merge"), "Merge", ladder, theme, merge)
                    .kind(design::Kind::Primary)
                    .enabled(mergeable && forge.may_change_repo()),
            );
    }
    top
}

/// The change's tabs, each with its count once it is known.
fn tabs(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    // manual: Files reads the whole diff and Commits a log, so the arrows
    // only move and Enter opens
    let active = forge.change_tab_cursor.unwrap_or(forge.nav().change_tab);
    let at = |tab| {
        ChangeTab::ALL
            .iter()
            .position(|it| *it == tab)
            .unwrap_or_default()
    };
    let mut bar = design::composite(id("forge-change-tabs"), Role::TabList, "Change")
        .orientation(design::Orientation::Horizontal)
        .wrap()
        .active(at(active), ChangeTab::ALL.len())
        .on_move(cx.processor(|forge, index: usize, _, cx| {
            forge.change_tab_cursor = Some(ChangeTab::ALL[index]);
            cx.notify();
        }))
        .on_press(cx.processor(|forge, index: usize, _, cx| {
            forge.open_change_tab(ChangeTab::ALL[index], cx)
        }))
        .build()
        .h(TAB_BAR_H)
        .flex()
        .items_center()
        .gap(design::space::XL)
        .px(PAGE_X)
        .border_b_1()
        .border_color(theme.border);
    for tab in ChangeTab::ALL {
        let pick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_change_tab(tab, cx));
        bar = bar.child(crate::ui::components::tab(
            id(format!("forge-change-tab-{}", tab.slug())),
            tab.label(),
            forge.change_tab_count(tab),
            forge.nav().change_tab == tab,
            active == tab,
            theme,
            ladder,
            pick,
        ));
    }
    bar
}

// ------------------------------------------------------------------ files

fn files(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let Some(query) = forge.diff_query() else {
        return quiet(words::COMPARING_ENDPOINTS, theme);
    };
    let columns = div()
        .id(id("forge-files"))
        .flex()
        .flex_1()
        .min_h(px(0.))
        .child(file_tree(forge, &query, cx, theme, ladder));
    let pane = div()
        .id(id("forge-files-pane"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(review_bar(forge, cx, theme, ladder))
        .child(diff::render(
            forge,
            &query,
            "forge-diff",
            true,
            cx,
            theme,
            ladder,
        ));
    columns.child(pane).into_any_element()
}

/// The Files tab's list of files, the id its cursor is kept under.
pub(crate) const FILE_LIST_ID: &str = "forge-file-tree-list";
/// A file row's height, as the approved mock draws it.
const FILE_ROW_H: f32 = 30.;

/// The reviewer's file list: how much each file changes, a tick for the
/// ones read, and a click that shows one file alone. A virtual list, so a
/// render draws the rows on screen and not every file of a release; one
/// Tab stop, a grid: ↑ ↓ walk the files, Enter opens the active one, and →
/// reaches its viewed tick (drawn left of the name, as the mock has it,
/// but the row's second cell: the press stays first so Enter opens), where
/// Enter or Space ticks it.
fn file_tree(
    forge: &Forge,
    query: &Query,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let ladder = *ladder;
    let column = div()
        .id(id("forge-file-tree"))
        .w(FILE_LIST_W)
        .flex_none()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .border_r_1()
        .border_color(theme.border);
    let Some(shared) = forge.shared(query) else {
        return column
            .child(quiet(words::READING_DIFF, theme))
            .into_any_element();
    };
    let Reply::Diff {
        page, total_files, ..
    } = &*shared
    else {
        return column.into_any_element();
    };
    let (added, deleted) = crate::select::totals(&page.items);
    let all = cx.listener(|forge, _: &ClickEvent, _, cx| forge.single_file(None, cx));
    let header = div()
        .id(id("forge-file-tree-header"))
        .flex_none()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .h(px(36.))
        .child(
            design::mono(words::files_changed(*total_files, added, deleted))
                .id(id("forge-files-title"))
                // a release's counts give way to All files, the way back to
                // every file, rather than push it out of the column
                .min_w(px(0.))
                .truncate()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        )
        .child(div().flex_1())
        .when(forge.nav().diff_path.is_some(), |header| {
            header.child(
                div().id(id("forge-files-all-box")).flex_none().child(
                    button(id("forge-files-all"), words::ALL_FILES, &ladder, theme, all)
                        .kind(design::Kind::Quiet),
                ),
            )
        });
    // what marks the rows, read once a render for the rows the list draws:
    // a render pays for the rows on screen, not for every file
    let count = page.items.len();
    let (at, cell) = crate::ui::components::cursor(forge, FILE_LIST_ID);
    let marks = Rc::new(FileMarks {
        viewed: viewed_paths(forge),
        comments: file_comments(forge),
        selected: forge.nav().diff_path.clone(),
        active: (at.min(count.saturating_sub(1)), cell),
        walked: crate::ui::components::walked(forge, FILE_LIST_ID),
    });
    let open: diff::Route<Vec<u8>> = Rc::new(
        cx.listener(|forge, path: &Vec<u8>, _, cx| forge.single_file(Some(path.clone()), cx)),
    );
    let tick: diff::Route<Vec<u8>> =
        Rc::new(cx.listener(|forge, path: &Vec<u8>, _, cx| forge.toggle_viewed(path, cx)));
    let pressed = shared.clone();
    let list = crate::ui::components::grid(
        FILE_LIST_ID,
        "Files",
        count,
        2,
        Some(&forge.files_scroll),
        forge,
        cx,
        move |forge, index, cell, _, cx| {
            let Some(path) = diff::file_at(&pressed, index).and_then(path_of) else {
                return;
            };
            match cell {
                0 => forge.single_file(Some(path), cx),
                _ => forge.toggle_viewed(&path, cx),
            }
        },
    )
    // as tall as its rows, so the hint sits under a short list; a long one
    // gives way to the column and scrolls
    .flex_none()
    .flex_shrink_1()
    .h(px(count as f32 * FILE_ROW_H));
    let theme = *theme;
    let rows = crate::ui::components::rows(
        "forge-file-tree-rows",
        count,
        None,
        Some(&forge.files_scroll),
        move |index| match diff::file_at(&shared, index) {
            Some(file) => file_row(file, index, &marks, &open, &tick, &theme, &ladder),
            None => div().into_any_element(),
        },
    );
    column
        .child(header)
        .child(list.child(rows))
        .child(
            div()
                .flex_none()
                .px(design::space::BLOCK)
                .py(design::space::LG)
                .child(quiet(words::TICK_A_FILE, &theme)),
        )
        .into_any_element()
}

/// What marks the file list's rows: the files ticked, the comments on
/// each, the file shown alone, and where the arrows are (row, cell).
struct FileMarks {
    viewed: BTreeSet<String>,
    comments: BTreeMap<Vec<u8>, usize>,
    selected: Option<Vec<u8>>,
    active: (usize, usize),
    walked: bool,
}

/// The paths of the open change's files the reader has ticked, as its
/// viewed keys spell them.
fn viewed_paths(forge: &Forge) -> BTreeSet<String> {
    let Some(prefix) = forge.file_key(b"") else {
        return BTreeSet::new();
    };
    forge
        .viewed
        .range(prefix.clone()..)
        .take_while(|key| key.starts_with(&prefix))
        .map(|key| key[prefix.len()..].to_owned())
        .collect()
}

/// One file of the diff, a grid row: its press (the path, its comments and
/// counts) and its viewed tick, each a cell; the tick is drawn first.
fn file_row(
    file: &FileDiff,
    index: usize,
    marks: &FileMarks,
    open: &diff::Route<Vec<u8>>,
    tick: &diff::Route<Vec<u8>>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(path) = path_of(file) else {
        return div().h(px(FILE_ROW_H)).into_any_element();
    };
    let label = path_text(&path);
    let viewed = marks.viewed.contains(&label);
    let comments = marks.comments.get(&path).copied().unwrap_or(0);
    let selected = marks.selected.as_deref() == Some(path.as_slice());
    let active = (index == marks.active.0).then_some(marks.active.1);
    let walked = marks.walked;
    let pick = {
        let (open, path) = (open.clone(), path.clone());
        move |_: &ClickEvent, window: &mut Window, app: &mut App| open(&path, window, app)
    };
    let tick = {
        let tick = tick.clone();
        move |_: &ClickEvent, window: &mut Window, app: &mut App| tick(&path, window, app)
    };
    let theme = *theme;
    let check = div()
        .id(id(format!("forge-viewed-{label}")))
        .size(px(14.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(if viewed {
            theme.primary
        } else {
            theme.border_strong
        })
        .when(viewed, |check| {
            design::focus_shown_on_ink(
                check.bg(theme.primary).text_color(theme.primary_foreground),
                &theme,
            )
        })
        // the arrows on the tick: the ring's ink marks the cell
        .when(walked && active == Some(1), |check| {
            check.border_2().border_color(theme.accent)
        })
        .text_size(px(10.))
        .aria_label(format!("Viewed {label}"))
        .aria_toggled(viewed.into())
        // a tick is not also a click on the row
        .occlude()
        .on_click(tick)
        .child(if viewed { "✓" } else { "" });
    let mut press = div()
        .id(id(format!("forge-file-{label}-open")))
        .flex_1()
        .min_w(px(0.))
        .self_stretch()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .when(selected, |press| {
            press.aria_current(view::accesskit::AriaCurrent::True)
        })
        .on_click(pick)
        .child(
            design::mono(label.clone())
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_size(design::text::CAPTION)
                .text_color(if viewed {
                    theme.muted
                } else {
                    theme.foreground
                }),
        );
    if comments > 0 {
        press = press.child(badge(
            id(format!("forge-file-comments-{label}")),
            comments.to_string(),
            ladder,
            theme.muted,
            theme.surface_raised,
        ));
    }
    press = press.child(
        div()
            .flex()
            .gap(design::space::XXS)
            .font_family(design::fonts::FAMILY_MONO)
            .text_size(design::text::CAPTION)
            .when(file.additions > 0, |counts| {
                counts.child(
                    div()
                        .text_color(theme.success)
                        .child(format!("+{}", file.additions)),
                )
            })
            .when(file.deletions > 0, |counts| {
                counts.child(
                    div()
                        .text_color(theme.danger)
                        .child(format!("−{}", file.deletions)),
                )
            }),
    );
    // the tick's cell beside the press's, as a button may not hold it
    // as wide as the list, so a long path is cut and its counts stay in view
    let row = div()
        .id(id(format!("forge-file-{label}")))
        .w_full()
        .h(px(FILE_ROW_H))
        .px(design::space::BLOCK)
        .flex()
        .items_center()
        .gap(design::space::SM)
        .when(selected, |row| row.bg(theme.surface_raised))
        .when(walked && active.is_some() && !selected, |row| {
            row.bg(theme.surface)
        })
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Row)
        .child(
            div()
                .id(id(format!("forge-file-{label}-tick")))
                .flex_none()
                .role(Role::GridCell)
                .child(design::item(check, Role::CheckBox, active == Some(1))),
        )
        .child(
            div()
                .id(id(format!("forge-file-{label}-open-cell")))
                .flex_1()
                .min_w(px(0.))
                .self_stretch()
                .flex()
                .role(Role::GridCell)
                .child(design::item(press, Role::Button, active == Some(0))),
        );
    row.into_any_element()
}

/// The line comments on each file: my review's drafts plus the published.
fn file_comments(forge: &Forge) -> BTreeMap<Vec<u8>, usize> {
    let drafts = forge
        .review()
        .into_iter()
        .flat_map(|review| review.comments.iter().map(|comment| &comment.path));
    let landed = forge
        .change()
        .into_iter()
        .flat_map(|(_, _, _, reviews)| reviews.items.iter())
        .flat_map(|review| review.draft.comments.iter().map(|comment| &comment.path));
    let mut counts = BTreeMap::new();
    for path in drafts.chain(landed) {
        *counts.entry(path.clone()).or_default() += 1;
    }
    counts
}

/// The strip above the diff: what a review is, or where this one stands.
fn strip(theme: &Theme) -> Stateful<Div> {
    div()
        .id(id("forge-review-bar"))
        .flex()
        .items_center()
        .gap(design::space::MD)
        .px(design::space::BLOCK)
        .h(px(36.))
        .flex_none()
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
}

/// No review under way: how to comment, and Start review.
fn idle_review_bar(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let start = cx.listener(|forge, _: &ClickEvent, _, cx| forge.start_review(cx));
    strip(theme)
        .child(quiet(
            "Comment on any line by clicking its gutter number.",
            theme,
        ))
        .child(div().flex_1())
        .child(
            button(
                id("forge-start-review"),
                words::START_REVIEW,
                ladder,
                theme,
                start,
            )
            .kind(design::Kind::Primary)
            .enabled(forge.may_write()),
        )
        .into_any_element()
}

/// Start review → staged comments → finish as exactly one operation. The
/// finish form opens as a panel under its button.
fn review_bar(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(review) = forge.review() else {
        return idle_review_bar(forge, cx, theme, ladder);
    };
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_review(cx));
    let finishing = cx.listener(|forge, _: &ClickEvent, _, cx| forge.finishing(true, cx));
    let pending = match review.comments.len() {
        1 => "1 line comment pending".to_owned(),
        n => format!("{n} line comments pending"),
    };
    let opener = button(
        id("forge-finish-review"),
        "Finish review ⌄",
        ladder,
        theme,
        finishing,
    )
    .kind(design::Kind::Primary)
    .enabled(forge.may_write());
    // the form is a dialog over the button: Tab stays in it, Escape or a
    // press outside folds it. Its layer puts the form's anchor on the
    // button's right edge, where it hangs from
    let panel = review
        .finishing
        .then(|| finish_panel(forge, review, cx, theme, ladder));
    let fold = cx.listener(|forge, _: &(), _, cx| forge.finishing(false, cx));
    let finish = div().relative().flex_none().child(
        modal_overlay(id("forge-finish"), words::FINISH_YOUR_REVIEW, opener, panel)
            .flex_col()
            .items_end()
            .on_dismiss(fold),
    );
    let bar = strip(theme)
        .child(badge(
            id("forge-review-state"),
            "Reviewing",
            ladder,
            theme.muted,
            theme.surface_raised,
        ))
        .child(quiet(
            format!("pinned at {} · {pending}", short_hex(&review.commit)),
            theme,
        ))
        .child(div().flex_1())
        .child(
            button(id("forge-cancel-review"), "Discard", ladder, theme, cancel)
                .kind(design::Kind::Quiet),
        )
        .child(finish);
    let mut column = div().flex().flex_col().child(bar);
    if let Some(refused) = review.refused {
        column = column.child(
            div()
                .id(id("forge-review-error"))
                .px(design::space::BLOCK)
                .py(design::space::XS)
                .text_size(design::text::SECONDARY)
                .text_color(theme.danger)
                .child(words::review_refusal(refused)),
        );
    }
    column.into_any_element()
}

/// The finish form, over the diff under its button: what the review says,
/// its verdict, Submit review. A press outside folds it; nothing typed is
/// lost. It hangs from the point its overlay's layer puts it at.
fn finish_panel(
    forge: &Forge,
    review: &crate::state::ReviewSession,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let fold = cx.listener(|forge, _: &MouseDownEvent, _, cx| forge.finishing(false, cx));
    let verdict = review.verdict.unwrap_or(Verdict::Comment);
    let submit = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.finish_review(verdict, cx));
    let panel = div()
        .id(id("forge-finish-panel"))
        .w(FINISH_W)
        .flex()
        .flex_col()
        .gap(design::space::LG)
        .p(design::space::BLOCK)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .shadow_lg()
        .occlude()
        .on_mouse_down_out(fold)
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(words::FINISH_YOUR_REVIEW),
        )
        .child(review_body(&review.body, theme))
        .child(verdicts(forge, verdict, cx, theme))
        .child(
            div().flex().justify_end().child(
                button(
                    id("forge-submit-review"),
                    words::SUBMIT_REVIEW,
                    ladder,
                    theme,
                    submit,
                )
                .kind(design::Kind::Primary)
                .enabled(forge.may_write()),
            ),
        );
    deferred(
        anchored()
            .anchor(Anchor::TopRight)
            .position_mode(AnchoredPositionMode::Local)
            .position(Point {
                x: px(0.),
                y: px(0.),
            })
            .offset(Point {
                x: px(0.),
                y: design::size::CONTROL + design::space::XXS,
            })
            .snap_to_window_with_margin(view::Edges::all(design::space::SM))
            .child(panel),
    )
    .into_any_element()
}

/// What the review says overall, typed while finishing.
fn review_body(body: &TextField, theme: &Theme) -> impl IntoElement + use<> {
    Textarea::new(id("forge-review-body"), body, words::REVIEW_BODY)
        .min_h(design::size::CONTROL * 2.5)
        .w_full()
        .px_2()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_color(theme.foreground)
        .placeholder(words::REVIEW_PROMPT)
}

/// The three verdicts as radio rows, each with what it means.
fn verdicts(
    forge: &Forge,
    picked: Verdict,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
    use words::VERDICTS;
    let theme = *theme;
    let active = VERDICTS
        .iter()
        .position(|(verdict, _, _)| *verdict == picked)
        .unwrap_or_default();
    // a radio group: one Tab stop, and ↑ ↓ check the next verdict
    let mut column = design::composite(id("forge-verdicts"), Role::RadioGroup, "Verdict")
        .active(active, VERDICTS.len())
        .wrap()
        .on_move(cx.processor(|forge, index: usize, _, cx| {
            if forge.may_write() {
                forge.pick_verdict(VERDICTS[index].0, cx);
            }
        }))
        .build()
        .flex()
        .flex_col()
        .gap(design::space::SM);
    for (verdict, slug, about) in VERDICTS {
        let on = verdict == picked;
        let pick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.pick_verdict(verdict, cx));
        let dot = div()
            .size(px(14.))
            .mt(px(2.))
            .flex_none()
            .rounded_full()
            .border_1()
            .border_color(if on {
                theme.foreground
            } else {
                theme.border_strong
            })
            .when(on, |dot| dot.bg(theme.foreground));
        let row = div()
            .id(id(format!("forge-verdict-{slug}")))
            .flex()
            .items_start()
            .gap(design::space::SM)
            .aria_toggled(on.into())
            .map(|row| match forge.may_write() {
                true => row.on_click(pick),
                false => row.aria_disabled(true),
            })
            .child(dot)
            .child(
                div().flex().flex_col().child(verdict_label(verdict)).child(
                    div()
                        .text_size(design::text::SECONDARY)
                        .text_color(theme.muted)
                        .child(about),
                ),
            );
        column = column.child(design::item(row, Role::RadioButton, on));
    }
    column
}
