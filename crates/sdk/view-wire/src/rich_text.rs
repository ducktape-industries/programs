//! Lossless serde adapters for GPUI rich text values.
use crate::{finite, style_sanitize};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HighlightStyle {
    pub color: Option<gpui::Hsla>,
    pub font_weight: Option<gpui::FontWeight>,
    pub font_style: Option<gpui::FontStyle>,
    pub background_color: Option<gpui::Hsla>,
    pub underline: Option<gpui::UnderlineStyle>,
    pub strikethrough: Option<gpui::StrikethroughStyle>,
    pub fade_out: Option<f32>,
}

impl From<gpui::HighlightStyle> for HighlightStyle {
    fn from(value: gpui::HighlightStyle) -> Self {
        Self {
            color: value.color,
            font_weight: value.font_weight,
            font_style: value.font_style,
            background_color: value.background_color,
            underline: value.underline,
            strikethrough: value.strikethrough,
            fade_out: value.fade_out,
        }
    }
}

impl From<HighlightStyle> for gpui::HighlightStyle {
    fn from(value: HighlightStyle) -> Self {
        Self {
            color: value.color,
            font_weight: value.font_weight,
            font_style: value.font_style,
            background_color: value.background_color,
            underline: value.underline,
            strikethrough: value.strikethrough,
            fade_out: value.fade_out,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextRun {
    /// Number of UTF-8 bytes in this run.
    pub len: usize,
    pub font_family: gpui::SharedString,
    pub font_features: gpui::FontFeatures,
    pub font_fallbacks: Option<gpui::FontFallbacks>,
    pub font_weight: gpui::FontWeight,
    pub font_style: gpui::FontStyle,
    pub color: gpui::Hsla,
    pub background_color: Option<gpui::Hsla>,
    pub underline: Option<gpui::UnderlineStyle>,
    pub strikethrough: Option<gpui::StrikethroughStyle>,
}

impl From<gpui::TextRun> for TextRun {
    fn from(value: gpui::TextRun) -> Self {
        Self {
            len: value.len,
            font_family: value.font.family,
            font_features: value.font.features,
            font_fallbacks: value.font.fallbacks,
            font_weight: value.font.weight,
            font_style: value.font.style,
            color: value.color,
            background_color: value.background_color,
            underline: value.underline,
            strikethrough: value.strikethrough,
        }
    }
}

impl From<TextRun> for gpui::TextRun {
    fn from(value: TextRun) -> Self {
        Self {
            len: value.len,
            font: gpui::Font {
                family: value.font_family,
                features: value.font_features,
                fallbacks: value.font_fallbacks,
                weight: value.font_weight,
                style: value.font_style,
            },
            color: value.color,
            background_color: value.background_color,
            underline: value.underline,
            strikethrough: value.strikethrough,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Runs {
    Highlights(Vec<(Range<usize>, HighlightStyle)>),
    Runs(Vec<TextRun>),
}

impl Default for Runs {
    fn default() -> Self {
        Self::Highlights(Vec::new())
    }
}

fn valid_range(text: &str, range: &Range<usize>) -> bool {
    range.start <= range.end
        && range.end <= text.len()
        && text.is_char_boundary(range.start)
        && text.is_char_boundary(range.end)
}

fn sanitize_highlight(value: &mut HighlightStyle) {
    let mut style = gpui::TextStyleRefinement {
        color: value.color,
        font_weight: value.font_weight,
        font_style: value.font_style,
        background_color: value.background_color,
        underline: value.underline,
        strikethrough: value.strikethrough,
        ..Default::default()
    };
    style_sanitize::sanitize_text(&mut style);
    value.color = style.color;
    value.font_weight = style.font_weight;
    value.font_style = style.font_style;
    value.background_color = style.background_color;
    value.underline = style.underline;
    value.strikethrough = style.strikethrough;
    value.fade_out = value.fade_out.map(|value| finite(value).clamp(0., 1.));
}

fn sanitize_run(value: &mut TextRun) {
    let mut style = gpui::TextStyleRefinement {
        font_family: Some(value.font_family.clone()),
        font_features: Some(value.font_features.clone()),
        font_fallbacks: value.font_fallbacks.clone(),
        font_weight: Some(value.font_weight),
        font_style: Some(value.font_style),
        color: Some(value.color),
        background_color: value.background_color,
        underline: value.underline,
        strikethrough: value.strikethrough,
        ..Default::default()
    };
    style_sanitize::sanitize_text(&mut style);
    value.font_family = style.font_family.expect("run font family");
    value.font_features = style.font_features.expect("run font features");
    value.font_fallbacks = style.font_fallbacks;
    value.font_weight = style.font_weight.expect("run font weight");
    value.font_style = style.font_style.expect("run font style");
    value.color = style.color.expect("run color");
    value.background_color = style.background_color;
    value.underline = style.underline;
    value.strikethrough = style.strikethrough;
}

pub(super) fn sanitize(
    text: &str,
    runs: &mut Runs,
    font_family_overrides: &mut Vec<(Range<usize>, gpui::SharedString)>,
    clickable_ranges: &mut Vec<Range<usize>>,
) {
    match runs {
        Runs::Highlights(highlights) => {
            let mut end = 0;
            highlights.retain_mut(|(range, style)| {
                let valid = valid_range(text, range) && range.start >= end;
                if valid {
                    end = range.end;
                    sanitize_highlight(style);
                }
                valid
            });
        }
        Runs::Runs(values) => {
            let mut end = 0usize;
            let valid = values.iter().all(|run| {
                end = end.saturating_add(run.len);
                end <= text.len() && text.is_char_boundary(end)
            }) && end == text.len();
            if valid {
                values.iter_mut().for_each(sanitize_run);
            } else {
                *runs = Runs::Highlights(Vec::new());
            }
        }
    }
    let mut override_end = 0;
    font_family_overrides.retain_mut(|(range, family)| {
        let valid = valid_range(text, range) && range.start >= override_end;
        if valid {
            override_end = range.end;
            let mut style = gpui::TextStyleRefinement {
                font_family: Some(family.clone()),
                ..Default::default()
            };
            style_sanitize::sanitize_text(&mut style);
            *family = style.font_family.expect("font override");
        }
        valid
    });
    for range in clickable_ranges {
        if !valid_range(text, range) || range.start >= range.end {
            *range = 0..0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styles::testing::sanitize_plain;
    use crate::{ElementIdWire, Frame, Node, StyleId};

    #[test]
    fn hostile_utf8_ranges_and_run_lengths_never_reach_gpui() {
        let text = "aéz".to_owned();
        let mut runs = Runs::Highlights(vec![
            (1..2, HighlightStyle::from(gpui::HighlightStyle::default())),
            (1..3, HighlightStyle::from(gpui::HighlightStyle::default())),
        ]);
        let mut overrides = vec![(3..99, "Mono".into()), (1..3, "Mono".into())];
        let mut clicks = vec![1..2, 1..3, 3..4, std::ops::Range { start: 4, end: 3 }];
        sanitize(&text, &mut runs, &mut overrides, &mut clicks);
        assert_eq!(
            runs,
            Runs::Highlights(vec![(1..3, gpui::HighlightStyle::default().into())])
        );
        assert_eq!(overrides, vec![(1..3, "Mono".into())]);
        assert_eq!(clicks, vec![0..0, 1..3, 3..4, 0..0]);

        let mut runs = Runs::Runs(vec![gpui::TextStyle::default().to_run(2).into()]);
        sanitize(&text, &mut runs, &mut vec![], &mut vec![]);
        assert_eq!(runs, Runs::Highlights(vec![]));
    }

    #[test]
    fn a_rich_texts_id_is_a_sibling_identity() {
        let rich = || Node::RichText {
            id: Some(ElementIdWire::Integer(1)),
            style: StyleId(0),
            text: "text".into(),
            runs: Runs::default(),
            font_family_overrides: vec![],
            clickable_ranges: vec![],
            on_click: None,
            on_hover: None,
            tooltip: None,
        };
        let mut duplicate = Frame {
            root: Some(Node::Container(crate::ContainerNode {
                id: Some(ElementIdWire::Name("root".into())),
                style: StyleId(0),
                interactivity: Default::default(),
                children: vec![rich(), rich()],
            })),
            ..Default::default()
        };
        assert!(matches!(
            sanitize_plain(&mut duplicate),
            Err(crate::Refused::Duplicate(_))
        ));
    }
}
