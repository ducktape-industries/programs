//! Code: a file tree beside one file's bytes. A directory opens its
//! children inline beneath it; a text file is drawn highlighted with a
//! numbered gutter, a markdown file rendered; a binary or oversize blob is
//! the header the program returned and nothing else.
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;

use view::design::{space, text};
use view::prelude::*;
use view::{Div, Stateful};

use crate::Forge;
use crate::tree::{Key, Row, Slot};
use crate::ui::components::{button, empty_state, heading, id, path_text, quiet};
use crate::ui::staged;
use crate::words;
use forge::{BlobView, Content, EntryKind, Query, Reply};
use markdown::desktop::source_line;
use view::design::Ladder;

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let mut columns = div().id(id("forge-code")).flex().flex_1().min_h(px(0.));
    // wide, the tree stands beside the file; narrow, an open file takes
    // its place until closed (the rail's toggle is the rail's alone)
    if !forge.layout.narrow() || forge.nav().blob.is_none() {
        columns = columns
            .child(tree(forge, cx, theme, ladder))
            .child(design::divider(
                id("forge-files-resize"),
                "Resize the file tree",
                theme,
                cx,
                |forge: &mut Forge, dx| {
                    forge.layout.files += dx;
                    forge.layout.clamp();
                },
            ));
    }
    columns
        .child(body(forge, cx, theme, ladder))
        .into_any_element()
}

/// How far each tree level indents its rows.
const INDENT_STEP: f32 = 14.;
/// The column a row's expand/kind glyph sits in.
const GLYPH: Pixels = px(12.);

fn tree(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let column = tree_column(forge, theme);
    let Some(query) = forge.tree_query(Vec::new()) else {
        // `refs()` lands (even empty) before `head_oid()` ever resolves for
        // a repo with no commits: without this, a freshly created repo sat
        // on "Resolving the ref…" forever instead of saying so.
        let waiting = match forge.refs() {
            Some(_) => no_commits(forge, theme, ladder),
            None => quiet(words::RESOLVING_REF, theme).into_any_element(),
        };
        return column.child(waiting).into_any_element();
    };
    if let Err(state) = staged(
        forge,
        &query,
        "forge-tree-list",
        words::READING_TREE,
        cx,
        theme,
        ladder,
    ) {
        return column.child(state).into_any_element();
    }
    let rows = forge.tree_rows();
    if rows.is_empty() {
        return column
            .child(empty_state(
                id("forge-tree-empty"),
                "folder-open",
                words::NOTHING_HERE,
                words::NO_FILE_KEPT,
                ladder,
                theme,
            ))
            .into_any_element();
    }
    column
        .child(tree_rows(forge, rows, cx, theme))
        .into_any_element()
}

/// The tree's column and its filter field.
fn tree_column(forge: &Forge, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id("forge-tree"))
        .w(px(forge.layout.files))
        .flex_none()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(
            div().id(id("forge-tree-header")).p_2().child(
                Input::new(
                    id("forge-tree-search"),
                    &forge.tree_search,
                    "Filter the file tree",
                )
                .h(design::size::ROW)
                .w_full()
                .px_2()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.surface)
                .text_color(theme.foreground)
                .placeholder(words::FILTER_FILES),
            ),
        )
}

/// What marks a row: the open file, the keyboard cursor, open folders.
struct Marks {
    open: Option<Vec<u8>>,
    /// where the keys put the cursor, drawn
    cursor: Option<Vec<u8>>,
    /// the row the keys start from, claimed for them
    claimed: Option<Vec<u8>>,
    expanded: BTreeSet<Vec<u8>>,
}

/// The rows, drawn whole or as a virtual list, under the tree's keys.
fn tree_rows(forge: &Forge, rows: Vec<Row>, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    let pressed = cx.listener(|forge, event: &KeyDownEvent, _, cx| {
        // unmodified keys only: a chord is the shell's or the editor's
        if event.keystroke.modifiers.modified() {
            return;
        }
        if let Some(key) = Key::parse(&event.keystroke.key) {
            forge.tree_key(key, cx);
        }
    });
    // the tree claims a row before any key: the cursor, else the open
    // file, else the first entry, which is where `tree_key` starts too.
    // The cursor is drawn once the arrows walk the tree: a press moves it
    // (the keys go on from there) and lights nothing, and at rest nothing
    // is lit, as the tree cannot see whether it holds the focus
    let open = forge.nav().blob.as_ref().map(|(path, _)| path.clone());
    let claimed = forge
        .nav()
        .cursor
        .clone()
        .or_else(|| open.clone())
        .or_else(|| {
            rows.iter()
                .find(|row| matches!(row.slot, Slot::Entry { .. }))
                .map(|row| row.path.clone())
        });
    let marks = Marks {
        open,
        cursor: forge
            .tree_walked
            .then(|| forge.nav().cursor.clone())
            .flatten(),
        claimed,
        expanded: forge.nav().expanded.clone(),
    };
    let theme = *theme;
    let press = Rc::new(cx.listener(|forge, row: &Row, _, cx| match &row.slot {
        Slot::Entry { .. } if row.is_dir() => forge.toggle_dir(row.path.clone(), cx),
        Slot::Entry { oid, .. } => forge.open_file(row.path.clone(), oid.clone(), cx),
        Slot::Loading | Slot::Failed(_) => {}
    }));
    let count = rows.len();
    let paint = move |index: usize| tree_row(&rows[index], &marks, press.clone(), &theme);
    // a virtual list, whose scroll handle keeps the keyboard cursor in view
    let list = uniform_list(
        id("forge-tree-list"),
        count,
        move |range: Range<usize>, _, _| range.map(&paint).collect::<Vec<_>>(),
    )
    .track_scroll(&forge.tree_scroll)
    .flex_1()
    .min_h(px(0.));
    div()
        .id(id("forge-tree-rows"))
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .pb_2()
        .role(Role::Tree)
        .aria_label("Files")
        .focusable()
        .on_key_down(pressed)
        .child(list)
        .into_any_element()
}

/// One tree row: an entry, or a folder still reading or refused.
fn tree_row(
    entry: &Row,
    marks: &Marks,
    press: Rc<impl Fn(&Row, &mut Window, &mut App) + 'static>,
    theme: &Theme,
) -> AnyElement {
    let indent = space::SM + px(entry.depth as f32 * INDENT_STEP);
    let kind = match &entry.slot {
        Slot::Entry { kind, .. } => *kind,
        Slot::Loading | Slot::Failed(_) => {
            let text = match &entry.slot {
                Slot::Failed(sentence) => sentence.clone(),
                _ => words::READING.to_owned(),
            };
            // a row's height, so a virtual tree measures one and knows all
            return div()
                .min_h(design::size::ROW)
                .flex()
                .items_center()
                .pl(indent + GLYPH + space::XS)
                .child(quiet(text, theme))
                .into_any_element();
        }
    };
    let is_dir = kind == EntryKind::Directory;
    let expanded = is_dir && marks.expanded.contains(&entry.path);
    let glyph = match kind {
        EntryKind::Directory if expanded => "▾",
        EntryKind::Directory => "▸",
        EntryKind::Gitlink => "◆",
        EntryKind::Symlink => "↪",
        EntryKind::Executable | EntryKind::File => "",
    };
    let row = entry.clone();
    // A row is pressed, never focused: the list holds focus, so Enter
    // reaches the tree's key handler alone and not a focused row too.
    let selected = marks.open.as_deref() == Some(&entry.path[..]);
    let cursor = marks.cursor.as_deref() == Some(&entry.path[..]);
    let claimed = marks.claimed.as_deref() == Some(&entry.path[..]);
    let mut line = div()
        .id(id(format!("forge-tree-{}", path_text(&entry.path))))
        .w_full()
        .flex()
        .items_center()
        .gap_1()
        .min_h(design::size::ROW)
        .pl(indent)
        .pr_2()
        .role(Role::TreeItem)
        // the entry's name alone: the glyph is drawn, `aria_expanded` says it
        .aria_label(entry.name.clone())
        .aria_level(entry.depth + 1)
        .aria_selected(selected)
        .hover(|style| style.bg(theme.hover))
        .on_click(move |_: &ClickEvent, window: &mut Window, app: &mut App| {
            press(&row, window, app)
        })
        .child(
            div()
                .w(GLYPH)
                .flex_none()
                .text_size(text::CAPTION)
                .text_color(theme.muted)
                .child(glyph),
        )
        .child(div().flex_1().truncate().child(entry.name.clone()));
    if is_dir {
        line = line.aria_expanded(expanded);
    }
    if claimed {
        line = line.aria_active_descendant();
    }
    if selected {
        line = line.bg(theme.accent_soft);
    } else if cursor {
        line = line.bg(theme.hover);
    }
    line.into_any_element()
}

pub(crate) fn no_commits(forge: &Forge, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let address = forge.repo_link(&forge.repo_name());
    empty_state(
        id("forge-tree-no-commits"),
        "git-commit-horizontal",
        words::NO_COMMITS_YET,
        format!("Push code to this ref to browse it here: `git push {address} main`."),
        ladder,
        theme,
    )
    .into_any_element()
}

fn body(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let Some((path, oid)) = forge.nav().blob.clone() else {
        return div()
            .id(id("forge-code-blank"))
            .flex_1()
            .child(empty_state(
                id("forge-code-empty"),
                "file-text",
                "Pick a file",
                "Open a folder in the tree to see what it holds; press a file to read it here.",
                ladder,
                theme,
            ))
            .into_any_element();
    };
    let pane = div()
        .id(id("forge-blob"))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .min_h(px(0.))
        .child(blob_header(&path, &oid, cx, theme, ladder));
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid: oid.clone(),
        range: None,
    };
    let reply = match staged(
        forge,
        &query,
        "forge-blob",
        words::READING_FILE,
        cx,
        theme,
        ladder,
    ) {
        Ok(reply) => reply,
        Err(state) => return pane.child(state).into_any_element(),
    };
    let Reply::Blob { blob, .. } = reply else {
        return pane.into_any_element();
    };
    pane.child(blob_content(forge, &path, &oid, blob, cx, theme, ladder))
        .into_any_element()
}

/// The open file's path, id and close button.
fn blob_header(
    path: &[u8],
    oid: &str,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.nav_close_blob(cx);
    });
    div()
        .id(id("forge-blob-header"))
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .child(heading(id("forge-blob-title"), path_text(path), 2, theme))
        .child(quiet(crate::ui::components::short_hex(oid), theme))
        .child(div().flex_1())
        .child(button(
            id("forge-blob-close"),
            "Close",
            ladder,
            theme,
            close,
        ))
}

/// A blob as its content reads: markdown rendered, text highlighted, or
/// the header the program returned for anything else.
fn blob_content(
    forge: &Forge,
    path: &[u8],
    oid: &str,
    blob: &BlobView,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let name = path_text(path);
    let dir = path[..path.iter().rposition(|b| *b == b'/').unwrap_or(0)].to_vec();
    match blob.content {
        Content::Text if is_markdown(&name) => div()
            .id(id("forge-blob-doc"))
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px_5()
            .py_4()
            .child(markdown::desktop::render_blocks(
                "forge-blob-markdown",
                &forge.blob_cache.doc(oid, &blob.bytes),
                theme,
                &crate::markdown::links(dir, cx),
            ))
            .into_any_element(),
        Content::Text => lines(forge.blob_cache.lines(oid, &name, &blob.bytes), theme),
        Content::Binary => empty_state(
            id("forge-blob-binary"),
            "file-question-mark",
            words::BINARY_FILE,
            words::not_text(blob.size),
            ladder,
            theme,
        )
        .into_any_element(),
        Content::Oversize => empty_state(
            id("forge-blob-oversize"),
            "file-question-mark",
            words::TOO_LARGE_TO_SHOW,
            words::too_large(blob.size),
            ladder,
            theme,
        )
        .into_any_element(),
        Content::Gitlink => empty_state(
            id("forge-blob-gitlink"),
            "file-question-mark",
            "Submodule",
            words::SUBMODULE_POINTS_AWAY,
            ladder,
            theme,
        )
        .into_any_element(),
    }
}

fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// One digit's advance in [`design::fonts::FAMILY_MONO`], in ems: the
/// font's published metric, since no text measurement reaches a view. The
/// gutter is sized from it before any line is laid out.
const MONO_DIGIT_EM: f32 = 0.6;

/// Source lines, highlighted, numbered in a mono gutter.
fn lines(lines: Rc<crate::blob::Lines>, theme: &Theme) -> AnyElement {
    let count = lines.rows.len();
    let digit = design::type_scale::SECONDARY as f32 * MONO_DIGIT_EM;
    let gutter = px(count.to_string().len() as f32 * digit) + space::BLOCK; // plus its padding and edge
    let theme = *theme;
    crate::ui::components::rows("forge-blob-lines", count, None, None, move |index| {
        div()
            .id(id(format!("forge-blob-line-{}", index + 1)))
            .flex()
            .gap_3()
            // the line's number is its marker, not its code: a drag down
            // the file copies the code alone
            .child(
                div()
                    .id(id(format!("forge-blob-number-{}", index + 1)))
                    .w(gutter)
                    .flex_none()
                    .role(Role::ListMarker)
                    .pr_2()
                    .flex()
                    .justify_end()
                    .border_r_1()
                    .border_color(theme.border)
                    .font_family(design::fonts::FAMILY_MONO)
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.faint)
                    .child((index + 1).to_string()),
            )
            .child(source_line(
                id(format!("forge-blob-text-{}", index + 1)),
                &lines.rows[index],
                &lines.tokens[index],
                &theme,
            ))
            .into_any_element()
    })
}

/// The full path of an entry inside the directory on screen.
pub(crate) fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    if dir.is_empty() {
        return name.to_vec();
    }
    let mut path = dir.to_vec();
    path.push(b'/');
    path.extend_from_slice(name);
    path
}
