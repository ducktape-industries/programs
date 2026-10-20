//! One table entry's bytes: a `StyleRefinement` as the bitmap of the fields
//! it sets, then those fields in declaration order. A nested refinement (an
//! edge set, the text style) is a field like any other, with a bitmap of
//! its own. No name crosses, and a field that says nothing costs a bit.
//!
//! The view writes these bytes for every node it lowers, since they are
//! what [`Interner`](super::Interner) tells two styles apart by: so nothing
//! here formats a number or converts a colour on the way out. A length is
//! one tag and one `f32`; a colour is its four `f32`.
//!
//! gpui's own serde for these types (a colour as `#rrggbbaa`, a length as
//! `16px`) stays what a JSON reader sees. Each struct is taken apart with
//! no `..`, so a field gpui adds fails to build here until it is listed.

use std::sync::Arc;

use gpui::{
    AbsoluteLength, AlignContent, AlignItems, Background, BorderStyle, BoxShadow, ColorSpace,
    CornersRefinement, CursorStyle, DefiniteLength, Display, EdgesRefinement, Fill, FlexDirection,
    FlexWrap, FontFallbacks, FontFeatures, FontStyle, FontWeight, GridLocation, GridPlacement,
    GridTemplate, GridTemplateMinSize, Hsla, Length, LinearColorStop, Overflow, Pixels,
    PointRefinement, Position, Rgba, SharedString, SizeRefinement, StrikethroughStyle,
    StyleRefinement, TextAlign, TextOverflow, TextStyleRefinement, UnderlineStyle, Visibility,
    WhiteSpace, px,
};

const CUT: &str = "a style entry ends before its fields do";

pub(super) struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], &'static str> {
        let (head, rest) = self.0.split_at_checked(len).ok_or(CUT)?;
        self.0 = rest;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], &'static str> {
        Ok(self.take(N)?.try_into().expect("N bytes were taken"))
    }
}

trait Value: Sized {
    fn put(&self, out: &mut Vec<u8>);
    fn get(input: &mut Reader) -> Result<Self, &'static str>;
}

trait Field: Sized {
    fn says(&self) -> bool;
    fn put(&self, out: &mut Vec<u8>);
    fn get(input: &mut Reader) -> Result<Self, &'static str>;
}

impl<T: Value> Field for Option<T> {
    fn says(&self) -> bool {
        self.is_some()
    }
    fn put(&self, out: &mut Vec<u8>) {
        if let Some(value) = self {
            value.put(out);
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        T::get(input).map(Some)
    }
}

trait Refinement: Default {
    const FIELDS: &'static [&'static str];
    fn present(&self) -> u64;
    fn write(&self, out: &mut Vec<u8>);
    fn read(input: &mut Reader) -> Result<Self, &'static str>;
}

macro_rules! refinement {
    (($($bound:ident)?) $type:ty { $($field:ident),+ $(,)? } $($unsent:tt)*) => {
        impl<$($bound: Value + Clone + Default + std::fmt::Debug + PartialEq)?> Refinement for $type {
            const FIELDS: &'static [&'static str] = &[$(stringify!($field)),+];

            fn present(&self) -> u64 {
                let Self { $($field,)+ $($unsent)* } = self;
                let (mut present, mut bit) = (0u64, 0);
                $(
                    present |= u64::from(Field::says($field)) << bit;
                    bit += 1;
                )+
                let _ = bit;
                present
            }

            fn write(&self, out: &mut Vec<u8>) {
                let Self { $($field,)+ $($unsent)* } = self;
                let present = self.present();
                let width = Self::FIELDS.len().div_ceil(8);
                out.extend_from_slice(&present.to_le_bytes()[..width]);
                let mut bit = 0;
                $(
                    if present >> bit & 1 == 1 {
                        Field::put($field, out);
                    }
                    bit += 1;
                )+
                let _ = bit;
            }

            fn read(input: &mut Reader) -> Result<Self, &'static str> {
                let width = Self::FIELDS.len().div_ceil(8);
                let mut bytes = [0; 8];
                bytes[..width].copy_from_slice(input.take(width)?);
                let present = u64::from_le_bytes(bytes);
                if present >> Self::FIELDS.len() != 0 {
                    return Err("a style entry sets a field this build does not have");
                }
                let mut value = Self::default();
                let mut bit = 0;
                $(
                    if present >> bit & 1 == 1 {
                        value.$field = Field::get(input)?;
                    }
                    bit += 1;
                )+
                let _ = bit;
                Ok(value)
            }
        }

        impl<$($bound: Value + Clone + Default + std::fmt::Debug + PartialEq)?> Field for $type {
            fn says(&self) -> bool {
                self.present() != 0
            }
            fn put(&self, out: &mut Vec<u8>) {
                self.write(out);
            }
            fn get(input: &mut Reader) -> Result<Self, &'static str> {
                Self::read(input)
            }
        }
    };
}

refinement!(() StyleRefinement {
    display,
    visibility,
    overflow,
    scrollbar_width,
    allow_concurrent_scroll,
    restrict_scroll_to_axis,
    position,
    inset,
    size,
    min_size,
    max_size,
    aspect_ratio,
    margin,
    padding,
    border_widths,
    align_items,
    align_self,
    align_content,
    justify_content,
    gap,
    flex_direction,
    flex_wrap,
    flex_basis,
    flex_grow,
    flex_shrink,
    background,
    border_color,
    border_style,
    corner_radii,
    box_shadow,
    text,
    mouse_cursor,
    opacity,
    grid_cols,
    grid_rows,
    grid_location,
}
#[cfg(debug_assertions)]
debug: _,
#[cfg(debug_assertions)]
debug_below: _,
);
refinement!(() TextStyleRefinement {
    color,
    font_family,
    font_features,
    font_fallbacks,
    font_size,
    line_height,
    font_weight,
    font_style,
    background_color,
    underline,
    strikethrough,
    white_space,
    text_overflow,
    text_align,
    line_clamp,
    letter_spacing,
});
refinement!((T) EdgesRefinement<T> { top, right, bottom, left });
refinement!((T) SizeRefinement<T> { width, height });
refinement!((T) PointRefinement<T> { x, y });
refinement!((T) CornersRefinement<T> {
    top_left,
    top_right,
    bottom_right,
    bottom_left,
});

pub(super) const EMPTY: [u8; StyleRefinement::FIELDS.len().div_ceil(8)] =
    [0; StyleRefinement::FIELDS.len().div_ceil(8)];

pub(super) fn write(style: &StyleRefinement, out: &mut Vec<u8>) {
    Refinement::write(style, out);
}

pub(super) fn read(bytes: &[u8]) -> Result<StyleRefinement, &'static str> {
    let mut input = Reader(bytes);
    let style = StyleRefinement::read(&mut input)?;
    let mut next = StyleRefinement::FIELDS.len() as u64;
    while !input.0.is_empty() {
        let field = get_count(&mut input)?;
        if field < next {
            return Err("a style entry runs past its fields");
        }
        next = field + 1;
        let len = get_count(&mut input)?;
        input.take(usize::try_from(len).map_err(|_| "a style entry runs past its fields")?)?;
    }
    Ok(style)
}

pub(super) fn fields() -> [(&'static str, &'static [&'static str]); 6] {
    [
        ("StyleRefinement", StyleRefinement::FIELDS),
        ("TextStyleRefinement", TextStyleRefinement::FIELDS),
        ("EdgesRefinement", EdgesRefinement::<Length>::FIELDS),
        ("SizeRefinement", SizeRefinement::<Length>::FIELDS),
        ("PointRefinement", PointRefinement::<Overflow>::FIELDS),
        (
            "CornersRefinement",
            CornersRefinement::<AbsoluteLength>::FIELDS,
        ),
    ]
}

fn put_seven_bit_count(out: &mut Vec<u8>, mut count: u64) {
    while count >= 0x80 {
        out.push(count as u8 | 0x80);
        count >>= 7;
    }
    out.push(count as u8);
}

fn get_count(input: &mut Reader) -> Result<u64, &'static str> {
    let mut count = 0;
    for shift in (0..64).step_by(7) {
        let [byte] = input.array()?;
        count |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(count);
        }
    }
    Err("a style entry's count does not end")
}

fn put_list<T>(out: &mut Vec<u8>, items: &[T], put: impl Fn(&T, &mut Vec<u8>)) {
    put_seven_bit_count(out, items.len() as u64);
    for item in items {
        put(item, out);
    }
}

fn get_list<T>(
    input: &mut Reader,
    get: impl Fn(&mut Reader) -> Result<T, &'static str>,
) -> Result<Vec<T>, &'static str> {
    let mut items = Vec::new();
    for _ in 0..get_count(input)? {
        items.push(get(input)?);
    }
    Ok(items)
}

impl Value for f32 {
    fn put(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        input.array().map(f32::from_le_bytes)
    }
}

impl Value for bool {
    fn put(&self, out: &mut Vec<u8>) {
        out.push(u8::from(*self));
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        match input.array()? {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err("a style entry's flag is neither set nor clear"),
        }
    }
}

impl Value for usize {
    fn put(&self, out: &mut Vec<u8>) {
        put_seven_bit_count(out, *self as u64);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        Ok(usize::try_from(get_count(input)?).unwrap_or(usize::MAX))
    }
}

impl Value for String {
    fn put(&self, out: &mut Vec<u8>) {
        put_seven_bit_count(out, self.len() as u64);
        out.extend_from_slice(self.as_bytes());
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let len = usize::try_from(get_count(input)?).map_err(|_| CUT)?;
        String::from_utf8(input.take(len)?.to_vec())
            .map_err(|_| "a style entry's text is not UTF-8")
    }
}

impl Value for SharedString {
    fn put(&self, out: &mut Vec<u8>) {
        put_seven_bit_count(out, self.len() as u64);
        out.extend_from_slice(self.as_bytes());
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        String::get(input).map(Into::into)
    }
}

impl Value for Pixels {
    fn put(&self, out: &mut Vec<u8>) {
        f32::from(*self).put(out);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        f32::get(input).map(px)
    }
}

impl Value for Hsla {
    fn put(&self, out: &mut Vec<u8>) {
        for channel in [self.h, self.s, self.l, self.a] {
            channel.put(out);
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let [h, s, l, a] = [(); 4].map(|()| f32::get(input));
        let color = Rgba::from(Hsla {
            h: h?,
            s: s?,
            l: l?,
            a: a?,
        });
        let channels = [color.r, color.g, color.b, color.a].map(|c| (c * 255.0).round() as u8);
        Ok(gpui::rgba(u32::from_be_bytes(channels)).into())
    }
}

const PIXELS: u8 = 0;
const REMS: u8 = 1;
const FRACTION: u8 = 2;
const AUTO: u8 = 3;

fn put_length(out: &mut Vec<u8>, unit: u8, amount: f32) {
    out.push(unit);
    amount.put(out);
}

impl Value for AbsoluteLength {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Pixels(pixels) => put_length(out, PIXELS, f32::from(*pixels)),
            Self::Rems(rems) => put_length(out, REMS, rems.0),
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        match DefiniteLength::get(input)? {
            DefiniteLength::Absolute(length) => Ok(length),
            DefiniteLength::Fraction(_) => Err("a fraction where a style takes pixels or rems"),
        }
    }
}

impl Value for DefiniteLength {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Absolute(length) => length.put(out),
            Self::Fraction(fraction) => put_length(out, FRACTION, *fraction),
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        match Length::get(input)? {
            Length::Definite(length) => Ok(length),
            Length::Auto => Err("auto where a style takes a length"),
        }
    }
}

impl Value for Length {
    fn put(&self, out: &mut Vec<u8>) {
        match self {
            Self::Definite(length) => length.put(out),
            Self::Auto => put_length(out, AUTO, 0.),
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let [unit] = input.array()?;
        let amount = f32::get(input)?;
        match unit {
            PIXELS => Ok(px(amount).into()),
            REMS => Ok(gpui::rems(amount).into()),
            FRACTION => Ok(gpui::relative(amount).into()),
            AUTO => Ok(Self::Auto),
            _ => Err("a style entry's length has no such unit"),
        }
    }
}

macro_rules! tags {
    ($($type:ident { $($variant:ident),+ $(,)? })+) => {$(
        impl Value for $type {
            fn put(&self, out: &mut Vec<u8>) {
                match self {
                    $(Self::$variant => {})+
                }
                out.push(*self as u8);
            }
            fn get(input: &mut Reader) -> Result<Self, &'static str> {
                let [tag] = input.array()?;
                [$(Self::$variant),+]
                    .into_iter()
                    .find(|variant| *variant as u8 == tag)
                    .ok_or(concat!("a style entry names no ", stringify!($type)))
            }
        }
    )+};
}

tags! {
    Display { Block, Flex, Grid, None }
    Visibility { Visible, Hidden }
    Overflow { Visible, Clip, Hidden, Scroll }
    Position { Relative, Absolute }
    AlignItems { Start, End, FlexStart, FlexEnd, Center, Baseline, Stretch }
    AlignContent {
        Start, End, FlexStart, FlexEnd, Center, Stretch, SpaceBetween, SpaceEvenly, SpaceAround,
    }
    FlexDirection { Row, Column, RowReverse, ColumnReverse }
    FlexWrap { NoWrap, Wrap, WrapReverse }
    BorderStyle { Solid, Dashed }
    FontStyle { Normal, Italic, Oblique }
    WhiteSpace { Normal, Nowrap }
    TextAlign { Left, Center, Right }
    GridTemplateMinSize { Zero, MinContent, MaxContent }
    ColorSpace { Srgb, Oklab }
    CursorStyle {
        Arrow, IBeam, Crosshair, ClosedHand, OpenHand, PointingHand, ResizeLeft, ResizeRight,
        ResizeLeftRight, ResizeUp, ResizeDown, ResizeUpDown, ResizeUpLeftDownRight,
        ResizeUpRightDownLeft, ResizeColumn, ResizeRow, IBeamCursorForVerticalLayout,
        OperationNotAllowed, DragLink, DragCopy, ContextualMenu,
    }
}

impl Value for FontWeight {
    fn put(&self, out: &mut Vec<u8>) {
        self.0.put(out);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        f32::get(input).map(Self)
    }
}

impl Value for FontFeatures {
    fn put(&self, out: &mut Vec<u8>) {
        put_list(out, &self.0, |(tag, value), out| {
            tag.put(out);
            out.extend_from_slice(&value.to_le_bytes());
        });
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let features = get_list(input, |input| {
            Ok((String::get(input)?, u32::from_le_bytes(input.array()?)))
        })?;
        Ok(Self(Arc::new(features)))
    }
}

impl Value for FontFallbacks {
    fn put(&self, out: &mut Vec<u8>) {
        put_list(out, &self.0, String::put);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        Ok(Self(Arc::new(get_list(input, String::get)?)))
    }
}

impl Value for UnderlineStyle {
    fn put(&self, out: &mut Vec<u8>) {
        let Self {
            thickness,
            color,
            wavy,
        } = self;
        thickness.put(out);
        wavy.put(out);
        put_presence_and_value(out, color);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        Ok(Self {
            thickness: Value::get(input)?,
            wavy: Value::get(input)?,
            color: get_optional(input)?,
        })
    }
}

impl Value for StrikethroughStyle {
    fn put(&self, out: &mut Vec<u8>) {
        let Self { thickness, color } = self;
        thickness.put(out);
        put_presence_and_value(out, color);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        Ok(Self {
            thickness: Value::get(input)?,
            color: get_optional(input)?,
        })
    }
}

fn put_presence_and_value<T: Value>(out: &mut Vec<u8>, value: &Option<T>) {
    value.is_some().put(out);
    if let Some(value) = value {
        value.put(out);
    }
}

fn get_optional<T: Value>(input: &mut Reader) -> Result<Option<T>, &'static str> {
    bool::get(input)?.then(|| T::get(input)).transpose()
}

impl Value for TextOverflow {
    fn put(&self, out: &mut Vec<u8>) {
        let (tag, ellipsis) = match self {
            Self::Truncate(ellipsis) => (0, ellipsis),
            Self::TruncateStart(ellipsis) => (1, ellipsis),
            Self::TruncateMiddle(ellipsis) => (2, ellipsis),
        };
        out.push(tag);
        ellipsis.put(out);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let [tag] = input.array()?;
        let ellipsis = SharedString::get(input)?;
        match tag {
            0 => Ok(Self::Truncate(ellipsis)),
            1 => Ok(Self::TruncateStart(ellipsis)),
            2 => Ok(Self::TruncateMiddle(ellipsis)),
            _ => Err("a style entry names no TextOverflow"),
        }
    }
}

impl Value for Vec<BoxShadow> {
    fn put(&self, out: &mut Vec<u8>) {
        put_list(out, self, |shadow, out| {
            let BoxShadow {
                color,
                offset,
                blur_radius,
                spread_radius,
                inset,
            } = shadow;
            color.put(out);
            for length in [offset.x, offset.y, *blur_radius, *spread_radius] {
                length.put(out);
            }
            inset.put(out);
        });
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        get_list(input, |input| {
            Ok(BoxShadow {
                color: Value::get(input)?,
                offset: gpui::point(Value::get(input)?, Value::get(input)?),
                blur_radius: Value::get(input)?,
                spread_radius: Value::get(input)?,
                inset: Value::get(input)?,
            })
        })
    }
}

const SOLID: u8 = 0;
const LINEAR_GRADIENT: u8 = 1;
const PATTERN: u8 = 2;

impl Value for Fill {
    fn put(&self, out: &mut Vec<u8>) {
        let Self::Color(background) = self;
        if let Some(color) = background.as_solid() {
            out.push(SOLID);
            color.put(out);
        } else if let Some((angle, stops, space)) = background.as_linear_gradient() {
            out.push(LINEAR_GRADIENT);
            angle.put(out);
            for LinearColorStop { color, percentage } in stops {
                color.put(out);
                percentage.put(out);
            }
            space.put(out);
        } else {
            out.push(PATTERN);
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let background: Background = match input.array()? {
            [SOLID] => Hsla::get(input)?.into(),
            [LINEAR_GRADIENT] => {
                let angle = f32::get(input)?;
                let [from, to] = [(); 2].map(|()| {
                    Ok::<_, &'static str>(gpui::linear_color_stop(
                        Hsla::get(input)?,
                        f32::get(input)?,
                    ))
                });
                gpui::linear_gradient(angle, from?, to?).color_space(Value::get(input)?)
            }
            [PATTERN] => return Err("a pattern background does not cross the view wire"),
            _ => return Err("a style entry names no background"),
        };
        Ok(Self::Color(background))
    }
}

impl Value for GridTemplate {
    fn put(&self, out: &mut Vec<u8>) {
        let Self { repeat, min_size } = self;
        out.extend_from_slice(&repeat.to_le_bytes());
        min_size.put(out);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        Ok(Self {
            repeat: u16::from_le_bytes(input.array()?),
            min_size: Value::get(input)?,
        })
    }
}

const LINE: u8 = 0;
const SPAN: u8 = 1;
const PLACED_AUTO: u8 = 2;

impl Value for GridPlacement {
    fn put(&self, out: &mut Vec<u8>) {
        let (tag, bytes) = match self {
            Self::Line(line) => (LINE, line.to_le_bytes()),
            Self::Span(span) => (SPAN, span.to_le_bytes()),
            Self::Auto => (PLACED_AUTO, [0; 2]),
        };
        out.push(tag);
        out.extend_from_slice(&bytes);
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let [tag] = input.array()?;
        let bytes = input.array()?;
        match tag {
            LINE => Ok(Self::Line(i16::from_le_bytes(bytes))),
            SPAN => Ok(Self::Span(u16::from_le_bytes(bytes))),
            PLACED_AUTO => Ok(Self::Auto),
            _ => Err("a style entry names no grid placement"),
        }
    }
}

impl Value for GridLocation {
    fn put(&self, out: &mut Vec<u8>) {
        let Self { row, column } = self;
        for placement in [row.start, row.end, column.start, column.end] {
            placement.put(out);
        }
    }
    fn get(input: &mut Reader) -> Result<Self, &'static str> {
        let [row_start, row_end, column_start, column_end] =
            [(); 4].map(|()| GridPlacement::get(input));
        Ok(Self {
            row: row_start?..row_end?,
            column: column_start?..column_end?,
        })
    }
}
