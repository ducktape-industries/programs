//! The desktop's documents: [`Block`]s drawn as elements, styled runs over one `StyledText` per
//! paragraph, links pressed through the caller's [`OnLink`], code as [`source_line`]s in the
//! [`highlight`]er's colours on a bordered panel that scrolls sideways.
use std::ops::Range;

use view::prelude::*;
use view::{
    Div, FontStyle, FontWeight, HighlightStyle, Stateful, StrikethroughStyle, UnderlineStyle,
};

use crate::highlight;
use crate::{Block, OnLink, Text, id};

// A list item's marker column: room for a two-digit number and its dot.
const LIST_MARKER_W: Pixels = px(18.);

// Rich text for one paragraph: styled runs, links pressed through `on_link`.
fn rich(element_id: String, text: &Text, theme: &Theme, on_link: &OnLink) -> AnyElement {
    let mut links = Vec::new();
    let mut targets = Vec::new();
    let mut mono = Vec::new();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = text
        .runs
        .iter()
        .map(|(range, marks)| {
            let mut style = HighlightStyle::default();
            if marks.bold {
                style.font_weight = Some(FontWeight::SEMIBOLD);
            }
            if marks.italic {
                style.font_style = Some(FontStyle::Italic);
            }
            if marks.strike {
                style.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: None,
                });
            }
            if marks.quiet {
                style.color = Some(theme.muted);
            }
            if marks.code {
                style.background_color = Some(theme.surface_raised);
                mono.push((range.clone(), design::fonts::FAMILY_MONO.into()));
            }
            if let Some(target) = &marks.link {
                style.color = Some(theme.link);
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.link),
                    wavy: false,
                });
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
    InteractiveText::new(id(element_id), styled)
        .on_click(links, move |index, window, cx| {
            if let Some(target) = targets.get(index) {
                on_link(target, window, cx);
            }
        })
        .into_any_element()
}

// A markdown body that is not a file (a change's), its links pressed
// through `on_link`.
#[cfg(feature = "parse")]
pub fn render(name: &str, text: &str, theme: &Theme, on_link: &OnLink) -> AnyElement {
    render_blocks(name, &crate::parse(text), theme, on_link)
}

// Blocks already made, drawn under the element `name`.
pub fn render_blocks(name: &str, blocks: &[Block], theme: &Theme, on_link: &OnLink) -> AnyElement {
    column(name, blocks, theme, on_link)
        .gap_2()
        .text_size(design::text::BODY)
        .text_color(theme.foreground)
        .into_any_element()
}

fn column(name: &str, blocks: &[Block], theme: &Theme, on_link: &OnLink) -> Stateful<Div> {
    let mut out = div().id(id(name.to_owned())).flex().flex_col().gap_2();
    for (at, each) in blocks.iter().enumerate() {
        out = out.child(block(&format!("{name}-{at}"), each, theme, on_link));
    }
    out
}

// One block under the element `key`, in the body text the caller sets: what
// a view that keeps blocks of its own draws each of them through.
pub fn block(key: &str, block: &Block, theme: &Theme, on_link: &OnLink) -> AnyElement {
    match block {
        Block::Heading(level, text) => {
            let size = match level {
                1 => design::text::TITLE,
                2 => design::text::SECTION,
                _ => design::text::BODY,
            };
            let mut heading = div()
                .id(id(format!("{key}-h")))
                .text_size(size)
                .font_weight(FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(*level)
                .pt_2()
                .child(rich(key.to_owned(), text, theme, on_link));
            if *level <= 2 {
                heading = heading.pb_1().border_b_1().border_color(theme.border);
            }
            heading.into_any_element()
        }
        Block::Paragraph(text) => rich(key.to_owned(), text, theme, on_link),
        Block::Item(marker, body) => div()
            .id(id(format!("{key}-li")))
            .flex()
            .gap_2()
            .child(
                div()
                    .id(id(format!("{key}-marker")))
                    .min_w(LIST_MARKER_W)
                    .role(Role::ListMarker)
                    .text_color(theme.muted)
                    .font_family(design::fonts::FAMILY_MONO)
                    .text_size(design::text::SECONDARY)
                    .child(marker.clone()),
            )
            .child(
                column(key, body, theme, on_link)
                    .gap_1()
                    .flex_1()
                    .min_w(px(0.)),
            )
            .into_any_element(),
        Block::Code(lang, text) => code(key, lang, text, theme),
        Block::Quote(body) => div()
            .id(id(format!("{key}-quote")))
            .pl_3()
            .border_l_1()
            .border_color(theme.border_strong)
            .text_color(theme.muted)
            .child(column(key, body, theme, on_link))
            .into_any_element(),
        Block::Rule => div().h(px(1.)).bg(theme.border).into_any_element(),
        Block::Table(rows) => table(key, rows, theme, on_link),
    }
}

// A fenced block: a bordered panel on `surface`, one line per line in the
// colours of `lang` (a file name or a fence's language), scrolling sideways.
pub fn code(key: &str, lang: &str, text: &str, theme: &Theme) -> AnyElement {
    let lines: Vec<&str> = text.split('\n').collect();
    let tokens = highlight::tokens(lang, &lines);
    let mut block = div()
        .id(id(format!("{key}-code")))
        .flex()
        .flex_col()
        .p_2()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .overflow_x_scroll();
    for (at, (line, tokens)) in lines.iter().zip(&tokens).enumerate() {
        block = block.child(source_line(id(format!("{key}-{at}")), line, tokens, theme));
    }
    block.into_any_element()
}

// A grid with a hairline around and between its cells, the header row first
// on `surface` and semibold.
fn table(key: &str, rows: &[Vec<Text>], theme: &Theme, on_link: &OnLink) -> AnyElement {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let blank = Text::default();
    let cells = rows.iter().enumerate().flat_map(|(r, row)| {
        let blank = &blank;
        (0..width).map(move |c| {
            div()
                .min_w(px(0.))
                .px_2()
                .py_1()
                .when(r > 0, |cell| cell.border_t_1().border_color(theme.border))
                .when(c > 0, |cell| cell.border_l_1().border_color(theme.border))
                .when(r == 0, |cell| {
                    cell.bg(theme.surface).font_weight(FontWeight::SEMIBOLD)
                })
                .child(rich(
                    format!("{key}-{r}-{c}"),
                    row.get(c).unwrap_or(blank),
                    theme,
                    on_link,
                ))
        })
    });
    let grid = div()
        .id(id(format!("{key}-table")))
        .min_w(px(0.))
        .grid()
        .grid_cols_max_content(width as u16)
        .border_1()
        .border_color(theme.border)
        .children(cells.collect::<Vec<_>>());
    div().flex().child(grid).into_any_element()
}

// One source line as mono rich text in its tokens' colours.
pub fn source_line(
    element_id: ElementId,
    text: &str,
    tokens: &[(Range<usize>, highlight::Token)],
    theme: &Theme,
) -> AnyElement {
    let styled = StyledText::new(text.to_owned()).with_highlights(
        tokens
            .iter()
            .map(|(range, token)| (range.clone(), highlight::style(*token, theme))),
    );
    div()
        .id(element_id)
        .flex_1()
        .min_w(px(0.))
        .font_family(design::fonts::FAMILY_MONO)
        .text_size(design::text::SECONDARY)
        .text_color(theme.foreground)
        .whitespace_nowrap()
        .child(styled)
        .into_any_element()
}

#[cfg(all(test, feature = "parse"))]
mod view_tests {
    use view::testing::TestAppContext;
    use view::{Context, IntoElement, Render, Styled, View, Window};

    // A document that keeps each destination its `OnLink` is handed.
    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct Doc {
        pressed: Vec<String>,
    }

    impl View for Doc {
        const NAME: &'static str = "Doc";
    }

    impl Render for Doc {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = *cx.global::<view::Theme>();
            let doc = cx.weak_entity();
            let on_link: crate::OnLink = std::rc::Rc::new(move |dest, _, cx| {
                doc.update(cx, |doc, cx| {
                    doc.pressed.push(dest.clone());
                    cx.notify();
                })
                .unwrap();
            });
            super::render(
                "doc",
                "# Title\n\nSee [rfcs](duck://net-1/forge/rfcs), [local](../a.md) or https://x.example",
                &theme,
                &on_link,
            )
        }
    }

    // Every link of a paragraph presses, a bare URL among them, and hands
    // the caller its destination as written: where it goes (the host, or a
    // file of the repository) is the caller's to decide.
    #[test]
    fn a_pressed_link_hands_its_destination_to_the_caller() {
        let mut cx = TestAppContext::new();
        let doc = cx.open::<Doc>();
        cx.run_until_parked();
        assert!(cx.has_text("Title"));
        for index in 0..3 {
            cx.simulate_rich_click("doc-1", index);
        }
        assert_eq!(
            doc.read(|doc| doc.pressed.clone()),
            ["duck://net-1/forge/rfcs", "../a.md", "https://x.example"]
        );
    }

    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct Sheet;

    impl View for Sheet {
        const NAME: &'static str = "Sheet";
    }

    impl Render for Sheet {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = *cx.global::<view::Theme>();
            let on_link: crate::OnLink = std::rc::Rc::new(|_, _, _| {});
            super::render(
                "sheet",
                "| # | Prefer this shape |\n|---|---|\n| 1 | A feature kind has one home; the registry only collects |",
                &theme,
                &on_link,
            )
        }
    }

    #[test]
    fn a_tables_columns_fit_their_content_and_its_cells_wrap() {
        let mut cx = TestAppContext::new();
        cx.open::<Sheet>();
        cx.run_until_parked();
        let mut max_content_columns = view::div().grid_cols_max_content(2);
        assert_eq!(
            cx.style("sheet-0-table").grid_cols,
            max_content_columns.style().grid_cols
        );
        let cell_white_space: Vec<_> = cx
            .node("sheet-0-table")
            .children()
            .iter()
            .map(|cell| {
                cell.style()
                    .map(|style| cx.styles()[style].text.white_space)
            })
            .collect();
        assert_eq!(cell_white_space, vec![Some(None); 4]);
    }
}
