//! A markdown document on the phone (Expo `src/forge/Document.tsx`): [`Block`]s as the kit's text
//! at the phone's scale, code on `surface_raised` in the [`highlight`]er's colours, scrolling
//! sideways, links pressed through the caller's [`OnLink`].
use std::ops::Range;

use view::design::{Ladder, TextType, Type};
use view::prelude::*;
use view::{
    Div, FontStyle, FontWeight, HighlightStyle, InteractiveText, Stateful, StrikethroughStyle,
    StyledText, UnderlineStyle,
};

use crate::highlight::{self, Token};
use crate::{Block, OnLink, Text, id};

// A list item's marker column and how far its marker sits down to meet
// the body's first line (`Document.tsx` `item`).
const MARKER_W: Pixels = px(18.);
const MARKER_DROP: Pixels = px(2.);

// `blocks` drawn under the element `name`: blocks 10 apart in body text;
// headings medium on the heading and title roles; lists with mono markers;
// quotes muted behind a rule; bold is medium.
pub fn markdown(
    name: &str,
    blocks: &[Block],
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    column(name, blocks, Voice::Plain, on_link, ladder, theme)
        .text_color(theme.foreground)
        .text_type(ladder.body)
        .into_any_element()
}

// One block under the element `key`, in the body text the caller sets: what
// a view that keeps blocks of its own draws each of them through.
pub fn block(
    key: &str,
    block: &Block,
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    block_element(key, block, Voice::Plain, on_link, ladder, theme)
}

// How a column's prose reads: as it is, or muted inside a quote.
#[derive(Clone, Copy)]
enum Voice {
    Plain,
    Quoted,
}

fn column(
    name: &str,
    blocks: &[Block],
    voice: Voice,
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let elements = blocks.iter().enumerate().map(|(at, block)| {
        let key = format!("{name}-{at}");
        block_element(&key, block, voice, on_link, ladder, theme)
    });
    div()
        .id(id(name.to_owned()))
        .flex()
        .flex_col()
        .gap(design::space::MD)
        .children(elements)
}

fn block_element(
    key: &str,
    block: &Block,
    voice: Voice,
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    match block {
        Block::Heading(level, text) => heading(key, *level, text, on_link, ladder, theme),
        Block::Paragraph(text) => {
            let tone = match voice {
                Voice::Plain => theme.foreground,
                Voice::Quoted => theme.muted,
            };
            div()
                .text_color(tone)
                .child(runs(key, text, on_link, theme))
                .into_any_element()
        }
        Block::Item(marker, body) => item(key, marker, body, voice, on_link, ladder, theme),
        Block::Code(lang, text) => code_box(key, lang, text, ladder, theme),
        Block::Quote(body) => div()
            .id(id(format!("{key}-quote")))
            .pl(design::space::LG)
            .border_l_1()
            .border_color(theme.border_strong)
            .child(column(key, body, Voice::Quoted, on_link, ladder, theme))
            .into_any_element(),
        Block::Rule => div().h(px(1.)).bg(theme.border).into_any_element(),
        Block::Table(rows) => table(key, rows, on_link, ladder, theme),
    }
}

// A heading: medium on the heading role (level 1), the title role
// (level 2) or body text; 8 over it, and under the first two levels 4
// more and a hairline.
fn heading(
    key: &str,
    level: usize,
    text: &Text,
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let role = match level {
        1 => ladder.heading,
        2 => ladder.title,
        _ => ladder.body,
    };
    div()
        .id(id(format!("{key}-h")))
        .role(Role::Heading)
        .aria_level(level)
        .pt(design::space::SM)
        .when(level <= 2, |heading| {
            heading
                .pb(design::space::XXS)
                .border_b_1()
                .border_color(theme.border)
        })
        .text_type(role)
        .font_weight(FontWeight::MEDIUM)
        .child(runs(key, text, on_link, theme))
        .into_any_element()
}

// A list item: its marker mono, muted, secondary, in its column; its
// body a column of its own.
fn item(
    key: &str,
    marker: &str,
    body: &[Block],
    voice: Voice,
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    div()
        .id(id(format!("{key}-li")))
        .flex()
        .gap(design::space::SM)
        .child(
            div()
                .id(id(format!("{key}-marker")))
                .min_w(MARKER_W)
                .pt(MARKER_DROP)
                .role(Role::ListMarker)
                .font_family(design::fonts::FAMILY_MONO)
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(marker.to_owned()),
        )
        .child(
            column(key, body, voice, on_link, ladder, theme)
                .flex_1()
                .min_w(px(0.)),
        )
        .into_any_element()
}

// A table: a hairline around and between its cells, the header row on
// `surface` and medium; cells secondary, 8 in, 4 over and under. Each
// column is as wide as its widest cell on one line; a table wider than the
// screen scrolls sideways, its words unbroken.
fn table(
    key: &str,
    rows: &[Vec<Text>],
    on_link: &OnLink,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let blank = Text::default();
    let cells = rows.iter().enumerate().flat_map(|(r, row)| {
        let blank = &blank;
        (0..width).map(move |c| {
            div()
                .px(design::space::SM)
                .py(design::space::XXS)
                .whitespace_nowrap()
                .when(r > 0, |cell| cell.border_t_1().border_color(theme.border))
                .when(c > 0, |cell| cell.border_l_1().border_color(theme.border))
                .when(r == 0, |cell| {
                    cell.bg(theme.surface).font_weight(FontWeight::MEDIUM)
                })
                .child(runs(
                    &format!("{key}-{r}-{c}"),
                    row.get(c).unwrap_or(blank),
                    on_link,
                    theme,
                ))
        })
    });
    let grid = div()
        .flex_none()
        .grid()
        .grid_cols_max_content(width as u16)
        .children(cells.collect::<Vec<_>>());
    div()
        .id(id(format!("{key}-table")))
        .flex()
        .overflow_x_scroll()
        .border_1()
        .border_color(theme.border)
        .text_type(ladder.secondary)
        .child(grid)
        .into_any_element()
}

// A fenced block (`CodeBox`): code on `surface_raised`, 12 in, in the
// colours of `lang` (a file name or a fence's language), scrolling sideways.
pub fn code_box(key: &str, lang: &str, text: &str, ladder: &Ladder, theme: &Theme) -> AnyElement {
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let tokens = highlight::tokens(lang, &lines);
    let rows = lines
        .iter()
        .zip(&tokens)
        .enumerate()
        .map(|(at, (line, tokens))| {
            code_line(id(format!("{key}-{at}")), line, tokens, ladder.code, theme)
        });
    div()
        .id(id(format!("{key}-code")))
        .flex()
        .flex_col()
        .p(design::space::LG)
        .bg(theme.surface_raised)
        .overflow_x_scroll()
        .children(rows)
        .into_any_element()
}

// One source line in its tokens' colours (`Code`): keywords agent,
// strings success, numbers warning, comments muted italic, types and
// keys medium; on one line, at `role`.
pub fn code_line(
    element_id: ElementId,
    line: &str,
    tokens: &[(Range<usize>, Token)],
    role: Type,
    theme: &Theme,
) -> Stateful<Div> {
    let styled = StyledText::new(line.to_owned()).with_highlights(
        tokens
            .iter()
            .map(|(range, token)| (range.clone(), token_style(*token, theme))),
    );
    div()
        .id(element_id)
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(role)
        .text_color(theme.foreground)
        .whitespace_nowrap()
        .child(styled)
}

// The tokens' shared colours ([`highlight::style`]), types and keys at
// the Expo app's medium where the desktop sets them semibold (`Document.tsx` `Code`).
fn token_style(token: Token, theme: &Theme) -> HighlightStyle {
    let style = highlight::style(token, theme);
    HighlightStyle {
        font_weight: style.font_weight.map(|_| FontWeight::MEDIUM),
        ..style
    }
}

// A run of prose (`Runs`): bold medium, italic, struck through, inline
// code mono on `surface_raised`, quiet muted, a link underlined and
// pressed through `on_link`.
fn runs(element_id: &str, text: &Text, on_link: &OnLink, theme: &Theme) -> AnyElement {
    let mut links = Vec::new();
    let mut targets = Vec::new();
    let mut mono = Vec::new();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = text
        .runs
        .iter()
        .map(|(range, marks)| {
            let style = HighlightStyle {
                font_weight: marks.bold.then_some(FontWeight::MEDIUM),
                font_style: marks.italic.then_some(FontStyle::Italic),
                strikethrough: marks.strike.then_some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                }),
                color: marks.quiet.then_some(theme.muted),
                background_color: marks.code.then_some(theme.surface_raised),
                underline: marks.link.as_ref().map(|_| UnderlineStyle {
                    thickness: px(1.),
                    color: None,
                    wavy: false,
                }),
                ..Default::default()
            };
            if marks.code {
                mono.push((range.clone(), design::fonts::FAMILY_MONO.into()));
            }
            if let Some(target) = &marks.link {
                links.push(range.clone());
                targets.push(target.clone());
            }
            (range.clone(), style)
        })
        .collect();
    let styled = StyledText::new(text.text.clone())
        .with_highlights(highlights)
        .with_font_family_overrides(mono);
    let on_link = on_link.clone();
    InteractiveText::new(id(element_id.to_owned()), styled)
        .on_click(links, move |index, window, cx| {
            if let Some(target) = targets.get(index) {
                on_link(target, window, cx);
            }
        })
        .into_any_element()
}
