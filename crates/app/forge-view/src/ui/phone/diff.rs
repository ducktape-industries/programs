//! One file of a change's diff, or All files (Expo
//! `app/(tabs)/forge/[repo]/changes/[n]/diff.tsx`, board 70): the file's
//! name over the change and where the file stands among its files, ‹ ›
//! to walk them (All files: every file in one list, `N files`); the lines
//! with what readers wrote on them ([`super::lines`]); the review bar at
//! the foot.
//!
//! Reads the change's diff, its reviews and the review being written;
//! writes the review: Start, a comment at a pressed line number, Discard,
//! and Finish (the sheet [`super::review`]).
use view::Div;
use view::design::{Kind, Ladder, Nav};
use view::prelude::*;

use super::lines::{self, Notes, Shown};
use super::parts;
use crate::id;
use crate::select::path_of;
use crate::state::change_key;
use crate::words::{self, path_text};
use crate::{Forge, Stage};
use forge::{FileDiff, Reply};

pub(super) fn render(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let path = forge.nav().diff_path.clone();
    let files = diff_files(forge);
    let n = forge.nav().change.unwrap_or_default();
    let title = match forge.change() {
        Some((change, ..)) => format!("{} · #{n}", change.title),
        None => format!("#{n}"),
    };
    let key = change_key(&forge.repo_name(), n);
    let page = match &path {
        None => design::page("forge-page-file", words::ALL_FILES, nav, ladder, theme)
            .caption(format!("{title} · {} files", files.len())),
        Some(path) => {
            let at = files
                .iter()
                .position(|file| path_of(file).as_ref() == Some(path));
            let caption = match at {
                Some(at) => format!("{title} · {} of {}", at + 1, files.len()),
                None => title,
            };
            let name = path_text(path);
            let name = name.rsplit('/').next().unwrap_or_default().to_owned();
            design::page("forge-page-file", name, nav, ladder, theme)
                .caption(caption)
                .trailing(walk(
                    "forge-file-previous",
                    ("chevron-left", "Previous file"),
                    at.and_then(|at| at.checked_sub(1)),
                    &files,
                    cx,
                    ladder,
                    theme,
                ))
                .trailing(walk(
                    "forge-file-next",
                    ("chevron-right", "Next file"),
                    at.map(|at| at + 1),
                    &files,
                    cx,
                    ladder,
                    theme,
                ))
        }
    };
    page.children(parts::notices(forge, ladder, theme))
        .children(parts::pending(forge, &key, cx, ladder, theme))
        .child(body(forge, (&key, path.as_deref()), cx, ladder, theme))
        .child(review_bar(forge, cx, ladder, theme))
        .into_any_element()
}

fn diff_files(forge: &Forge) -> Vec<&FileDiff> {
    match forge.diff_query().and_then(|query| forge.ready(&query)) {
        Some(Reply::Diff { page, .. }) => page.items.iter().collect(),
        _ => Vec::new(),
    }
}

fn walk(
    key: &'static str,
    (glyph, name): (&str, &'static str),
    to: Option<usize>,
    files: &[&FileDiff],
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let next = to
        .and_then(|to| files.get(to))
        .and_then(|file| path_of(file));
    let enabled = next.is_some();
    let go = cx.listener(move |forge, _: &ClickEvent, _, cx| {
        if let Some(path) = &next {
            forge.single_file(Some(path.clone()), cx);
        }
    });
    design::icon_button(
        id(key),
        design::icon(glyph, ladder.icon),
        name,
        ladder,
        theme,
        go,
    )
    .enabled(enabled)
}

fn body(
    forge: &Forge,
    (key, path): (&str, Option<&[u8]>),
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let Some(query) = forge.diff_query() else {
        let waiting = match forge.change() {
            Some(_) => words::COMPARING_ENDPOINTS,
            None => words::READING_CHANGE,
        };
        return parts::quiet(id("forge-diff-waiting"), waiting, ladder, theme)
            .flex_1()
            .into_any_element();
    };
    let reply = match forge.stage(&query) {
        Stage::Ready(reply) => reply,
        _ => {
            let state = parts::stage(
                forge,
                &query,
                "forge-diff",
                words::READING_DIFF,
                cx,
                (ladder, theme),
            )
            .err();
            return div().flex_1().children(state).into_any_element();
        }
    };
    let Reply::Diff { page, .. } = reply else {
        return div().flex_1().into_any_element();
    };
    let files: Vec<&FileDiff> = page
        .items
        .iter()
        .filter(|file| path.is_none() || path_of(file).as_deref() == path)
        .collect();
    let notes = notes(forge);
    let shown = Shown {
        key: "forge-diff",
        page: lines::page_of((key, path)),
        rows: lines::diff_rows(&files, &notes),
        head: None,
        wrapped: false,
    };
    lines::block(shown, forge, cx, ladder, theme)
}

fn notes(forge: &Forge) -> Notes {
    let published = forge.published_comments();
    let review = forge.review();
    let staged = review
        .map(|review| {
            review
                .comments
                .iter()
                .map(|comment| {
                    (
                        (comment.path.clone(), comment.new_side, comment.line),
                        comment.body.text(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let open = review
        .and_then(|review| review.open.as_ref())
        .map(|open| (open.path.clone(), open.new_side, open.line));
    Notes {
        commentable: forge.me_principal().is_some() && !parts::sending(forge),
        published,
        staged,
        open,
    }
}

fn review_bar(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Div {
    let bar = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .py(design::space::MD)
        .border_t_1()
        .border_color(theme.border);
    let said = |text: String| {
        div()
            .flex_1()
            .min_w(px(0.))
            .text_type(ladder.secondary)
            .text_color(theme.muted)
            .child(text)
    };
    let Some(review) = forge.review() else {
        let start = cx.listener(|forge, _: &ClickEvent, _, cx| forge.start_review(cx));
        let startable = forge.me_principal().is_some() && forge.reviewable();
        return bar
            .child(said(
                "Comment on any line by pressing its number.".to_owned(),
            ))
            .child(
                design::button(
                    id("forge-start-review"),
                    words::START_REVIEW,
                    ladder,
                    theme,
                    start,
                )
                .kind(Kind::Outline)
                .enabled(startable),
            );
    };
    let discard = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_review(cx));
    let finish = cx.listener(|forge, _: &ClickEvent, _, cx| forge.finishing(true, cx));
    bar.child(said(format!(
        "Reviewing · {} staged",
        review.comments.len()
    )))
    .child(
        design::button(
            id("forge-discard-review"),
            "Discard",
            ladder,
            theme,
            discard,
        )
        .kind(Kind::Quiet)
        .enabled(!parts::sending(forge)),
    )
    .child(
        design::button(
            id("forge-finish-review"),
            "Finish review",
            ladder,
            theme,
            finish,
        )
        .kind(Kind::Outline)
        .enabled(!parts::sending(forge)),
    )
}
