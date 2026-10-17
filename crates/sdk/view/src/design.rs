//! The shapes every view repeats, in one visual language: the design tokens
//! (re-exported whole), the type scale, heights and spacing as [`Pixels`]
//! (`text`, `size`, `space`), and the empty state, refused-with-retry
//! screen, quiet line, heading and mono run views used to copy between
//! them, and the controls: buttons, tabs, the segmented choice, the switch
//! and the row it sits in, the pane divider, the link and the menu at the
//! pointer ([`menu`]). The number and time formatters (`format.rs`) sit
//! beside them for the same reason. The phone's shapes sit here too: the
//! width class a view lays out by ([`width_class`]), the size ladder it
//! picks by the session's pointer ([`ladder`]), the page stack ([`Pages`])
//! and the page frame ([`page`]) a Compact layout is made of, and the sheet
//! ([`sheet`]). The words every view says of a refusal and of a write that
//! did not land ([`sentence`], [`failure`]) sit here too.
//!
//! The kit a phone page reaches for, each piece as the Expo app's kit
//! draws it under a finger and as the desktop always has under a pointer
//! (pick the ladder first: `design::ladder(window.touch())`):
//! - a list: [`row`], its words ([`row_words`], [`row_title`],
//!   [`row_caption`]), [`lead`], [`chevron`], [`section_head`]
//! - a form: [`field`], [`button`] (`.small()`, `.busy(..)`), [`form_verbs`]
//! - a sheet: [`sheet`], [`sheet_head`], [`sheet_title`], [`sheet_row`],
//!   [`SHEET_INSET`]
//! - a choice: [`strip`] of [`tab`]s, [`segmented`], [`switch`]
//! - a state: [`empty_state`], [`refused`], [`quiet`]
//! - a press: [`pressed`], [`hit`] for a control smaller than a finger,
//!   [`dense`] for a dense list's row
//! - words: [`sentence`], [`failure`], [`fingerprint`], [`short_hex`]
pub use ::design::*;

mod format;
mod pages;
mod words;
pub(crate) use format::set_utc_offset;
pub use format::{ago, clock, date, day, grouped, initial, local, plural};
pub use pages::{CAPSULE_CLEARANCE, Nav, Page, Pages, page};
pub use words::{failure, sentence};

use crate::element::Capitals;
use crate::prelude::*;
use crate::{AnyElement, Div, FontWeight, Hsla, Pixels, Stateful, StyleRefinement};
pub use gpui::Orientation;
use gpui::{BoxShadow, ScrollStrategy};

/// [`type_scale`] as sizes an element takes.
pub mod text {
    use crate::{Pixels, px};

    pub const TITLE: Pixels = px(::design::type_scale::TITLE as f32);
    pub const SECTION: Pixels = px(::design::type_scale::SECTION as f32);
    pub const BODY: Pixels = px(::design::type_scale::BODY as f32);
    pub const SECONDARY: Pixels = px(::design::type_scale::SECONDARY as f32);
    pub const CAPTION: Pixels = px(::design::type_scale::CAPTION as f32);
    pub const MONO: Pixels = px(::design::type_scale::MONO as f32);
}

/// [`height`] as sizes an element takes.
pub mod size {
    use crate::{Pixels, px};

    pub const ROW: Pixels = px(::design::height::ROW as f32);
    pub const CONTROL: Pixels = px(::design::height::CONTROL as f32);
    pub const AVATAR_SM: Pixels = px(::design::height::AVATAR_SM as f32);
    pub const AVATAR: Pixels = px(::design::height::AVATAR as f32);
    pub const AVATAR_LG: Pixels = px(::design::height::AVATAR_LG as f32);
    /// The host's vertical scroll bar, its hover width and insets: a
    /// scroller keeps this much of its right edge clear.
    pub const SCROLLBAR: Pixels = px(16.);
}

/// [`spacing`] as gaps and insets an element takes.
pub mod space {
    use crate::{Pixels, px};

    pub const HAIR: Pixels = px(::design::spacing::HAIR as f32);
    pub const XXS: Pixels = px(::design::spacing::XXS as f32);
    pub const XS: Pixels = px(::design::spacing::XS as f32);
    pub const SM: Pixels = px(::design::spacing::SM as f32);
    pub const MD: Pixels = px(::design::spacing::MD as f32);
    pub const LG: Pixels = px(::design::spacing::LG as f32);
    pub const BLOCK: Pixels = px(::design::spacing::BLOCK as f32);
    pub const XL: Pixels = px(::design::spacing::XL as f32);
}

/// How wide the body a view is laid out in is, for the one-pane question
/// ([`width::COMPACT_BELOW`], [`width::WIDE_FROM`]): a Compact body shows
/// one page of the view's [`Pages`] at a time; Medium and Wide dock panes,
/// by the view's own [`docks`] arithmetic. Read once per render from the
/// viewport (`design::width_class(window.viewport_size().width.into())`)
/// and matched visibly, never a view-private threshold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidthClass {
    Compact,
    Medium,
    Wide,
}

/// The [`WidthClass`] of a body `width` logical px wide.
pub fn width_class(width: f32) -> WidthClass {
    if width < width::COMPACT_BELOW as f32 {
        WidthClass::Compact
    } else if width < width::WIDE_FROM as f32 {
        WidthClass::Medium
    } else {
        WidthClass::Wide
    }
}

/// A text role: its size, and the line height it is set on and the space
/// after each letter, where the role names them. Set on an element with
/// [`TextType::text_type`]. A pointer's roles name neither (the host's line
/// height and the font's own spacing stand, as on the desktop today); a
/// finger's are the Expo app's (`theme.ts` `type`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Type {
    pub size: Pixels,
    pub line: Option<Pixels>,
    /// Added after each letter (CSS `letter-spacing`, Expo's
    /// `letterSpacing`); negative sets the letters tighter.
    pub letter_spacing: Option<Pixels>,
}

impl Type {
    /// A role at `size` on the host's line height.
    pub const fn size(size: Pixels) -> Self {
        Self {
            size,
            line: None,
            letter_spacing: None,
        }
    }
    /// A role at `size` on a `line` tall line.
    pub const fn lined(size: f64, line: f64) -> Self {
        Self {
            size: px(size as f32),
            line: Some(px(line as f32)),
            letter_spacing: None,
        }
    }
    /// This role with `letter_spacing` px after each letter.
    pub const fn spaced(self, letter_spacing: f64) -> Self {
        Self {
            letter_spacing: Some(px(letter_spacing as f32)),
            ..self
        }
    }
}

/// [`Type`] on any styled element: `div().text_type(ladder.body)`.
pub trait TextType: Styled + Sized {
    /// The role's size, and its line height and letter spacing where it
    /// names them.
    fn text_type(self, role: Type) -> Self {
        let element = self.text_size(role.size);
        let element = match role.line {
            Some(line) => element.line_height(line),
            None => element,
        };
        match role.letter_spacing {
            Some(letter_spacing) => element.letter_spacing(letter_spacing),
            None => element,
        }
    }
}

impl<E: Styled> TextType for E {}

/// An [`avatar`]'s size: its side, its initial's text role and weight. A
/// side alone (`design::size::AVATAR`) sets its initial at 0.45 of it; a
/// finger's rungs ([`Ladder::avatar`] and the rest) are the Expo kit's
/// `Avatar`, whose initial sizes are a table, not one ratio, and medium.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Face {
    pub side: Pixels,
    pub text: Type,
    pub weight: Option<FontWeight>,
}

impl Face {
    /// A side of `side` px with its initial at 0.45 of it, in the face's
    /// own weight.
    pub const fn of(side: f32) -> Self {
        Self {
            side: px(side),
            text: Type::size(px(side * 0.45)),
            weight: None,
        }
    }
    /// The Expo kit's face: `text` medium on a line 1.25 times it
    /// (`Avatar.tsx:43-44`).
    const fn kit(side: f64, text: f64) -> Self {
        Self {
            side: px(side as f32),
            text: Type::lined(text, text * 1.25),
            weight: Some(FontWeight::MEDIUM),
        }
    }
}

impl From<Pixels> for Face {
    fn from(side: Pixels) -> Self {
        Self::of(side.into())
    }
}

/// The sizes a view draws its controls, rows and text at: one set under a
/// pointer ([`height`], [`type_scale`]), one under a finger (their `touch`
/// modules, the Expo app's theme; every pressable thing at least
/// [`Ladder::hit`] each way). Two constant sets, nothing computed. Picked
/// once per render from the device (`design::ladder(window.touch())`) and
/// passed down to every helper that sizes something, so the choice is
/// visible where it is made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ladder {
    /// The session's pointer is a finger: the set this is.
    pub touch: bool,
    /// a page's title row
    pub header: Pixels,
    /// a list row
    pub row: Pixels,
    /// a list row with a caption under its title; under a pointer, the row
    pub row_tall: Pixels,
    /// a bar of [`tab`]s, its hairline included; under a pointer, a tab's
    /// least height
    pub strip: Pixels,
    /// a row of a sheet
    pub sheet_row: Pixels,
    /// a control on one line
    pub control: Pixels,
    /// the smallest box a press lands in, each way
    pub hit: Pixels,
    /// a person's initial in a row or beside a message
    pub avatar: Face,
    /// a person's initial inline: a title row, a mention
    pub avatar_inline: Face,
    /// a person's initial heading a sheet
    pub avatar_sheet: Face,
    /// a person's initial heading their own page
    pub avatar_detail: Face,
    /// an icon on an icon button or in a title row
    pub icon: Pixels,
    /// an icon leading a list row
    pub icon_row: Pixels,
    /// an icon leading a sheet row
    pub icon_sheet: Pixels,
    /// a code to read out or compare (the join code, a key's
    /// fingerprint). The role is a size, line and spacing; the caller adds
    /// the data face and weight Expo sets beside it,
    /// `.font_family(design::fonts::FAMILY_MONO).font_weight(FontWeight::MEDIUM)`
    /// (`join/code.tsx:63`)
    pub display: Type,
    /// a view's first page's title
    pub big_title: Type,
    /// a detail head's name
    pub heading: Type,
    /// a page title
    pub title: Type,
    /// body text
    pub body: Type,
    /// a control's label
    pub label: Type,
    /// strip items, key/value rows
    pub small: Type,
    /// secondary copy
    pub secondary: Type,
    /// a caption, a timestamp
    pub caption: Type,
    /// a badge, a count
    pub tiny: Type,
    /// a fact's key, a code box's label: drawn by [`eyebrow`], which adds
    /// the data face, the muted colour and the capitals
    pub eyebrow: Type,
    /// code. The role is a size and line; the caller adds the data face
    /// Expo sets beside it, `.font_family(design::fonts::FAMILY_MONO)`
    /// (`forge/Lines.tsx:104`)
    pub code: Type,
}

/// The pointer's ladder: today's desktop numbers, unchanged, on the host's
/// line height.
pub const POINTER: Ladder = Ladder {
    touch: false,
    header: size::ROW,
    row: size::ROW,
    row_tall: size::ROW,
    strip: px(::design::height::HIT as f32),
    sheet_row: size::ROW,
    control: size::CONTROL,
    hit: px(::design::height::HIT as f32),
    avatar: Face::of(::design::height::AVATAR as f32),
    avatar_inline: Face::of(::design::height::AVATAR_SM as f32),
    avatar_sheet: Face::of(::design::height::AVATAR_LG as f32),
    avatar_detail: Face::of(::design::height::AVATAR_LG as f32),
    icon: px(::design::height::ICON as f32),
    icon_row: px(::design::height::ICON as f32),
    icon_sheet: px(::design::height::ICON as f32),
    // the desktop sets nothing louder than its title
    display: Type::size(text::TITLE),
    big_title: Type::size(text::TITLE),
    heading: Type::size(text::TITLE),
    title: Type::size(text::TITLE),
    body: Type::size(text::BODY),
    label: Type::size(text::SECONDARY),
    small: Type::size(text::BODY),
    secondary: Type::size(text::SECONDARY),
    caption: Type::size(text::CAPTION),
    tiny: Type::size(text::CAPTION),
    // the desktop's capitals over a group (chat's emoji picker) are a
    // caption
    eyebrow: Type::size(text::CAPTION),
    code: Type::size(text::MONO),
};

/// The finger's ladder: the Expo app's numbers (mobile `src/design/theme.ts`
/// `type` and `size`, `theme.ts:40-88`; each design token names its line).
pub const TOUCH: Ladder = {
    use ::design::{height::touch as h, type_scale::touch as t};
    Ladder {
        touch: true,
        header: px(h::HEADER as f32),
        row: px(h::ROW as f32),
        row_tall: px(h::ROW_TALL as f32),
        strip: px(h::STRIP as f32),
        sheet_row: px(h::SHEET_ROW as f32),
        control: px(h::CONTROL as f32),
        hit: px(h::HIT as f32),
        avatar: Face::kit(h::AVATAR, h::AVATAR_TEXT),
        avatar_inline: Face::kit(h::AVATAR_INLINE, h::AVATAR_INLINE_TEXT),
        avatar_sheet: Face::kit(h::AVATAR_SHEET, h::AVATAR_SHEET_TEXT),
        avatar_detail: Face::kit(h::AVATAR_DETAIL, h::AVATAR_DETAIL_TEXT),
        icon: px(h::ICON as f32),
        icon_row: px(h::ICON_ROW as f32),
        icon_sheet: px(h::ICON_SHEET as f32),
        display: Type::lined(t::DISPLAY, t::DISPLAY_LINE).spaced(t::DISPLAY_LETTER_SPACING),
        big_title: Type::lined(t::BIG_TITLE, t::BIG_TITLE_LINE).spaced(t::BIG_TITLE_LETTER_SPACING),
        heading: Type::lined(t::HEADING, t::HEADING_LINE),
        title: Type::lined(t::TITLE, t::TITLE_LINE),
        body: Type::lined(t::BODY, t::BODY_LINE),
        // a button's label is body text (Expo `Button.tsx:76`)
        label: Type::lined(t::BODY, t::BODY_LINE),
        small: Type::lined(t::SMALL, t::SMALL_LINE),
        secondary: Type::lined(t::SECONDARY, t::SECONDARY_LINE),
        caption: Type::lined(t::CAPTION, t::CAPTION_LINE),
        tiny: Type::lined(t::TINY, t::TINY_LINE),
        eyebrow: Type::lined(t::EYEBROW, t::EYEBROW_LINE).spaced(t::EYEBROW_LETTER_SPACING),
        code: Type::lined(t::CODE, t::CODE_LINE),
    }
};

/// The [`Ladder`] for a device: [`TOUCH`] when its primary pointer is a
/// finger (`window.touch()`), else [`POINTER`].
pub const fn ladder(touch: bool) -> Ladder {
    if touch { TOUCH } else { POINTER }
}

/// A [`Sheet`](crate::Sheet) in the view's colours. Under a finger it is
/// the Expo kit's `Sheet` (`Sheet.tsx`): its panel on the window's ground
/// under a `border_strong` hairline, edge to edge, so a row's ground and a
/// hairline reach both sides; what it holds pads itself ([`sheet_head`],
/// [`sheet_row`]). Under a pointer the panel pads its content by
/// [`space::BLOCK`]. `design::sheet("actions", "Message actions", ladder,
/// theme).on_dismiss(..).child(..)`; the host draws the scrim and the
/// handle, docks or centres it by the body's width class, and makes it as
/// tall as what it holds unless `.detents(..)` gives it snap points.
pub fn sheet(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    ladder: &Ladder,
    theme: &Theme,
) -> crate::Sheet {
    let sheet = crate::sheet(id, label).bg(theme.background);
    if ladder.touch {
        sheet.border_t_1().border_color(theme.border_strong)
    } else {
        sheet.p(space::BLOCK)
    }
}

/// How far a sheet's head, its rows and a form sheet's body sit in from
/// its sides (the Expo kit's `SheetHead` and `SheetRow`, `Sheet.tsx:148`,
/// `:162`; a form's body, `Agents.tsx:194`, `programs.tsx:465`). A form
/// sheet's body: `div().flex().flex_col().gap(space::LG).p(SHEET_INSET)`.
pub const SHEET_INSET: Pixels = px(20.);

/// The gap between a sheet row's icon and its label (`Sheet.tsx:162`).
const SHEET_ROW_GAP: Pixels = px(14.);

/// What a sheet is about, at its top (the Expo kit's `SheetHead`,
/// `Sheet.tsx:144-152`): 4 over, 12 under, 20 in from each side, a
/// hairline under it. The caller adds its lines (who and when in
/// `ladder.caption`, then the subject in `ladder.body`); a form sheet's
/// head is [`sheet_title`].
pub fn sheet_head(id: impl Into<ElementId>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .pt(space::XXS)
        .pb(space::LG)
        .px(SHEET_INSET)
        .border_b_1()
        .border_color(theme.border)
}

/// A form sheet's head (a `SheetHead` holding the form's title,
/// `Agents.tsx:189-193`, `programs.tsx:460-464`): [`sheet_head`] with
/// `title` in `ladder.title` medium, a level-2 heading (`{id}-title`).
pub fn sheet_title(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let title_id = ElementId::Name(format!("{id}-title").into());
    sheet_head(id, theme).child(
        heading(title_id, title, 2, theme)
            .text_type(ladder.title)
            .font_weight(FontWeight::MEDIUM),
    )
}

/// A form sheet's foot (`Agents.tsx:224-232`, `programs.tsx:506-509`):
/// `cancel`, then `verb` taking the rest of the row, 8 apart. The buttons
/// are the caller's as built; Expo's are a quiet Cancel and the primary
/// verb: `design::form_verbs(design::button("add-cancel", "Cancel",
/// &ladder, &theme, cancel).kind(Kind::Quiet), design::button("add",
/// "Add", &ladder, &theme, add).kind(Kind::Primary).busy(adding))`.
pub fn form_verbs<C, V>(cancel: Button<C>, verb: Button<V>) -> Div
where
    C: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    V: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    div()
        .flex()
        .gap(space::SM)
        .child(cancel)
        // a column stretches the button it holds across its width
        .child(div().flex_1().flex().flex_col().child(verb))
}

/// One action of a sheet (the Expo kit's `SheetRow`, `Sheet.tsx:154-175`):
/// [`Ladder::sheet_row`] tall, the whole row the press, `label` in body
/// text taking the slack; [`SheetRow::icon`] leads it, [`SheetRow::hint`]
/// trails it, and [`SheetRow::danger`] marks a row that destroys:
/// `design::sheet_row("delete", "Delete", &ladder, &theme, delete)
/// .icon("trash").danger()`.
pub fn sheet_row<F>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    click: F,
) -> SheetRow<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    SheetRow {
        id: id.into(),
        label: label.into(),
        icon: None,
        hint: None,
        danger: false,
        ladder: *ladder,
        theme: *theme,
        click,
    }
}

/// A [`sheet_row`] being built.
pub struct SheetRow<F> {
    id: ElementId,
    label: SharedString,
    icon: Option<SharedString>,
    hint: Option<SharedString>,
    danger: bool,
    ladder: Ladder,
    theme: Theme,
    click: F,
}

impl<F> SheetRow<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    /// A Lucide glyph before the label (`icons/<name>.svg`), at
    /// [`Ladder::icon_sheet`], muted.
    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self
    }
    /// A value at the row's right (`Sheet.tsx:168-172`): caption, muted,
    /// on one line; the label gives way to it.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }
    /// The row destroys something (Delete, Remove): its glyph and its
    /// label wear the danger tone.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
}

impl<F> gpui::prelude::FluentBuilder for SheetRow<F> where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static
{
}

impl<F> IntoElement for SheetRow<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    type Element = Stateful<Div>;
    fn into_element(self) -> Self::Element {
        let (ladder, theme) = (self.ladder, self.theme);
        let (glyph, words) = match self.danger {
            true => (theme.danger, theme.danger),
            false => (theme.muted, theme.foreground),
        };
        div()
            .id(self.id)
            .w_full()
            .h(ladder.sheet_row)
            .flex()
            .items_center()
            .gap(SHEET_ROW_GAP)
            .px(SHEET_INSET)
            .map(|row| pressed(row, &ladder))
            .role(Role::Button)
            .focusable()
            .on_click(self.click)
            .children(
                self.icon
                    .map(|name| icon(&name, ladder.icon_sheet).text_color(glyph)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_type(ladder.body)
                    .text_color(words)
                    .child(self.label),
            )
            .children(self.hint.map(|hint| {
                div()
                    .flex_none()
                    .truncate()
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .child(hint)
            }))
    }
}

/// A Lucide icon of the set the app bundles (`icons/<name>.svg`), `size`
/// each way, drawn in the text colour it is given or inherits: the caller
/// names the tone (`.text_color(theme.muted)`). It says nothing to a
/// screen reader; the control it sits in is named.
///
/// These are the glyphs the Expo kit's `Icon` draws (`Icon.tsx:7-11`),
/// but not at its line weight: Expo strokes them at 1.75 (`Icon.tsx:10`),
/// the bundled files at 2. The SDK cannot set a stroke (the wire's `Svg`
/// carries a path and a colour). The phone shell is to serve its own
/// files at 1.75 (fidelity F7); until then a touch screen draws 2, and the
/// desktop keeps 2.
///
/// `name` is the icon's Lucide 1.x file name (the app registers gpui-kit's
/// `AllAssets`, Lucide 1.43). An older name is an alias there and has no
/// file: the Expo app's `Trash2` is `trash`, its `AlertCircle` is
/// `circle-alert`.
pub fn icon(name: &str, size: Pixels) -> crate::Svg {
    crate::svg()
        .path(format!("icons/{name}.svg"))
        .size(size)
        .flex_none()
}

/// A press target bigger than what it draws (React Native's `hitSlop`):
/// a transparent box over its parent, taking no layout space, that
/// carries the press and the Aria while the parent only draws. `width` x
/// `height` is the parent's box, its border included, and `edge` the
/// width of that border (`px(0.)` for none): an absolute box is placed
/// inside its parent's border, so it reaches past the border too. Under a
/// finger the box reaches past each edge until it is [`Ladder::hit`] each
/// way (an axis already that long gets no reach); under a pointer it
/// covers the parent exactly. The reach lies over what sits around the
/// parent, so rows keep their spacing, and the focus ring is drawn on the
/// whole box. Where the box reaches past its parent,
/// [`assert_fits`](crate::testing::assert_fits) measures it off the
/// parent's declared height less its border, so the parent declares one;
/// a box that reaches nowhere is measured as any press, off the nearest
/// declared row above it.
/// The 44 is our rule, not Expo's: React Native's `hitSlop` is set per
/// control, and the Expo kit's reaction chips set none. A reaction chip 30
/// tall and at least 40 wide:
/// `div().h(px(30.)).min_w(px(40.)).child("👍 2").child(design::hit("react",
/// px(40.), px(30.), px(0.), &ladder).role(Role::Button).aria_label("👍 2")
/// .focusable().on_click(..))`.
pub fn hit(
    id: impl Into<ElementId>,
    width: Pixels,
    height: Pixels,
    edge: Pixels,
    ladder: &Ladder,
) -> Stateful<Div> {
    // the inset on each side of an axis, from inside the parent's border:
    // negative, it reaches past
    let inset = |side: Pixels| {
        let reach = match ladder.touch {
            true => px(((f32::from(side) - f32::from(ladder.hit)) / 2.).min(0.)),
            false => px(0.),
        };
        reach - edge
    };
    let (x, y) = (inset(width), inset(height));
    div().id(id).absolute().top(y).bottom(y).left(x).right(x)
}

/// A press in a dense list, which keeps its row's own box under a
/// finger's 44, as the Expo app's dense lists do: a chat message row, a
/// diff line at 20. Every other press is held to [`Ladder::hit`] by the
/// sweep ([`assert_fits`](crate::testing::assert_fits)); this one is let
/// through. It changes nothing drawn and nothing sent to the host, only
/// what the sweep accepts, and it is said where the row is built:
/// `design::dense(div().id(("line", n)).h(px(20.)).role(Role::Button)
/// .on_click(..))`.
pub fn dense(mut element: Stateful<Div>) -> Stateful<Div> {
    element.interactivity().interactions().dense = true;
    element
}

/// How far a control dims while a finger holds it (the Expo kit's
/// `platform.pressed`, iOS's look, `platform.tsx:36`).
const PRESSED_OPACITY: f32 = 0.6;

/// What a press shows: under a finger the control dims while it is held,
/// since a finger has no hover; under a pointer, nothing. Most of the
/// kit's own controls add it (not [`tab`] or a touch segment, whose Expo
/// pieces set no pressed look); a caller adds it to what it makes
/// pressable itself, so only something that presses dims (Expo
/// `Row.tsx:39`):
/// `design::pressed(design::row("ada", &ladder, &theme), &ladder)
/// .role(Role::Button).focusable().on_click(..)`.
pub fn pressed<E: StatefulInteractiveElement>(element: E, ladder: &Ladder) -> E {
    match ladder.touch {
        true => element.active(|style| style.opacity(PRESSED_OPACITY)),
        false => element,
    }
}

/// What a [`field`] holds: a one-line [`Input`](crate::Input) or a
/// many-line [`Textarea`](crate::Textarea). Sealed: what the field does to
/// its control is the field's own.
pub trait FieldControl: control::Control {}

impl FieldControl for crate::Input {}

impl FieldControl for crate::Textarea {}

/// What [`field`] sets on its control; private, so a view sets it through
/// the field alone.
mod control {
    use super::space;
    use crate::prelude::*;

    pub trait Control: Styled + IntoElement {
        /// The value will not do, and `problem` says why: the control is
        /// invalid and the problem its description, so a screen reader
        /// hears what the eye reads (rule 18).
        fn refused(self, problem: SharedString) -> Self;
        /// The words a screen reader reads after the control's name.
        fn described(self, caption: SharedString) -> Self;
        /// The text's inset inside an edge `thicker` than a hairline: 12
        /// in, less that (`Field.tsx:52`), and for many lines 10 over and
        /// under, less the same (`Field.tsx:79`), so the text stays put.
        fn text_inset(self, thicker: Pixels) -> Self;
    }

    impl Control for crate::Input {
        fn refused(self, problem: SharedString) -> Self {
            self.invalid(gpui::accesskit::Invalid::True)
                .description(problem)
        }
        fn described(self, caption: SharedString) -> Self {
            self.description(caption)
        }
        fn text_inset(self, thicker: Pixels) -> Self {
            self.px(space::LG - thicker)
        }
    }

    impl Control for crate::Textarea {
        fn refused(self, problem: SharedString) -> Self {
            self.invalid(gpui::accesskit::Invalid::True)
                .description(problem)
        }
        fn described(self, caption: SharedString) -> Self {
            self.description(caption)
        }
        fn text_inset(self, thicker: Pixels) -> Self {
            self.px(space::LG - thicker).py(space::MD - thicker)
        }
    }
}

/// A text field in its box (the Expo kit's `Field`, `Field.tsx:22-97`):
/// `control` at least [`Ladder::control`] tall, a `border_strong` hairline
/// on the window's ground, body text 12 in; the host draws the edge the
/// focused field wears. [`Field::label`] goes over the box,
/// [`Field::caption`] or [`Field::problem`] under it:
/// `design::field(Input::new("name", &form.name, "Agent name"), &ladder,
/// &theme).label("Agent name").when_some(problem, Field::problem)`.
pub fn field<C: FieldControl>(control: C, ladder: &Ladder, theme: &Theme) -> Field<C> {
    Field {
        control,
        label: None,
        caption: None,
        problem: None,
        mono: false,
        lead: None,
        ladder: *ladder,
        theme: *theme,
    }
}

/// A [`field`] being built.
pub struct Field<C> {
    control: C,
    label: Option<SharedString>,
    caption: Option<SharedString>,
    problem: Option<SharedString>,
    mono: bool,
    lead: Option<SharedString>,
    ladder: Ladder,
    theme: Theme,
}

impl<C: FieldControl> Field<C> {
    /// The words over the box, in caption muted, 6 over it
    /// (`Field.tsx:41-45`). They are for the eye: a screen reader reads
    /// the control's own name.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    /// A rule or a hint under the box, in caption muted, 6 under it
    /// (`Field.tsx:90-93`), and the control's description. A problem
    /// takes its place.
    pub fn caption(mut self, caption: impl Into<SharedString>) -> Self {
        self.caption = Some(caption.into());
        self
    }
    /// What is wrong with the value (`Field.tsx:37-38`, `:86-89`): under
    /// the box in caption danger, and the box's edge 2 thick in danger,
    /// its text kept where it was; the control is invalid and the problem
    /// its description.
    pub fn problem(mut self, problem: impl Into<SharedString>) -> Self {
        self.problem = Some(problem.into());
        self
    }
    /// The text in the data face at `ladder.secondary`
    /// (`Field.tsx:76-77`): a key, an account number.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }
    /// A Lucide glyph before the text, inside the box (`Field.tsx:58`, a
    /// search field's `Search`): at [`Ladder::icon_row`], muted, 12 in,
    /// the text 8 after it. It sits in the control's own box, so the edge
    /// the host draws on a focused field is the one edge round both.
    pub fn lead(mut self, name: impl Into<SharedString>) -> Self {
        self.lead = Some(name.into());
        self
    }
}

impl<C: FieldControl> gpui::prelude::FluentBuilder for Field<C> {}

impl<C: FieldControl> IntoElement for Field<C> {
    type Element = Div;
    fn into_element(self) -> Div {
        let (ladder, theme) = (self.ladder, self.theme);
        // a value that will not do: the edge 2 thick in danger, one past
        // the hairline (`Field.tsx:37-38`)
        let (thicker, edge) = match self.problem {
            Some(_) => (HAIRLINE, theme.danger),
            None => (px(0.), theme.border_strong),
        };
        let control = self.control.text_inset(thicker);
        let control = match self.problem {
            Some(_) => control.border_2(),
            None => control.border_1(),
        };
        let control = control
            .border_color(edge)
            .min_h(ladder.control)
            .bg(theme.background)
            .text_color(theme.foreground);
        let control = match self.mono {
            true => control
                .font_family(fonts::FAMILY_MONO)
                .text_type(ladder.secondary),
            false => control.text_type(ladder.body),
        };
        // the lead sits in the box's padding, 12 in from inside the edge
        // as the box's own padding is, and the text 8 after it
        let control = match &self.lead {
            Some(_) => control.pl(space::LG - thicker + ladder.icon_row + space::SM),
            None => control,
        };
        let line = |words: SharedString, tone: Hsla| {
            div()
                .mt(space::XS)
                .text_type(ladder.caption)
                .text_color(tone)
                .child(words)
        };
        let (control, under) = match (self.problem, self.caption) {
            (Some(problem), _) => (
                control.refused(problem.clone()),
                Some(line(problem, theme.danger)),
            ),
            (None, Some(caption)) => (
                control.described(caption.clone()),
                Some(line(caption, theme.muted)),
            ),
            (None, None) => (control, None),
        };
        let boxed = match self.lead {
            Some(name) => div()
                .relative()
                .child(control)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(space::LG + HAIRLINE)
                        .flex()
                        .items_center()
                        .child(icon(&name, ladder.icon_row).text_color(theme.muted)),
                )
                .into_any_element(),
            None => control.into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .children(self.label.map(|label| {
                div()
                    .mb(space::XS)
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .child(label)
            }))
            .child(boxed)
            .children(under)
    }
}

/// How wide a row's lead column is (`Row.tsx:57`).
const LEAD: Pixels = px(24.);

/// The column a [`row`] leads with (the Expo kit's `Row` `lead`): 24 wide,
/// its child centred in it: an icon at [`Ladder::icon_row`], an avatar.
pub fn lead(child: impl IntoElement) -> Div {
    div()
        .w(LEAD)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .child(child)
}

/// A list row (the Expo kit's `Row`, `Row.tsx:32-56`): at least
/// [`Ladder::row`] tall, 16 in from each side, 12 between its parts, 6
/// over and under, a hairline under it. The caller adds a [`lead`], the
/// title in `ladder.body` (with a caption under it in `ladder.caption`,
/// and the row then `.min_h(ladder.row_tall)`) and what trails it. A row
/// that presses is the press whole: the caller adds its role and click,
/// and [`pressed`] so it dims while held; a row that does not press does
/// not dim (`Row.tsx:39`).
pub fn row(id: impl Into<ElementId>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .min_h(ladder.row)
        .flex()
        .items_center()
        .gap(space::LG)
        .py(space::XS)
        .px(space::BLOCK)
        .border_b_1()
        .border_color(theme.border)
}

/// The gap between a row's title and its caption (`Row.tsx:58`).
const TITLE_GAP: Pixels = px(1.);

/// A [`row`]'s words (the Expo kit's `Row`, `Row.tsx:58-73`): a column
/// taking the row's slack, `title` on top (a [`row_title`]) and, 1 under
/// it, a caption the caller adds as a child (a [`row_caption`]). A row
/// with a caption is [`Ladder::row_tall`] (`Row.tsx:44`):
/// `design::row("ada", &ladder, &theme).min_h(ladder.row_tall)
/// .child(design::row_words(design::row_title("Ada", &ladder, &theme))
/// .child(design::row_caption("#42", &ladder, &theme)))
/// .child(design::chevron(&ladder, &theme))`.
pub fn row_words(title: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(TITLE_GAP)
        .child(title)
}

/// A row's title (`Row.tsx:59-62`): one line of `ladder.body` in the
/// foreground. A muted title (an archived room) recolours it; one in the
/// data face is `.font_family(fonts::FAMILY_MONO).text_type(ladder.secondary)`.
pub fn row_title(text: impl Into<SharedString>, ladder: &Ladder, theme: &Theme) -> Div {
    div()
        .truncate()
        .text_type(ladder.body)
        .text_color(theme.foreground)
        .child(text.into())
}

/// A row's caption under its title (`Row.tsx:66-69`): one line of
/// `ladder.caption`, muted.
pub fn row_caption(text: impl Into<SharedString>, ladder: &Ladder, theme: &Theme) -> Div {
    div()
        .truncate()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(text.into())
}

/// The chevron at a row's end, for a row that opens a page
/// (`Row.tsx:75`): Lucide `chevron-right` at [`Ladder::icon_row`], faint.
pub fn chevron(ladder: &Ladder, theme: &Theme) -> crate::Svg {
    icon("chevron-right", ladder.icon_row).text_color(theme.faint)
}

/// How tall a section head is at least (`Row.tsx:86`).
const SECTION_HEAD: Pixels = px(40.);

/// A list's section head (the Expo kit's `SectionHead`, `Row.tsx:80-107`):
/// at least 40 tall, 12 over and 4 under, 16 in; `label` in caption
/// medium and muted. What the caller adds sits after it at the right, in
/// the head's caption muted, as Expo's one `right` does: a count, or
/// "3 · newest first":
/// `design::section_head("keys", "Keys", &ladder, &theme)
/// .child(count.to_string())`. Under a finger the label takes the slack
/// and what follows sits 8 after it; under a pointer the two sit at the
/// head's ends, as the desktop has always drawn them.
pub fn section_head(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let head = div().id(id).min_h(SECTION_HEAD).flex().items_center();
    let label = div().font_weight(FontWeight::MEDIUM).child(label.into());
    let (head, label) = match ladder.touch {
        true => (head.gap(space::SM), label.flex_1().min_w_0()),
        false => (head.justify_between(), label),
    };
    head.pt(space::LG)
        .pb(space::XXS)
        .px(space::BLOCK)
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .role(Role::Heading)
        .aria_level(2)
        .child(label)
}

/// How far an empty state sits in from its top and bottom
/// (`States.tsx:26`).
const EMPTY_Y: Pixels = px(40.);

/// How far an empty state sits in from its sides (`States.tsx:26`).
const EMPTY_X: Pixels = px(32.);

/// How wide an empty state's detail runs at most (`States.tsx:31`).
const EMPTY_DETAIL: Pixels = px(280.);

/// Nothing to show yet: what is missing, then what would fill it. Under a
/// finger it is the Expo kit's `EmptyState` (`States.tsx:11-37`): centred,
/// 40 over and under, 32 in, 8 apart, the Lucide `glyph` at 28 in faint,
/// the title in `ladder.body` medium, the detail in `ladder.secondary`
/// muted at most 280 wide. Its one action, a small button, is the caller's
/// child, 8 further down (`States.tsx:34`):
/// `.child(div().mt(space::SM).child(design::button(..).small()))`.
/// Under a pointer it is the title and the detail alone, at the start.
pub fn empty_state(
    id: impl Into<ElementId>,
    glyph: &str,
    title: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let (title, detail) = (title.into(), detail.into());
    match ladder.touch {
        true => div()
            .id(id)
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(space::SM)
            .py(EMPTY_Y)
            .px(EMPTY_X)
            .child(
                icon(glyph, px(::design::height::touch::ICON_EMPTY as f32)).text_color(theme.faint),
            )
            .child(
                div()
                    .text_type(ladder.body)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .text_center()
                    .child(title),
            )
            .child(
                div()
                    .max_w(EMPTY_DETAIL)
                    .text_type(ladder.secondary)
                    .text_color(theme.muted)
                    .text_center()
                    .child(detail),
            ),
        false => div()
            .id(id)
            .flex()
            .flex_col()
            .gap_1()
            .p_6()
            .max_w(px(420.))
            .child(
                div()
                    .text_size(text::SECTION)
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
            .child(
                div()
                    .text_size(text::SECONDARY)
                    .text_color(theme.muted)
                    .child(detail),
            ),
    }
}

/// A refused read: the sentence, and Retry. The screen is
/// `{name}-refused`, its retry control `{name}-retry`. Under a finger it
/// is the Expo kit's `Refused` (`Notice.tsx:70-77`), a danger `Notice`
/// ([`notice_refused`]); under a pointer a bordered box with a Retry
/// button.
pub fn refused(
    name: &str,
    sentence: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let id = ElementId::Name(format!("{name}-refused").into());
    let retry_id = ElementId::Name(format!("{name}-retry").into());
    if ladder.touch {
        return notice_refused(id, retry_id, sentence.into(), ladder, theme, retry);
    }
    let theme = *theme;
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(theme.danger)
        .bg(theme.danger_soft)
        .text_size(text::SECONDARY)
        .child(sentence.into())
        .child(
            div()
                .id(retry_id)
                .px_2()
                .py_1()
                .w(px(64.))
                .min_h(ladder.hit)
                .bg(theme.surface)
                .hover(move |style| style.bg(theme.surface_raised))
                .role(Role::Button)
                .focusable()
                .on_click(retry)
                .child("Retry"),
        )
}

/// How tall a `Notice`'s action is: its `secondary` line and 6 over and
/// under (`Notice.tsx:60-61`).
const NOTICE_ACTION: Pixels = px(30.);

/// How far a `Notice`'s action presses past itself on every side: Expo's
/// `hitSlop` (`Notice.tsx:60`).
const NOTICE_SLOP: Pixels = px(8.);

/// A refused read under a finger (`Notice.tsx:15-77`): a full-width strip
/// on `danger_soft`, 10 over and under, 16 in, 10 apart, the sentence in
/// `ladder.secondary` danger taking the slack, then Retry, medium, 8 in
/// from its left; an alert a screen reader says at once. Retry's press
/// reaches 8 past it on every side, as Expo's does: the press box is
/// padded by the reach and a negative margin hands the room back, so the
/// word sits where Expo's does.
fn notice_refused(
    id: ElementId,
    retry_id: ElementId,
    sentence: SharedString,
    ladder: &Ladder,
    theme: &Theme,
    retry: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let press = div()
        .id(retry_id)
        .flex_none()
        .h(NOTICE_ACTION + NOTICE_SLOP * 2.)
        .flex()
        .items_center()
        .pl(space::SM + NOTICE_SLOP)
        .pr(NOTICE_SLOP)
        .m(-NOTICE_SLOP)
        .font_weight(FontWeight::MEDIUM)
        .role(Role::Button)
        .focusable()
        .on_click(retry)
        .child("Retry");
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(space::MD)
        .py(space::MD)
        .px(space::BLOCK)
        .bg(theme.danger_soft)
        .text_type(ladder.secondary)
        .text_color(theme.danger)
        .role(Role::Alert)
        .aria_live(gpui::accesskit::Live::Assertive)
        .child(div().flex_1().min_w_0().child(sentence))
        .child(press)
}

/// One muted line of status text.
pub fn quiet(text: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(text::SECONDARY)
        .text_color(theme.muted)
        .child(text.into())
}

/// A heading: the title size at level 1, the section size under it.
pub fn heading(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    level: usize,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .text_size(if level == 1 {
            text::TITLE
        } else {
            text::SECTION
        })
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.foreground)
        .role(Role::Heading)
        .aria_level(level)
        .child(text.into())
}

/// A run of monospaced text on one line: hashes, keys, code.
pub fn mono(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(fonts::FAMILY_MONO)
        .text_size(text::MONO)
        .whitespace_nowrap()
        .child(text.into())
}

/// A fact's key, a code box's label (the Expo kit's `<Text role="eyebrow"
/// mono tone="muted">`): `words` at [`Ladder::eyebrow`], in the data face,
/// muted, drawn in capitals. Pass the words as written: the host draws
/// them in capitals (Expo's `textTransform: 'uppercase'`) and a screen
/// reader hears them as written, not spelled out. Add
/// `.font_weight(FontWeight::MEDIUM)` where Expo sets it (`Facts.tsx:28`).
pub fn eyebrow(words: impl Into<SharedString>, ladder: &Ladder, theme: &Theme) -> Div {
    div()
        .text_type(ladder.eyebrow)
        .font_family(fonts::FAMILY_MONO)
        .text_color(theme.muted)
        .child(Capitals(words.into()))
}

/// The focus ring: `color`, 2 px, just inside the edge, on a control the
/// keyboard has reached. The SDK draws it on every focusable node that has
/// no `focus_visible` of its own; a control whose own edge is already ink
/// shows it in the ink's foreground ([`focus_shown_on_ink`]).
pub(crate) fn focus_ring(color: Hsla) -> StyleRefinement {
    StyleRefinement::default()
        .shadow(vec![ring_shadow(color, 2., true)])
        // inset shadows paint under the border: a bordered control would
        // otherwise show one pixel of the two
        .border_color(color)
}

fn ring_shadow(color: Hsla, spread: f32, inset: bool) -> BoxShadow {
    BoxShadow {
        color,
        offset: gpui::point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(spread),
        inset,
    }
}

/// The focus ring in ink, for an element whose own `focus_visible` would
/// otherwise replace it: `style` is what the element adds to it.
pub fn focus_shown<E: InteractiveElement>(
    element: E,
    theme: &Theme,
    style: impl FnOnce(StyleRefinement) -> StyleRefinement,
) -> E {
    let ring = focus_ring(theme.accent);
    element.focus_visible(|_| {
        use gpui::Refineable;
        let mut shown = style(StyleRefinement::default());
        // the ring's shadow joins the element's own (a menu's `shadow_lg`)
        // instead of replacing it
        let mut shadows = ring.box_shadow.clone().unwrap_or_default();
        shadows.extend(shown.box_shadow.take().unwrap_or_default());
        shown.refine(&ring);
        shown.box_shadow = Some(shadows);
        shown
    })
}

/// The focus ring on an ink-filled control (a primary button): the ink's
/// foreground, since ink on ink shows nothing.
pub fn focus_shown_on_ink<E: InteractiveElement>(element: E, theme: &Theme) -> E {
    element.focus_visible(|_| focus_ring(theme.primary_foreground))
}

/// What a [`Button`] is among its neighbours.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// a surface fill
    #[default]
    Plain,
    /// the one action a screen leads with: the primary fill
    Primary,
    /// a choice among many: no fill, muted text
    Quiet,
    /// a secondary action beside the primary one: the window's ground in a
    /// hairline box
    Outline,
}

/// A button. Disabled keeps it visible, drops the click and says so.
/// Busy is an op in flight: the click is dropped and a screen reader hears
/// it is busy. Selected is the chosen one: fg text on the window and an fg
/// edge. A button told whether it is selected is a toggle, pressed or not;
/// one never told is a plain button.
#[derive(IntoElement)]
pub struct Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    id: ElementId,
    label: SharedString,
    ladder: Ladder,
    theme: Theme,
    enabled: bool,
    kind: Kind,
    selected: Option<bool>,
    /// `Some(active)`: a cell of a [`composite`] grid, never focusable
    item: Option<bool>,
    small: bool,
    busy: bool,
    click: F,
}

pub fn button<F>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    click: F,
) -> Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    Button {
        id: id.into(),
        label: label.into(),
        ladder: *ladder,
        theme: *theme,
        enabled: true,
        kind: Kind::Plain,
        selected: None,
        item: None,
        small: false,
        busy: false,
        click,
    }
}

impl<F> Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn kind(mut self, kind: Kind) -> Self {
        self.kind = kind;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }
    /// The button as an [`item`] of a composite: it leaves the Tab order
    /// and claims the active descendant when `active`.
    pub fn item(mut self, active: bool) -> Self {
        self.item = Some(active);
        self
    }
    /// The small button (`Button.tsx:16`): under a finger 36 tall
    /// (`theme.ts:76`), 12 in, the label in [`Ladder::secondary`], and at
    /// least [`Ladder::hit`] wide. It still presses at [`Ladder::hit`]
    /// tall, which is our rule: Expo's small button sets no `hitSlop`, so
    /// its press is the 36. A pointer has one button size, so under a
    /// pointer it changes nothing.
    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }
    /// An op in flight (`Button.tsx:20-21`): the click is dropped and the
    /// button says it is busy. Under a finger it keeps its look, as the
    /// Expo kit's does; under a pointer it looks disabled. The Expo kit's
    /// spinner beside the label is not drawn: the view wire carries no
    /// animation.
    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }
}

impl<F> RenderOnce for Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        match self.ladder.touch {
            true => self.touched(),
            false => self.pointed().into_any_element(),
        }
    }
}

impl<F> Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    /// Under a pointer: compact, a surface fill, hover and press shades.
    fn pointed(self) -> Stateful<Div> {
        let theme = self.theme;
        let selected = self.selected == Some(true);
        let enabled = self.enabled && !self.busy;
        let mut element = div()
            .id(self.id)
            .px_2()
            .py_1()
            .min_h(self.ladder.hit)
            .flex()
            .items_center()
            .text_type(self.ladder.label)
            .role(Role::Button)
            .child(self.label);
        // The chosen one is the ink one: fg text on the window, an fg edge
        // around it; the rest stay quiet.
        element = match (self.kind, selected) {
            // a primary that cannot run greys out but keeps its place
            (Kind::Primary, _) if !enabled => {
                element.bg(theme.faint).text_color(theme.primary_foreground)
            }
            (Kind::Primary, _) => focus_shown_on_ink(
                element
                    .bg(theme.primary)
                    .text_color(theme.primary_foreground),
                &theme,
            ),
            // the edge is ink already: the ring reaches one pixel further in
            (_, true) => element
                .bg(theme.background)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.foreground)
                .focus_visible(move |style| {
                    style.shadow(vec![ring_shadow(theme.accent, 3., true)])
                }),
            (Kind::Quiet, false) => element
                .text_color(theme.muted)
                .border_1()
                .border_color(Hsla::transparent_black()),
            (Kind::Plain, false) => element
                .bg(theme.surface)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.surface),
            (Kind::Outline, false) => element
                .bg(theme.background)
                .text_color(theme.foreground)
                .border_1()
                .border_color(theme.border_strong),
        };
        if let Some(on) = self.selected {
            element = element.aria_toggled(on.into());
        }
        if selected {
            element = element.font_weight(FontWeight::MEDIUM);
        }
        if !enabled {
            let element = match self.kind {
                Kind::Primary => element.aria_disabled(true),
                _ => element.text_color(theme.muted).aria_disabled(true),
            };
            return element.when(self.busy, |element| element.aria_busy(true));
        }
        element = match (self.kind, selected) {
            (Kind::Quiet, false) => element.hover(move |style| style.text_color(theme.foreground)),
            (Kind::Plain, false) => element
                .hover(move |style| style.bg(theme.surface_raised))
                .active(move |style| style.bg(theme.accent_soft)),
            (Kind::Outline, false) => element.hover(move |style| style.bg(theme.surface)),
            _ => element,
        };
        takes_press(element, self.item, self.click)
    }
}

impl<F> Button<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    /// Under a finger, the Expo kit's `Button` (`Button.tsx:30-81`):
    /// square, [`Ladder::control`] tall (48 for the primary, 36 small), 16
    /// in (12 small), a hairline edge; the label in body text (secondary
    /// small), medium but for a quiet one. A plain and an outline button
    /// draw the kit's outline (it has no grey fill); a disabled one goes
    /// faint on a quiet edge, a busy one keeps its look. No hover: a press
    /// dims it. A small button's press is a box [`Ladder::hit`] tall round
    /// its face, and dims the face too.
    fn touched(self) -> AnyElement {
        let Self {
            id,
            label,
            ladder,
            theme,
            enabled,
            kind,
            selected,
            item,
            small,
            busy,
            click,
        } = self;
        let chosen = selected == Some(true);
        let clear = Hsla::transparent_black();
        // (ground, edge, text)
        let (ground, edge, tone) = match (kind, enabled, chosen) {
            (Kind::Primary, false, _) => (
                Some(theme.surface_raised),
                theme.surface_raised,
                theme.faint,
            ),
            (_, false, _) => (None, theme.border, theme.faint),
            (Kind::Primary, true, _) => {
                (Some(theme.primary), theme.primary, theme.primary_foreground)
            }
            // the chosen one of a toggle wears the ink edge
            (_, true, true) => (None, theme.foreground, theme.foreground),
            (Kind::Quiet, true, false) => (None, clear, theme.muted),
            (Kind::Plain | Kind::Outline, true, false) => {
                (None, theme.border_strong, theme.foreground)
            }
        };
        // (height, inset, words)
        let (height, inset, words) = match (small, kind) {
            (true, _) => (
                px(::design::height::touch::SMALL as f32),
                space::LG,
                ladder.secondary,
            ),
            (false, Kind::Primary) => (
                px(::design::height::touch::PRIMARY as f32),
                space::BLOCK,
                ladder.label,
            ),
            (false, _) => (ladder.control, space::BLOCK, ladder.label),
        };
        let face = div()
            .h(height)
            .px(inset)
            .flex()
            .items_center()
            .justify_center()
            .gap(space::SM)
            .border_1()
            .border_color(edge)
            .text_color(tone)
            .text_type(words)
            .when(kind != Kind::Quiet || chosen, |button| {
                button.font_weight(FontWeight::MEDIUM)
            })
            .when_some(ground, |button, ground| button.bg(ground))
            .child(label.clone());
        let press = ButtonPress {
            selected,
            enabled,
            busy,
            item,
        };
        let live = enabled && !busy;
        if !small {
            let face = face.id(id);
            let face = match (live, kind) {
                (true, Kind::Primary) => focus_shown_on_ink(pressed(face, &ladder), &theme),
                (true, _) => pressed(face, &ladder),
                (false, _) => face,
            };
            return press.takes(face, click).into_any_element();
        }
        // The press box is the face's parent, so a press anywhere on it
        // dims the face: [`Ladder::hit`] tall, a negative margin handing
        // back what it reaches past the 36 face. The 44 is our rule; Expo's
        // small button sets no `hitSlop`.
        let reach = (ladder.hit - height) / 2.;
        let press_box = div()
            .id(id)
            .h(ladder.hit)
            .my(-reach)
            .flex()
            .flex_col()
            .justify_center();
        let press_box = match live {
            true => pressed(press_box, &ladder),
            false => press_box,
        };
        press
            .takes(press_box, click)
            .child(face.min_w(ladder.hit))
            .into_any_element()
    }
}

/// What a touch button's press says and takes: a button, toggled or not,
/// then busy or disabled (no click) or its click.
struct ButtonPress {
    selected: Option<bool>,
    enabled: bool,
    busy: bool,
    item: Option<bool>,
}

impl ButtonPress {
    fn takes(
        self,
        element: Stateful<Div>,
        click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        let element = element
            .role(Role::Button)
            .when_some(self.selected, |element, on| element.aria_toggled(on.into()));
        match (self.enabled, self.busy) {
            (true, false) => takes_press(element, self.item, click),
            (_, busy) => element
                .aria_disabled(true)
                .when(busy, |element| element.aria_busy(true)),
        }
    }
}

/// The last step of a control that presses: its `click`, and either the
/// composite's claim ([`item`], when `item_of` is `Some(active)`) or a Tab
/// stop of its own.
fn takes_press(
    element: Stateful<Div>,
    item_of: Option<bool>,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let element = element.on_click(click);
    match item_of {
        Some(active) => item(element, Role::Button, active),
        None => element.focusable(),
    }
}

/// A control drawn as a glyph alone (the Expo kit's `Hit`,
/// `Button.tsx:83-128`): an [`icon`] at [`Ladder::icon`], a cross, a plus.
/// `name` is what it does, in words, since the glyph says nothing to a
/// screen reader. Under a pointer the box is no smaller than
/// [`Ladder::hit`] each way and the glyph is muted until the pointer is on
/// it; under a finger the box is [`Ladder::hit`] each way exactly
/// (`Button.tsx:116-117`), the glyph in the foreground (`Button.tsx:89`),
/// and a press dims it. A caller may recolour the glyph (`.text_color(..)`).
pub fn icon_button<F>(
    id: impl Into<ElementId>,
    glyph: impl IntoElement,
    name: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    click: F,
) -> IconButton<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    let theme = *theme;
    let element = div().id(id).flex().items_center().justify_center();
    let element = match ladder.touch {
        true => element
            .size(ladder.hit)
            .flex_none()
            .text_color(theme.foreground),
        false => element
            .min_w(ladder.hit)
            .min_h(ladder.hit)
            .px_1()
            .text_color(theme.muted)
            .cursor_pointer()
            .hover(move |style| style.text_color(theme.foreground)),
    };
    IconButton {
        element: element.role(Role::Button).aria_label(name).child(glyph),
        ladder: *ladder,
        theme,
        filled: false,
        enabled: true,
        toggled: None,
        item: None,
        click,
    }
}

/// An [`icon_button`] being built.
#[derive(IntoElement)]
pub struct IconButton<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    element: Stateful<Div>,
    ladder: Ladder,
    theme: Theme,
    filled: bool,
    enabled: bool,
    toggled: Option<bool>,
    /// `Some(active)`: an item of a [`composite`], never focusable
    item: Option<bool>,
    click: F,
}

impl<F> IconButton<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    /// `true` makes it the ink square with the glyph in the ink's
    /// foreground (the Expo `Hit`'s `filled`, `Button.tsx:120`, `:125`): a
    /// composer's Send once there is something to send.
    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }
    /// `false` keeps it visible and unpressable, and says so: the glyph's
    /// colour goes faint, the Expo `Hit`'s disabled icon (`Button.tsx:125`).
    /// A glyph drawn in its own colours (a colour emoji) does not change,
    /// as the `Hit` draws such children as they are.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Says it is on or off and draws nothing for it: a toggle to a
    /// screen reader (`aria_toggled`, as chat's reaction chip says
    /// `accessibilityState.selected`, `MessageRow.tsx:173`). The look of
    /// on is the caller's: a chosen reaction's accent edge and ground.
    pub fn toggled(mut self, on: bool) -> Self {
        self.toggled = Some(on);
        self
    }
    /// Makes the button an [`item`] of a composite, `active` or not: it
    /// leaves the Tab order and claims the active descendant when
    /// `active`.
    pub fn item(mut self, active: bool) -> Self {
        self.item = Some(active);
        self
    }
}

impl<F> Styled for IconButton<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn style(&mut self) -> &mut StyleRefinement {
        self.element.style()
    }
}

impl<F> RenderOnce for IconButton<F>
where
    F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
{
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let mut element = self.element;
        if self.filled {
            element = element
                .bg(theme.primary)
                .text_color(theme.primary_foreground);
        }
        if let Some(on) = self.toggled {
            element = element.aria_toggled(on.into());
        }
        if !self.enabled {
            return element.text_color(theme.faint).aria_disabled(true);
        }
        element = pressed(element, &self.ladder);
        if self.filled {
            element = focus_shown_on_ink(element, &theme);
        }
        takes_press(element, self.item, self.click)
    }
}

/// How thick the chosen tab's underline is under a finger
/// (`Choice.tsx:138`).
const UNDERLINE: Pixels = px(2.);

/// A hairline (`size.hairline`): a strip's under its tabs
/// (`Choice.tsx:117`), a field's edge (`Field.tsx:38`).
const HAIRLINE: Pixels = px(1.);

/// A tab: quiet text, the chosen one fg, medium and underlined, no fill. It
/// may label a glyph. It is an [`item`] of a `TabList` [`composite`], which
/// holds the focus and the arrows: the caller wraps it in
/// `item(.., Role::Tab, active)`.
///
/// Under a finger a tab bar is `.h(ladder.strip)` tall, its hairline
/// included ([`strip`] declares it). Under a pointer a tab is at least
/// [`Ladder::hit`] tall, and the caller sizes it to its bar (`h_full`,
/// `flex_1`).
///
/// Under a finger it is an item of the Expo kit's `Strip`
/// (`Choice.tsx:113-145`): drawn in the bar less its hairline
/// (`Choice.tsx:127`), 10 in, `ladder.small` on one line, the chosen one's
/// underline 10 in from each side just above the hairline; no hover, and a
/// press does not dim (`Strip` sets no pressed look). Its own box is the
/// strip's whole [`Ladder::strip`], reaching over the bar's hairline, so
/// the press is a finger tall where Expo's is 43, and nothing drawn moves.
/// A size or text style the caller sets after it replaces the strip's.
pub fn tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = *theme;
    let tab = div()
        .id(id)
        .flex()
        .items_center()
        .text_color(if selected {
            theme.foreground
        } else {
            theme.muted
        })
        .when(selected, |tab| tab.font_weight(FontWeight::MEDIUM))
        .role(Role::Tab)
        .aria_selected(selected)
        .on_click(click)
        .child(label.into());
    match ladder.touch {
        // the press is the whole strip: the box reaches over the bar's
        // hairline, and its label and underline keep to the bar above it
        true => tab
            .relative()
            .h(ladder.strip)
            .pb(HAIRLINE)
            .px(space::MD)
            .text_type(ladder.small)
            .whitespace_nowrap()
            .when(selected, |tab| {
                tab.child(
                    div()
                        .absolute()
                        .left(space::MD)
                        .right(space::MD)
                        .bottom(HAIRLINE)
                        .h(UNDERLINE)
                        .bg(theme.foreground),
                )
            }),
        false => tab
            .px_2()
            .py_1()
            .min_h(ladder.hit)
            .text_type(ladder.label)
            .border_b_2()
            .border_color(if selected {
                theme.foreground
            } else {
                theme.background
            })
            .hover(move |style| style.text_color(theme.foreground)),
    }
}

/// A bar of [`tab`]s (the Expo kit's `Strip`, `Choice.tsx:113-145`):
/// `bar` is the built `TabList` [`composite`], which holds the focus and
/// the arrows and says whether a move opens a tab or only a press does;
/// each tab is `item(tab(..), Role::Tab, active)`. It is 8 in, a hairline
/// under it, and scrolls sideways. Under a finger it is [`Ladder::strip`]
/// tall, its hairline included, and its tabs span it; under a pointer it
/// grows to hold its tabs above the hairline, since a pointer tab's own
/// bottom edge would cover it:
/// `design::strip(design::composite("pages", Role::TabList, "Pages")
/// .orientation(Orientation::Horizontal).wrap().active(at, count)
/// .on_move(..).build(), &ladder, &theme).children(tabs)`.
pub fn strip(bar: Stateful<Div>, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let bar = match ladder.touch {
        true => bar.h(ladder.strip),
        false => bar.min_h(ladder.strip),
    };
    bar.flex_none()
        .flex()
        .px(space::SM)
        .overflow_x_scroll()
        .border_b_1()
        .border_color(theme.border)
}

/// One Tab stop whose items the arrows pick: a tab list, a radio group, a
/// menu, a list box, a tree, a grid. The composite takes the focus and the
/// keys; the picked item is its active descendant ([`item`]) and is never
/// focusable. [`Composite::build`] gives the element; the caller styles it
/// and adds the items.
pub fn composite(
    id: impl Into<ElementId>,
    role: Role,
    label: impl Into<SharedString>,
) -> Composite {
    Composite {
        element: div().id(id).role(role).aria_label(label),
        items: Vec::new(),
        orientation: Orientation::Vertical,
        columns: None,
        cells: None,
        active: 0,
        count: 0,
        wrap: false,
        scroll: None,
        on_move: None,
        on_move_cell: None,
        on_press: None,
    }
}

type Picked = Box<dyn Fn(usize, &mut Window, &mut App)>;

/// A [`composite`] being built: its role, name, axis, active item and the
/// callbacks its keys reach; [`Composite::build`] is the element.
pub struct Composite {
    element: Stateful<Div>,
    items: Vec<AnyElement>,
    orientation: Orientation,
    columns: Option<usize>,
    /// the active row's active cell among its cells
    cells: Option<(usize, usize)>,
    active: usize,
    count: usize,
    wrap: bool,
    /// The uniform list the items are rows of: a move scrolls it to the
    /// row moved to, ahead of the render that claims it.
    scroll: Option<crate::UniformListScrollHandle>,
    on_move: Option<Picked>,
    on_move_cell: Option<Picked>,
    on_press: Option<Picked>,
}

impl Composite {
    /// Which arrows step: `Horizontal` ← →, `Vertical` ↑ ↓ (the default).
    /// Sent as `aria_orientation` too.
    pub fn orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        self
    }
    /// A grid of `columns` cells a row: ← → step by one, ↑ ↓ by a row;
    /// Home/End go to the row's ends, Ctrl+Home/End to the grid's.
    pub fn grid(mut self, columns: usize) -> Self {
        self.columns = Some(columns.max(1));
        self
    }
    /// The active item among `count`. The view keeps the index; a live list
    /// keeps the item's id and maps it to an index each render.
    pub fn active(mut self, index: usize, count: usize) -> Self {
        self.active = index;
        self.count = count;
        self
    }
    /// A grid whose rows have cells of their own (a message and its
    /// controls): `active` is the active row's active cell among `count`.
    /// ← → step the cells ([`Self::on_move_cell`]), ↑ ↓ the rows, Home/End
    /// reach the row's ends, Ctrl+Home/End the first and last row; Enter
    /// and Space press the row, whose active cell the view knows.
    pub fn cells(mut self, active: usize, count: usize) -> Self {
        self.cells = Some((active, count));
        self
    }
    /// ← → or Home/End picked cell `index` of the active row.
    pub fn on_move_cell(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_move_cell = Some(Box::new(f));
        self
    }
    /// The arrows wrap at the ends (a tab list, a radio group); the default
    /// stops there (a list box, a tree, a menu, a grid).
    pub fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }
    /// An arrow, Home or End picked item `index`: the view stores it and
    /// renders that item active.
    pub fn on_move(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Box::new(f));
        self
    }
    /// The items are the rows of a `uniform_list` tracking `handle`: a move
    /// scrolls the list to the row moved to (`Nearest`) before the view
    /// hears of it, so the frame that answers the key lowers that row and
    /// its claim, and a row off screen is scrolled to, not skipped.
    pub fn track_scroll(mut self, handle: &crate::UniformListScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }
    /// Enter or Space on the active item.
    pub fn on_press(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_press = Some(Box::new(f));
        self
    }
    /// The items, each an [`item`] (or a row holding them); a caller may
    /// add more on the built element.
    pub fn children(mut self, items: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.items
            .extend(items.into_iter().map(IntoElement::into_any_element));
        self
    }
    /// The element: roled, named, oriented, focusable (so a Tab stop) and
    /// hearing the keys of §3 — the orientation's arrows, Home/End, Enter
    /// and Space, unmodified; a grid's ↑ ↓ by a row and Ctrl+Home/End. A
    /// move the view answers by re-rendering is remembered until then, so
    /// a press in the same frame lands on the item just moved to.
    pub fn build(self) -> Stateful<Div> {
        let Self {
            element,
            items,
            orientation,
            columns,
            cells,
            active,
            count,
            wrap,
            scroll,
            on_move,
            on_move_cell,
            on_press,
        } = self;
        let on_move = on_move.map(|moved| -> Picked {
            match scroll {
                Some(scroll) => Box::new(move |index, window, app| {
                    scroll.scroll_to_item(index, ScrollStrategy::Nearest);
                    moved(index, window, app)
                }),
                None => moved,
            }
        });
        // the active item and cell as of the last key, ahead of the render
        // that shows the move
        let at = std::cell::Cell::new(active);
        let at_cell = std::cell::Cell::new(cells.map_or(0, |(cell, _)| cell));
        let keys = move |event: &KeyDownEvent, window: &mut Window, app: &mut App| {
            let keystroke = &event.keystroke;
            let modifiers = keystroke.modifiers;
            let plain = !modifiers.modified();
            let ctrl = modifiers.control
                && !(modifiers.alt || modifiers.shift || modifiers.platform || modifiers.function);
            let key = keystroke.key.as_str();
            if count == 0 {
                return;
            }
            let last = count - 1;
            let active = at.get();
            if plain && matches!(key, "enter" | "space") {
                if let Some(press) = &on_press {
                    press(active, window, app);
                }
                return;
            }
            // one step along the arrows' axis, or a row up or down a grid
            let step = |by: isize| -> Option<usize> {
                let to = active as isize + by;
                match (wrap, columns) {
                    (true, None) => Some(to.rem_euclid(count as isize) as usize),
                    _ => usize::try_from(to).ok().filter(|to| *to <= last),
                }
            };
            if let Some((_, cells)) = cells {
                let cell = at_cell.get();
                let last_cell = cells.saturating_sub(1);
                let (to_row, to_cell) = match (key, plain, ctrl) {
                    ("up", true, _) => (step(-1), None),
                    ("down", true, _) => (step(1), None),
                    ("left", true, _) => (None, cell.checked_sub(1)),
                    ("right", true, _) => (None, Some((cell + 1).min(last_cell))),
                    ("home", true, _) => (None, Some(0)),
                    ("end", true, _) => (None, Some(last_cell)),
                    ("home", _, true) => (Some(0), None),
                    ("end", _, true) => (Some(last), None),
                    _ => (None, None),
                };
                if let (Some(to), Some(moved)) = (to_row.filter(|to| *to != active), &on_move) {
                    at.set(to);
                    at_cell.set(0);
                    moved(to, window, app);
                }
                if let (Some(to), Some(moved)) = (to_cell.filter(|to| *to != cell), &on_move_cell) {
                    at_cell.set(to);
                    moved(to, window, app);
                }
                return;
            }
            let row_start = |columns: usize| active - active % columns;
            let next = match (key, plain, ctrl, orientation, columns) {
                ("left", true, _, Orientation::Horizontal, None)
                | ("up", true, _, Orientation::Vertical, None)
                | ("left", true, _, _, Some(_)) => step(-1),
                ("right", true, _, Orientation::Horizontal, None)
                | ("down", true, _, Orientation::Vertical, None)
                | ("right", true, _, _, Some(_)) => step(1),
                ("up", true, _, _, Some(columns)) => step(-(columns as isize)),
                ("down", true, _, _, Some(columns)) => step(columns as isize),
                ("home", true, _, _, None) | ("home", _, true, _, Some(_)) => Some(0),
                ("end", true, _, _, None) | ("end", _, true, _, Some(_)) => Some(last),
                ("home", true, _, _, Some(columns)) => Some(row_start(columns)),
                ("end", true, _, _, Some(columns)) => {
                    Some((row_start(columns) + columns - 1).min(last))
                }
                _ => None,
            };
            if let (Some(next), Some(moved)) = (next.filter(|next| *next != active), &on_move) {
                at.set(next);
                moved(next, window, app);
            }
        };
        element
            .aria_orientation(orientation)
            .focusable()
            .on_key_down(keys)
            .children(items)
    }
}

/// An item of a [`composite`]: its role, and the claim when it is the
/// active one. It keeps its `on_click` and the role's state
/// (`aria_selected`, `aria_toggled`) and is never focusable: the composite
/// holds the focus, so a control built focusable (a [`button`], a
/// [`link`]) leaves the Tab order here. A grid row is never an item:
/// the claim goes on a cell, or on the one control inside it.
pub fn item(mut element: Stateful<Div>, role: Role, active: bool) -> Stateful<Div> {
    let interactions = element.interactivity().interactions();
    interactions.focusable = false;
    interactions.tab_stop = None;
    element
        .role(role)
        .when(active, |item| item.aria_active_descendant())
}

/// The least a [`menu`] is wide: an item's words widen it past this.
const MENU_W: Pixels = px(200.);

/// A menu at a point in the window, as a right press, the Menu key or
/// Shift+F10 opens one over a page: a row's actions, a folder's writes.
/// One Tab stop ([`composite`], [`Role::Menu`]) whose ↑ ↓ walk its
/// [`menu_item`]s and whose Enter or Space presses the active one, in a
/// raised frame at `at` (in the window's pixels, a mouse event's
/// `position`) that the host keeps inside the pane, on the other side of
/// `at` where it does not fit. The frame takes the press: nothing under it
/// hears one.
///
/// The view keeps whether it is open, where, and which item is active. It
/// draws the menu as the modal of a [`modal_overlay`](crate::modal_overlay)
/// over the page, whose `on_dismiss` (a press outside, Escape) closes it,
/// and hands it the keyboard as it opens (`window.focus(id)`). The keys
/// never reach an item's element, the menu holds them: `on_press` runs
/// what the active item's click runs.
/// `modal_overlay("menu-overlay", "Actions", page, Some(design::menu("menu",
/// "Actions", at, &theme).active(view.active).on_move(..).on_press(..)
/// .item(design::menu_item("menu/open", "Open", &ladder, &theme, open)
/// .icon("folder-open"))))`.
pub fn menu(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    at: gpui::Point<Pixels>,
    theme: &Theme,
) -> Menu {
    Menu {
        composite: composite(id, Role::Menu, label),
        at,
        theme: *theme,
        items: Vec::new(),
        active: 0,
    }
}

/// A [`menu`] being built.
pub struct Menu {
    composite: Composite,
    at: gpui::Point<Pixels>,
    theme: Theme,
    items: Vec<MenuItem>,
    active: usize,
}

impl Menu {
    /// The item the arrows are on, among those given; past the last, the
    /// last.
    pub fn active(mut self, index: usize) -> Self {
        self.active = index;
        self
    }
    /// An arrow, Home or End picked item `index`: the view stores it and
    /// renders it active.
    pub fn on_move(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.composite = self.composite.on_move(f);
        self
    }
    /// Enter or Space on the active item, `index`.
    pub fn on_press(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.composite = self.composite.on_press(f);
        self
    }
    /// An item, under those given before it.
    pub fn item(mut self, item: MenuItem) -> Self {
        self.items.push(item);
        self
    }
}

impl IntoElement for Menu {
    type Element = crate::Anchored;
    fn into_element(self) -> crate::Anchored {
        let Self {
            composite,
            at,
            theme,
            items,
            active,
        } = self;
        let count = items.len();
        let active = active.min(count.saturating_sub(1));
        let items = items
            .into_iter()
            .enumerate()
            .map(|(index, item)| item.drawn(index == active));
        let frame = composite
            .active(active, count)
            .children(items)
            .build()
            .min_w(MENU_W)
            .flex()
            .flex_col()
            .gap(px(::design::spacing::HAIR as f32))
            .p(space::XXS)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .occlude();
        crate::anchored()
            .position(at)
            .snap_to_window_with_margin(gpui::Edges::all(space::SM))
            .child(focus_shown(frame, &theme, |style| style.shadow_lg()))
    }
}

/// One item of a [`menu`]: `label` in body text on one line, the whole row
/// the press (`click`), [`Ladder::control`] tall; [`MenuItem::icon`] leads
/// it with a glyph, and [`MenuItem::danger`] marks one that destroys:
/// `design::menu_item("menu/remove", "Remove", &ladder, &theme, remove)
/// .icon("trash").danger()`.
pub fn menu_item(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> MenuItem {
    MenuItem {
        id: id.into(),
        label: label.into(),
        icon: None,
        danger: false,
        ladder: *ladder,
        theme: *theme,
        click: Box::new(click),
    }
}

/// A [`menu_item`] being built.
pub struct MenuItem {
    id: ElementId,
    label: SharedString,
    icon: Option<SharedString>,
    danger: bool,
    ladder: Ladder,
    theme: Theme,
    click: crate::interactivity::EventListener<ClickEvent>,
}

impl gpui::prelude::FluentBuilder for MenuItem {}

impl MenuItem {
    /// A Lucide glyph before the label (`icons/<name>.svg`), at
    /// [`Ladder::icon`], muted.
    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self
    }
    /// The item destroys something (Remove, Delete): its glyph and its
    /// label wear the danger tone.
    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }
    /// The item as its menu draws it, `active` where the arrows are.
    fn drawn(self, active: bool) -> Stateful<Div> {
        let (ladder, theme) = (self.ladder, self.theme);
        let (glyph, words) = match self.danger {
            true => (theme.danger, theme.danger),
            false => (theme.muted, theme.foreground),
        };
        let row = div()
            .id(self.id)
            .w_full()
            .h(ladder.control)
            .flex()
            .items_center()
            .gap(space::SM)
            .px(space::SM)
            .text_type(ladder.body)
            .text_color(words)
            .whitespace_nowrap()
            .cursor_pointer()
            .hover(move |style| style.bg(theme.surface_raised))
            .active(move |style| style.bg(theme.accent_soft))
            .when(active, |row| row.bg(theme.surface_raised))
            .on_click(self.click)
            .children(
                self.icon
                    .map(|name| icon(&name, ladder.icon).text_color(glyph)),
            )
            .child(self.label);
        item(row, Role::MenuItem, active)
    }
}

/// A few choices side by side in one box, the picked one ink-filled: a
/// state filter, an object format, an invite's lifetime. `label` names the
/// choice, `choices` are each segment's id and label, `picked` the one
/// that is, or none (the arrows then start at the first); the box draws
/// the edge they share. A radio group: one Tab stop, and ← → pick the
/// next choice (`on_pick`), wrapping at the ends. Under a finger it is the
/// Expo kit's `Segmented` (`Choice.tsx:12-43`): 36 tall, the choices
/// sharing its width.
pub fn segmented(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    picked: Option<usize>,
    choices: impl IntoIterator<Item = (ElementId, SharedString)>,
    on_pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let on_pick = std::rc::Rc::new(on_pick);
    let choices: Vec<_> = choices.into_iter().collect();
    let count = choices.len();
    let moved = on_pick.clone();
    let active = picked.unwrap_or(0);
    let segments = choices.into_iter().enumerate().map(|(index, (id, label))| {
        let pick = on_pick.clone();
        let click =
            move |_: &ClickEvent, window: &mut Window, app: &mut App| pick(index, window, app);
        let (selected, active) = (picked == Some(index), index == active);
        match ladder.touch {
            true => {
                touch_segment(id, label, selected, active, ladder, theme, click).into_any_element()
            }
            false => item(
                segment(id, label, selected, ladder, theme, click),
                Role::RadioButton,
                active,
            )
            .into_any_element(),
        }
    });
    composite(id, Role::RadioGroup, label)
        .orientation(Orientation::Horizontal)
        .wrap()
        .active(active, count)
        .on_move(move |index, window, app| moved(index, window, app))
        .build()
        .flex()
        .flex_none()
        .items_center()
        .border_t_1()
        .border_b_1()
        .border_r_1()
        .border_color(theme.border_strong)
        .children(segments)
}

/// One choice of a [`segmented`] box under a pointer.
fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id)
        .h(ladder.row)
        .px(space::MD)
        .flex()
        .items_center()
        .border_l_1()
        .border_color(theme.border_strong)
        .text_type(ladder.label)
        .whitespace_nowrap()
        .map(|segment| match selected {
            true => segment
                .bg(theme.primary)
                .text_color(theme.primary_foreground),
            false => segment
                .text_color(theme.muted)
                .hover(move |style| style.text_color(theme.foreground)),
        })
        .role(Role::RadioButton)
        .aria_toggled(selected.into())
        .on_click(click)
        .child(label.into())
}

/// A segment's height under a finger: the box's
/// [`SEGMENTED`](::design::height::touch::SEGMENTED) less its top and
/// bottom hairlines, which React Native draws inside the box's height
/// (`Choice.tsx:16`).
const SEGMENT_HEIGHT: Pixels = px((::design::height::touch::SEGMENTED - 2.) as f32);

/// One choice of a [`segmented`] box under a finger (the Expo kit's
/// `Segmented`, `Choice.tsx:12-43`): the choices share the box's width,
/// each [`SEGMENT_HEIGHT`] tall, 10 in, its label centred in
/// `ladder.secondary` on one line, the chosen one medium on the ink; a
/// press does not dim (`Segmented` sets no pressed look). It only draws:
/// the press is a [`hit`] over it, the composite's item, reaching 5 over
/// and under it to [`Ladder::hit`] and none sideways, since a segment
/// shares a box as wide as the phone.
fn touch_segment(
    id: ElementId,
    label: SharedString,
    selected: bool,
    active: bool,
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    // `hit`'s width is the segment's own, which only layout knows. A
    // segmented choice holds 2-3 choices (Expo's own rule, `Choice.tsx:1`)
    // in a box as wide as the phone, so each is at least a finger wide and
    // the press takes none of its neighbours' width
    let press = hit(id, ladder.hit, SEGMENT_HEIGHT, px(0.), ladder)
        .aria_label(label.clone())
        .aria_toggled(selected.into())
        .on_click(click);
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .h(SEGMENT_HEIGHT)
        .px(space::MD)
        .flex()
        .items_center()
        .justify_center()
        .border_l_1()
        .border_color(theme.border_strong)
        .text_type(ladder.secondary)
        .map(|segment| match selected {
            true => segment
                .bg(theme.primary)
                .text_color(theme.primary_foreground)
                .font_weight(FontWeight::MEDIUM),
            false => segment.text_color(theme.muted),
        })
        .child(div().min_w_0().truncate().child(label))
        .child(item(press, Role::RadioButton, active))
}

/// An on/off switch: a pill with its knob at the on or the off end, in a
/// box no smaller than [`Ladder::hit`] each way.
pub fn switch(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    enabled: bool,
    ladder: &Ladder,
    theme: &Theme,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let knob = div().size(px(12.)).rounded_full().bg(theme.background);
    let pill = div()
        .w(px(28.))
        .h(px(16.))
        .flex_none()
        .flex()
        .items_center()
        .px(px(2.))
        .rounded_full()
        .bg(if on {
            theme.primary
        } else {
            theme.border_strong
        })
        .when(on, |pill| pill.justify_end())
        .child(knob);
    let element = div()
        .id(id)
        .min_w(ladder.hit)
        .min_h(ladder.hit)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .role(Role::Switch)
        .aria_label(label.into())
        .aria_toggled(on.into())
        .child(pill);
    match enabled {
        // the hit box around the pill sits on the window's ground, so the
        // ring is the plain one whether the pill is ink or not
        true => focus_shown(
            element.cursor_pointer().focusable().on_click(toggle),
            theme,
            |style| style,
        ),
        false => element.opacity(0.5).aria_disabled(true),
    }
}

/// One setting: its title and a line about it on the left, its control on
/// the right, a hairline under it.
pub fn setting_row(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .w_full()
        .min_h(ladder.row)
        // wrapped, it grows; a column around it must not squeeze it
        .flex_none()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space::LG)
        .py(space::MD)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .min_w(px(200.))
                .flex()
                .flex_col()
                .gap(space::HAIR)
                .child(
                    div()
                        .text_type(ladder.body)
                        .font_weight(FontWeight::MEDIUM)
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(text::SECONDARY)
                        .text_color(theme.muted)
                        .child(description.into()),
                ),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(space::SM)
                .child(control),
        )
}

/// A person's round initial at `size` (a [`Face`]: a ladder's rung, or a
/// side alone), on the raised surface. A caller recolours it (an agent, a
/// speaker) with `bg` / `text_color`.
pub fn avatar(name: &str, size: impl Into<Face>, theme: &Theme) -> Div {
    let Face { side, text, weight } = size.into();
    div()
        .size(side)
        .flex_shrink_0()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.surface_raised)
        .text_color(theme.muted)
        .text_type(text)
        .when_some(weight, |face, weight| face.font_weight(weight))
        .child(initial(name))
}

/// How far an arrow key moves a [`divider`]; shift moves it four times as far.
const STEP: f32 = 8.;

/// The line between two panes, dragged to move it: `drag` takes the
/// horizontal delta (and clamps the layout it moves). `label` names it
/// ("Resize the room list"); focused, left and right move it by [`STEP`].
pub fn divider<V: 'static>(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    theme: &Theme,
    cx: &mut crate::Context<V>,
    drag: impl Fn(&mut V, f32) + 'static,
) -> crate::ResizeHandle {
    let drag = std::rc::Rc::new(drag);
    let dragged = cx.listener({
        let drag = drag.clone();
        move |view, delta: &(Pixels, Pixels), _window, cx| {
            drag(view, delta.0.into());
            cx.notify();
        }
    });
    let stepped = cx.listener(move |view, event: &KeyDownEvent, _window, cx| {
        let step = match event.keystroke.modifiers.shift {
            true => STEP * 4.,
            false => STEP,
        };
        let delta = match event.keystroke.key.as_str() {
            "left" => -step,
            "right" => step,
            _ => return,
        };
        drag(view, delta);
        cx.notify();
    });
    crate::resize_handle(id, div().w(crate::px(1.)).h_full().bg(theme.border))
        .on_drag(dragged)
        .role(Role::Splitter)
        .aria_label(label)
        .aria_orientation(gpui::Orientation::Vertical)
        .focusable()
        .tab_stop(true)
        // the line covers an inset ring whole: this one sits outside it
        .focus_visible(|style| style.shadow(vec![ring_shadow(theme.accent, 2., false)]))
        .on_key_down(stepped)
}

/// Whether a side pane `side` wide fits in `width` beside what the screen
/// `keeps` (its list and its body's narrowest). When it does not, the pane
/// floats over the body ([`over`]) and the body keeps its whole width.
pub fn docks(width: f32, keeps: f32, side: f32) -> bool {
    width >= keeps + side
}

/// A side pane that does not [`docks`]: it covers the whole of its
/// `relative` parent, list and body alike, at the pane's own width no
/// more, so nothing underneath stays half in view; a click on it stops
/// there. The pane carries its own close control.
pub fn over(
    id: impl Into<ElementId>,
    pane: impl IntoElement + Styled,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .flex()
        .bg(theme.background)
        .occlude()
        .child(pane.w_full().h_full())
}

/// A small tag: a state, a role, a count, in its own colours.
pub fn badge(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    foreground: Hsla,
    background: Hsla,
) -> Stateful<Div> {
    let tag = div().id(id).bg(background).text_color(foreground);
    match ladder.touch {
        // the Expo kit's `Badge`: 20 tall, 6 in, tiny medium (`Badge.tsx:20-22`)
        true => tag
            .h(px(::design::height::touch::BADGE as f32))
            .flex()
            .flex_none()
            .items_center()
            .px(space::XS)
            .text_type(ladder.tiny)
            .font_weight(FontWeight::MEDIUM),
        false => tag.px_1().py_0p5().text_type(ladder.caption),
    }
    .child(label.into())
}

/// A link: subdued mono text that underlines under the pointer and opens
/// `href` through `link.open`, in a box no smaller than [`Ladder::hit`]
/// with the text centred down it. Its text is its name; a caller that
/// says more adds `.aria_label(..)`. Its click consumes the press: a
/// clickable card under it does not hear it.
pub fn link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    href: impl Into<String>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    let href = href.into();
    div()
        .id(id)
        .min_w(ladder.hit)
        .min_h(ladder.hit)
        .flex()
        .items_center()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .font_family(fonts::FAMILY_MONO)
        .whitespace_nowrap()
        .cursor_pointer()
        .hover(move |style| style.text_color(theme.foreground).text_decoration_1())
        .role(Role::Link)
        .focusable()
        .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| cx.host().open_link(&href))
        .consumes_click()
        .consumes_keys(["enter", "space"])
        .child(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire;
    use gpui::Toggled;

    fn lower(element: impl IntoElement) -> wire::Node {
        lower_styled(element).0
    }

    /// The lowered element, and the table its nodes' styles are in.
    fn lower_styled(element: impl IntoElement) -> (wire::Node, wire::Styles) {
        let (node, styles, _) = crate::testing::lower(|_: &mut Context<()>| element);
        (node, styles)
    }

    fn interactivity(node: &wire::Node) -> &wire::Interactivity {
        node.interactivity()
            .unwrap_or_else(|| panic!("no interactivity: {node:?}"))
    }

    fn faults(node: &wire::Node) -> Vec<wire::FaultKind> {
        wire::audit(node)
            .into_iter()
            .map(|fault| fault.kind)
            .collect()
    }

    /// A glyph alone names nothing: the button is named in words, and is
    /// at least a press target each way.
    #[test]
    fn an_icon_button_is_a_focusable_button_named_in_words_and_a_press_target() {
        let theme = Theme::light();
        let (node, styles) = lower_styled(icon_button(
            "close",
            "✕",
            "Close",
            &POINTER,
            &theme,
            |_, _, _| {},
        ));
        let control = interactivity(&node);
        assert_eq!(control.role, Some(Role::Button));
        assert_eq!(control.aria.label.as_deref(), Some("Close"));
        assert!(control.focusable && control.on_click.is_some());
        assert_eq!(faults(&node), []);
        let style = &styles[node.style().expect("a styled container")];
        let floor = Some(POINTER.hit.into());
        assert_eq!(
            (style.min_size.width, style.min_size.height),
            (floor, floor)
        );
    }

    /// A menu is one Tab stop at its point, which the arrows hold: its
    /// items take no focus, and the active one claims the menu's.
    #[test]
    fn a_menu_is_one_tab_stop_at_its_point_whose_active_item_claims_it() {
        let theme = Theme::light();
        let item = |id: &'static str, label: &'static str| {
            menu_item(id, label, &POINTER, &theme, |_, _, _| {})
        };
        let node = lower(
            menu("menu", "Actions", gpui::point(px(40.), px(60.)), &theme)
                .active(1)
                .on_move(|_, _, _| {})
                .on_press(|_, _, _| {})
                .item(item("open", "Open").icon("folder-open"))
                .item(item("remove", "Remove").icon("trash").danger()),
        );
        let wire::Node::Anchored {
            position, children, ..
        } = &node
        else {
            panic!("a popup at a point: {node:?}");
        };
        assert_eq!(*position, Some([40., 60.]));
        let [frame] = children.as_slice() else {
            panic!("one frame: {children:?}");
        };
        let held = interactivity(frame);
        assert_eq!(held.role, Some(Role::Menu));
        assert!(held.focusable && held.on_key_down.is_some());
        let claims: Vec<bool> = frame
            .children()
            .iter()
            .map(|item| {
                let item = interactivity(item);
                assert_eq!(item.role, Some(Role::MenuItem));
                assert!(!item.focusable && item.on_click.is_some());
                item.aria.active_descendant
            })
            .collect();
        assert_eq!(claims, [false, true]);
        assert_eq!(faults(&node), []);
    }

    /// An eyebrow crosses as the words were written, marked for the host to
    /// draw in capitals: a screen reader hears the words a text holds, so
    /// it reads the label, not letter after letter. The role's size and
    /// spacing, the data face and the muted colour are on its box.
    #[test]
    fn an_eyebrow_is_drawn_in_capitals_and_named_as_written() {
        let theme = Theme::light();
        let (node, styles) = lower_styled(eyebrow("This phone's code", &TOUCH, &theme));
        let [wire::Node::Text(words)] = node.children() else {
            panic!("one text: {node:?}");
        };
        assert_eq!(words.content, "This phone's code");
        assert_eq!(words.transform, Some(wire::TextTransform::Uppercase));
        let text = &styles[node.style().expect("a styled box")].text;
        assert_eq!(text.font_size, Some(TOUCH.eyebrow.size.into()));
        assert_eq!(text.letter_spacing, TOUCH.eyebrow.letter_spacing);
        assert_eq!(text.font_family.as_deref(), Some(fonts::FAMILY_MONO));
        assert_eq!(text.color, Some(theme.muted));
    }

    /// The finger's ladder is the Expo app's theme (`theme.ts` `type`,
    /// `size`), number for number.
    #[test]
    fn the_touch_ladder_is_the_expo_theme() {
        let role = |(size, line, letter_spacing): (f32, f32, Option<f32>)| Type {
            size: px(size),
            line: Some(px(line)),
            letter_spacing: letter_spacing.map(px),
        };
        let roles = |l: Ladder| {
            [
                l.display,
                l.big_title,
                l.heading,
                l.title,
                l.body,
                l.label,
                l.small,
                l.secondary,
                l.caption,
                l.tiny,
                l.eyebrow,
                l.code,
            ]
        };
        let t = TOUCH;
        assert_eq!(
            [
                t.header,
                t.row,
                t.row_tall,
                t.strip,
                t.sheet_row,
                t.control,
                t.hit,
                t.icon,
                t.icon_row,
                t.icon_sheet
            ],
            [56., 48., 56., 44., 52., 44., 44., 22., 18., 20.].map(px)
        );
        assert_eq!(
            roles(t),
            [
                (36., 44., Some(1.44)),
                (22., 27., Some(-0.22)),
                (20., 24., None),
                (17., 21., None),
                (15., 21., None),
                (15., 21., None),
                (14., 19., None),
                (13., 18., None),
                (12., 16., None),
                (11., 14., None),
                (11., 14., Some(0.66)),
                (12., 20., None)
            ]
            .map(role)
        );
        let faces = [t.avatar_inline, t.avatar, t.avatar_sheet, t.avatar_detail];
        assert_eq!(
            faces.map(|face| (face.side, face.text.size, face.weight)),
            [(24., 11.), (32., 13.), (40., 16.), (56., 22.)].map(|(side, text)| (
                px(side),
                px(text),
                Some(FontWeight::MEDIUM)
            ))
        );
        // a pointer's roles name no line height and no letter spacing: the
        // desktop's text stands
        assert!(
            roles(POINTER)
                .iter()
                .all(|role| role.line.is_none() && role.letter_spacing.is_none())
        );
    }

    /// Under a finger an icon button is the kit's `Hit`: 44 each way, the
    /// glyph in the foreground; filled it is the ink square, disabled it
    /// is faint and hears no press.
    #[test]
    fn under_touch_an_icon_button_is_the_kits_hit() {
        let theme = Theme::light();
        let send = |filled: bool| {
            lower_styled(
                icon_button(
                    "send",
                    icon("send-horizontal", TOUCH.icon),
                    "Send",
                    &TOUCH,
                    &theme,
                    |_, _, _| {},
                )
                .filled(filled)
                .enabled(filled),
            )
        };
        let (node, styles) = send(true);
        let style = &styles[node.style().expect("a styled container")];
        assert_eq!(
            (style.size.width, style.size.height),
            (Some(px(44.).into()), Some(px(44.).into()))
        );
        assert_eq!(style.background, Some(theme.primary.into()));
        assert_eq!(style.text.color, Some(theme.primary_foreground));
        assert!(interactivity(&node).on_click.is_some());
        let (node, styles) = send(false);
        let style = &styles[node.style().expect("a styled container")];
        assert_eq!(style.background, None);
        assert_eq!(style.text.color, Some(theme.faint));
        let control = interactivity(&node);
        assert!(control.on_click.is_none() && control.aria.disabled == Some(true));
        assert_eq!(faults(&node), []);
    }

    /// Under a finger a button is the kit's: 44 tall (48 the primary), an
    /// outline for a plain one, the ink fill for the primary.
    #[test]
    fn under_touch_a_button_is_the_kits_button() {
        let theme = Theme::light();
        let style = |kind: Kind| {
            let (node, styles) =
                lower_styled(button("save", "Save", &TOUCH, &theme, |_, _, _| {}).kind(kind));
            styles[node.style().expect("a styled container")].clone()
        };
        let plain = style(Kind::Plain);
        assert_eq!(plain.size.height, Some(px(44.).into()));
        assert_eq!(plain.border_color, Some(theme.border_strong));
        assert_eq!(plain.background, None);
        let primary = style(Kind::Primary);
        assert_eq!(primary.size.height, Some(px(48.).into()));
        assert_eq!(primary.background, Some(theme.primary.into()));
    }

    /// The look of a lowered node: its row of the style table.
    fn look(node: &wire::Node, styles: &wire::Styles) -> StyleRefinement {
        styles[node.style().expect("a styled node")].clone()
    }

    /// Under a finger a small button's face is 36 tall, 12 in, in
    /// secondary, at least 44 wide (`Button.tsx:60-61`, `:76`). Its press
    /// is the face's parent, 44 tall, handing back the 4 it reaches over
    /// and under, so a press anywhere on it dims the face.
    #[test]
    fn under_touch_a_small_buttons_press_is_44_round_its_36_face_and_dims_it() {
        let theme = Theme::light();
        let retry = button("retry", "Retry", &TOUCH, &theme, |_, _, _| {}).small();
        let (row, styles) = lower_styled(div().w(px(360.)).child(retry));
        assert_eq!(faults(&row), []);
        crate::testing::assert_fits(&row, &styles, &[], 360., &TOUCH);
        let press = &row.children()[0];
        let reach = look(press, &styles);
        assert_eq!(reach.size.height, Some(px(44.).into()));
        assert_eq!(
            (reach.margin.top, reach.margin.bottom),
            (Some(px(-4.).into()), Some(px(-4.).into()))
        );
        let control = interactivity(press);
        assert_eq!(control.role, Some(Role::Button));
        assert!(control.focusable && control.on_click.is_some());
        assert_eq!(
            control.active.and_then(|active| styles[active].opacity),
            Some(0.6)
        );
        let face = look(&press.children()[0], &styles);
        assert_eq!(face.size.height, Some(px(36.).into()));
        assert_eq!(face.min_size.width, Some(px(44.).into()));
        assert_eq!(face.padding.left, Some(px(12.).into()));
        assert_eq!(face.text.font_size, Some(TOUCH.secondary.size.into()));
    }

    /// A pointer has one button size: `.small()` changes nothing under it.
    #[test]
    fn under_a_pointer_a_small_button_is_the_same_button() {
        let theme = Theme::light();
        let save = || button("save", "Save", &POINTER, &theme, |_, _, _| {});
        assert_eq!(
            format!("{:?}", lower_styled(save().small())),
            format!("{:?}", lower_styled(save()))
        );
    }

    /// A busy button drops its click and says it is busy. Under a finger
    /// it keeps its look, as Expo's does (`Button.tsx:35-56`); under a
    /// pointer it looks disabled.
    #[test]
    fn a_busy_button_drops_its_click_and_says_it_is_busy() {
        let theme = Theme::light();
        let busy = |ladder: &Ladder| {
            let save = button("save", "Save", ladder, &theme, |_, _, _| {});
            lower_styled(save.kind(Kind::Primary).busy(true))
        };
        let (node, styles) = busy(&TOUCH);
        assert_eq!(faults(&node), []);
        let control = interactivity(&node);
        assert!(control.on_click.is_none() && !control.focusable);
        assert_eq!(control.aria.disabled, Some(true));
        assert!(control.aria.busy);
        assert_eq!(look(&node, &styles).background, Some(theme.primary.into()));
        let (node, styles) = busy(&POINTER);
        let control = interactivity(&node);
        assert!(control.on_click.is_none() && control.aria.busy);
        assert_eq!(look(&node, &styles).background, Some(theme.faint.into()));
    }

    /// The first [`wire::Node::Field`] in `node`'s tree.
    fn field_node(node: &wire::Node) -> Option<&wire::Node> {
        match node {
            wire::Node::Field { .. } => Some(node),
            _ => node.children().iter().find_map(field_node),
        }
    }

    /// A field's problem is its control's: invalid, and the problem its
    /// description, so a screen reader hears what the eye reads (rule 18);
    /// without one, its caption is the description. One line or many.
    #[test]
    fn a_fields_problem_makes_its_control_invalid_and_says_why() {
        let theme = Theme::light();
        let text = crate::TextField::new("ada");
        let said = |node: wire::Node| {
            assert_eq!(faults(&node), []);
            let Some(wire::Node::Field { options, .. }) = field_node(&node) else {
                panic!("a field: {node:?}")
            };
            (options.invalid, options.description.clone())
        };
        let refused = (Some(wire::Invalid::True), Some("Too long.".to_owned()));
        let line = || {
            field(crate::Input::new("name", &text, "Name"), &TOUCH, &theme)
                .label("Name")
                .caption("Up to 32 letters.")
        };
        assert_eq!(
            said(lower(line())),
            (None, Some("Up to 32 letters.".to_owned()))
        );
        assert_eq!(said(lower(line().problem("Too long."))), refused);
        let lines = || {
            field(
                crate::Textarea::new("about", &text, "About"),
                &TOUCH,
                &theme,
            )
        };
        assert_eq!(said(lower(lines())), (None, None));
        assert_eq!(said(lower(lines().problem("Too long."))), refused);
    }

    /// A field's problem thickens its edge to 2 in danger and pulls the
    /// text in by 1, so the text stays where it was (`Field.tsx:37-38`,
    /// `:52`); a lead glyph puts the text 8 after it, 39 in (`:58`); mono
    /// is the data face at secondary (`:76-77`).
    #[test]
    fn a_fields_problem_thickens_its_edge_and_keeps_its_text_in_place() {
        let theme = Theme::light();
        let text = crate::TextField::new("ada");
        let control = |built: Field<crate::Input>| {
            let (node, styles) = lower_styled(built);
            look(field_node(&node).expect("a field"), &styles)
        };
        let key = || field(crate::Input::new("key", &text, "Key"), &TOUCH, &theme);
        let calm = control(key());
        assert_eq!(calm.border_widths.left, Some(px(1.).into()));
        assert_eq!(calm.padding.left, Some(px(12.).into()));
        let refused = control(key().problem("Too long."));
        assert_eq!(refused.border_widths.left, Some(px(2.).into()));
        assert_eq!(refused.border_color, Some(theme.danger));
        assert_eq!(refused.padding.left, Some(px(11.).into()));
        assert_eq!(
            control(key().lead("search")).padding.left,
            Some(px(38.).into())
        );
        let mono = control(key().mono());
        assert_eq!(mono.text.font_family, Some(fonts::FAMILY_MONO.into()));
        assert_eq!(mono.text.font_size, Some(TOUCH.secondary.size.into()));
    }

    /// Under a pointer a field is the desktop's control tall, its text the
    /// desktop's body.
    #[test]
    fn under_a_pointer_a_field_is_the_desktops_control() {
        let theme = Theme::light();
        let text = crate::TextField::new("");
        let built = field(crate::Input::new("name", &text, "Name"), &POINTER, &theme);
        let (node, styles) = lower_styled(built.label("Name"));
        assert_eq!(faults(&node), []);
        let control = look(field_node(&node).expect("a field"), &styles);
        assert_eq!(control.min_size.height, Some(POINTER.control.into()));
        assert_eq!(control.text.font_size, Some(POINTER.body.size.into()));
    }

    /// A row's words are its title in body over a caption in caption
    /// muted, 1 apart (`Row.tsx:58-69`), and the chevron after them is the
    /// row icon's size, faint (`:75`); each by the ladder.
    #[test]
    fn a_rows_words_are_its_title_over_its_caption_one_apart() {
        let theme = Theme::light();
        for ladder in [&TOUCH, &POINTER] {
            let words = row_words(row_title("Ada", ladder, &theme))
                .child(row_caption("#42", ladder, &theme));
            let (words, styles) = lower_styled(words);
            assert_eq!(look(&words, &styles).gap.height, Some(px(1.).into()));
            let [title, caption] = words.children() else {
                panic!("a title and a caption: {words:?}")
            };
            let (title, caption) = (look(title, &styles), look(caption, &styles));
            assert_eq!(title.text.font_size, Some(ladder.body.size.into()));
            assert_eq!(title.text.color, Some(theme.foreground));
            assert_eq!(caption.text.font_size, Some(ladder.caption.size.into()));
            assert_eq!(caption.text.color, Some(theme.muted));
            let (glyph, styles) = lower_styled(chevron(ladder, &theme));
            let glyph = look(&glyph, &styles);
            assert_eq!(glyph.size.height, Some(ladder.icon_row.into()));
            assert_eq!(glyph.text.color, Some(theme.faint));
        }
    }

    /// A form sheet's head holds its title as a level-2 heading in title
    /// medium (`Agents.tsx:189-193`).
    #[test]
    fn a_sheet_title_is_a_level_2_heading_in_title_medium() {
        let theme = Theme::light();
        let (head, styles) = lower_styled(sheet_title("add", "Add an agent", &TOUCH, &theme));
        assert_eq!(faults(&head), []);
        let title = &head.children()[0];
        let heading = interactivity(title);
        assert_eq!(heading.role, Some(Role::Heading));
        assert_eq!(heading.aria.level, Some(2));
        let look = look(title, &styles);
        assert_eq!(look.text.font_size, Some(TOUCH.title.size.into()));
        assert_eq!(look.text.font_weight, Some(FontWeight::MEDIUM));
    }

    /// A form's verbs keep the kinds their caller built: Cancel, then the
    /// verb taking the rest of the row (`Agents.tsx:224-232`).
    #[test]
    fn form_verbs_keep_the_kinds_their_caller_built() {
        let theme = Theme::light();
        let verbs = |cancel_kind: Kind, verb_kind: Kind| {
            let (row, styles) = lower_styled(form_verbs(
                button("cancel", "Cancel", &TOUCH, &theme, |_, _, _| {}).kind(cancel_kind),
                button("add", "Add", &TOUCH, &theme, |_, _, _| {}).kind(verb_kind),
            ));
            let [cancel, rest] = row.children() else {
                panic!("Cancel, then the verb's column: {row:?}")
            };
            assert_eq!(look(rest, &styles).flex_grow, Some(1.));
            let verb = &rest.children()[0];
            (
                look(cancel, &styles).border_color,
                look(verb, &styles).background,
            )
        };
        assert_eq!(
            verbs(Kind::Quiet, Kind::Primary),
            (Some(Hsla::transparent_black()), Some(theme.primary.into()))
        );
        assert_eq!(
            verbs(Kind::Outline, Kind::Outline),
            (Some(theme.border_strong), None)
        );
    }

    /// A sheet row is one press the row's height; a hint trails its label
    /// in caption muted, and a row that destroys wears the danger tone.
    #[test]
    fn under_touch_a_sheet_rows_hint_trails_and_its_danger_shows() {
        let theme = Theme::light();
        let destroys = true;
        let (row, styles) = lower_styled(
            sheet_row("delete", "Delete", &TOUCH, &theme, |_, _, _| {})
                .icon("trash")
                .hint("3 files")
                .when(destroys, SheetRow::danger),
        );
        let control = interactivity(&row);
        assert_eq!(control.role, Some(Role::Button));
        assert!(control.focusable && control.on_click.is_some());
        assert_eq!(faults(&row), []);
        assert_eq!(
            look(&row, &styles).size.height,
            Some(TOUCH.sheet_row.into())
        );
        let [glyph, label, hint] = row.children() else {
            panic!("a glyph, a label and a hint: {row:?}")
        };
        assert_eq!(look(glyph, &styles).text.color, Some(theme.danger));
        assert_eq!(look(label, &styles).text.color, Some(theme.danger));
        let hint = look(hint, &styles);
        assert_eq!(hint.text.color, Some(theme.muted));
        assert_eq!(hint.text.font_size, Some(TOUCH.caption.size.into()));
    }

    /// Under a pointer a sheet row is the desktop's row tall and does not
    /// dim.
    #[test]
    fn under_a_pointer_a_sheet_row_is_a_row_tall_and_does_not_dim() {
        let theme = Theme::light();
        let copy = sheet_row("copy", "Copy", &POINTER, &theme, |_, _, _| {}).hint("⌘C");
        let (row, styles) = lower_styled(copy);
        assert_eq!(faults(&row), []);
        assert_eq!(
            look(&row, &styles).size.height,
            Some(POINTER.sheet_row.into())
        );
        assert_eq!(interactivity(&row).active, None);
    }

    /// A strip is the ladder's strip tall under a finger, and at least that
    /// under a pointer, a hairline under it either way.
    #[test]
    fn a_strip_is_the_strip_tall_under_a_finger_and_at_least_that_under_a_pointer() {
        let theme = Theme::light();
        let bar = |ladder: &Ladder| {
            let bar = composite("pages", Role::TabList, "Pages").build();
            let files = tab("files", "Files", true, ladder, &theme, |_, _, _| {});
            let (node, styles) =
                lower_styled(strip(bar, ladder, &theme).child(item(files, Role::Tab, true)));
            assert_eq!(faults(&node), []);
            look(&node, &styles)
        };
        let touch = bar(&TOUCH);
        assert_eq!(touch.size.height, Some(px(44.).into()));
        assert_eq!(touch.border_widths.bottom, Some(px(1.).into()));
        let pointer = bar(&POINTER);
        assert_eq!(pointer.size.height, None);
        assert_eq!(pointer.min_size.height, Some(POINTER.strip.into()));
    }

    /// Under a finger an empty state is the kit's: centred, 40 over and 32
    /// in, the glyph 28 faint over the title and the detail
    /// (`States.tsx:26-31`).
    #[test]
    fn under_touch_an_empty_state_centres_its_glyph_over_its_words() {
        let theme = Theme::light();
        let (empty, styles) = lower_styled(empty_state(
            "none",
            "users",
            "No accounts",
            "Nothing yet.",
            &TOUCH,
            &theme,
        ));
        assert_eq!(faults(&empty), []);
        let state = look(&empty, &styles);
        assert_eq!(state.align_items, Some(gpui::AlignItems::Center));
        assert_eq!(state.padding.top, Some(px(40.).into()));
        assert_eq!(state.padding.left, Some(px(32.).into()));
        let glyph = look(&empty.children()[0], &styles);
        assert_eq!(glyph.size.height, Some(px(28.).into()));
        assert_eq!(glyph.text.color, Some(theme.faint));
    }

    /// Under a pointer an empty state is its title and detail alone, as the
    /// desktop has always drawn it.
    #[test]
    fn under_a_pointer_an_empty_state_is_its_words_alone() {
        let theme = Theme::light();
        let (empty, _) = lower_styled(empty_state(
            "none",
            "users",
            "No accounts",
            "Nothing yet.",
            &POINTER,
            &theme,
        ));
        assert_eq!(faults(&empty), []);
        assert_eq!(empty.children().len(), 2);
    }

    /// Under a finger a refused read is the kit's danger notice, an alert
    /// a screen reader says at once (`Notice.tsx:37`). Retry's press
    /// reaches 8 past it on every side (`hitSlop`, `Notice.tsx:60`): 46
    /// tall, a margin of -8 handing the room back, the word 16 in.
    #[test]
    fn under_touch_a_refused_read_is_an_alert_whose_retry_reaches_8_past_it() {
        let theme = Theme::light();
        let notice = refused("rooms", "The node refused.", &TOUCH, &theme, |_, _, _| {});
        let (screen, styles) = lower_styled(div().w(px(360.)).child(notice));
        assert_eq!(faults(&screen), []);
        crate::testing::assert_fits(&screen, &styles, &[], 360., &TOUCH);
        let notice = &screen.children()[0];
        let alert = interactivity(notice);
        assert_eq!(alert.role, Some(Role::Alert));
        assert_eq!(alert.aria.live, Some(wire::Live::Assertive));
        let retry = &notice.children()[1];
        let control = interactivity(retry);
        assert!(control.focusable && control.on_click.is_some());
        let reach = look(retry, &styles);
        assert_eq!(reach.size.height, Some(px(46.).into()));
        assert_eq!(
            (reach.margin.top, reach.margin.left),
            (Some(px(-8.).into()), Some(px(-8.).into()))
        );
        assert_eq!(reach.padding.left, Some(px(16.).into()));
    }

    /// What the Expo kit sets `weight="medium"` asks for 500 under a
    /// finger (the app draws Instrument Sans Medium); a pointer's weights
    /// are the desktop's.
    #[test]
    fn under_touch_the_kits_medium_roles_ask_for_500() {
        let theme = Theme::light();
        let weight = |(node, styles): (wire::Node, wire::Styles)| {
            styles[node.style().expect("a styled node")]
                .text
                .font_weight
        };
        let medium = Some(FontWeight::MEDIUM);
        let label = |kind: Kind| {
            weight(lower_styled(
                button("save", "Save", &TOUCH, &theme, |_, _, _| {}).kind(kind),
            ))
        };
        assert_eq!(label(Kind::Plain), medium);
        assert_eq!(label(Kind::Quiet), None);
        let tag = |ladder: &Ladder| {
            weight(lower_styled(badge(
                "n",
                "3",
                ladder,
                theme.foreground,
                theme.surface,
            )))
        };
        assert_eq!(tag(&TOUCH), medium);
        let chosen = |ladder: &Ladder| {
            let (node, styles) = lower_styled(segmented(
                "mode",
                "Mode",
                ladder,
                &theme,
                Some(1),
                [("a".into(), "A".into()), ("b".into(), "B".into())],
                |_, _, _| {},
            ));
            let picked = node.children()[1].clone();
            [
                weight((node.children()[0].clone(), styles.clone())),
                weight((picked, styles)),
            ]
        };
        assert_eq!(chosen(&TOUCH), [None, medium]);
        assert_eq!(chosen(&POINTER), [None, None]);
        let retry = |ladder: &Ladder| {
            let (node, styles) = lower_styled(refused("feed", "No.", ladder, &theme, |_, _, _| {}));
            weight((node.children()[1].clone(), styles))
        };
        assert_eq!(retry(&TOUCH), medium);
        assert_eq!(retry(&POINTER), None);
    }

    #[test]
    fn a_segmented_choice_is_a_radio_group_with_its_name() {
        let theme = Theme::light();
        let node = lower(segmented(
            "format",
            "Object format",
            &POINTER,
            &theme,
            Some(0),
            [("sha1".into(), "SHA-1".into())],
            |_, _, _| {},
        ));
        let group = interactivity(&node);
        assert_eq!(group.role, Some(Role::RadioGroup));
        assert_eq!(group.aria.label.as_deref(), Some("Object format"));
        assert!(group.focusable && group.on_key_down.is_some());
        assert_eq!(faults(&node), []);
    }

    #[test]
    fn a_focusable_node_shows_the_focus_ring_unless_it_draws_its_own() {
        let theme = Theme::light();
        // the style a node shows while it holds the keyboard
        let shown = |element: crate::AnyElement| {
            let (node, styles) = lower_styled(element);
            node.interactivity()
                .and_then(|interactivity| interactivity.focus_visible)
                .map(|style| styles[style].clone())
        };
        let plain = button("save", "Save", &POINTER, &theme, |_, _, _| {});
        assert_eq!(
            shown(plain.into_any_element()),
            Some(focus_ring(theme.accent))
        );
        let ink = button("send", "Send", &POINTER, &theme, |_, _, _| {}).kind(Kind::Primary);
        assert_eq!(
            shown(ink.into_any_element()),
            Some(focus_ring(theme.primary_foreground))
        );
        let own = div()
            .id("menu")
            .focusable()
            .focus_visible(|style| style.opacity(0.5));
        assert_eq!(
            shown(own.into_any_element()),
            Some(StyleRefinement::default().opacity(0.5))
        );
        let still = div().id("box").child("text");
        assert_eq!(shown(still.into_any_element()), None);
        // a frame that adds to the ring keeps its own shadow beside it
        let frame = focus_shown(div().id("frame").focusable(), &theme, |style| {
            style.shadow_lg()
        });
        let shown = shown(frame.into_any_element()).unwrap();
        assert_eq!(shown.border_color, Some(theme.accent));
        let shadows = shown.box_shadow.unwrap();
        assert_eq!(shadows.len(), 3);
        assert_eq!(shadows[0], ring_shadow(theme.accent, 2., true));
    }

    #[test]
    fn a_switch_that_is_on_reports_toggled_true() {
        let theme = Theme::light();
        for (on, toggled) in [(true, Toggled::True), (false, Toggled::False)] {
            let node = lower(switch(
                "dark",
                "Dark",
                on,
                true,
                &POINTER,
                &theme,
                |_, _, _| {},
            ));
            let control = interactivity(&node);
            assert_eq!(control.role, Some(Role::Switch));
            assert_eq!(control.aria.toggled, Some(toggled));
            assert_eq!(control.aria.selected, None);
            assert_eq!(faults(&node), []);
        }
    }

    #[test]
    fn the_picked_segment_reports_toggled_true_and_is_the_active_one() {
        let theme = Theme::light();
        let node = lower(segment(
            "sha1",
            "SHA-1",
            true,
            &POINTER,
            &theme,
            |_, _, _| {},
        ));
        let control = interactivity(&node);
        assert_eq!(control.role, Some(Role::RadioButton));
        assert_eq!(control.aria.toggled, Some(Toggled::True));
        assert_eq!(control.aria.selected, None);
        assert!(!control.focusable);
        let group = lower(segmented(
            "format",
            "Object format",
            &POINTER,
            &theme,
            Some(1),
            [
                ("sha256".into(), "SHA-256".into()),
                ("sha1".into(), "SHA-1".into()),
            ],
            |_, _, _| {},
        ));
        let claims: Vec<bool> = group
            .children()
            .iter()
            .map(|segment| interactivity(segment).aria.active_descendant)
            .collect();
        assert_eq!(claims, [false, true]);
    }

    /// Under a finger a segmented choice is the kit's `Segmented`: 36 tall
    /// with its hairlines, the choices sharing the width, each label
    /// secondary text that only draws; the press is a `hit` over each,
    /// reaching to 44 and none sideways, and it is the radio group's item.
    #[test]
    fn under_touch_segments_share_a_36_box_and_press_through_a_44_hit() {
        let theme = Theme::light();
        let (node, styles) = lower_styled(segmented(
            "mode",
            "Mode",
            &TOUCH,
            &theme,
            Some(1),
            [("a".into(), "A".into()), ("b".into(), "B".into())],
            |_, _, _| {},
        ));
        let style = |node: &wire::Node| styles[node.style().expect("a styled node")].clone();
        for (segment, chosen) in node.children().iter().zip([false, true]) {
            let look = style(segment);
            assert_eq!(look.size.height, Some(px(34.).into()));
            assert_eq!(look.flex_grow, Some(1.));
            assert_eq!(look.text.font_size, Some(px(13.).into()));
            assert_eq!(look.text.line_height, Some(px(18.).into()));
            assert_eq!(segment.interactivity().and_then(|i| i.on_click), None);
            let press = segment.children().last().expect("the press");
            let control = interactivity(press);
            assert_eq!(control.role, Some(Role::RadioButton));
            assert_eq!(control.aria.toggled, Some(chosen.into()));
            assert_eq!(control.aria.active_descendant, chosen);
            assert!(control.on_click.is_some() && !control.focusable);
            let inset = style(press).inset;
            assert_eq!(
                (inset.top, inset.bottom),
                (Some(px(-5.).into()), Some(px(-5.).into()))
            );
            assert_eq!(
                (inset.left, inset.right),
                (Some(px(0.).into()), Some(px(0.).into()))
            );
        }
        assert_eq!(faults(&node), []);
        crate::testing::assert_fits(&node, &styles, &[], 360., &TOUCH);
    }

    /// Under a finger a tab is an item of the kit's `Strip`: the bar's
    /// height, 10 in, small text; the chosen one medium in the foreground
    /// over a 2 underline 10 in from each side; no hover, no dim. It fits a
    /// phone on the bar's 44.
    #[test]
    fn under_touch_a_tab_spans_its_strip_and_underlines_the_chosen_one_above_the_hairline() {
        let theme = Theme::light();
        let strip = |chosen: bool| {
            let tab = item(
                tab("files", "Files", chosen, &TOUCH, &theme, |_, _, _| {}),
                Role::Tab,
                chosen,
            );
            let bar = div()
                .id("bar")
                .h(TOUCH.strip)
                .flex()
                .border_b_1()
                .child(tab);
            lower_styled(bar)
        };
        let (bar, styles) = strip(true);
        crate::testing::assert_fits(&bar, &styles, &[], 360., &TOUCH);
        let tab = &bar.children()[0];
        let look = &styles[tab.style().expect("a styled tab")];
        assert_eq!(look.size.height, Some(px(44.).into()));
        assert_eq!(look.padding.bottom, Some(px(1.).into()));
        assert_eq!(look.padding.left, Some(px(10.).into()));
        assert_eq!(look.text.font_size, Some(px(14.).into()));
        assert_eq!(look.text.font_weight, Some(FontWeight::MEDIUM));
        assert_eq!(look.text.color, Some(theme.foreground));
        assert_eq!(look.border_widths.bottom, None);
        let control = interactivity(tab);
        assert_eq!((control.hover, control.active), (None, None));
        let underline = &styles[tab.children()[1].style().expect("the underline")];
        assert_eq!(underline.position, Some(gpui::Position::Absolute));
        assert_eq!(
            (
                underline.inset.left,
                underline.inset.right,
                underline.inset.bottom
            ),
            (
                Some(px(10.).into()),
                Some(px(10.).into()),
                Some(px(1.).into())
            )
        );
        assert_eq!(underline.size.height, Some(px(2.).into()));
        assert_eq!(underline.background, Some(theme.foreground.into()));
        let (bar, styles) = strip(false);
        let tab = &bar.children()[0];
        assert_eq!(
            styles[tab.style().expect("a styled tab")].text.color,
            Some(theme.muted)
        );
        assert_eq!(tab.children().len(), 1, "no underline");
    }

    /// A row dims while a finger holds it only once the caller makes it a
    /// press (Expo `Row.tsx:39`); a pointer's press never dims.
    #[test]
    fn a_row_dims_only_once_it_presses() {
        let theme = Theme::light();
        let dim = |element: Stateful<Div>| {
            let (node, styles) = lower_styled(element);
            interactivity(&node)
                .active
                .and_then(|active| styles[active].opacity)
        };
        let ada = || row("ada", &TOUCH, &theme).role(Role::Button).focusable();
        assert_eq!(dim(ada().on_click(|_, _, _| {})), None);
        assert_eq!(
            dim(pressed(ada(), &TOUCH).on_click(|_, _, _| {})),
            Some(0.6)
        );
        assert_eq!(dim(pressed(ada(), &POINTER).on_click(|_, _, _| {})), None);
    }

    /// An icon button says whether it is on once told, and as an item of
    /// a composite it leaves the Tab order and keeps its press.
    #[test]
    fn an_icon_button_told_it_is_on_says_so_and_as_an_item_leaves_the_tab_order() {
        let theme = Theme::light();
        let cell = || icon_button("react", "👍", "React with 👍", &TOUCH, &theme, |_, _, _| {});
        let node = lower(cell());
        assert_eq!(interactivity(&node).aria.toggled, None);
        assert!(interactivity(&node).focusable);
        let node = lower(cell().toggled(true).item(true));
        let control = interactivity(&node);
        assert_eq!(control.aria.toggled, Some(Toggled::True));
        assert!(!control.focusable && control.aria.active_descendant);
        assert!(control.on_click.is_some());
    }

    /// What a caller adds to a section head sits after its label, at the
    /// right, in the head's caption muted: under a finger the label takes
    /// the slack; under a pointer the two sit at the head's ends.
    #[test]
    fn a_section_heads_trailing_child_sits_after_its_label() {
        let theme = Theme::light();
        let (head, styles) = lower_styled(section_head("keys", "Keys", &TOUCH, &theme).child("3"));
        let look = &styles[head.style().expect("a styled head")];
        assert_eq!(look.text.font_size, Some(px(12.).into()));
        assert_eq!(look.text.color, Some(theme.muted));
        assert_eq!(look.justify_content, None);
        let [label, count] = head.children() else {
            panic!("the label and the count: {head:?}")
        };
        assert_eq!(
            styles[label.style().expect("a styled label")].flex_grow,
            Some(1.)
        );
        assert!(
            matches!(count, wire::Node::Text(wire::TextNode { content, .. }) if content.as_str() == "3"),
            "{count:?}"
        );
        // under a pointer the two sit at the head's ends, as on the desktop
        let (head, styles) =
            lower_styled(section_head("keys", "Keys", &POINTER, &theme).child("3"));
        assert_eq!(
            styles[head.style().expect("a styled head")].justify_content,
            Some(gpui::JustifyContent::SpaceBetween)
        );
    }

    #[test]
    fn a_button_is_a_toggle_only_once_told_it_is_selected() {
        let theme = Theme::light();
        let plain = lower(button("save", "Save", &POINTER, &theme, |_, _, _| {}));
        assert_eq!(interactivity(&plain).aria.toggled, None);
        for (selected, toggled) in [(true, Toggled::True), (false, Toggled::False)] {
            let node =
                lower(button("tree", "Tree", &POINTER, &theme, |_, _, _| {}).selected(selected));
            let control = interactivity(&node);
            assert_eq!(control.role, Some(Role::Button));
            assert_eq!(control.aria.toggled, Some(toggled));
            assert_eq!(control.aria.selected, None);
        }
    }

    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct Panes {
        moved: Vec<f32>,
    }

    impl crate::View for Panes {
        const NAME: &'static str = "Panes";
        fn new(_: &mut Window, _: &mut crate::Context<Self>) -> Self {
            Self::default()
        }
    }

    impl crate::Render for Panes {
        fn render(&mut self, _: &mut Window, cx: &mut crate::Context<Self>) -> impl IntoElement {
            let theme = Theme::light();
            divider(
                "panes-resize",
                "Resize the list",
                &theme,
                cx,
                |panes: &mut Self, dx| panes.moved.push(dx),
            )
        }
    }

    #[test]
    fn a_divider_is_a_named_focusable_splitter_the_arrows_move() {
        let mut cx = crate::testing::TestAppContext::new();
        let panes = cx.open::<Panes>();
        let node = cx.find("panes-resize").expect("the divider").clone();
        let handle = interactivity(&node);
        assert_eq!(handle.role, Some(Role::Splitter));
        assert_eq!(handle.aria.label.as_deref(), Some("Resize the list"));
        assert_eq!(handle.aria.orientation, Some(gpui::Orientation::Vertical));
        assert!(handle.focusable && handle.tab_stop == Some(true));
        assert_eq!(faults(&node), []);
        cx.simulate_focus("panes-resize");
        for keystroke in ["left", "right", "shift-left", "shift-right", "up", "a"] {
            cx.simulate_key_down("panes-resize", keystroke);
        }
        cx.simulate_drag("panes-resize", 5., 0.);
        panes.read(|panes| assert_eq!(panes.moved, [-8., 8., -32., 32., 5.]));
    }

    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct Format {
        picked: usize,
    }

    impl crate::View for Format {
        const NAME: &'static str = "Format";
        fn new(_: &mut Window, _: &mut crate::Context<Self>) -> Self {
            Self::default()
        }
    }

    impl crate::Render for Format {
        fn render(&mut self, _: &mut Window, cx: &mut crate::Context<Self>) -> impl IntoElement {
            segmented(
                "format",
                "Object format",
                &POINTER,
                &Theme::light(),
                Some(self.picked),
                [
                    ("sha256".into(), "SHA-256".into()),
                    ("sha1".into(), "SHA-1".into()),
                ],
                cx.processor(|view: &mut Self, index, _, cx| {
                    view.picked = index;
                    cx.notify();
                }),
            )
        }
    }

    #[test]
    fn a_segmented_choice_checks_the_next_choice_on_an_arrow() {
        let mut cx = crate::testing::TestAppContext::new();
        let format = cx.open::<Format>();
        cx.simulate_focus("format");
        cx.simulate_key_down("format", "right");
        format.read(|view| assert_eq!(view.picked, 1));
        assert_eq!(
            interactivity(cx.find("sha1").expect("the picked segment"))
                .aria
                .toggled,
            Some(Toggled::True)
        );
        // a radio group wraps
        cx.simulate_key_down("format", "right");
        format.read(|view| assert_eq!(view.picked, 0));
        cx.simulate_click("sha1");
        format.read(|view| assert_eq!(view.picked, 1));
    }

    thread_local! {
        /// The moves and presses of a `silent` [`Picker`], which keeps no
        /// state of them and so never re-renders.
        static SILENT: std::cell::RefCell<(Vec<usize>, Vec<usize>)> = Default::default();
    }

    /// A composite of `count` items under one test's settings.
    #[derive(Default, serde::Serialize, serde::Deserialize)]
    struct Picker {
        /// moves and presses go to [`SILENT`], and the view does not re-render
        silent: bool,
        role: Option<Role>,
        horizontal: bool,
        columns: Option<usize>,
        /// the active row's cells, and the active one
        cells: Option<(usize, usize)>,
        wrap: bool,
        active: usize,
        count: usize,
        moved: Vec<usize>,
        moved_cell: Vec<usize>,
        pressed: Vec<usize>,
        /// a focusable link inside the composite, beside its items
        link: bool,
    }

    impl crate::View for Picker {
        const NAME: &'static str = "Picker";
        const CAPABILITIES: &'static [crate::methods::Capability] =
            &[crate::methods::Capability::Link];
        fn new(_: &mut Window, _: &mut crate::Context<Self>) -> Self {
            Self {
                role: Some(Role::ListBox),
                count: 3,
                ..Self::default()
            }
        }
    }

    impl crate::Render for Picker {
        fn render(&mut self, _: &mut Window, cx: &mut crate::Context<Self>) -> impl IntoElement {
            let role = self.role.unwrap_or(Role::ListBox);
            let mut list = composite("picker", role, "Pick one")
                .active(self.active, self.count)
                .on_move(cx.processor(|view: &mut Self, index, _, cx| {
                    if view.silent {
                        return SILENT.with_borrow_mut(|log| log.0.push(index));
                    }
                    view.moved.push(index);
                    view.active = index;
                    cx.notify();
                }))
                .on_press(cx.processor(|view: &mut Self, index, _, cx| {
                    if view.silent {
                        return SILENT.with_borrow_mut(|log| log.1.push(index));
                    }
                    view.pressed.push(index);
                    cx.notify();
                }));
            if self.horizontal {
                list = list.orientation(Orientation::Horizontal);
            }
            if let Some(columns) = self.columns {
                list = list.grid(columns);
            }
            if let Some((cell, cells)) = self.cells {
                list = list.cells(cell, cells).on_move_cell(cx.processor(
                    |view: &mut Self, index, _, cx| {
                        if view.silent {
                            return SILENT.with_borrow_mut(|log| log.0.push(index));
                        }
                        view.moved_cell.push(index);
                        view.cells = view.cells.map(|(_, cells)| (index, cells));
                        cx.notify();
                    },
                ));
            }
            if self.wrap {
                list = list.wrap();
            }
            let item_role = match role {
                Role::TabList => Role::Tab,
                Role::Grid => Role::GridCell,
                _ => Role::ListBoxOption,
            };
            let active = self.active;
            let link = self.link;
            list.build()
                .children((0..self.count).map(move |index| {
                    let row = div()
                        .id(format!("pick-{index}"))
                        .on_click(|_, _, _| {})
                        .child(format!("Choice {index}"));
                    let row = match item_role {
                        Role::GridCell => row,
                        _ => row.aria_selected(index == active),
                    };
                    let cell = item(row, item_role, index == active);
                    match item_role {
                        Role::GridCell => {
                            div().id(format!("row-{index}")).role(Role::Row).child(cell)
                        }
                        _ => cell,
                    }
                }))
                .when(link, |list| {
                    list.child(super::link(
                        "inside",
                        "block 12",
                        "duck://a/b",
                        &POINTER,
                        &Theme::light(),
                    ))
                })
        }
    }

    fn picker(
        set: impl FnOnce(&mut Picker),
    ) -> (crate::testing::TestAppContext, crate::Entity<Picker>) {
        let mut cx = crate::testing::TestAppContext::new();
        let picker = cx.open::<Picker>();
        cx.update(&picker, |view, _, cx| {
            set(view);
            cx.notify();
        });
        cx.run_until_parked();
        // a person Tabs to the composite: its one Tab stop
        cx.simulate_focus("picker");
        (cx, picker)
    }

    fn found(cx: &crate::testing::TestAppContext, key: &str) -> wire::Interactivity {
        interactivity(cx.find(key).expect(key)).clone()
    }

    #[test]
    fn a_composite_is_one_tab_stop_whose_arrows_pick_its_items() {
        let (mut cx, picker) = picker(|_| {});
        let list = found(&cx, "picker");
        assert_eq!(list.role, Some(Role::ListBox));
        assert_eq!(list.aria.label.as_deref(), Some("Pick one"));
        assert_eq!(list.aria.orientation, Some(Orientation::Vertical));
        assert!(list.focusable && list.tab_stop == Some(true));
        assert!(list.on_key_down.is_some());
        for index in 0..3 {
            let row = found(&cx, &format!("pick-{index}"));
            assert!(!row.focusable && row.tab_stop.is_none());
            assert_eq!(row.aria.active_descendant, index == 0);
        }
        cx.simulate_key_down("picker", "down");
        assert!(found(&cx, "pick-1").aria.active_descendant);
        cx.simulate_key_down("picker", "end");
        cx.simulate_key_down("picker", "left");
        cx.simulate_key_down("picker", "enter");
        cx.simulate_key_down("picker", "home");
        cx.simulate_key_down("picker", "up");
        cx.simulate_key_down("picker", "space");
        cx.simulate_key_down("picker", "a");
        picker.read(|view| {
            assert_eq!(view.moved, [1, 2, 0]);
            assert_eq!(view.pressed, [2, 0]);
        });
    }

    /// A link inside a composite consumes the keys that press it:
    /// Space or Enter on it opens the link (gpui's click on the key's way
    /// up) and never reaches the composite around it, which would press its
    /// own active item too.
    #[test]
    fn a_link_inside_a_composite_keeps_its_press_from_it() {
        let (mut cx, picker) = picker(|view| view.link = true);
        cx.simulate_focus("inside");
        cx.simulate_key_down("inside", "space");
        cx.simulate_key_down("inside", "enter");
        picker.read(|view| assert_eq!(view.pressed, Vec::<usize>::new()));
        // a press opens the href it was given
        cx.simulate_click("inside");
        assert_eq!(
            cx.host().requests::<crate::methods::LinkOpen>(),
            ["duck://a/b"]
        );
        // the composite's own Enter still presses
        cx.simulate_focus("picker");
        cx.simulate_key_down("picker", "enter");
        picker.read(|view| assert_eq!(view.pressed, [0]));
    }

    /// The helper remembers where the arrows went before the view draws
    /// it: ↓ then Enter in one frame presses the item moved to.
    #[test]
    fn a_press_right_after_a_move_lands_on_the_item_moved_to() {
        SILENT.take();
        let (mut cx, _) = picker(|view| view.silent = true);
        cx.simulate_key_down("picker", "down");
        cx.simulate_key_down("picker", "enter");
        assert_eq!(SILENT.take(), (vec![1], vec![1]));
        // a grid of rows with cells: the cell moves, then the row (its
        // cell back at 0), then the cell again
        let (mut cx, _) = picker(|view| {
            view.silent = true;
            view.role = Some(Role::Grid);
            view.cells = Some((0, 3));
            view.count = 2;
        });
        for keystroke in ["right", "right", "down", "right", "enter"] {
            cx.simulate_key_down("picker", keystroke);
        }
        assert_eq!(SILENT.take(), (vec![1, 2, 1, 1], vec![1]));
    }

    /// A control built focusable (a button, a link) made an item leaves
    /// the Tab order, keeps its role and its press, and claims when active.
    #[test]
    fn a_focusable_control_as_an_item_leaves_the_tab_order_and_claims() {
        let theme = Theme::light();
        let cells = [
            (
                lower(button("compare", "Compare", &POINTER, &theme, |_, _, _| {}).item(true)),
                Role::Button,
            ),
            (
                lower(item(
                    link("activity", "block 12", "duck://a/b", &POINTER, &theme),
                    Role::Link,
                    true,
                )),
                Role::Link,
            ),
        ];
        for (cell, role) in cells {
            let control = interactivity(&cell);
            assert!(!control.focusable && control.tab_stop.is_none(), "{role:?}");
            assert!(control.aria.active_descendant && control.on_click.is_some());
            assert_eq!(control.role, Some(role));
        }
    }

    #[test]
    fn a_tab_list_wraps_and_a_list_box_stops() {
        let (mut cx, tabs) = picker(|view| {
            view.role = Some(Role::TabList);
            view.horizontal = true;
            view.wrap = true;
            view.active = 2;
        });
        assert_eq!(
            found(&cx, "picker").aria.orientation,
            Some(Orientation::Horizontal)
        );
        cx.simulate_key_down("picker", "right");
        cx.simulate_key_down("picker", "left");
        cx.simulate_key_down("picker", "down");
        tabs.read(|view| assert_eq!(view.moved, [0, 2]));
        let (mut cx, list) = picker(|view| view.active = 2);
        cx.simulate_key_down("picker", "down");
        cx.simulate_key_down("picker", "home");
        cx.simulate_key_down("picker", "up");
        list.read(|view| assert_eq!(view.moved, [0]));
    }

    #[test]
    fn a_modified_arrow_is_not_the_composites() {
        let (mut cx, list) = picker(|_| {});
        for keystroke in [
            "alt-down",
            "cmd-down",
            "shift-down",
            "ctrl-down",
            "ctrl-end",
        ] {
            cx.simulate_key_down("picker", keystroke);
        }
        cx.simulate_key_down("picker", "shift-enter");
        list.read(|view| assert!(view.moved.is_empty() && view.pressed.is_empty()));
    }

    #[test]
    fn a_grid_steps_a_row_by_its_columns_and_home_end_by_its_row() {
        let (mut cx, grid) = picker(|view| {
            view.role = Some(Role::Grid);
            view.columns = Some(3);
            view.count = 8;
            view.active = 4;
        });
        for keystroke in [
            "down",
            "up",
            "up",
            "down",
            "home",
            "end",
            "right",
            "left",
            "ctrl-home",
            "ctrl-end",
            "down",
            "end",
        ] {
            cx.simulate_key_down("picker", keystroke);
        }
        grid.read(|view| assert_eq!(view.moved, [7, 4, 1, 4, 3, 5, 6, 5, 0, 7]));
        assert!(found(&cx, "pick-7").aria.active_descendant);
    }

    #[test]
    fn a_grid_of_rows_with_their_own_cells_steps_cells_sideways_and_rows_up_and_down() {
        let (mut cx, grid) = picker(|view| {
            view.role = Some(Role::Grid);
            view.cells = Some((0, 3));
            view.count = 4;
            view.active = 1;
        });
        for keystroke in [
            "right",
            "right",
            "right",
            "down",
            "home",
            "left",
            "end",
            "up",
            "up",
            "up",
            "ctrl-end",
            "ctrl-home",
            "enter",
        ] {
            cx.simulate_key_down("picker", keystroke);
        }
        grid.read(|view| {
            assert_eq!(view.moved_cell, [1, 2, 0, 2]);
            assert_eq!(view.moved, [2, 1, 0, 3, 0]);
            assert_eq!(view.pressed, [0]);
        });
    }
}
