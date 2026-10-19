//! Code and diff rows on the phone (Expo `src/forge/Lines.tsx`; a diff's
//! rows laid out as `src/forge/diff.ts` `paint` does, its cards from
//! `src/forge/DiffList.tsx`): the code in the data face at 12/20 after a
//! 40 gutter, unwrapped, the whole block scrolling sideways as one; a
//! file's head, a published comment, my staged comment and the open
//! composer as cards at the window's width between the lines. A change's
//! file ([`super::diff`]), a commit ([`super::commit`]) and a file of the
//! Code tab ([`super::code`]) draw through it.
//!
//! Reads the rows a page hands it, the review being written (its open
//! composer) and the window's width; writes the composer (Stage, Cancel)
//! and opens one at a pressed gutter number.
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;

use view::design::{Kind, Ladder};
use view::prelude::*;
use view::{Div, FontWeight, ListAlignment, ListOffset, ListState, Stateful};

use super::parts::{self, Tint};
use super::review;
use crate::Forge;
use crate::id;
use crate::select::{Published, path_of};
use crate::state::Anchor;
use crate::words::path_text;
use crate::words::{self, file_status, why_no_lines};
use forge::{FileDiff, LineKind};
use markdown::highlight::{self, Token};
use markdown::phone as document;

/// The gutter's width and the marker's column (`Lines.tsx` `GUTTER`,
/// `MARKER`).
const GUTTER: Pixels = px(40.);
const MARKER: Pixels = px(16.);
/// A mono cell's advance in ems of the code size: the data face's
/// 600/1000 (`Lines.tsx` `ADVANCE`).
const ADVANCE: f32 = 0.6;
/// How far past the window the list lays rows out ahead.
const OVERDRAW: Pixels = px(400.);
/// The edge of the composer's buttons from each other (`DiffList.tsx`
/// `Composer`: `gap.xxs`).
const BUTTONS_GAP: Pixels = design::space::XXS;
/// A card's own icon (`DiffList.tsx` `DraftCard`, `Composer`).
const CARD_ICON: Pixels = px(16.);

/// A line's ground: as it is, added, or deleted.
#[derive(Clone, Copy, Hash)]
pub(super) enum Ground {
    Plain,
    Added,
    Deleted,
}

/// One line of code.
pub(super) struct Line {
    /// the number in the gutter
    pub(super) number: Option<u64>,
    /// where a comment on it goes: its gutter number is then the button
    pub(super) anchor: Option<Anchor>,
    /// `+`, `-` or a space in a column of its own: a diff's lines; none
    /// for a file's own
    pub(super) marker: Option<char>,
    pub(super) text: String,
    pub(super) tokens: Vec<(Range<usize>, Token)>,
    pub(super) ground: Ground,
}

/// One row of a block.
pub(super) enum Item {
    /// what the page shows over the rows (a commit's facts), drawn by
    /// the page's [`Head`]
    Head,
    /// a file's head: its path (`old → new` once renamed), state, delta
    File {
        path: String,
        status: &'static str,
        additions: u64,
        deletions: u64,
    },
    /// a hunk's `@@` head, or why a file has no lines
    Hunk(String),
    Line(Line),
    Comment(Published),
    /// my staged comment at an anchor; `editable` while the lines take
    /// comments
    Draft {
        anchor: Anchor,
        body: String,
        editable: bool,
    },
    /// the open composer under its anchor
    Composer(Anchor),
}

/// What a page draws over the rows, as the block's first row.
pub(super) type Head = Rc<dyn Fn(&Forge, &mut Context<Forge>) -> AnyElement>;

/// What readers have written on a diff's lines, and whether its gutter
/// numbers are comment buttons: a change's file under a reader with an
/// account (`onGutter`), never a commit's diff.
#[derive(Default)]
pub(super) struct Notes {
    pub(super) commentable: bool,
    pub(super) published: Vec<Published>,
    pub(super) staged: Vec<(Anchor, String)>,
    pub(super) open: Option<Anchor>,
}

impl Notes {
    /// The cards under the line at `anchor`: what was published there,
    /// then my staged comment, or the composer while it is open there.
    fn under(&self, anchor: &Anchor, rows: &mut Vec<Item>) {
        rows.extend(
            self.published
                .iter()
                .filter(|comment| comment.anchor == *anchor)
                .cloned()
                .map(Item::Comment),
        );
        if self.open.as_ref() == Some(anchor) {
            rows.push(Item::Composer(anchor.clone()));
            return;
        }
        if let Some((_, body)) = self.staged.iter().find(|(staged, _)| staged == anchor) {
            rows.push(Item::Draft {
                anchor: anchor.clone(),
                body: body.clone(),
                editable: self.commentable,
            });
        }
    }
}

// ------------------------------------------------------------------ rows

/// The rows of `files` (`paint`): each file's head, then its hunks'
/// heads and lines (each file highlighted as one run of source), or why
/// it has none; the notes under their lines.
pub(super) fn diff_rows(files: &[&FileDiff], notes: &Notes) -> Vec<Item> {
    let mut rows = Vec::new();
    for file in files {
        file_rows(file, notes, &mut rows);
    }
    rows
}

fn file_rows(file: &FileDiff, notes: &Notes, rows: &mut Vec<Item>) {
    let path = path_of(file).unwrap_or_default();
    rows.push(Item::File {
        path: words::file_path(file),
        status: file_status(file.status),
        additions: file.additions,
        deletions: file.deletions,
    });
    if file.hunks.is_empty() {
        rows.push(Item::Hunk(why_no_lines(file).to_owned()));
        return;
    }
    let texts: Vec<String> = file
        .hunks
        .iter()
        .flat_map(|hunk| &hunk.lines)
        .map(|line| {
            String::from_utf8_lossy(&line.bytes)
                .trim_end_matches('\n')
                .to_owned()
        })
        .collect();
    let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
    let mut tokens = highlight::tokens(&path_text(&path), &refs).into_iter();
    let mut texts = texts.into_iter();
    for hunk in &file.hunks {
        rows.push(Item::Hunk(words::hunk(hunk)));
        for line in &hunk.lines {
            // anchored on the new side when the line has one
            let number = line.new_line.or(line.old_line);
            let anchor: Anchor = (
                path.clone(),
                line.new_line.is_some(),
                number.unwrap_or_default(),
            );
            rows.push(Item::Line(Line {
                number,
                anchor: notes.commentable.then(|| anchor.clone()),
                marker: Some(marker(line.kind)),
                text: texts.next().unwrap_or_default(),
                tokens: tokens.next().unwrap_or_default(),
                ground: match line.kind {
                    LineKind::Added => Ground::Added,
                    LineKind::Deleted => Ground::Deleted,
                    LineKind::Context => Ground::Plain,
                },
            }));
            notes.under(&anchor, rows);
        }
    }
}

/// The marker a diff line wears (`diff.ts` `marker`): an ASCII hyphen
/// for a deleted line.
fn marker(kind: LineKind) -> char {
    match kind {
        LineKind::Added => '+',
        LineKind::Deleted => '-',
        LineKind::Context => ' ',
    }
}

/// A file's own lines, numbered from 1, with their tokens.
pub(super) fn file_lines(texts: &[String], tokens: &[Vec<(Range<usize>, Token)>]) -> Vec<Item> {
    texts
        .iter()
        .enumerate()
        .map(|(at, text)| {
            Item::Line(Line {
                number: Some(at as u64 + 1),
                anchor: None,
                marker: None,
                text: text.clone(),
                tokens: tokens.get(at).cloned().unwrap_or_default(),
                ground: Ground::Plain,
            })
        })
        .collect()
}

/// How many mono cells `line` takes (`diff.ts` `cells`): an East Asian
/// wide character two, a tab four.
fn cells(line: &str) -> usize {
    line.chars()
        .map(|ch| match ch as u32 {
            0x1100..=0x115f
            | 0x2e80..=0xa4cf
            | 0xac00..=0xd7a3
            | 0xf900..=0xfaff
            | 0xfe30..=0xfe4f
            | 0xff00..=0xff60
            | 0xffe0..=0xffe6
            | 0x1f300.. => 2,
            0x09 => 4,
            _ => 1,
        })
        .sum()
}

// ------------------------------------------------------------------ the list

/// The list a block is drawn in, kept across frames: the same page's
/// rows changing (a composer opened, a comment staged) keep its place,
/// another page starts at its top.
#[derive(Default)]
pub(crate) struct Block(RefCell<Option<Held>>);

struct Held {
    page: u64,
    signature: u64,
    state: ListState,
}

impl Block {
    /// The list for `count` rows signed `signature` on `page`: the one
    /// held while both hold, else a new one at `reveal` (the open
    /// composer's line), at the held one's top on the same page, or at
    /// the top.
    fn state(&self, page: u64, signature: u64, count: usize, reveal: Option<usize>) -> ListState {
        let mut held = self.0.borrow_mut();
        if let Some(held) = held.as_ref()
            && held.page == page
            && held.signature == signature
        {
            return held.state.clone();
        }
        let kept = held
            .as_ref()
            .filter(|held| held.page == page)
            .map(|held| held.state.logical_scroll_top().item_ix);
        let state = ListState::new(count, ListAlignment::Top, OVERDRAW);
        if let Some(at) = reveal.or(kept) {
            state.scroll_to(ListOffset {
                item_ix: at,
                offset_in_item: px(0.),
            });
        }
        *held = Some(Held {
            page,
            signature,
            state: state.clone(),
        });
        state
    }

    /// Brings row `at` to the top: a commit's file row, its diff.
    pub(super) fn scroll_to(&self, at: usize) {
        if let Some(held) = self.0.borrow().as_ref() {
            held.state.scroll_to(ListOffset {
                item_ix: at,
                offset_in_item: px(0.),
            });
        }
    }
}

/// What a block's rows are of (a file of a diff, a blob, a commit), as
/// [`Shown::page`] names it.
pub(super) fn page_of(what: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    what.hash(&mut hasher);
    hasher.finish()
}

/// What tells two sets of rows apart on one page: their kinds, numbers
/// and cards (a page's own lines are its page's).
fn signature(rows: &[Item]) -> u64 {
    let mut hasher = DefaultHasher::new();
    rows.len().hash(&mut hasher);
    for row in rows {
        match row {
            Item::Head => 0.hash(&mut hasher),
            Item::File { path, .. } => path.hash(&mut hasher),
            Item::Hunk(text) => text.hash(&mut hasher),
            Item::Line(line) => line.number.hash(&mut hasher),
            Item::Comment(comment) => {
                (&comment.anchor, &comment.body, comment.outdated).hash(&mut hasher)
            }
            Item::Draft {
                anchor,
                body,
                editable,
            } => (anchor, body, editable).hash(&mut hasher),
            Item::Composer(anchor) => (1, anchor).hash(&mut hasher),
        }
    }
    hasher.finish()
}

/// A page's block: `rows` under the element `key`, `page` naming what
/// they are of (a file of a diff, a blob), `head` drawing its
/// [`Item::Head`]. Unwrapped, the block is as wide as its widest line and
/// scrolls sideways as one; wrapped, its lines wrap at the window's
/// width.
pub(super) struct Shown {
    pub(super) key: &'static str,
    pub(super) page: u64,
    pub(super) rows: Vec<Item>,
    pub(super) head: Option<Head>,
    pub(super) wrapped: bool,
}

/// How the rows of a block are drawn, the same for each.
#[derive(Clone, Copy)]
struct Look {
    key: &'static str,
    /// the gutter and the marker's column
    lead: Pixels,
    /// the window's width: the cards' and a wrapped line's
    window: Pixels,
    wrapped: bool,
    ladder: Ladder,
    theme: Theme,
}

pub(super) fn block(
    shown: Shown,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let Shown {
        key,
        page,
        rows,
        head,
        wrapped,
    } = shown;
    let reveal = rows
        .iter()
        .position(|row| matches!(row, Item::Composer(_)))
        .map(|at| at.saturating_sub(1));
    let state = forge
        .lines
        .state(page, signature(&rows), rows.len(), reveal);
    let marked = rows.iter().any(|row| {
        matches!(
            row,
            Item::Line(Line {
                marker: Some(_),
                ..
            })
        )
    });
    let look = Look {
        key,
        lead: GUTTER + if marked { MARKER } else { design::space::SM },
        window: px(forge.layout.width),
        wrapped,
        ladder: *ladder,
        theme: *theme,
    };
    let width = match wrapped {
        true => look.window,
        false => look
            .window
            .max(look.lead + widest(&rows, ladder) + design::space::BLOCK),
    };
    let rows = Rc::new(rows);
    let paint = cx.processor(
        move |forge: &mut Forge, index: usize, _, cx| match &rows[index] {
            Item::Head => head
                .as_ref()
                .map_or_else(|| div().into_any_element(), |head| head(forge, cx)),
            row => draw(row, index, &look, forge, cx),
        },
    );
    let list = view::list(id(format!("{key}-rows")), state, paint).size_full();
    div()
        .id(id(key))
        .flex_1()
        .min_h(px(0.))
        .overflow_x_scroll()
        .child(div().w(width).h_full().child(list))
        .into_any_element()
}

/// The widest line's run of cells at the code size.
fn widest(rows: &[Item], ladder: &Ladder) -> Pixels {
    let most = rows
        .iter()
        .filter_map(|row| match row {
            Item::Line(line) => Some(cells(&line.text)),
            Item::Hunk(text) => Some(cells(text)),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    ladder.code.size * (most as f32 * ADVANCE)
}

fn draw(
    row: &Item,
    index: usize,
    look: &Look,
    forge: &Forge,
    cx: &mut Context<Forge>,
) -> AnyElement {
    match row {
        Item::Head => div().into_any_element(),
        Item::File {
            path,
            status,
            additions,
            deletions,
        } => file_head(index, path, status, (*additions, *deletions), look).into_any_element(),
        Item::Hunk(text) => hunk_row(index, text, look).into_any_element(),
        Item::Line(line) => line_row(index, line, look, cx).into_any_element(),
        Item::Comment(comment) => comment_card(index, comment, look).into_any_element(),
        Item::Draft {
            anchor,
            body,
            editable,
        } => draft_card(index, (anchor, *editable), body, look, cx).into_any_element(),
        Item::Composer(_) => composer_card(forge, look, cx).into_any_element(),
    }
}

// ------------------------------------------------------------------ rows drawn

/// A line of code (`Lines.tsx` row): at least the code's line tall (20,
/// `Lines.tsx:100`), its ground, the gutter number at the gutter's right
/// (faint), the marker, the code highlighted.
fn line_row(index: usize, line: &Line, look: &Look, cx: &mut Context<Forge>) -> Stateful<Div> {
    let theme = &look.theme;
    let ground = match line.ground {
        Ground::Plain => theme.background,
        Ground::Added => theme.success_soft,
        Ground::Deleted => theme.danger_soft,
    };
    let code = document::code_line(
        id(format!("{}-code-{index}", look.key)),
        &line.text,
        &line.tokens,
        look.ladder.code,
        theme,
    )
    .when(look.wrapped, |code| code.whitespace_normal());
    div()
        .id(id(format!("{}-line-{index}", look.key)))
        .w_full()
        .min_h(look.ladder.code.line.unwrap_or(look.ladder.code.size))
        .flex()
        .items_center()
        .bg(ground)
        .child(gutter(index, line, look, cx))
        .children(
            line.marker
                .map(|marker| code_cell(marker.to_string(), theme.foreground, look).w(MARKER)),
        )
        .when(line.marker.is_none(), |row| row.pl(design::space::SM))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .pr(design::space::BLOCK)
                .child(code),
        )
}

/// The gutter: the line's number at its right, faint. While the line
/// takes comments it is the comment button, pressed in the line's own box
/// as Expo's is (`Lines.tsx:112-123`): a dense list's press
/// (`design::dense`), under a finger's 44.
fn gutter(index: usize, line: &Line, look: &Look, cx: &mut Context<Forge>) -> AnyElement {
    let number = line
        .number
        .map(|number| number.to_string())
        .unwrap_or_default();
    let cell = div()
        .w(GUTTER)
        .flex_none()
        .flex()
        .justify_end()
        .pr(design::space::SM)
        .child(code_cell(number.clone(), look.theme.faint, look));
    let Some((path, new_side, at)) = line.anchor.clone() else {
        return cell.into_any_element();
    };
    let open = cx.listener(move |forge, _: &ClickEvent, window, cx| {
        forge.open_comment(path.clone(), new_side, at, window, cx)
    });
    design::dense(cell.id(id(format!("{}-gutter-{index}", look.key))))
        .role(Role::Button)
        .aria_label(format!("Comment on line {number}"))
        .focusable()
        .on_click(open)
        .into_any_element()
}

/// One cell of code text: mono at the code size, in `tone`.
fn code_cell(text: String, tone: Hsla, look: &Look) -> Div {
    div()
        .flex_none()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(look.ladder.code)
        .text_color(tone)
        .whitespace_nowrap()
        .child(text)
}

/// A hunk's head, or why a file has no lines: muted code on `surface`
/// past the gutter.
fn hunk_row(index: usize, text: &str, look: &Look) -> Stateful<Div> {
    let line = look.ladder.code.line.unwrap_or(look.ladder.code.size);
    div()
        .id(id(format!("{}-hunk-{index}", look.key)))
        .w_full()
        .min_h(line)
        .flex()
        .items_center()
        .pl(look.lead)
        .bg(look.theme.surface)
        .child(code_cell(text.to_owned(), look.theme.muted, look))
}

/// A file's head over its lines (`FileHead`): the path in the data face,
/// `<state> · +a −d` at its right, on `surface` between hairlines.
fn file_head(
    index: usize,
    path: &str,
    status: &str,
    (additions, deletions): (u64, u64),
    look: &Look,
) -> Stateful<Div> {
    let (ladder, theme) = (&look.ladder, &look.theme);
    div()
        .id(id(format!("{}-file-{index}", look.key)))
        .w(look.window)
        .min_h(ladder.control)
        .flex()
        .items_center()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .bg(theme.surface)
        .border_t_1()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .font_family(design::fonts::FAMILY_MONO)
                .text_type(ladder.secondary)
                .line_clamp(2)
                .child(path.to_owned()),
        )
        .child(
            parts::caption(
                parts::delta(&format!("{status} · "), additions, deletions, theme),
                ladder,
                theme,
            )
            .flex_none(),
        )
}

/// A card under a line (`Card`): past the gutter, a hairline box on the
/// window's ground; a draft's edge warns.
fn card(key: String, warn: bool, look: &Look) -> (Stateful<Div>, Div) {
    let theme = &look.theme;
    let outer = div()
        .id(id(key))
        .w(look.window)
        .pl(GUTTER + design::space::SM)
        .pr(design::space::BLOCK)
        .py(design::space::SM);
    let inner = div()
        .flex()
        .flex_col()
        .gap(design::space::XS)
        .px(design::space::LG)
        .py(design::space::MD)
        .border_1()
        .border_color(if warn {
            theme.warning
        } else {
            theme.border_strong
        })
        .bg(theme.background);
    (outer, inner)
}

/// A published comment (`CommentCard`): its author, `line N · new ·
/// block H` and `outdated`, then what it says.
fn comment_card(index: usize, comment: &Published, look: &Look) -> Stateful<Div> {
    let (ladder, theme) = (&look.ladder, &look.theme);
    let (_, new_side, line) = &comment.anchor;
    let side = if *new_side { "new" } else { "old" };
    let head = div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(
            div()
                .text_type(ladder.secondary)
                .font_weight(FontWeight::MEDIUM)
                .child(comment.author.clone()),
        )
        .child(
            parts::caption(
                format!("line {line} · {side} · block {}", comment.height),
                ladder,
                theme,
            )
            .flex_1(),
        )
        .children(comment.outdated.then(|| {
            parts::badge(
                id(format!("{}-outdated-{index}", look.key)),
                words::OUTDATED,
                Tint::Warn,
                ladder,
                theme,
            )
        }));
    let (outer, inner) = card(format!("{}-comment-{index}", look.key), false, look);
    outer.child(
        inner.child(head).child(
            div()
                .text_type(ladder.secondary)
                .child(comment.body.clone()),
        ),
    )
}

/// My staged comment (`DraftCard`): "Your pending comment", its pencil
/// to open it again while the line takes comments, what it says.
fn draft_card(
    index: usize,
    (anchor, editable): (&Anchor, bool),
    body: &str,
    look: &Look,
    cx: &mut Context<Forge>,
) -> Stateful<Div> {
    let (ladder, theme) = (&look.ladder, &look.theme);
    let (path, new_side, line) = anchor.clone();
    let edit = editable.then(|| {
        let open = cx.listener(move |forge, _: &ClickEvent, window, cx| {
            forge.open_comment(path.clone(), new_side, line, window, cx)
        });
        design::icon_button(
            id(format!("{}-draft-edit-{index}", look.key)),
            design::icon("pencil", ladder.icon),
            "Edit your pending comment",
            ladder,
            theme,
            open,
        )
    });
    let head = div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(design::icon("pencil", CARD_ICON).text_color(theme.muted))
        .child(
            div()
                .flex_1()
                .text_type(ladder.secondary)
                .font_weight(FontWeight::MEDIUM)
                .child(words::YOUR_PENDING_COMMENT),
        )
        .children(edit);
    let (outer, inner) = card(format!("{}-draft-{index}", look.key), true, look);
    outer.child(
        inner
            .child(head)
            .child(div().text_type(ladder.secondary).child(body.to_owned())),
    )
}

/// The composer under the pressed line (`Composer`): "Your pending
/// comment" and Reviewing, the field, Cancel and Stage.
fn composer_card(forge: &Forge, look: &Look, cx: &mut Context<Forge>) -> Stateful<Div> {
    let (ladder, theme) = (&look.ladder, &look.theme);
    let (outer, inner) = card(format!("{}-composer", look.key), true, look);
    let Some((open, error)) = forge
        .review()
        .and_then(|review| Some((review.open.as_ref()?, review::refusal(review).to_owned())))
    else {
        return outer;
    };
    let head = div()
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(design::icon("pencil", CARD_ICON).text_color(theme.foreground))
        .child(
            div()
                .text_type(ladder.secondary)
                .font_weight(FontWeight::MEDIUM)
                .child(words::YOUR_PENDING_COMMENT),
        )
        .child(parts::badge(
            id("forge-comment-reviewing"),
            "Reviewing",
            Tint::Warn,
            ladder,
            theme,
        ));
    let field = Textarea::new(id("forge-comment-body"), &open.body, "Comment")
        .autocorrect(true)
        .autocapitalize(Autocapitalize::Sentences)
        .placeholder(words::COMMENT_PROMPT);
    let field = design::field(field, ladder, theme)
        .when(!error.is_empty(), |field| field.problem(error.clone()));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.discard_comment(cx));
    let stage = cx.listener(|forge, _: &ClickEvent, _, cx| forge.stage_comment(cx));
    let buttons = div()
        .flex()
        .justify_end()
        .gap(BUTTONS_GAP)
        .child(
            design::button(id("forge-comment-cancel"), "Cancel", ladder, theme, cancel)
                .small()
                .kind(Kind::Quiet),
        )
        .child(design::button(id("forge-comment-stage"), "Stage", ladder, theme, stage).small());
    outer.child(inner.child(head).child(field).child(buttons))
}

#[cfg(test)]
mod tests {
    use super::cells;

    #[test]
    fn a_wide_character_takes_two_cells_and_a_tab_four() {
        assert_eq!(cells("abc"), 3);
        assert_eq!(cells("한글"), 4);
        assert_eq!(cells("\tx"), 5);
    }
}
