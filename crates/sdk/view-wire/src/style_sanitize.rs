//! Bounds applied before a view refinement reaches native layout or painting.
//!
//! The rule for this file: make numbers finite, hold a value to the domain
//! it has (a size is not negative, a colour channel is `0..=1`), refuse a
//! shape it cannot keep, and never silently delete a valid value. A
//! background this file cannot read back (a pattern) refuses the frame. A
//! value inside its domain passes unchanged, so a negative margin stays
//! negative and a gradient stays a gradient.
//!
//! The host clips the entire view slot (`render.rs`, the slot's
//! `overflow_hidden`): a negative margin or inset moves an element only
//! inside that clip, and local clipping cannot contain a deferred or
//! anchored element on its own.
use gpui::{
    AbsoluteLength, Background, DefiniteLength, Fill, GridPlacement, Hsla, Length, StyleRefinement,
    TextStyleRefinement, px,
};

pub(crate) fn clamp_finite(value: &mut f32, min: f32, max: f32) {
    *value = if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    };
}
fn bound(value: &mut f32, signed: bool) {
    match signed {
        true if value.is_nan() => *value = 0.,
        true => *value = value.clamp(f32::MIN, f32::MAX),
        false => clamp_finite(value, 0., f32::MAX),
    }
}
fn absolute(value: &mut AbsoluteLength, signed: bool) {
    match value {
        AbsoluteLength::Pixels(value) => {
            let mut number = f32::from(*value);
            bound(&mut number, signed);
            *value = px(number);
        }
        AbsoluteLength::Rems(value) => bound(&mut value.0, signed),
    }
}
fn definite(value: &mut DefiniteLength, signed: bool) {
    match value {
        DefiniteLength::Absolute(value) => absolute(value, signed),
        DefiniteLength::Fraction(value) => bound(value, signed),
    }
}
fn length(value: &mut Length, signed: bool) {
    if let Length::Definite(value) = value {
        definite(value, signed);
    }
}
pub(crate) fn sanitize_hsla(value: &mut Hsla) {
    for number in [&mut value.h, &mut value.s, &mut value.l, &mut value.a] {
        clamp_finite(number, 0., 1.);
    }
}
fn background(value: Background) -> Result<Background, &'static str> {
    if let Some(mut color) = value.as_solid() {
        sanitize_hsla(&mut color);
        return Ok(color.into());
    }
    let Some((mut angle, mut stops, space)) = value.as_linear_gradient() else {
        return Err("a pattern background does not cross the view wire");
    };
    if !angle.is_finite() {
        angle = 0.;
    }
    for stop in &mut stops {
        sanitize_hsla(&mut stop.color);
        clamp_finite(&mut stop.percentage, 0., 1.);
    }
    Ok(gpui::linear_gradient(angle, stops[0], stops[1]).color_space(space))
}
fn grid(value: &mut GridPlacement) {
    match value {
        GridPlacement::Span(value) => *value = (*value).max(1),
        GridPlacement::Line(_) | GridPlacement::Auto => {}
    }
}

pub(crate) fn sanitize(style: &mut StyleRefinement) -> Result<(), &'static str> {
    for value in [
        &mut style.size.width,
        &mut style.size.height,
        &mut style.min_size.width,
        &mut style.min_size.height,
        &mut style.max_size.width,
        &mut style.max_size.height,
    ]
    .into_iter()
    .flatten()
    {
        length(value, false);
    }
    for value in [
        &mut style.inset.top,
        &mut style.inset.right,
        &mut style.inset.bottom,
        &mut style.inset.left,
        &mut style.margin.top,
        &mut style.margin.right,
        &mut style.margin.bottom,
        &mut style.margin.left,
        &mut style.flex_basis,
    ]
    .into_iter()
    .flatten()
    {
        length(value, true);
    }
    for value in [
        &mut style.padding.top,
        &mut style.padding.right,
        &mut style.padding.bottom,
        &mut style.padding.left,
        &mut style.gap.width,
        &mut style.gap.height,
    ]
    .into_iter()
    .flatten()
    {
        definite(value, false);
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
    ]
    .into_iter()
    .flatten()
    {
        absolute(value, false);
    }
    for value in [&mut style.flex_grow, &mut style.flex_shrink]
        .into_iter()
        .flatten()
    {
        clamp_finite(value, 0., f32::MAX);
    }
    if let Some(value) = &mut style.aspect_ratio {
        clamp_finite(value, f32::MIN_POSITIVE, f32::MAX);
    }
    if let Some(value) = &mut style.opacity {
        clamp_finite(value, 0., 1.);
    }
    if let Some(value) = &mut style.border_color {
        sanitize_hsla(value);
    }
    if let Some(Fill::Color(value)) = &mut style.background {
        *value = background(*value)?;
    }
    if let Some(shadows) = &mut style.box_shadow {
        for shadow in shadows {
            sanitize_hsla(&mut shadow.color);
            for offset in [&mut shadow.offset.x, &mut shadow.offset.y] {
                let mut number = f32::from(*offset);
                bound(&mut number, true);
                *offset = px(number);
            }
            let mut blur = f32::from(shadow.blur_radius);
            bound(&mut blur, false);
            shadow.blur_radius = px(blur);
            let mut spread = f32::from(shadow.spread_radius);
            bound(&mut spread, true);
            shadow.spread_radius = px(spread);
        }
    }
    for template in [&mut style.grid_cols, &mut style.grid_rows]
        .into_iter()
        .flatten()
    {
        template.repeat = template.repeat.max(1);
    }
    if let Some(location) = &mut style.grid_location {
        for value in [
            &mut location.row.start,
            &mut location.row.end,
            &mut location.column.start,
            &mut location.column.end,
        ] {
            grid(value);
        }
    }
    sanitize_text(&mut style.text);
    #[cfg(debug_assertions)]
    {
        style.debug = None;
        style.debug_below = None;
    }
    Ok(())
}

pub(crate) fn sanitize_text(text: &mut TextStyleRefinement) {
    if let Some(value) = &mut text.color {
        sanitize_hsla(value);
    }
    if let Some(value) = &mut text.background_color {
        sanitize_hsla(value);
    }
    if let Some(value) = &mut text.font_size {
        absolute(value, false);
    }
    if let Some(value) = &mut text.line_height {
        definite(value, false);
    }
    if let Some(value) = &mut text.font_weight {
        clamp_finite(&mut value.0, 1., 1000.);
    }
    if let Some(value) = &mut text.line_clamp {
        *value = (*value).max(1);
    }
    if let Some(value) = &mut text.letter_spacing {
        let mut number = f32::from(*value);
        bound(&mut number, true);
        *value = px(number);
    }
    if let Some(features) = &mut text.font_features {
        let features = std::sync::Arc::make_mut(&mut features.0);
        features.retain(|(tag, _)| tag.len() == 4 && tag.is_ascii());
    }
    if let Some(value) = &mut text.underline {
        let mut thickness = f32::from(value.thickness);
        bound(&mut thickness, false);
        value.thickness = px(thickness);
        if let Some(value) = &mut value.color {
            sanitize_hsla(value);
        }
    }
    if let Some(value) = &mut text.strikethrough {
        let mut thickness = f32::from(value.thickness);
        bound(&mut thickness, false);
        value.thickness = px(thickness);
        if let Some(value) = &mut value.color {
            sanitize_hsla(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Overflow, Position, Styled, relative, rems, rgb};

    #[test]
    fn negative_margins_and_insets_keep_their_sign() {
        let mut style = StyleRefinement::default()
            .absolute()
            .left(px(-50.))
            .m(px(-20.))
            .mt(rems(-1e6))
            .mr(relative(-3.))
            .flex_basis(px(-1e9));
        style.margin.bottom = Some(px(f32::NAN).into());
        sanitize(&mut style).unwrap();
        assert_eq!(style.position, Some(Position::Absolute));
        assert_eq!(style.inset.left, Some(px(-50.).into()));
        assert_eq!(style.margin.left, Some(px(-20.).into()));
        assert_eq!(style.margin.top, Some(rems(-1e6).into()));
        assert_eq!(style.margin.right, Some(relative(-3.).into()));
        assert_eq!(style.margin.bottom, Some(px(0.).into()));
        assert_eq!(style.flex_basis, Some(px(-1e9).into()));
        let mut size = StyleRefinement::default().w(px(-5.));
        sanitize(&mut size).unwrap();
        assert_eq!(size.size.width, Some(px(0.).into()));
    }
    #[test]
    fn a_nonfinite_dimension_is_zero_and_a_finite_one_is_kept() {
        let mut style = StyleRefinement::default()
            .w(px(f32::INFINITY))
            .h(rems(10000.))
            .min_w(relative(1000.));
        style.max_size.height = Some(px(1e20).into());
        sanitize(&mut style).unwrap();
        assert_eq!(style.size.width, Some(px(0.).into()));
        assert_eq!(style.size.height, Some(rems(10000.).into()));
        assert_eq!(style.min_size.width, Some(relative(1000.).into()));
        assert_eq!(style.max_size.height, Some(px(1e20).into()));
    }
    #[test]
    fn opacity_and_color_are_finite_and_bounded() {
        let mut style = StyleRefinement::default().opacity(f32::NAN);
        style.border_color = Some(Hsla {
            h: -1.,
            s: 2.,
            l: f32::INFINITY,
            a: 9.,
        });
        sanitize(&mut style).unwrap();
        assert_eq!(style.opacity, Some(0.));
        assert_eq!(
            style.border_color,
            Some(Hsla {
                h: 0.,
                s: 1.,
                l: 0.,
                a: 1.
            })
        );
    }
    #[test]
    fn a_shadow_is_finite_and_its_blur_is_not_negative() {
        let mut style = StyleRefinement {
            box_shadow: Some(vec![
                gpui::BoxShadow {
                    color: rgb(0).into(),
                    offset: gpui::point(px(f32::NAN), px(1000.)),
                    blur_radius: px(-1e9),
                    spread_radius: px(-1e9),
                    inset: false,
                };
                100
            ]),
            ..Default::default()
        };
        sanitize(&mut style).unwrap();
        let shadows = style.box_shadow.unwrap();
        assert_eq!(shadows.len(), 100);
        assert_eq!(shadows[0].blur_radius, px(0.));
        assert_eq!(shadows[0].offset.x, px(0.));
        assert_eq!(shadows[0].offset.y, px(1000.));
        assert_eq!(shadows[0].spread_radius, px(-1e9));
    }
    #[test]
    fn letter_spacing_keeps_its_sign_and_is_finite() {
        for (authored, bounded) in [
            (-0.22, -0.22),
            (1.44, 1.44),
            (f32::NAN, 0.),
            (f32::INFINITY, f32::MAX),
            (f32::NEG_INFINITY, f32::MIN),
        ] {
            let mut style = StyleRefinement::default().letter_spacing(px(authored));
            sanitize(&mut style).unwrap();
            assert_eq!(style.text.letter_spacing, Some(px(bounded)), "{authored}");
        }
    }
    #[test]
    fn gpuis_own_shadows_cross_as_authored() {
        for style in [
            StyleRefinement::default().shadow_sm(),
            StyleRefinement::default().shadow_md(),
            StyleRefinement::default().shadow_lg(),
            StyleRefinement::default().shadow_xl(),
            StyleRefinement::default().shadow_2xl(),
        ] {
            let mut sanitized = style.clone();
            sanitize(&mut sanitized).unwrap();
            assert_eq!(sanitized.box_shadow, style.box_shadow);
        }
    }
    #[test]
    fn a_grid_repeat_or_span_of_zero_is_one() {
        let mut style = StyleRefinement::default().text_size(px(-1.));
        style.grid_cols = Some(gpui::GridTemplate {
            repeat: 0,
            ..Default::default()
        });
        style.grid_location = Some(gpui::GridLocation {
            row: GridPlacement::Span(0)..GridPlacement::Line(i16::MIN),
            column: GridPlacement::Auto..GridPlacement::Auto,
        });
        sanitize(&mut style).unwrap();
        assert_eq!(style.text.font_size, Some(px(0.).into()));
        assert_eq!(style.grid_cols.unwrap().repeat, 1);
        assert_eq!(
            style.grid_location.unwrap().row,
            GridPlacement::Span(1)..GridPlacement::Line(i16::MIN)
        );
    }
    #[test]
    fn preserves_normal_line_spacing_and_is_idempotent() {
        let mut style = StyleRefinement::default()
            .text_size(px(16.))
            .line_height(relative(1.5));
        style.overflow.x = Some(Overflow::Visible);
        sanitize(&mut style).unwrap();
        assert_eq!(style.text.line_height, Some(relative(1.5)));
        assert_eq!(style.overflow.x, Some(Overflow::Visible));
        let once = style.clone();
        sanitize(&mut style).unwrap();
        assert_eq!(style, once);
    }
    #[test]
    fn a_gradient_keeps_its_stops_bounded_and_a_pattern_is_refused() {
        let gradient = gpui::linear_gradient(
            30.,
            gpui::linear_color_stop(rgb(0xff0000), 0.25),
            gpui::linear_color_stop(rgb(0x0000ff), 1.),
        )
        .color_space(gpui::ColorSpace::Oklab);
        let mut style = StyleRefinement::default().bg(gradient);
        sanitize(&mut style).unwrap();
        assert_eq!(style.background, Some(gradient.into()));
        let hostile = gpui::linear_gradient(
            f32::NAN,
            gpui::linear_color_stop(
                Hsla {
                    h: 2.,
                    s: -1.,
                    l: 0.5,
                    a: f32::INFINITY,
                },
                -4.,
            ),
            gpui::linear_color_stop(rgb(0x0000ff), f32::NAN),
        );
        let mut style = StyleRefinement::default().bg(hostile);
        sanitize(&mut style).unwrap();
        let Some(Fill::Color(kept)) = style.background else {
            panic!("the gradient is kept")
        };
        let (angle, [from, to], _) = kept.as_linear_gradient().expect("still a gradient");
        assert_eq!(angle, 0.);
        assert_eq!(
            (from.color, from.percentage, to.percentage),
            (
                Hsla {
                    h: 1.,
                    s: 0.,
                    l: 0.5,
                    a: 0.
                },
                0.,
                0.
            )
        );
        let mut solid = StyleRefinement::default().bg(rgb(0x123456));
        let before = solid.background.clone();
        sanitize(&mut solid).unwrap();
        assert_eq!(solid.background, before);
        let mut pattern = StyleRefinement::default().bg(gpui::pattern_slash(rgb(0), 1., 4.));
        assert_eq!(
            sanitize(&mut pattern),
            Err("a pattern background does not cross the view wire")
        );
    }
}
