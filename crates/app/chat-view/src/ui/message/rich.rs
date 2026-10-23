//! The desktop's runs of a message's styled lines, over the machinery both
//! designs share ([`crate::message::line`]).
use super::*;
use crate::find::Found;
use crate::message::{Run, marked_line};

// The desktop's runs: bold is bold, a link and a mention in the link
// tone, inline code on the raised ground at the paragraph's size with a
// space either side, as the phone pads it and forge-view grounds it.
pub(crate) fn rich_line(
    id: ElementId,
    spans: &[Span],
    names: &Names,
    found: &Found,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> InteractiveText {
    let theme = *theme;
    marked_line(id, spans, names, found, cx, move |span| {
        let mut style = HighlightStyle::default();
        match &span.style {
            SpanStyle::Plain => {}
            SpanStyle::Bold => style.font_weight = Some(FontWeight::BOLD),
            SpanStyle::Italic => style.font_style = Some(FontStyle::Italic),
            SpanStyle::BoldItalic => {
                style.font_weight = Some(FontWeight::BOLD);
                style.font_style = Some(FontStyle::Italic);
            }
            SpanStyle::Link(target) => {
                style.color = Some(theme.link);
                style.font_weight = Some(FontWeight::MEDIUM);
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(theme.link),
                    wavy: false,
                });
                return Run::of(span, style, Some(target));
            }
            SpanStyle::Mention(account) => {
                style.color = Some(theme.link);
                style.font_weight = Some(FontWeight::MEDIUM);
                return Run::of(span, style, Some(account));
            }
            SpanStyle::Code => {
                style.background_color = Some(theme.surface_raised);
                return Run {
                    text: format!(" {} ", span.text),
                    mono: true,
                    ..Run::of(span, style, None)
                };
            }
        }
        Run::of(span, style, None)
    })
}
