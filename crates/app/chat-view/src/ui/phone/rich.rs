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
use crate::markup::{Piece, opens, pieces};
use crate::message::{ChatMessage, ChatSpan, Run, SpanStyle, line};
use chat::view::Names;
use chat::{Block, Span};
use view::design::Ladder;

const QUOTE_INSET: Pixels = px(10.);
const MARKER: Pixels = px(16.);
const CODE_LANG: Pixels = px(10.);

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
    let key = format!("chat-message-{}", message.id);
    drawn(chat, &key, &message.blocks, cx, theme, ladder)
}

pub(super) fn drawn(
    chat: &Chat,
    key: &str,
    blocks: &[Block],
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    pieces(blocks, names)
        .into_iter()
        .map(|(index, piece)| {
            let id = format!("{key}-block-{index}");
            match piece {
                Piece::Chat(block) => block_view(chat, id, block, names, cx, theme, ladder),
                Piece::Markdown(block) => {
                    markdown::phone::block(&id, &block, &opens(cx), ladder, theme)
                }
            }
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

// A code block: its language over it, then the shared code panel, the
// code in the data face on the raised ground in the colours of its
// language, scrolling sideways.
fn code(id: String, lang: Option<&str>, text: &str, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let lang = lang.unwrap_or_default();
    let eyebrow = (!lang.is_empty()).then(|| {
        design::eyebrow(lang.to_owned(), ladder, theme)
            .text_type(design::Type {
                size: CODE_LANG,
                ..ladder.eyebrow
            })
            .mb(design::space::XXS)
    });
    div()
        .id(id.clone())
        .flex()
        .flex_col()
        .my(design::space::XXS)
        .children(eyebrow)
        .child(markdown::phone::code_box(&id, lang, text, ladder, theme))
        .into_any_element()
}

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
