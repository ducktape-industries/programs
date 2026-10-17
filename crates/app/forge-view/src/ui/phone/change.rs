//! A change (Expo `app/(tabs)/forge/[repo]/changes/[n]/index.tsx`, boards
//! 68, 69; its tab bodies in `src/forge/change.tsx`): `<title> · #n` over
//! its repository and endpoints, Actions behind ⋮ (Edit, Close, Copy link);
//! its state and who wants what; Conversation · Commits · Files ·
//! Details. The conversation ends in the Merge / review bar and the
//! reply box.
//!
//! Reads `Query::Change`, its comparison, its log (Commits), its diff
//! (Files) and its channel's rows; writes Merge, Close (asked twice), a
//! reply (`post_reply`), the files ticked as read, and starts a review.
use view::design::{Kind, Ladder, Nav};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::document;
use super::parts::{self, Row, Tint, Tone};
use super::tabs;
use crate::Forge;
use crate::actions::MergeBlock;
use crate::id;
use crate::markdown;
use crate::select::{ChangeEvent, Standings};
use crate::state::{ChangeTab, Sheet, change_key, verdict_label};
use crate::words::{self, path_text};
use forge::{Change, ChangeState, Content, FileDiff, Principal, Reply, Review, Verdict};
use view::design::short_hex;

pub(super) fn render(
    forge: &Forge,
    nav: Nav,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let repo = forge.repo_name();
    let n = forge.nav().change.unwrap_or_default();
    let (title, caption) = match forge.change() {
        Some((change, ..)) => (
            format!("{} · #{n}", change.title),
            format!("{repo} · {}", parts::endpoints(&change.from, &change.into)),
        ),
        None => (format!("#{n}"), repo.clone()),
    };
    let actions =
        cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_sheet(Sheet::ChangeActions, cx));
    let page = design::page("forge-page-change", title, nav, ladder, theme)
        .caption(caption)
        .trailing(design::icon_button(
            id("forge-change-actions"),
            design::icon("ellipsis-vertical", ladder.icon),
            words::ACTIONS,
            ladder,
            theme,
            actions,
        ))
        .children(parts::notices(forge, ladder, theme));
    let Some((change, source_head, _, _)) = forge.change() else {
        let Some(query) = forge.change_query() else {
            return page.into_any_element();
        };
        let reading = parts::stage(
            forge,
            &query,
            "forge-change",
            words::READING_CHANGE,
            cx,
            (ladder, theme),
        );
        return page.children(reading.err()).into_any_element();
    };
    page.child(state_line(
        forge,
        change,
        source_head.as_deref(),
        ladder,
        theme,
    ))
    .child(strip(forge, cx, ladder, theme))
    .children(parts::pending(
        forge,
        &change_key(&repo, n),
        cx,
        ladder,
        theme,
    ))
    .child(body(forge, change, cx, ladder, theme))
    .into_any_element()
}

/// The state badge and `<author> wants <source> → <target> · head
/// <short>` in the data face (`change.tsx` `wants`); 16 in, 12 over, 6
/// under.
fn state_line(
    forge: &Forge,
    change: &Change,
    head: Option<&str>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let author = forge.principal_name(&change.author);
    let wants = format!(
        "{author} wants {}",
        parts::endpoints(&change.from, &change.into)
    );
    let wants = match head {
        Some(head) => format!("{wants} · head {}", short_hex(head)),
        None => wants,
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .pt(design::space::LG)
        .pb(design::space::XS)
        .child(parts::state_badge(
            change.state,
            "forge-change-state",
            ladder,
            theme,
        ))
        .child(
            parts::caption(wants, ladder, theme)
                .flex_1()
                .font_family(design::fonts::FAMILY_MONO),
        )
}

// ------------------------------------------------------------------ tabs

/// A tab of the change page: one of the model's, or Details (which the
/// desktop docks beside them).
#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Of(ChangeTab),
    Details,
}

const TABS: [Tab; 4] = [
    Tab::Of(ChangeTab::Conversation),
    Tab::Of(ChangeTab::Commits),
    Tab::Of(ChangeTab::Files),
    Tab::Details,
];

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Of(tab) => tab.label(),
            Tab::Details => "Details",
        }
    }

    fn slug(self) -> &'static str {
        match self {
            Tab::Of(tab) => tab.slug(),
            Tab::Details => "details",
        }
    }

    fn open(self, forge: &mut Forge, cx: &mut Context<Forge>) {
        match self {
            Tab::Of(tab) => forge.open_change_tab(tab, cx),
            Tab::Details => forge.open_details(cx),
        }
    }

    /// What the tab holds, where the view has read it
    /// ([`Forge::change_tab_count`]); Details counts nothing.
    fn count(self, forge: &Forge) -> Option<u64> {
        match self {
            Tab::Of(tab) => forge.change_tab_count(tab),
            Tab::Details => None,
        }
    }
}

fn shown(forge: &Forge) -> Tab {
    match forge.details {
        true => Tab::Details,
        false => Tab::Of(forge.nav().change_tab),
    }
}

/// The four tabs (`Strip`), one Tab stop whose arrows open the next tab
/// as they reach it.
fn strip(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let open = shown(forge);
    let at = TABS.iter().position(|tab| *tab == open).unwrap_or(0);
    let pick = move |forge: &mut Forge, index: usize, _: &mut Window, cx: &mut Context<Forge>| {
        if let Some(tab) = TABS.get(index) {
            tab.open(forge, cx);
        }
    };
    let bar = design::composite(id("forge-change-tabs"), Role::TabList, "Change")
        .orientation(design::Orientation::Horizontal)
        .wrap()
        .active(at, TABS.len())
        .on_move(cx.processor(pick))
        .on_press(cx.processor(pick))
        .build();
    let mut bar = design::strip(bar, ladder, theme);
    for tab in TABS {
        let press = cx.listener(move |forge, _: &ClickEvent, _, cx| tab.open(forge, cx));
        bar = bar.child(parts::strip_tab(
            id(format!("forge-change-tab-{}", tab.slug())),
            tab.label(),
            tab.count(forge),
            (tab == open, tab == open),
            ladder,
            theme,
            press,
        ));
    }
    bar
}

/// The open tab's body, filling the rest of the page.
fn body(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    match shown(forge) {
        Tab::Of(ChangeTab::Conversation) => conversation(forge, change, cx, ladder, theme),
        Tab::Of(ChangeTab::Commits) => commits(forge, cx, ladder, theme),
        Tab::Of(ChangeTab::Files) => files(forge, cx, ladder, theme),
        Tab::Details => details(forge, change, cx, ladder, theme),
    }
}

// ------------------------------------------------------------------ Conversation

/// The body, the change's own events, its reviews (advisory), the
/// replies in its channel; under them, while it is open, why Merge is off
/// and the Merge / review bar; then the reply box.
fn conversation(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let open = change.state == ChangeState::Open;
    div()
        .id(id("forge-conversation-pane"))
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col()
        .child(timeline(forge, change, cx, ladder, theme))
        .children(open.then(|| merge_gate(forge, ladder, theme)).flatten())
        .children(open.then(|| review_bar(forge, cx, ladder, theme)))
        .child(composer(forge, cx, ladder, theme))
        .into_any_element()
}

fn timeline(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let reviews = forge
        .change()
        .map(|(.., reviews)| reviews.items.as_slice())
        .unwrap_or_default();
    let body = (!change.body.trim().is_empty()).then(|| {
        let blocks = markdown::parse(&change.body);
        let links = markdown::links(Vec::new(), cx);
        div().p(design::space::BLOCK).child(document::markdown(
            "forge-change-body",
            &blocks,
            &links,
            ladder,
            theme,
        ))
    });
    let reviewed = match reviews.is_empty() {
        true => vec![
            parts::quiet(
                id("forge-reviews-none"),
                words::NOBODY_REVIEWED,
                ladder,
                theme,
            )
            .into_any_element(),
        ],
        false => reviews
            .iter()
            .map(|review| review_line(forge, review, ladder, theme))
            .collect(),
    };
    div()
        .id(id("forge-conversation"))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .pb(design::space::BLOCK)
        .children(body)
        .children(events(forge, change, ladder, theme))
        .child(section_aside(
            "forge-reviews-head",
            "Reviews",
            words::ADVISORY.to_owned(),
            ladder,
            theme,
        ))
        .children(reviewed)
        .child(section_aside(
            "forge-replies-head",
            "Replies",
            words::channel(&change.channel),
            ladder,
            theme,
        ))
        .children(replies(forge, change, cx, ladder, theme))
}

/// A section head with a word at its right (`SectionHead` `right`).
fn section_aside(
    key: &str,
    label: &str,
    aside: String,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    design::section_head(id(key.to_owned()), label.to_owned(), ladder, theme).child(aside)
}

/// The change's own events from forge's lines that are no review's
/// (`eventsOf`, as [`Forge::change_event`] reads them): the first opened
/// it, a later one is how it ended.
fn events(forge: &Forge, change: &Change, ladder: &Ladder, theme: &Theme) -> Vec<AnyElement> {
    if forge.names.ready().is_none() {
        return Vec::new();
    }
    let Some(rows) = forge.conversation() else {
        return Vec::new();
    };
    let name =
        |who: Option<&Principal>| who.map(|who| forge.principal_name(who)).unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            let (who, what) = match forge.change_event(row)? {
                ChangeEvent::Opened => (
                    forge.principal_name(&change.author),
                    words::OPENED_IT.to_owned(),
                ),
                ChangeEvent::Merged { by, oid } => {
                    (name(by), words::merged_into(&change.into, oid))
                }
                ChangeEvent::Closed { by } => (name(by), words::CLOSED_IT.to_owned()),
                ChangeEvent::Reply | ChangeEvent::Review(_) => return None,
            };
            let said = div().text_color(theme.muted).child(what).into_any_element();
            let line = TimelineLine {
                key: format!("forge-event-{}", row.message_id),
                name: who,
                agent: false,
                meta: words::block(row.height),
            };
            Some(line.draw(vec![said], ladder, theme))
        })
        .collect()
}

/// A timeline line (`Line`): the avatar, the name medium (and Agent),
/// a quiet meta beside it, what it says under the name; 16 in, 8 over and
/// under, 12 between the avatar and the words.
struct TimelineLine {
    key: String,
    name: String,
    agent: bool,
    meta: String,
}

impl TimelineLine {
    fn draw(self, said: Vec<AnyElement>, ladder: &Ladder, theme: &Theme) -> AnyElement {
        let face = parts::avatar(&self.name, self.agent, ladder, theme);
        let head = div()
            .flex()
            .items_center()
            .gap(design::space::SM)
            .child(
                div()
                    .min_w(px(0.))
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(self.name),
            )
            .when(self.agent, |head| {
                head.child(parts::badge(
                    id(format!("{}-agent", self.key)),
                    "Agent",
                    Tint::Agent,
                    ladder,
                    theme,
                ))
            })
            .child(
                div()
                    .flex_none()
                    .text_type(ladder.secondary)
                    .text_color(theme.muted)
                    .child(self.meta),
            );
        div()
            .id(id(self.key))
            .flex()
            .gap(design::space::LG)
            .px(design::space::BLOCK)
            .py(design::space::SM)
            .child(face)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(design::space::HAIR)
                    .child(head)
                    .children(said),
            )
            .into_any_element()
    }
}

/// One review (`ReviewLine`): its verdict as a badge under the name (and
/// `outdated`), what it says, each line comment as `path:line — body` in
/// the data face.
fn review_line(forge: &Forge, review: &Review, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let label = verdict_label(review.draft.verdict);
    let tint = match review.draft.verdict {
        Verdict::Approve => Tint::Ok,
        Verdict::RequestChanges => Tint::Warn,
        Verdict::Comment => Tint::Plain,
    };
    let outdated = forge.outdated(&review.draft.commit_oid).then(|| {
        parts::badge(
            id(format!("forge-review-outdated-{}", review.id)),
            words::OUTDATED,
            Tint::Warn,
            ladder,
            theme,
        )
    });
    let badges = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::SM)
        .child(parts::badge(
            id(format!("forge-review-verdict-{}", review.id)),
            label,
            tint,
            ladder,
            theme,
        ))
        .children(outdated);
    let body = (!review.draft.body.trim().is_empty())
        .then(|| div().child(review.draft.body.clone()).into_any_element());
    let comments = review.draft.comments.iter().map(|comment| {
        div()
            .font_family(design::fonts::FAMILY_MONO)
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words::line_comment(
                &comment.path,
                comment.line,
                &comment.body,
            ))
            .into_any_element()
    });
    let said = std::iter::once(badges.into_any_element())
        .chain(body)
        .chain(comments)
        .collect();
    let line = TimelineLine {
        key: format!("forge-review-{}", review.id),
        name: forge.principal_name(&review.author),
        agent: forge.is_agent(&review.author),
        meta: words::block(review.height),
    };
    line.draw(said, ladder, theme)
}

/// The replies in the change's channel: what people said there, forge's
/// own lines left out; until the channel and the roster are read, the
/// quiet line; a refusal and Retry.
fn replies(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    if let Some(Loadable::Failed(refusal)) = forge.messages.get(&change.channel) {
        let retry = cx.listener(|forge, _: &ClickEvent, _, cx| forge.retry_conversation(cx));
        return vec![
            design::refused(
                "forge-conversation",
                design::sentence(refusal),
                ladder,
                theme,
                retry,
            )
            .into_any_element(),
        ];
    }
    let (Some(rows), Some(_)) = (forge.conversation(), forge.names.ready()) else {
        return vec![
            parts::quiet(
                id("forge-conversation-loading"),
                words::READING_CONVERSATION,
                ladder,
                theme,
            )
            .into_any_element(),
        ];
    };
    let said: Vec<AnyElement> = rows
        .iter()
        .filter(|row| !forge.is_forge_line(row))
        .map(|row| reply_line(forge, row, ladder, theme))
        .collect();
    match said.is_empty() {
        true => vec![
            parts::quiet(
                id("forge-conversation-empty"),
                "No replies yet. This change's channel is quiet.",
                ladder,
                theme,
            )
            .into_any_element(),
        ],
        false => said,
    }
}

fn reply_line(forge: &Forge, row: &chat::MsgRow, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let meta = match row.time {
        0 => words::block(row.height),
        time => design::clock(time),
    };
    let said = match row.deleted {
        true => "Message deleted".to_owned(),
        false => row.text.clone(),
    };
    let line = TimelineLine {
        key: format!("forge-message-{}", row.message_id),
        name: forge.principal_name(&row.author),
        agent: forge.is_agent(&row.author),
        meta,
    };
    line.draw(vec![div().child(said).into_any_element()], ladder, theme)
}

/// Why Merge is off, on the warning strip over the bar, once the change
/// has loaded (`Notice kind="warning"`).
fn merge_gate(forge: &Forge, ladder: &Ladder, theme: &Theme) -> Option<AnyElement> {
    let block = forge
        .merge_block()
        .filter(|block| *block != MergeBlock::Loading)?;
    Some(
        parts::notice(
            id("forge-merge-block"),
            block.sentence(),
            Tone::Warning,
            ladder,
            theme,
        )
        .into_any_element(),
    )
}

/// Merge (primary) and Start review, or Finish review once one is under
/// way, each half the row; 16 in, 12 over, a hairline above.
fn review_bar(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Div {
    let merge = cx.listener(|forge, _: &ClickEvent, _, cx| forge.merge(cx));
    let mergeable = forge.merge_block().is_none() && forge.writes_repo() && !parts::sending(forge);
    let me = forge.me_principal().is_some();
    let review = match forge.review() {
        Some(_) => {
            let finish = cx.listener(|forge, _: &ClickEvent, _, cx| forge.finishing(true, cx));
            design::button(
                id("forge-finish-review"),
                "Finish review",
                ladder,
                theme,
                finish,
            )
            .kind(Kind::Outline)
            .enabled(me && !parts::sending(forge))
            .into_any_element()
        }
        None => {
            let start = cx.listener(|forge, _: &ClickEvent, _, cx| {
                forge.start_review(cx);
                forge.open_change_tab(ChangeTab::Files, cx);
            });
            design::button(
                id("forge-start-review"),
                words::START_REVIEW,
                ladder,
                theme,
                start,
            )
            .kind(Kind::Outline)
            .enabled(me && forge.reviewable())
            .into_any_element()
        }
    };
    div()
        .flex()
        .flex_none()
        .gap(design::space::LG)
        .px(design::space::BLOCK)
        .pt(design::space::LG)
        .border_t_1()
        .border_color(theme.border)
        .child(parts::share(
            1.,
            design::button(id("forge-merge"), "Merge", ladder, theme, merge)
                .kind(Kind::Primary)
                .enabled(mergeable),
        ))
        .child(parts::share(1., review))
}

/// "Reply in this change" (`Composer`): a reply on its way (or one that
/// did not land) over the kit's field, many lines, beside Send, ink once
/// there is something to send and off while a reply is out; 16 in, 12
/// over and under.
fn composer(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Div {
    let send = cx.listener(|forge, _: &ClickEvent, window, cx| forge.post_reply(window, cx));
    let me = forge.me_principal().is_some();
    let scope = forge.reply_scope().unwrap_or_default();
    let blank = forge.reply.is_blank();
    let field = Textarea::new(id("forge-reply"), &forge.reply, words::REPLY_IN_CHANGE)
        .autocorrect(true)
        .autocapitalize(Autocapitalize::Sentences)
        .read_only(!me)
        .placeholder(words::REPLY_IN_CHANGE);
    let row = div()
        .flex()
        .items_end()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .py(design::space::LG)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .child(design::field(field, ladder, theme)),
        )
        .child(
            design::icon_button(
                id("forge-reply-send"),
                design::icon("send", ladder.icon),
                "Send",
                ladder,
                theme,
                send,
            )
            .filled(!blank)
            .enabled(me && !blank && !forge.busy(&scope)),
        );
    div()
        .flex()
        .flex_col()
        .flex_none()
        .children(parts::pending(forge, &scope, cx, ladder, theme))
        .child(row)
}

// ------------------------------------------------------------------ Commits

/// The commits the change brings (`Log` from its source, less what its
/// target reaches).
fn commits(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    match forge.shown_log() {
        Some(query) => {
            let ids = ("forge-change-log", "forge-change-log-list");
            tabs::log(forge, &query, ids, cx, ladder, theme)
        }
        None => div().into_any_element(),
    }
}

// ------------------------------------------------------------------ Files

/// The file strip (`Files`): `N files · +a −d` and All files, a row per
/// file with its state, its delta and a tick for "read", and what the
/// tick is for.
fn files(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let column = div()
        .id(id("forge-files"))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    let Some(query) = forge.diff_query() else {
        return column
            .child(parts::quiet(
                id("forge-files-comparing"),
                words::COMPARING_ENDPOINTS,
                ladder,
                theme,
            ))
            .into_any_element();
    };
    let (files, total) = match parts::stage(
        forge,
        &query,
        "forge-diff",
        words::READING_DIFF,
        cx,
        (ladder, theme),
    ) {
        Ok(Reply::Diff {
            page, total_files, ..
        }) => (&page.items, *total_files),
        Ok(_) => return column.into_any_element(),
        Err(state) => return column.child(state).into_any_element(),
    };
    if files.is_empty() {
        return column
            .child(design::empty_state(
                id("forge-files-empty"),
                "send",
                words::NO_CHANGES,
                words::SAME_TREE,
                ladder,
                theme,
            ))
            .into_any_element();
    }
    column
        .child(files_head(files, total, cx, ladder, theme))
        .child(file_list(files, forge, cx, ladder, theme))
        .child(parts::quiet(
            id("forge-files-footer"),
            words::TICK_A_FILE,
            ladder,
            theme,
        ))
        .into_any_element()
}

/// `N files · +a −d` in the data face, and All files (every file on one
/// page).
fn files_head(
    files: &[FileDiff],
    total: u64,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let (added, deleted) = crate::select::totals(files);
    let all = cx.listener(|forge, _: &ClickEvent, _, cx| forge.open_all_files(cx));
    let said = words::files_changed(total, added, deleted);
    div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .py(design::space::MD)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .font_family(design::fonts::FAMILY_MONO)
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(said),
        )
        .child(design::button(id("forge-all-files"), words::ALL_FILES, ladder, theme, all).small())
}

fn file_list(
    files: &[FileDiff],
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let paths: Vec<Vec<u8>> = files
        .iter()
        .map(|file| crate::select::path_of(file).unwrap_or_default())
        .collect();
    let rows = files
        .iter()
        .zip(&paths)
        .map(|(file, path)| {
            let opened = path.clone();
            let open: parts::Press = Box::new(cx.listener(move |forge, _: &ClickEvent, _, cx| {
                forge.single_file(Some(opened.clone()), cx)
            }));
            (file_row(file, path, forge, cx, ladder, theme), open)
        })
        .collect();
    parts::rows_list(
        "forge-files-list",
        "Files",
        rows,
        move |forge, index, _, cx| forge.single_file(paths.get(index).cloned(), cx),
        forge,
        cx,
        (ladder, theme),
    )
}

/// A file (`Files` row): its path in the data face (muted once read),
/// `<state> · +a −d` (or `· binary`), and the tick.
fn file_row(
    file: &FileDiff,
    path: &[u8],
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Row {
    let read = forge
        .file_key(path)
        .is_some_and(|key| forge.viewed.contains(&key));
    let shown = path_text(path);
    let binary = file.hunks.is_empty() && file.content != Content::Text;
    let counted = file.additions > 0 || file.deletions > 0;
    let state = words::file_status(file.status);
    let facts = match (binary, counted) {
        (true, _) => parts::caption(format!("{state} · binary"), ladder, theme),
        (false, true) => {
            let delta = parts::delta(
                &format!("{state} · "),
                file.additions,
                file.deletions,
                theme,
            );
            parts::caption(delta, ladder, theme)
        }
        (false, false) => parts::caption(state, ladder, theme),
    };
    let toggled = path.to_vec();
    let tick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.toggle_viewed(&toggled, cx));
    let title = div()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.secondary)
        .text_color(if read { theme.muted } else { theme.foreground })
        .truncate()
        .child(shown.clone());
    Row::new(id(format!("forge-file-{shown}")), title)
        .caption(facts)
        .tail(parts::checkbox(
            id(format!("forge-viewed-{shown}")),
            read,
            format!("Viewed {shown}"),
            ladder,
            theme,
            tick,
        ))
}

// ------------------------------------------------------------------ Details

/// The desktop's dock as a tab (`Details`): the reviews (each reviewer's
/// latest verdict, then who is asked), the merge status, the line
/// comments, the channel.
fn details(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    div()
        .id(id("forge-details"))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(section_aside(
            "forge-details-reviews",
            "Reviews",
            words::ADVISORY.to_owned(),
            ladder,
            theme,
        ))
        .children(reviewers(forge, ladder, theme))
        .child(design::section_head(
            id("forge-details-merge"),
            words::MERGE_STATUS,
            ladder,
            theme,
        ))
        .children(merge_status(forge, ladder, theme))
        .children(line_comments(forge, cx, ladder, theme))
        .child(design::section_head(
            id("forge-details-channel"),
            "Channel",
            ladder,
            theme,
        ))
        .child(channel_row(forge, change, cx, ladder, theme))
        .into_any_element()
}

/// Each reviewer's latest verdict, then each asked who has not answered
/// ([`Forge::standings`]).
fn reviewers(forge: &Forge, ladder: &Ladder, theme: &Theme) -> Vec<AnyElement> {
    let Some(Standings { answered, asked }) = forge.standings() else {
        return Vec::new();
    };
    let answered = answered.into_iter().map(|(who, verdict)| {
        let tint = match verdict {
            Verdict::Approve => Tint::Ok,
            Verdict::RequestChanges => Tint::Danger,
            Verdict::Comment => Tint::Plain,
        };
        person_row(forge, who, words::standing(verdict), tint, ladder, theme)
    });
    let waiting = asked
        .into_iter()
        .map(|who| person_row(forge, who, words::ASKED, Tint::Plain, ladder, theme));
    let rows: Vec<AnyElement> = answered.chain(waiting).collect();
    match rows.is_empty() {
        true => vec![
            parts::quiet(
                id("forge-details-reviews-none"),
                words::NOBODY_REVIEWED,
                ladder,
                theme,
            )
            .into_any_element(),
        ],
        false => rows,
    }
}

/// A reviewer: the avatar at 24, the name, what they said as a badge.
fn person_row(
    forge: &Forge,
    who: &Principal,
    word: &str,
    tint: Tint,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let name = forge.principal_name(who);
    let key = format!("forge-details-{}-{word}", who.account().unwrap_or_default());
    Row::titled(id(key.clone()), name.clone())
        .lead(design::avatar(&name, ladder.avatar_inline, theme))
        .tail(parts::badge(
            id(format!("{key}-badge")),
            word.to_owned(),
            tint,
            ladder,
            theme,
        ))
        .draw(ladder, theme)
        .into_any_element()
}

/// Mergeability, the distance, both heads (`gone` once deleted).
fn merge_status(forge: &Forge, ladder: &Ladder, theme: &Theme) -> Vec<AnyElement> {
    let comparison = forge.compare();
    let word = comparison.map_or(words::COMPARING, |comparison| {
        words::merge_status(comparison.mergeability)
    });
    let head = |oid: Option<&String>| oid.map_or(words::GONE.to_owned(), |oid| short_hex(oid));
    let (source, target) = forge
        .change()
        .map(|(_, source, target, _)| (source.as_ref(), target.as_ref()))
        .unwrap_or_default();
    let distance = comparison.map(|comparison| {
        parts::key_value(
            "Distance",
            parts::data(words::distance(comparison)),
            ladder,
            theme,
        )
    });
    std::iter::once(parts::key_value(
        "Mergeability",
        div().child(word),
        ladder,
        theme,
    ))
    .chain(distance)
    .chain([
        parts::key_value(words::SOURCE_HEAD, parts::data(head(source)), ladder, theme),
        parts::key_value(words::TARGET_HEAD, parts::data(head(target)), ladder, theme),
    ])
    .map(IntoElement::into_any_element)
    .collect()
}

/// "Line comments" and every one, mine pending first, each opening its
/// file.
fn line_comments(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    let pending = forge.review().into_iter().flat_map(|review| {
        review.comments.iter().map(|comment| {
            (
                comment.path.clone(),
                words::pending_at(&comment.path, comment.line),
            )
        })
    });
    let published = forge.published_comments().into_iter().map(|comment| {
        let (path, _, line) = comment.anchor;
        let first = comment.body.lines().next().unwrap_or_default();
        let said = format!("{}:{line} · {} · {first}", path_text(&path), comment.author);
        (path, said)
    });
    let all: Vec<(Vec<u8>, String)> = pending.chain(published).collect();
    let head = design::section_head(
        id("forge-details-comments"),
        words::LINE_COMMENTS,
        ladder,
        theme,
    )
    .children((!all.is_empty()).then(|| all.len().to_string()))
    .into_any_element();
    if all.is_empty() {
        let none = parts::quiet(
            id("forge-details-comments-none"),
            words::NOBODY_ON_A_LINE,
            ladder,
            theme,
        );
        return vec![head, none.into_any_element()];
    }
    let rows = all.into_iter().enumerate().map(|(at, (path, line))| {
        let open = cx.listener(move |forge, _: &ClickEvent, _, cx| {
            forge.open_file_comment(path.clone(), cx)
        });
        Row::data(id(format!("forge-details-comment-{at}")), line)
            .press(ladder, theme)
            .role(Role::Button)
            .focusable()
            .on_click(open)
            .into_any_element()
    });
    std::iter::once(head).chain(rows).collect()
}

/// `# <channel>`, opening the room in Chat.
fn channel_row(
    forge: &Forge,
    change: &Change,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let link = forge.link(chat::PROGRAM, &[&change.channel]);
    let open = cx.listener(move |_, _: &ClickEvent, _, cx| {
        if let Some(link) = &link {
            cx.host().open_link(link);
        }
    });
    Row::data(
        id("forge-details-channel-row"),
        words::channel(&change.channel),
    )
    .chevron()
    .press(ladder, theme)
    .role(Role::Link)
    .focusable()
    .on_click(open)
    .into_any_element()
}

// --------------------------------------------------------------- Actions

/// What an open change offers behind ⋮ (`Sheet`): Edit to its author,
/// Close to a writer, Copy link to all. Close asks again on the sheet
/// itself (the Expo app's `Dialog`): "Close for good".
pub(super) fn actions_sheet(
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let dismiss = cx.listener(|forge, _: &(), _, cx| forge.close_sheet(cx));
    let asking = forge.closing.is_some() && forge.review_key() == forge.closing;
    let body = match asking {
        true => close_for_good(cx, ladder, theme),
        false => actions(forge, cx, ladder, theme),
    };
    design::sheet(
        id("forge-change-actions-sheet"),
        words::ACTIONS,
        ladder,
        theme,
    )
    .on_dismiss(dismiss)
    .child(body)
    .into_any_element()
}

fn actions(forge: &Forge, cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Div {
    let Some((change, ..)) = forge.change() else {
        return div();
    };
    let open = change.state == ChangeState::Open;
    let mine = forge.me_principal().as_ref() == Some(&change.author);
    let edit = cx.listener(|forge, _: &ClickEvent, window, cx| {
        forge.close_sheet(cx);
        forge.start_edit(window, cx);
    });
    let close = cx.listener(|forge, _: &ClickEvent, _, cx| forge.close_change(cx));
    // nothing to copy while no chain is known: the row waits for one
    let copy = forge
        .link(forge::PROGRAM, &[&forge.repo_name(), &change.n.to_string()])
        .map(|link| {
            let copy = cx.listener(move |forge, _: &ClickEvent, _, cx| {
                cx.host()
                    .notify::<view::methods::ClipboardWrite>(link.clone());
                forge.close_sheet(cx);
            });
            design::sheet_row(
                id("forge-change-copy-link"),
                "Copy link",
                ladder,
                theme,
                copy,
            )
            .icon("link")
        });
    div()
        .flex()
        .flex_col()
        .children((open && mine).then(|| {
            design::sheet_row(id("forge-change-edit"), "Edit", ladder, theme, edit).icon("pencil")
        }))
        .children((open && forge.writes_repo()).then(|| {
            design::sheet_row(id("forge-change-close"), "Close", ladder, theme, close)
                .icon("x")
                .danger()
        }))
        .children(copy)
}

/// Close asked again (`Dialog`): closing is final; Close for good, or
/// Cancel. Asked inside the Actions sheet, in Expo's words, until the SDK
/// draws a dialog.
// SDK gap: the kit's `Dialog` (`kit/Dialog.tsx`); the SDK builds it later
fn close_for_good(cx: &mut Context<Forge>, ladder: &Ladder, theme: &Theme) -> Div {
    let confirm = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.close_change(cx);
        forge.close_sheet(cx);
    });
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| {
        forge.closing = None;
        cx.notify();
    });
    let head = design::sheet_title(id("forge-close-head"), words::CLOSE_FOR_GOOD, ladder, theme)
        .child(
            div()
                .text_type(ladder.small)
                .text_color(theme.muted)
                .child(words::CLOSING_IS_FINAL),
        );
    div()
        .flex()
        .flex_col()
        .child(head)
        .child(
            design::sheet_row(
                id("forge-close-confirm"),
                words::CLOSE_FOR_GOOD,
                ladder,
                theme,
                confirm,
            )
            .icon("x")
            .danger(),
        )
        .child(design::sheet_row(
            id("forge-close-cancel"),
            "Cancel",
            ladder,
            theme,
            cancel,
        ))
}
