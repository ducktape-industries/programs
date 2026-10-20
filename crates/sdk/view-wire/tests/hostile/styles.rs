//! A hostile `StyleRefinement`, and the bounds a sanitized one meets, field
//! by field.
use super::*;
use gpui::{AbsoluteLength, DefiniteLength, Hsla, Length, StyleRefinement};

fn in_range(value: f32, min: f32, max: f32) {
    assert!(
        value.is_finite() && (min..=max).contains(&value),
        "{value} outside {min}..={max}"
    );
}
fn length_in(value: f32, signed: bool) {
    let min = if signed { f32::MIN } else { 0. };
    in_range(value, min, f32::MAX);
}
fn absolute_in(value: AbsoluteLength) {
    signed_absolute_in(value, false);
}
fn signed_absolute_in(value: AbsoluteLength, signed: bool) {
    match value {
        AbsoluteLength::Pixels(value) => length_in(value.into(), signed),
        AbsoluteLength::Rems(value) => length_in(value.0, signed),
    }
}
fn definite_in(value: DefiniteLength) {
    signed_definite_in(value, false);
}
fn signed_definite_in(value: DefiniteLength, signed: bool) {
    match value {
        DefiniteLength::Absolute(value) => signed_absolute_in(value, signed),
        DefiniteLength::Fraction(value) => length_in(value, signed),
    }
}
fn color_in(value: Hsla) {
    for value in [value.h, value.s, value.l, value.a] {
        in_range(value, 0., 1.);
    }
}
pub(super) fn check_native_style(style: &StyleRefinement) {
    let sizes = [
        style.size.width,
        style.size.height,
        style.min_size.width,
        style.min_size.height,
        style.max_size.width,
        style.max_size.height,
    ];
    let signed = [
        style.inset.top,
        style.inset.right,
        style.inset.bottom,
        style.inset.left,
        style.margin.top,
        style.margin.right,
        style.margin.bottom,
        style.margin.left,
        style.flex_basis,
    ];
    for (values, signed) in [(&sizes[..], false), (&signed[..], true)] {
        for value in values.iter().flatten() {
            if let Length::Definite(value) = value {
                signed_definite_in(*value, signed);
            }
        }
    }
    for value in [
        style.padding.top,
        style.padding.right,
        style.padding.bottom,
        style.padding.left,
        style.gap.width,
        style.gap.height,
    ]
    .into_iter()
    .flatten()
    {
        definite_in(value);
    }
    for value in [
        style.border_widths.top,
        style.border_widths.right,
        style.border_widths.bottom,
        style.border_widths.left,
        style.corner_radii.top_left,
        style.corner_radii.top_right,
        style.corner_radii.bottom_left,
        style.corner_radii.bottom_right,
        style.scrollbar_width,
    ]
    .into_iter()
    .flatten()
    {
        absolute_in(value);
    }
    for (value, min, max) in [
        (style.flex_grow, 0., f32::MAX),
        (style.flex_shrink, 0., f32::MAX),
        (style.aspect_ratio, f32::MIN_POSITIVE, f32::MAX),
        (style.opacity, 0., 1.),
    ] {
        if let Some(value) = value {
            in_range(value, min, max);
        }
    }
    if let Some(gpui::Fill::Color(background)) = &style.background {
        match (background.as_solid(), background.as_linear_gradient()) {
            (Some(color), _) => color_in(color),
            (None, Some((angle, stops, _))) => {
                assert!(angle.is_finite());
                for stop in stops {
                    color_in(stop.color);
                    in_range(stop.percentage, 0., 1.);
                }
            }
            (None, None) => panic!("a pattern background was kept"),
        }
    }
    let shadows = style.box_shadow.as_deref().unwrap_or_default();
    for shadow in shadows {
        color_in(shadow.color);
        length_in(shadow.offset.x.into(), true);
        length_in(shadow.offset.y.into(), true);
        length_in(shadow.blur_radius.into(), false);
        length_in(shadow.spread_radius.into(), true);
    }
    for template in [style.grid_cols, style.grid_rows].into_iter().flatten() {
        assert!(template.repeat >= 1);
    }
    if let Some(location) = &style.grid_location {
        for placement in [
            &location.row.start,
            &location.row.end,
            &location.column.start,
            &location.column.end,
        ] {
            match placement {
                gpui::GridPlacement::Span(value) => assert!(*value >= 1),
                gpui::GridPlacement::Line(_) | gpui::GridPlacement::Auto => {}
            }
        }
    }
    for color in [
        style.border_color,
        style.text.color,
        style.text.background_color,
    ]
    .into_iter()
    .flatten()
    {
        color_in(color);
    }
    if let Some(size) = style.text.font_size {
        absolute_in(size);
    }
    if let Some(height) = style.text.line_height {
        definite_in(height);
    }
    if let Some(weight) = style.text.font_weight {
        in_range(weight.0, 1., 1000.);
    }
    if let Some(clamp) = style.text.line_clamp {
        assert!(clamp >= 1);
    }
    if let Some(spacing) = style.text.letter_spacing {
        length_in(spacing.into(), true);
    }
    if let Some(underline) = &style.text.underline {
        length_in(underline.thickness.into(), false);
        if let Some(color) = underline.color {
            color_in(color);
        }
    }
    if let Some(strike) = &style.text.strikethrough {
        length_in(strike.thickness.into(), false);
        if let Some(color) = strike.color {
            color_in(color);
        }
    }
}

pub(super) fn gen_native_style(rng: &mut Rng) -> gpui::StyleRefinement {
    use gpui::{AbsoluteLength, DefiniteLength, px, rems};
    let number = gen_f32;
    let absolute = |rng: &mut Rng| -> AbsoluteLength {
        if rng.next_bool() {
            px(number(rng)).into()
        } else {
            rems(number(rng)).into()
        }
    };
    let definite = |rng: &mut Rng| -> DefiniteLength {
        if rng.next_bool() {
            absolute(rng).into()
        } else {
            DefiniteLength::Fraction(number(rng))
        }
    };
    let mut style = gpui::StyleRefinement::default();
    for value in [
        &mut style.inset.top,
        &mut style.inset.right,
        &mut style.inset.bottom,
        &mut style.inset.left,
        &mut style.size.width,
        &mut style.size.height,
        &mut style.min_size.width,
        &mut style.min_size.height,
        &mut style.max_size.width,
        &mut style.max_size.height,
        &mut style.margin.top,
        &mut style.margin.right,
        &mut style.margin.bottom,
        &mut style.margin.left,
        &mut style.flex_basis,
    ] {
        *value = Some(definite(rng).into());
    }
    for value in [
        &mut style.padding.top,
        &mut style.padding.right,
        &mut style.padding.bottom,
        &mut style.padding.left,
        &mut style.gap.width,
        &mut style.gap.height,
    ] {
        *value = Some(definite(rng));
    }
    for value in [
        &mut style.border_widths.top,
        &mut style.border_widths.right,
        &mut style.border_widths.bottom,
        &mut style.border_widths.left,
        &mut style.corner_radii.top_left,
        &mut style.corner_radii.top_right,
        &mut style.corner_radii.bottom_left,
        &mut style.corner_radii.bottom_right,
        &mut style.scrollbar_width,
    ] {
        *value = Some(absolute(rng));
    }
    style.flex_grow = Some(gen_f32(rng));
    style.flex_shrink = Some(gen_f32(rng));
    style.aspect_ratio = Some(gen_f32(rng));
    style.opacity = Some(gen_f32(rng));
    style.border_color = Some(gen_color(rng));
    style.background = Some(match rng.next_bool() {
        true => gen_color(rng).into(),
        false => gpui::linear_gradient(
            number(rng),
            gpui::linear_color_stop(gen_color(rng), number(rng)),
            gpui::linear_color_stop(gen_color(rng), number(rng)),
        )
        .into(),
    });
    style.box_shadow = Some(
        (0..rng.next_range(20))
            .map(|_| gpui::BoxShadow {
                color: gen_color(rng),
                offset: gpui::point(px(gen_f32(rng)), px(gen_f32(rng))),
                blur_radius: px(gen_f32(rng)),
                spread_radius: px(gen_f32(rng)),
                inset: false,
            })
            .collect(),
    );
    style.grid_cols = Some(gpui::GridTemplate {
        repeat: rng.next_u64() as u16,
        ..Default::default()
    });
    style.grid_rows = Some(gpui::GridTemplate {
        repeat: rng.next_u64() as u16,
        ..Default::default()
    });
    style.grid_location = Some(gpui::GridLocation {
        row: gpui::GridPlacement::Line(rng.next_u64() as i16)
            ..gpui::GridPlacement::Span(rng.next_u64() as u16),
        column: gpui::GridPlacement::Span(rng.next_u64() as u16)
            ..gpui::GridPlacement::Line(rng.next_u64() as i16),
    });
    style.text.color = Some(gen_color(rng));
    style.text.background_color = Some(gen_color(rng));
    style.text.font_size = Some(absolute(rng));
    style.text.line_height = Some(definite(rng));
    style.text.font_weight = Some(gpui::FontWeight(gen_f32(rng)));
    style.text.line_clamp = Some(rng.next_u64() as usize);
    style.text.underline = Some(gpui::UnderlineStyle {
        thickness: px(gen_f32(rng)),
        color: Some(gen_color(rng)),
        wavy: true,
    });
    style.text.strikethrough = Some(gpui::StrikethroughStyle {
        thickness: px(gen_f32(rng)),
        color: Some(gen_color(rng)),
    });
    style.text.letter_spacing = Some(px(gen_f32(rng)));
    style
}

pub(super) fn gen_color(rng: &mut Rng) -> gpui::Hsla {
    gpui::Hsla {
        h: gen_f32(rng),
        s: gen_f32(rng),
        l: gen_f32(rng),
        a: gen_f32(rng),
    }
}

#[test]
fn a_style_entry_is_the_same_bytes_however_the_writer_walks_it() {
    use gpui::{Styled, px};
    let mut corpus = vec![
        StyleRefinement::default(),
        StyleRefinement::default().opacity(0.5),
        StyleRefinement::default().w(px(2.)),
        StyleRefinement::default()
            .flex()
            .pt(px(1.))
            .text_color(gpui::red()),
    ];
    let mut rng = Rng::new(0x5717);
    for round in 0..256u32 {
        let mut style = gen_native_style(&mut rng);
        let mut emptied = |bit: u32, empty: &mut dyn FnMut(&mut StyleRefinement)| {
            if round >> bit & 1 == 1 {
                empty(&mut style);
            }
        };
        emptied(0, &mut |style| style.inset = Default::default());
        emptied(1, &mut |style| style.size = Default::default());
        emptied(2, &mut |style| style.min_size.width = None);
        emptied(3, &mut |style| style.margin = Default::default());
        emptied(4, &mut |style| style.padding = Default::default());
        emptied(5, &mut |style| style.border_widths.left = None);
        emptied(6, &mut |style| style.corner_radii = Default::default());
        emptied(7, &mut |style| style.text = Default::default());
        emptied(0, &mut |style| style.gap = Default::default());
        emptied(1, &mut |style| style.max_size = Default::default());
        emptied(2, &mut |style| style.background = None);
        emptied(3, &mut |style| style.box_shadow = None);
        corpus.push(style);
    }
    let mut digest = 0xcbf2_9ce4_8422_2325u64;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            digest = (digest ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for style in &corpus {
        let entry = encode(&Style::new(style));
        feed(&(entry.len() as u64).to_le_bytes());
        feed(&entry);
    }
    assert_eq!(corpus.len(), 260);
    assert_eq!(digest, 0x025d_432c_889f_c584, "{digest:#018x}");
}
