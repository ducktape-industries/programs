//! A change's details, one sidebar beside its conversation: who reviewed
//! it and how, what stands between it and its target ref, every line
//! comment in one place, and the channel it talks in.
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::Forge;
use crate::select::{Picture, Standings};
use crate::ui::components::{badge, id, path_text, quiet, short_hex};
use crate::words;
use forge::{Mergeability, Verdict};
use view::design::Ladder;

/// The sidebar's width.
const SIDEBAR_W: Pixels = px(280.);
/// A fact's label column in Merge status.
const LABEL_W: Pixels = px(92.);

pub(crate) fn sidebar(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let column = div()
        .id(id("forge-dock"))
        .w(SIDEBAR_W)
        .flex_none()
        .flex()
        .flex_col()
        .min_h(px(0.))
        .overflow_y_scroll()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.background);
    let Some((change, _, _, _)) = forge.change() else {
        return column.child(quiet(words::READING_CHANGE, theme));
    };
    let who = crate::policies::change_row(forge, cx, ladder, theme)
        .map(|row| row.border_b_1().border_color(theme.border));
    column
        .children(who)
        .child(reviews(forge, cx, theme, ladder))
        .child(merge_status(forge, theme))
        .child(comments(forge, cx, theme))
        .child(
            section("forge-channel", "Channel", None, theme).child(
                div()
                    .id(id("forge-channel-name"))
                    .child(words::channel(&change.channel)),
            ),
        )
}

/// A sidebar section: its title (and a quiet note on the right), a
/// hairline under the whole of it.
fn section(key: &str, title: &str, note: Option<String>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id(key.to_owned()))
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .px(design::space::BLOCK)
        .py(design::space::LG)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .items_baseline()
                .mb(design::space::XXS)
                .child(
                    div()
                        .id(id(format!("{key}-title")))
                        .flex_1()
                        .text_size(design::text::SECONDARY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child(title.to_owned()),
                )
                .children(note.map(|note| {
                    design::mono(note)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted)
                })),
        )
}

/// Each reviewer's latest verdict, then who was asked and has not
/// answered. Approvals are advisory: the merge is a compare-and-set on
/// both heads.
fn reviews(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let mut section = section(
        "forge-reviews",
        "Reviews",
        Some(words::ADVISORY.into()),
        theme,
    );
    let Some(Standings {
        answered: latest,
        asked: waiting,
    }) = forge.standings()
    else {
        return section;
    };
    if latest.is_empty() && waiting.is_empty() {
        return section.child(quiet(words::NOBODY_REVIEWED, theme));
    }
    for (index, (who, verdict)) in latest.into_iter().enumerate() {
        let label = words::standing(verdict);
        let (foreground, background) = match verdict {
            Verdict::Approve => (theme.success, theme.success_soft),
            Verdict::RequestChanges => (theme.danger, theme.danger_soft),
            Verdict::Comment => (theme.muted, theme.surface_raised),
        };
        section = section.child(person(
            forge.principal_name(who),
            forge.picture(who, cx),
            badge(
                id(format!("forge-reviewer-verdict-{index}")),
                label,
                ladder,
                foreground,
                background,
            ),
            theme,
        ));
    }
    for (index, who) in waiting.into_iter().enumerate() {
        section = section.child(person(
            forge.principal_name(who),
            forge.picture(who, cx),
            badge(
                id(format!("forge-reviewer-asked-{index}")),
                words::ASKED,
                ladder,
                theme.muted,
                theme.surface_raised,
            ),
            theme,
        ));
    }
    section
}

/// A reviewer's line: avatar, name, their verdict on the right.
fn person(name: String, picture: Picture, verdict: impl IntoElement, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(
            design::avatar_with(&name, picture, design::size::AVATAR_SM, theme)
                .border_1()
                .border_color(theme.border)
                .font_weight(FontWeight::SEMIBOLD),
        )
        .child(div().flex_1().min_w(px(0.)).truncate().child(name))
        .child(verdict)
}

/// Whether the change can merge, how far apart its endpoints are, and the
/// two heads.
fn merge_status(forge: &Forge, theme: &Theme) -> Stateful<Div> {
    let mut section = section("forge-merge-status", words::MERGE_STATUS, None, theme);
    let Some((_, source, target, _)) = forge.change() else {
        return section;
    };
    let comparison = forge.compare();
    let verdict = comparison.map_or(words::COMPARING, |c| words::merge_status(c.mergeability));
    let tone = match comparison.map(|c| c.mergeability) {
        Some(Mergeability::UpToDate) | None => theme.muted,
        Some(Mergeability::FastForward) => theme.success,
        Some(Mergeability::Diverged) => theme.warning,
        Some(Mergeability::Unrelated) => theme.danger,
    };
    section = section.child(fact(
        "Mergeability",
        div().text_color(tone).child(verdict),
        theme,
    ));
    if let Some(comparison) = comparison {
        section = section.child(fact(
            "Distance",
            design::mono(words::distance(comparison)),
            theme,
        ));
    }
    let head =
        |oid: &Option<String>| design::mono(oid.as_deref().map_or(words::GONE.into(), short_hex));
    section
        .child(fact(words::SOURCE_HEAD, head(source), theme))
        .child(fact(words::TARGET_HEAD, head(target), theme))
}

fn fact(label: &str, value: impl IntoElement, theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .min_h(px(24.))
        .child(
            div()
                .w(LABEL_W)
                .flex_none()
                .text_color(theme.muted)
                .child(label.to_owned()),
        )
        .child(div().min_w(px(0.)).child(value))
}

/// Every line comment of this change, mine pending first, each opening
/// its file in the Files tab.
fn comments(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> Stateful<Div> {
    let Some((_, _, _, reviews)) = forge.change() else {
        return section("forge-comments", words::LINE_COMMENTS, None, theme);
    };
    let mut rows = Vec::new();
    if let Some(review) = forge.review() {
        for staged in &review.comments {
            let path = staged.path.clone();
            let jump = cx.listener(move |forge, _: &ClickEvent, _, cx| {
                forge.open_file_comment(path.clone(), cx)
            });
            rows.push(comment_row(
                id(format!("forge-comment-draft-{}", staged.anchor())),
                words::pending_at(&staged.path, staged.line),
                jump,
                theme,
            ));
        }
    }
    for review in &reviews.items {
        let author = forge.principal_name(&review.author);
        for comment in &review.draft.comments {
            let path = comment.path.clone();
            let jump = cx.listener(move |forge, _: &ClickEvent, _, cx| {
                forge.open_file_comment(path.clone(), cx)
            });
            rows.push(comment_row(
                id(format!(
                    "forge-comment-{}-{}-{}",
                    review.id,
                    path_text(&comment.path),
                    comment.line
                )),
                format!("{}:{} · {author}", path_text(&comment.path), comment.line),
                jump,
                theme,
            ));
        }
    }
    let count = rows.len();
    let section = section(
        "forge-comments",
        words::LINE_COMMENTS,
        (count > 0).then(|| count.to_string()),
        theme,
    );
    match count {
        0 => section.child(quiet(words::NOBODY_ON_A_LINE, theme)),
        _ => section.children(rows),
    }
}

fn comment_row(
    element_id: ElementId,
    text: String,
    jump: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    design::mono(text)
        .id(element_id)
        .truncate()
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .hover(move |style| style.text_color(theme.foreground))
        .role(Role::Link)
        .focusable()
        .on_click(jump)
}
