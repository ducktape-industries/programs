//! A folder or a file of the browsed ref (Expo
//! `app/(tabs)/forge/[repo]/code/[...path].tsx`, boards 62, 63). A folder:
//! `name/` over its parent and the ref, the crumbs and the ref over its
//! rows. A file: its name over its folder and the ref, Copy the path and
//! Copy link in the title row; `lang · N lines · size` with Wrap (and
//! Source for markdown) over its lines, or the markdown rendered; why a
//! file that is not text shows nothing.
//!
//! Reads the open folder (`Nav::folder`), blob (`Nav::blob`) and the
//! path a link is landing on (`Nav::goto`); writes
//! the clipboard, the page's wrap and source toggles, and the folder a
//! crumb goes back to.
use view::Div;
use view::design::{Kind, Ladder, Nav};
use view::prelude::*;

use super::lines::{self, Shown};
use super::parts;
use super::tabs;
use crate::Forge;
use crate::id;
use crate::words::{self, path_text, ref_label};
use forge::{BlobView, Content, Query, Reply};
use markdown::highlight;
use markdown::phone as document;

pub(super) fn render(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let (path, oid) = forge.nav().blob.clone().unwrap_or_default();
    let path = path_text(&path);
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", &path));
    file_page("forge-page-blob", &path, forge, nav, cx, (ladder, theme))
        .child(body(forge, (&oid, dir, name), cx, ladder, theme))
        .into_any_element()
}

pub(super) fn missing(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let gone = crate::ui::code::gone(forge, cx, theme, ladder);
    if let (Some((_, path)), Some(gone)) = (forge.nav().gone.clone(), gone) {
        return file_page("forge-page-missing", &path, forge, nav, cx, (ladder, theme))
            .child(gone)
            .into_any_element();
    }
    let path = forge.nav().missing.clone().unwrap_or_default();
    file_page(
        "forge-page-missing",
        &path_text(&path),
        forge,
        nav,
        cx,
        (ladder, theme),
    )
    .child(design::empty_state(
        id("forge-missing-empty"),
        "file-question",
        words::no_path(&path),
        words::NOT_ON_THIS_REF,
        ladder,
        theme,
    ))
    .into_any_element()
}

pub(super) fn landing(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let path = match &forge.nav().linked {
        Some(segments) => segments.join("/").into_bytes(),
        None => forge.nav().goto.clone().unwrap_or_default(),
    };
    let dir = forge.nav().folder.clone().unwrap_or_default();
    let reading = match forge.tree_query(dir) {
        Some(tree) => parts::stage(
            forge,
            &tree,
            "forge-landing",
            words::READING_TREE,
            cx,
            (ladder, theme),
        )
        .err(),
        None => Some(
            parts::quiet(
                id("forge-landing-resolving"),
                words::RESOLVING_REF,
                ladder,
                theme,
            )
            .into_any_element(),
        ),
    };
    file_page(
        "forge-page-landing",
        &path_text(&path),
        forge,
        nav,
        cx,
        (ladder, theme),
    )
    .children(reading)
    .into_any_element()
}

fn file_page(
    key: &'static str,
    path: &str,
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    (ladder, theme): (&Ladder, &Theme),
) -> design::Page {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    let repo = forge.repo_name();
    let folder = if dir.is_empty() { repo.as_str() } else { dir };
    let caption = format!("{folder} · {}", ref_label(&forge.head_name()));
    let route = forge.blob_route(path);
    let segments: Vec<&str> = std::iter::once(repo.as_str())
        .chain(route.iter().map(String::as_str))
        .collect();
    let link = forge.link(forge::PROGRAM, &segments);
    design::page(key, name.to_owned(), nav, ladder, theme)
        .caption(caption)
        .trailing(parts::copy_button(
            "forge-blob-copy-path",
            ("copy", "Copy the path"),
            Some(path.to_owned()),
            forge,
            cx,
            (ladder, theme),
        ))
        .trailing(parts::copy_button(
            "forge-blob-copy-link",
            ("link", "Copy link"),
            link,
            forge,
            cx,
            (ladder, theme),
        ))
        .children(parts::notices(forge, ladder, theme))
}

pub(super) fn folder(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let path = forge.nav().folder.clone().unwrap_or_default();
    let shown = path_text(&path);
    let (dir, name) = shown.rsplit_once('/').unwrap_or(("", &shown));
    let repo = forge.repo_name();
    let rev = ref_label(&forge.head_name());
    let above = if dir.is_empty() { repo.as_str() } else { dir };
    let caption = format!("{above} · {rev}");
    let bar = parts::bar(
        "forge-folder-bar",
        crumbs(&repo, &shown, cx, ladder, theme),
        parts::caption(rev, ladder, theme),
        ladder,
        theme,
    );
    design::page("forge-page-folder", format!("{name}/"), nav, ladder, theme)
        .caption(caption)
        .children(parts::notices(forge, ladder, theme))
        .child(tabs::folder_body(forge, path, bar, cx, ladder, theme))
        .into_any_element()
}

fn crumbs(repo: &str, path: &str, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Div {
    let parts: Vec<&str> = path.split('/').collect();
    let depth = parts.len();
    let mut line = div()
        .flex()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.secondary)
        .whitespace_nowrap();
    for (at, part) in std::iter::once(repo)
        .chain(parts.iter().copied())
        .enumerate()
    {
        let said = format!("{}{part}", if at == 0 { "" } else { " / " });
        if at == depth {
            line = line.child(div().text_color(theme.foreground).child(said));
            continue;
        }
        let up = (at > 0).then(|| parts[..at].join("/").into_bytes());
        let press =
            cx.listener(move |forge, _: &ClickEvent, _, cx| forge.open_folder(up.clone(), cx));
        line = line.child(
            div()
                .id(id(format!("forge-crumb-{at}")))
                .text_color(theme.muted)
                .role(Role::Button)
                .focusable()
                .on_click(press)
                .child(said),
        );
    }
    line
}

fn body(
    forge: &Forge,
    (oid, dir, name): (&str, &str, &str),
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid: oid.to_owned(),
        range: None,
    };
    let blob = match parts::stage(
        forge,
        &query,
        "forge-blob",
        words::READING_FILE,
        cx,
        (ladder, theme),
    ) {
        Ok(Reply::Blob { blob, .. }) => blob,
        Ok(_) => return div().into_any_element(),
        Err(state) => return state,
    };
    if let Some(none) = unreadable(blob, ladder, theme) {
        return none;
    }
    let markdown = highlight::language_of(name) == Some("markdown");
    let shown = forge.blob_cache.lines(oid, name, &blob.bytes);
    let facts = [
        highlight::language_of(name).map(str::to_owned),
        Some(design::plural(shown.rows.len() as u64, "line", "lines")),
        Some(parts::size_of(blob.size)),
    ];
    let facts = facts.into_iter().flatten().collect::<Vec<_>>().join(" · ");
    let bar = parts::bar(
        "forge-blob-bar",
        parts::caption(facts, ladder, theme).text_type(ladder.secondary),
        toggles(forge, markdown, cx, ladder, theme),
        ladder,
        theme,
    );
    let content = match markdown && !forge.source {
        true => {
            let blocks = forge.blob_cache.doc(oid, &blob.bytes);
            let links = crate::markdown::links(dir.as_bytes().to_vec(), cx);
            div()
                .id(id("forge-blob-document"))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p(design::space::BLOCK)
                .child(document::markdown(
                    "forge-blob-doc",
                    &blocks,
                    &links,
                    ladder,
                    theme,
                ))
                .into_any_element()
        }
        false => {
            let shown = Shown {
                key: "forge-blob-lines",
                page: lines::page_of(oid),
                rows: lines::file_lines(&shown.rows, &shown.tokens),
                head: None,
                wrapped: forge.wrap,
            };
            lines::block(shown, forge, cx, ladder, theme)
        }
    };
    div()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(bar)
        .child(content)
        .into_any_element()
}

fn unreadable(blob: &BlobView, ladder: &Ladder, theme: &Theme) -> Option<AnyElement> {
    let (title, body) = match blob.content {
        Content::Text => return None,
        Content::Binary => (words::BINARY_FILE, words::not_text(blob.size)),
        Content::Oversize => (words::TOO_LARGE_TO_SHOW, words::too_large(blob.size)),
        Content::Gitlink => ("Submodule", words::SUBMODULE_POINTS_AWAY.to_owned()),
    };
    Some(
        design::empty_state(
            id("forge-blob-unreadable"),
            "file-question",
            title,
            body,
            ladder,
            theme,
        )
        .into_any_element(),
    )
}

fn toggles(
    forge: &Forge,
    markdown: bool,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let ink = |on: bool| if on { Kind::Primary } else { Kind::Outline };
    let source = markdown.then(|| {
        let flip = cx.listener(|forge, _: &ClickEvent, _, cx| {
            forge.source = !forge.source;
            cx.notify();
        });
        design::button(id("forge-source"), "Source", ladder, theme, flip)
            .small()
            .kind(ink(forge.source))
    });
    let wrap = (!markdown || forge.source).then(|| {
        let flip = cx.listener(|forge, _: &ClickEvent, _, cx| {
            forge.wrap = !forge.wrap;
            cx.notify();
        });
        design::button(id("forge-wrap"), "Wrap", ladder, theme, flip)
            .small()
            .kind(ink(forge.wrap))
    });
    div()
        .flex()
        .gap(design::space::SM)
        .children(source)
        .children(wrap)
}
