//! A message's blocks as the row draws them (Expo `src/chat/Rich.tsx`):
//! a line of runs per paragraph or quote, list lines with their marker,
//! code blocks that scroll sideways, dividers. Bold is medium, a link
//! underlined in the line's own tone, a mention medium on the accent's
//! soft ground, inline code in the data face on the raised ground. Reads
//! the message and the roster; a press on a link or a mention opens it
//! (the shared runs machinery, `ui::message::rich`).
use view::prelude::*;
use view::{FontStyle, FontWeight, HighlightStyle, UnderlineStyle};

use super::parts::HAIRLINE;
use crate::Chat;
use crate::message::{ChatMessage, ChatSpan, Run, SpanStyle, line, plain_line};
use chat::view::Names;
use chat::{Block, Span};
use view::design::Ladder;

/// A quote's inset past its 2 rule (mobile `src/chat/Rich.tsx:34`).
const QUOTE_INSET: Pixels = px(10.);
/// A list line's marker column, 22 less the line's inset (mobile
/// `src/chat/Rich.tsx:26`).
const MARKER: Pixels = px(16.);
/// A code block's language: the eyebrow at 10 (mobile
/// `src/chat/Rich.tsx:42`).
const CODE_LANG: Pixels = px(10.);
/// A code block's line (mobile `src/chat/Rich.tsx:47`).
const CODE_LINE: Pixels = px(18.);

pub(super) fn blocks(
    chat: &Chat,
    message: &ChatMessage,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    if message.blocks.is_empty() {
        let text = div()
            .id(format!("chat-message-{}-text", message.id))
            .child(message.body.clone());
        return vec![text.into_any_element()];
    }
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    message
        .blocks
        .iter()
        .enumerate()
        .map(|(index, block)| {
            let id = format!("chat-message-{}-block-{index}", message.id);
            block_view(chat, id, block, names, cx, theme, ladder)
        })
        .collect()
}

fn block_view(
    chat: &Chat,
    id: String,
    block: &Block,
    names: &Names,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match block {
        Block::Divider => div()
            .id(id)
            .h(HAIRLINE)
            .my(design::space::SM)
            .w_full()
            .bg(theme.border_strong)
            .into_any_element(),
        Block::Code { lang, text } => code(id, lang.as_deref(), text, theme, ladder),
        Block::Quote(spans) => div()
            .border_l_2()
            .border_color(theme.border_strong)
            .pl(QUOTE_INSET)
            .text_color(theme.muted)
            .child(runs(id.into(), spans, names, cx, theme))
            .into_any_element(),
        Block::Attachment(file) => crate::ui::attachment::view(chat, id, file, cx, theme, ladder),
        Block::Paragraph(spans) => match chat::list_item(spans) {
            Some((marker, item)) => div()
                .id(format!("{id}-item"))
                .flex()
                .pl(design::space::XS)
                .child(div().w(MARKER).flex_none().child(match marker {
                    chat::ListMarker::Bullet => "•".to_owned(),
                    chat::ListMarker::Ordered(number) => format!("{number}."),
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(runs(id.into(), &item, names, cx, theme)),
                )
                .into_any_element(),
            None => runs(id.into(), spans, names, cx, theme).into_any_element(),
        },
    }
}

/// A code block: its language over it, the code in the data face on the
/// raised ground, one line per line, scrolling sideways.
fn code(id: String, lang: Option<&str>, text: &str, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let lang = lang.filter(|lang| !lang.is_empty()).map(|lang| {
        design::eyebrow(lang.to_owned(), ladder, theme)
            .text_type(design::Type {
                size: CODE_LANG,
                ..ladder.eyebrow
            })
            .px(design::space::LG)
            .mb(design::space::XXS)
    });
    let lines = div()
        .id(format!("{id}-scroll"))
        .overflow_x_scroll()
        .px(design::space::LG)
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(design::Type {
            line: Some(CODE_LINE),
            ..ladder.code
        })
        .whitespace_nowrap()
        .child(plain_line(format!("{id}-code").into(), text, false));
    div()
        .id(id)
        .flex()
        .flex_col()
        .my(design::space::XXS)
        .py(design::space::SM)
        .bg(theme.surface_raised)
        .children(lang)
        .child(lines)
        .into_any_element()
}

/// One line of runs in the phone's styles.
fn runs(
    id: ElementId,
    spans: &[Span],
    names: &Names,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> InteractiveText {
    let theme = *theme;
    line(id, spans, names, cx, move |span| run(span, &theme))
}

fn run(span: &ChatSpan, theme: &Theme) -> Run {
    let medium = Some(FontWeight::MEDIUM);
    let style = |style: HighlightStyle| Run::of(span, style, None);
    match &span.style {
        SpanStyle::Plain => style(HighlightStyle::default()),
        SpanStyle::Bold => style(HighlightStyle {
            font_weight: medium,
            ..Default::default()
        }),
        SpanStyle::Italic => style(HighlightStyle {
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        }),
        SpanStyle::BoldItalic => style(HighlightStyle {
            font_weight: medium,
            font_style: Some(FontStyle::Italic),
            ..Default::default()
        }),
        SpanStyle::Link(target) => Run::of(
            span,
            HighlightStyle {
                underline: Some(UnderlineStyle {
                    thickness: HAIRLINE,
                    color: None,
                    wavy: false,
                }),
                ..Default::default()
            },
            Some(target),
        ),
        SpanStyle::Mention(account) => Run::of(
            span,
            HighlightStyle {
                font_weight: medium,
                background_color: Some(theme.accent_soft),
                ..Default::default()
            },
            Some(account),
        ),
        // a space either side, as the Expo span pads it
        SpanStyle::Code => Run {
            text: format!(" {} ", span.text),
            mono: true,
            ..Run::of(
                span,
                HighlightStyle {
                    background_color: Some(theme.surface_raised),
                    ..Default::default()
                },
                None,
            )
        },
    }
}
