//! What more than one forge phone screen draws, in one home: the Expo
//! app's `src/forge/parts.tsx` (the change row, the commit row, the ref
//! sheet, the outbox's rows of a scope), the pieces of
//! `src/forge/repo.tsx` its screens share (`FileBar`, `sizeOf`, `Stage`),
//! and the kit pieces of `src/kit/*.tsx` the SDK does not ship yet (the
//! small button's glyph, chips, the notice, key/value). Each kit piece is
//! marked `SDK gap`: every view needs it, so it lands once in the SDK and
//! this copy goes. Each part reads the model and draws; none writes but
//! through the press it is handed.
use std::ops::Range;
use std::rc::Rc;

use view::design::{Ladder, TextType};
use view::prelude::*;
use view::{Div, FontWeight, HighlightStyle, Stateful, StyledText};

use crate::id;
use crate::list;
use crate::select::{revision_name, summary_of};
use crate::state::{Menu, Progress};
use crate::words::{self, ref_label};
use crate::{Forge, Stage};
use forge::{ChangeState, ChangeSummary, CommitInfo, Judgment, Query, Reply, Revision};

/// A press, as a row or a control takes it.
pub(crate) type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
/// A press that carries what was pressed: a row's index, a ref's name.
pub(crate) type Route<T> = Rc<dyn Fn(T, &mut Window, &mut App)>;

/// The kit's small button (`theme.ts` `size.small`).
pub(crate) const SMALL: Pixels = px(36.);
/// A small button's icon (`Button.tsx`: 16 small, 18 otherwise).
const SMALL_ICON: Pixels = px(16.);
/// A filter chip's height (`theme.ts` `size.chip`).
pub(crate) const CHIP: Pixels = px(32.);
/// The label column of a key/value row (`Facts.tsx` `KeyValue`).
const KEY_W: Pixels = px(96.);
/// A change row's state dot (`parts.tsx` `StateDot`).
const DOT: Pixels = px(8.);
/// A checkbox's or a radio's box, its edge, and what it draws when on
/// (`Toggle.tsx` `Checkbox`, `Radio`).
const TICK_BOX: Pixels = px(20.);
const TICK_EDGE: Pixels = px(1.5);
const TICK_ICON: Pixels = px(14.);
const RADIO_DOT: Pixels = px(9.);

// ------------------------------------------------------------------ text

/// One line of caption text, muted (the kit's `Text role="caption"`):
/// plain words, or runs of their own (a `+a −d` delta).
// SDK gap: a caption of runs (`design::row_caption` takes plain words); the
// SDK builds it later
pub(crate) fn caption(text: impl IntoElement, ladder: &Ladder, theme: &Theme) -> Div {
    div()
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .truncate()
        .child(text)
}

/// `text` then `tail` in muted, one run that truncates as one
/// (`<Text>{title} <Text tone="muted">#n</Text></Text>`).
pub(crate) fn muted_tail(text: &str, tail: &str, theme: &Theme) -> StyledText {
    let whole = format!("{text}{tail}");
    tinted(
        whole,
        vec![(text.len()..text.len() + tail.len(), theme.muted)],
    )
}

/// `whole` with each range in its colour.
fn tinted(whole: String, runs: Vec<(Range<usize>, Hsla)>) -> StyledText {
    let highlights = runs
        .into_iter()
        .map(|(range, color)| {
            let style = HighlightStyle {
                color: Some(color),
                ..Default::default()
            };
            (range, style)
        })
        .collect::<Vec<_>>();
    StyledText::new(whole).with_highlights(highlights)
}

/// `{before}+a −d` with `+a` in success and `−d` in danger
/// (`DiffList.tsx` `Delta`): a file that only adds or only removes shows
/// its one side.
pub(crate) fn delta(before: &str, additions: u64, deletions: u64, theme: &Theme) -> StyledText {
    let mut whole = before.to_owned();
    let mut runs = Vec::new();
    if additions > 0 || deletions == 0 {
        let start = whole.len();
        whole.push_str(&format!("+{additions}"));
        runs.push((start..whole.len(), theme.success));
    }
    if additions > 0 && deletions > 0 {
        whole.push(' ');
    }
    if deletions > 0 {
        let start = whole.len();
        whole.push_str(&format!("−{deletions}"));
        runs.push((start..whole.len(), theme.danger));
    }
    tinted(whole, runs)
}

/// `212 B`, `6.8 KB`, `12 KB`, `1.4 MB` (`repo.tsx` `sizeOf`).
pub(crate) fn size_of(bytes: u64) -> String {
    const KB: f64 = 1024.;
    let value = bytes as f64;
    if value < KB {
        return format!("{bytes} B");
    }
    let (amount, unit) = match value < KB * KB {
        true => (value / KB, "KB"),
        false => (value / (KB * KB), "MB"),
    };
    match amount < 10. {
        true => format!("{amount:.1} {unit}"),
        false => format!("{} {unit}", amount.round()),
    }
}

/// `<source> → <target>` of a change, short names (`parts.tsx`
/// `endpoints`).
pub(crate) fn endpoints(from: &Revision, into: &[u8]) -> String {
    words::endpoints(&revision_name(from), into)
}

// ------------------------------------------------------------------ rows

/// A Lucide icon leading a row, muted.
pub(crate) fn lead_icon(name: &str, ladder: &Ladder, theme: &Theme) -> AnyElement {
    design::icon(name, ladder.icon_row)
        .text_color(theme.muted)
        .into_any_element()
}

/// A list row (`Row.tsx`, the SDK's `design::row` and its words): at
/// least a row tall (56 with a caption), the lead in its 24 column, the
/// title over its caption, what trails it, a chevron when it pushes a
/// screen. The caller makes it a press ([`Row::press`]).
pub(crate) struct Row {
    id: ElementId,
    title: Title,
    lead: Option<AnyElement>,
    caption: Option<AnyElement>,
    tail: Vec<AnyElement>,
    chevron: bool,
    chosen: bool,
}

/// What a [`Row`]'s title is.
enum Title {
    /// `design::row_title`
    Words(SharedString),
    /// `design::row_title` in the data face at the secondary size
    Data(SharedString),
    /// drawn by the caller: runs of their own
    Drawn(AnyElement),
}

impl Row {
    /// A row whose title is `title` as it is drawn.
    pub(crate) fn new(id: impl Into<ElementId>, title: impl IntoElement) -> Row {
        Row::of(id.into(), Title::Drawn(title.into_any_element()))
    }

    /// A row titled in body text on one line.
    pub(crate) fn titled(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Row {
        Row::of(id.into(), Title::Words(title.into()))
    }

    /// A row titled in the data face, at the secondary size (`mono`).
    pub(crate) fn data(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Row {
        Row::of(id.into(), Title::Data(title.into()))
    }

    fn of(id: ElementId, title: Title) -> Row {
        Row {
            id,
            title,
            lead: None,
            caption: None,
            tail: Vec::new(),
            chevron: false,
            chosen: false,
        }
    }

    pub(crate) fn lead(mut self, lead: impl IntoElement) -> Row {
        self.lead = Some(lead.into_any_element());
        self
    }

    pub(crate) fn caption(mut self, caption: impl IntoElement) -> Row {
        self.caption = Some(caption.into_any_element());
        self
    }

    pub(crate) fn tail(mut self, tail: impl IntoElement) -> Row {
        self.tail.push(tail.into_any_element());
        self
    }

    /// The row pushes a screen.
    pub(crate) fn chevron(mut self) -> Row {
        self.chevron = true;
        self
    }

    /// The row is the picked one: on `accent_soft`.
    pub(crate) fn chosen(mut self, chosen: bool) -> Row {
        self.chosen = chosen;
        self
    }

    /// [`Row::draw`] as a press: it dims while a finger holds it
    /// (`design::pressed`). The caller adds its role and its click.
    pub(crate) fn press(self, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
        design::pressed(self.draw(ladder, theme), ladder)
    }

    pub(crate) fn draw(self, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
        let captioned = self.caption.is_some();
        let title = match self.title {
            Title::Words(words) => design::row_title(words, ladder, theme).into_any_element(),
            Title::Data(words) => design::row_title(words, ladder, theme)
                .font_family(design::fonts::FAMILY_MONO)
                .text_type(ladder.secondary)
                .into_any_element(),
            Title::Drawn(title) => title,
        };
        let lines = design::row_words(title).children(self.caption);
        design::row(self.id, ladder, theme)
            .when(captioned, |row| row.min_h(ladder.row_tall))
            .when(self.chosen, |row| row.bg(theme.accent_soft))
            .children(self.lead.map(design::lead))
            .child(lines)
            .when(!self.tail.is_empty(), |row| {
                row.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(design::space::SM)
                        .children(self.tail),
                )
            })
            .when(self.chevron, |row| {
                row.child(design::chevron(ladder, theme))
            })
    }
}

/// A list's rows as one Tab stop (`components::list`), each row an option
/// the arrows walk; `open` presses row `index`.
pub(crate) fn rows_list(
    list_id: &'static str,
    label: &str,
    rows: Vec<(Row, Press)>,
    open: impl Fn(&mut Forge, usize, &mut Window, &mut Context<Forge>) + 'static,
    forge: &Forge,
    cx: &mut Context<Forge>,
    (ladder, theme): (&Ladder, &Theme),
) -> Stateful<Div> {
    let (at, _) = list::cursor(forge, list_id);
    let walked = list::walked(forge, list_id);
    let at = at.min(rows.len().saturating_sub(1));
    let mut list = list::list(list_id, label, rows.len(), None, forge, cx, open).flex_none();
    for (index, (row, press)) in rows.into_iter().enumerate() {
        let row = row.press(ladder, theme).on_click(press);
        list = list.child(option(row, walked && index == at));
    }
    list
}

/// A row as an option of a list (`design::item`), selected while the
/// arrows are on it.
pub(crate) fn option(row: Stateful<Div>, active: bool) -> Stateful<Div> {
    design::item(row.aria_selected(active), Role::ListBoxOption, active)
}

/// A virtual list of `count` rows as one Tab stop ([`list::list`]),
/// named `label`: the arrows walk them, the list scrolled to the active
/// one; `paint` draws row `index` (and whether it is the active one) as
/// it comes into view, `open` presses it.
pub(crate) fn virtual_list(
    (list_id, label): (&'static str, &str),
    count: usize,
    scroll: &UniformListScrollHandle,
    open: impl Fn(&mut Forge, usize, &mut Window, &mut Context<Forge>) + 'static,
    paint: impl Fn(usize, bool) -> AnyElement + 'static,
    forge: &Forge,
    cx: &mut Context<Forge>,
) -> Stateful<Div> {
    let (at, _) = list::cursor(forge, list_id);
    let walked = list::walked(forge, list_id);
    let at = at.min(count.saturating_sub(1));
    let rows = list::window(
        &format!("{list_id}-rows"),
        count,
        None,
        Some(scroll),
        move |range, _, _| {
            range
                .map(|index| paint(index, walked && index == at))
                .collect()
        },
    );
    list::list(list_id, label, count, Some(scroll), forge, cx, open).child(rows)
}

/// A key/value row (`Facts.tsx` `KeyValue`): at least a control tall, the
/// label `small` muted in its column, the value `small` beside it.
// SDK gap: the kit's `KeyValue`; the SDK builds it later
pub(crate) fn key_value(
    label: &str,
    value: impl IntoElement,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    div()
        .w_full()
        .min_h(ladder.control)
        .flex()
        .items_start()
        .gap(design::space::LG)
        .py(design::space::MD)
        .px(design::space::BLOCK)
        .border_b_1()
        .border_color(theme.border)
        .text_type(ladder.small)
        .child(
            div()
                .w(KEY_W)
                .flex_none()
                .text_color(theme.muted)
                .child(label.to_owned()),
        )
        .child(div().flex_1().min_w(px(0.)).child(value))
}

/// A value in the data face: a hash, a ref, a number. The caller sizes it
/// (`design::mono` is the pointer's size).
pub(crate) fn data(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .child(text.into())
}

// ------------------------------------------------------------------ states

/// One muted status line, 16 all round (`States.tsx` `Quiet`).
// SDK gap: the kit's `Quiet` at the ladder's sizes (`design::quiet` is the
// pointer's); the SDK builds it later
pub(crate) fn quiet(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .p(design::space::BLOCK)
        .text_type(ladder.secondary)
        .text_color(theme.muted)
        .child(text.into())
}

/// A notice's tone (`Notice.tsx` `NoticeKind`): its ground and its words'
/// colour.
#[derive(Clone, Copy)]
// SDK gap: the kit's `Notice` (the SDK draws only its refusal,
// `design::refused`); the SDK builds it later
pub(crate) enum Tone {
    /// a refusal
    Danger,
    /// a gate
    Warning,
    /// a status
    Quiet,
}

impl Tone {
    fn colours(self, theme: &Theme) -> (Hsla, Hsla) {
        match self {
            Tone::Danger => (theme.danger_soft, theme.danger),
            Tone::Warning => (theme.warning_soft, theme.warning),
            Tone::Quiet => (theme.surface, theme.muted),
        }
    }
}

/// A strip above or under content (`Notice.tsx`): edge to edge, 10 over
/// and under, 16 in, secondary text in its tone on its ground; a quiet one
/// keeps a hairline under it.
// SDK gap: the kit's `Notice`; the SDK builds it later
pub(crate) fn notice(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    tone: Tone,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let (ground, words) = tone.colours(theme);
    div()
        .id(id)
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .gap(design::space::MD)
        .py(design::space::MD)
        .px(design::space::BLOCK)
        .bg(ground)
        .when(matches!(tone, Tone::Quiet), |strip| {
            strip.border_b_1().border_color(theme.border)
        })
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_type(ladder.secondary)
                .text_color(words)
                .child(text.into()),
        )
}

/// A read as a screen shows it (`States.tsx` `ReadView`): the reply once
/// it landed, else the quiet line `loading` says, or the refusal and
/// Retry in its place (`{id}-loading`, `{id}-refused`, `{id}-retry`).
pub(crate) fn stage<'a>(
    forge: &'a Forge,
    query: &Query,
    element_id: &str,
    loading: &str,
    cx: &mut Context<Forge>,
    (ladder, theme): (&Ladder, &Theme),
) -> Result<&'a Reply, AnyElement> {
    match forge.stage(query) {
        Stage::Ready(reply) => Ok(reply),
        Stage::Loading => Err(quiet(
            id(format!("{element_id}-loading")),
            loading.to_owned(),
            ladder,
            theme,
        )
        .into_any_element()),
        Stage::Failed(refusal) => {
            let query = query.clone();
            let retry =
                cx.listener(move |forge, _: &ClickEvent, _, cx| forge.retry(query.clone(), cx));
            Err(
                design::refused(element_id, design::sentence(refusal), ladder, theme, retry)
                    .into_any_element(),
            )
        }
    }
}

/// How tall a notice's action is: its `secondary` line and 6 over and
/// under (`Notice.tsx`), and how far its press reaches past it on every
/// side (its `hitSlop`).
const NOTICE_ACTION: Pixels = px(30.);
const NOTICE_SLOP: Pixels = px(8.);

/// A notice's one action at its right (`Notice.tsx` `action`): medium, in
/// the notice's tone, 8 in from its left; its press reaches 8 past it on
/// every side, padded by the reach and handed back by a negative margin,
/// so the word sits where Expo's does.
// SDK gap: the kit's `Notice` action (`design::refused` has its Retry
// alone); the SDK builds it later
fn notice_action(
    id: impl Into<ElementId>,
    label: &'static str,
    tone: Tone,
    (ladder, theme): (&Ladder, &Theme),
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .h(NOTICE_ACTION + NOTICE_SLOP * 2.)
        .flex()
        .items_center()
        .pl(design::space::SM + NOTICE_SLOP)
        .pr(NOTICE_SLOP)
        .m(-NOTICE_SLOP)
        .text_type(ladder.secondary)
        .text_color(tone.colours(theme).1)
        .font_weight(FontWeight::MEDIUM)
        .role(Role::Button)
        .aria_label(label)
        .focusable()
        .on_click(press)
        .child(label)
}

/// The writes of `scope` (`parts.tsx` `PendingRows`): each a quiet strip
/// saying where it is, or, once it did not land, a danger strip saying
/// why in every view's words (`design::failure`), with Dismiss.
pub(crate) fn pending(
    forge: &Forge,
    scope: &str,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    forge
        .pending_in(scope)
        .into_iter()
        .map(|op| {
            let strip = |said: &str, tone| {
                notice(
                    id(format!("forge-pending-{}", op.id)),
                    format!("{} · {said}", op.label),
                    tone,
                    ladder,
                    theme,
                )
            };
            match &op.progress {
                Progress::Submitting => strip(words::SUBMITTING, Tone::Quiet),
                Progress::Accepted => strip(words::WAITING_FOR_BLOCK, Tone::Quiet),
                Progress::Refused(answer) => {
                    let at = op.id;
                    let dismiss =
                        cx.listener(move |forge, _: &ClickEvent, _, cx| forge.dismiss(at, cx));
                    let said = design::failure(answer).unwrap_or_default();
                    strip(&said, Tone::Danger)
                        .role(Role::Alert)
                        .aria_live(view::accesskit::Live::Assertive)
                        .child(notice_action(
                            id(format!("forge-pending-{}-dismiss", op.id)),
                            "Dismiss",
                            Tone::Danger,
                            (ladder, theme),
                            dismiss,
                        ))
                }
            }
            .into_any_element()
        })
        .collect()
}

/// Whether a write of the open change (a review among them) is on its
/// way: a second built from what is on screen could land twice.
pub(crate) fn sending(forge: &Forge) -> bool {
    forge.review_key().is_some_and(|key| forge.busy(&key))
}

/// What every screen says over its body: the last refusal of a read the
/// view made on its own (the session, a followed link).
pub(crate) fn notices(forge: &Forge, ladder: &Ladder, theme: &Theme) -> Vec<AnyElement> {
    let refusal = (!forge.notice.is_empty()).then(|| {
        notice(
            id("forge-notice"),
            forge.notice.clone(),
            Tone::Danger,
            ladder,
            theme,
        )
        .role(Role::Alert)
        .aria_live(view::accesskit::Live::Assertive)
        .into_any_element()
    });
    refusal.into_iter().collect()
}

// ------------------------------------------------------------------ controls

/// A copy control: `text` to the clipboard, its glyph turning to a check
/// once it is there (the Expo app says so in a toast the view has none
/// of); off while there is nothing to copy (a link with no chain known).
pub(crate) fn copy_button(
    key: &'static str,
    (glyph, name): (&'static str, &'static str),
    text: Option<String>,
    forge: &Forge,
    cx: &mut Context<Forge>,
    (ladder, theme): (&Ladder, &Theme),
) -> impl IntoElement {
    let copied = text.is_some() && forge.copied == text;
    let glyph = if copied { "check" } else { glyph };
    let enabled = text.is_some();
    let press = cx.listener(move |forge, _: &ClickEvent, _, cx| {
        let Some(text) = text.clone() else { return };
        cx.host()
            .notify::<view::methods::ClipboardWrite>(text.clone());
        forge.copied = Some(text);
        cx.notify();
    });
    design::icon_button(
        id(key),
        design::icon(glyph, ladder.icon),
        name,
        ladder,
        theme,
        press,
    )
    .enabled(enabled)
}

/// The kit's small outline button with its glyph (`Button.tsx` `small`
/// with `icon`): `design::button(..).small()`'s face, 36 tall, 12 in, the
/// glyph at 16 before the label in secondary medium; its press reaches
/// past its edges to a finger's box. A small button with no glyph is
/// `design::button(..).small()`.
// SDK gap: a button's glyph (`Button.tsx` `icon`); the SDK builds it later
pub(crate) struct SmallButton {
    id: ElementId,
    label: SharedString,
    icon: &'static str,
    enabled: bool,
    ladder: Ladder,
    theme: Theme,
    click: Press,
}

pub(crate) fn small_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: &'static str,
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> SmallButton {
    SmallButton {
        id: id.into(),
        label: label.into(),
        icon,
        enabled: true,
        ladder: *ladder,
        theme: *theme,
        click: Box::new(click),
    }
}

impl SmallButton {
    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl IntoElement for SmallButton {
    type Element = Div;

    fn into_element(self) -> Div {
        let theme = self.theme;
        // the kit's outline; a disabled one faint on a quiet edge
        let (edge, words) = match self.enabled {
            true => (theme.border_strong, theme.foreground),
            false => (theme.border, theme.faint),
        };
        let press = design::hit(self.id, self.ladder.hit, SMALL, px(1.), &self.ladder)
            .role(Role::Button)
            .aria_label(self.label.clone());
        let press = match self.enabled {
            true => press.focusable().on_click(self.click),
            false => press.aria_disabled(true),
        };
        div()
            .relative()
            .h(SMALL)
            .px(design::space::LG)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(design::space::SM)
            .border_1()
            .border_color(edge)
            .text_color(words)
            .text_type(self.ladder.secondary)
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .child(design::icon(self.icon, SMALL_ICON))
            .child(self.label)
            .child(press)
    }
}

/// A button taking `share` parts of its row (`Button` `wide` with
/// `flex: share`): it grows from nothing by its share of the room the
/// row has, its box stretched across it.
// SDK gap: the kit's `Button` `wide` with its `flex` share (`form_verbs`
// gives only cancel and the verb taking the rest); the SDK builds it later
pub(crate) fn share(share: f32, button: impl IntoElement) -> Div {
    let mut cell = div().min_w(px(0.)).flex().flex_col().child(button);
    let style = cell.style();
    style.flex_grow = Some(share);
    style.flex_basis = Some(px(0.).into());
    cell
}

/// A checkbox (`Toggle.tsx` `Checkbox`): a 20 box, a `border_strong`
/// edge, ink with a check once ticked; its press reaches a finger's box
/// and stops there, so a row around it does not open too.
// SDK gap: the kit's `Checkbox`; the SDK builds it later
pub(crate) fn checkbox(
    id: impl Into<ElementId>,
    on: bool,
    label: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
    toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let edge = if on {
        theme.primary
    } else {
        theme.border_strong
    };
    let press = design::hit(id, TICK_BOX, TICK_BOX, TICK_EDGE, ladder)
        .role(Role::CheckBox)
        .aria_label(label.into())
        .aria_toggled(on.into())
        .focusable()
        .consumes_click()
        .on_click(toggle);
    div()
        .relative()
        .size(TICK_BOX)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .border(TICK_EDGE)
        .border_color(edge)
        .when(on, |tick| {
            tick.bg(theme.primary)
                .child(design::icon("check", TICK_ICON).text_color(theme.primary_foreground))
        })
        .child(press)
}

/// A radio's mark (`Toggle.tsx` `Radio`): a 20 ring, ink with a 9 dot
/// once picked. The row it sits in is the radio: it carries the press
/// and the state.
// SDK gap: the kit's `Radio` mark; the SDK builds it later
pub(crate) fn radio_mark(on: bool, theme: &Theme) -> Div {
    let edge = if on {
        theme.foreground
    } else {
        theme.border_strong
    };
    div()
        .size(TICK_BOX)
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border(TICK_EDGE)
        .border_color(edge)
        .when(on, |ring| {
            ring.child(div().size(RADIO_DOT).rounded_full().bg(theme.foreground))
        })
}

/// One filter chip (`Choice.tsx` `Chips`): its label, and its count in
/// the data face.
// SDK gap: the kit's `Chips`; the SDK builds it later
pub(crate) struct Chip {
    pub(crate) id: ElementId,
    pub(crate) label: &'static str,
    pub(crate) count: Option<usize>,
}

/// Filter chips (`Choice.tsx` `Chips`): 32 tall outline boxes 8 apart,
/// the chosen one in ink, scrolling sideways; each press reaches a
/// finger's box.
pub(crate) fn chips(
    row_id: &str,
    chips: Vec<Chip>,
    chosen: usize,
    ladder: &Ladder,
    theme: &Theme,
    pick: impl Fn(usize, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let pick = std::rc::Rc::new(pick);
    let theme = *theme;
    let row = div()
        .id(id(row_id.to_owned()))
        .flex()
        .flex_none()
        .gap(design::space::SM)
        .py(design::space::SM)
        .px(design::space::BLOCK)
        .overflow_x_scroll();
    row.children(chips.into_iter().enumerate().map(|(index, chip)| {
        let pick = pick.clone();
        let click =
            move |_: &ClickEvent, window: &mut Window, app: &mut App| pick(index, window, app);
        chip_box(chip, index == chosen, ladder, &theme, click)
    }))
}

fn chip_box(
    chip: Chip,
    chosen: bool,
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let (ground, edge, words, count) = match chosen {
        true => (
            Some(theme.primary),
            theme.primary,
            theme.primary_foreground,
            theme.primary_foreground,
        ),
        false => (None, theme.border_strong, theme.foreground, theme.muted),
    };
    div()
        .relative()
        .h(CHIP)
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::XS)
        .px(design::space::MD)
        .border_1()
        .border_color(edge)
        .when_some(ground, |chip, ground| chip.bg(ground))
        .text_type(ladder.secondary)
        .text_color(words)
        .whitespace_nowrap()
        .child(chip.label)
        .children(chip.count.map(|count_| {
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(ladder.tiny.size)
                .text_color(count)
                .child(count_.to_string())
        }))
        .child(
            design::hit(chip.id, ladder.hit, CHIP, px(1.), ladder)
                .role(Role::Button)
                .aria_label(chip.label)
                .aria_toggled(chosen.into())
                .focusable()
                .on_click(click),
        )
}

/// One tab of a `design::strip` (`design::tab`), its count after its
/// label in the data face, faint (`Choice.tsx` `Strip`). `(chosen,
/// active)`: it is the open one, and the arrows are on it.
pub(crate) fn strip_tab(
    element_id: ElementId,
    label: &str,
    count: Option<u64>,
    (chosen, active): (bool, bool),
    ladder: &Ladder,
    theme: &Theme,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let tab = design::tab(element_id, label.to_owned(), chosen, ladder, theme, click).children(
        count.map(|count| {
            div()
                .ml(design::space::XXS)
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(ladder.tiny.size)
                .font_weight(FontWeight::NORMAL)
                .text_color(theme.faint)
                .child(count.to_string())
        }),
    );
    design::item(tab, Role::Tab, active).into_any_element()
}

/// The strip over a document or a folder (`repo.tsx` `FileBar`): at least
/// a control tall on `surface`, 16 in, 6 over and under, a hairline under
/// it; the title secondary in the data face at the left, `right` a caption
/// at the right.
pub(crate) fn file_bar(
    key: &str,
    title: impl Into<SharedString>,
    right: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let title = div()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.secondary)
        .child(title.into());
    let right: SharedString = right.into();
    bar(key, title, caption(right, ladder, theme), ladder, theme)
}

/// A [`file_bar`] holding `left` (truncating) and `right` as they are
/// drawn: a file's facts beside its Source and Wrap.
pub(crate) fn bar(
    key: &str,
    left: impl IntoElement,
    right: impl IntoElement,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id(key.to_owned()))
        .w_full()
        .min_h(ladder.control)
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .px(design::space::BLOCK)
        .py(design::space::XS)
        .bg(theme.surface)
        .border_b_1()
        .border_color(theme.border)
        .child(div().flex_1().min_w(px(0.)).truncate().child(left))
        .child(div().flex_none().child(right))
}

// ------------------------------------------------------------------ forge rows

/// A badge's tint (`Badge.tsx` `Tint`).
#[derive(Clone, Copy)]
// SDK gap: the kit's `Badge` tints (`design::badge` takes the colours);
// the SDK builds them later
pub(crate) enum Tint {
    /// on `surface_raised`, muted
    Plain,
    Ok,
    Warn,
    Danger,
    Agent,
}

// A name's initial, or its picture once read, at the list's size
// (`Avatar.tsx`); in the agent tint for an agent, a picture ringed in it.
// SDK gap: the kit's `Avatar` `agent`; the SDK builds it later
pub(crate) fn avatar(
    name: &str,
    picture: crate::select::Picture,
    agent: bool,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let ringed = agent && picture.is_some();
    design::avatar_with(name, picture, ladder.avatar, theme)
        .when(agent, |face| {
            face.bg(theme.agent_soft).text_color(theme.agent)
        })
        .when(ringed, |face| face.border_2().border_color(theme.agent))
}

/// A state word on the kit's 20 tall tag (`Badge.tsx`), in its tint.
// SDK gap: the kit's `Badge` tints; the SDK builds them later
pub(crate) fn badge(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    tint: Tint,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let (words, ground) = match tint {
        Tint::Plain => (theme.muted, theme.surface_raised),
        Tint::Ok => (theme.success, theme.success_soft),
        Tint::Warn => (theme.warning, theme.warning_soft),
        Tint::Danger => (theme.danger, theme.danger_soft),
        Tint::Agent => (theme.agent, theme.agent_soft),
    };
    design::badge(id, label, ladder, words, ground)
}

/// A change's state as a badge (`parts.tsx` `StateBadge`): open ok,
/// merged agent, closed plain.
pub(crate) fn state_badge(
    state: ChangeState,
    key: &str,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let tint = match state {
        ChangeState::Open => Tint::Ok,
        ChangeState::Merged => Tint::Agent,
        ChangeState::Closed => Tint::Plain,
    };
    badge(
        id(key.to_owned()),
        words::change_state(state),
        tint,
        ladder,
        theme,
    )
}

/// The dot a change row leads with (`parts.tsx` `StateDot`).
fn state_dot(state: ChangeState, theme: &Theme) -> Div {
    let colour = match state {
        ChangeState::Open => theme.success,
        ChangeState::Merged => theme.agent,
        ChangeState::Closed => theme.faint,
    };
    div().size(DOT).rounded_full().bg(colour)
}

/// Where a change row is listed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Listed {
    /// the root's, across every repository: a branch leads, its
    /// repository is named first, in the reading face
    Root,
    /// its repository's Changes tab: its state dot leads, its comments
    /// are counted, in the data face
    Repo,
}

/// One change (`parts.tsx` `ChangeRow`): `<title> #n`, the caption
/// `<source> → <target> · <author> · N comments`; listed by judgment,
/// what it asks of me. Where it is listed ([`Listed`]) sets its lead and
/// its caption.
pub(crate) fn change_row(
    summary: &ChangeSummary,
    judgment: Option<&Judgment>,
    listed: Listed,
    forge: &Forge,
    ladder: &Ladder,
    theme: &Theme,
) -> Row {
    let across = listed == Listed::Root;
    let author = forge.principal_name(&summary.author);
    let asks = judgment.into_iter().flat_map(|judgment| {
        let requested = judgment
            .requested
            .then_some(words::REVIEW_REQUESTED.to_owned());
        let replied = judgment
            .replies
            .is_some()
            .then(|| words::NEW_REPLY.to_owned());
        requested.into_iter().chain(replied)
    });
    let comments = words::comments(summary.comment_count);
    let words: Vec<String> = across
        .then(|| summary.repo.clone())
        .into_iter()
        .chain([endpoints(&summary.from, &summary.into), author])
        .chain((!across).then_some(comments))
        .chain(asks)
        .collect();
    let caption_line = caption(words.join(" · "), ladder, theme)
        .when(!across, |line| line.font_family(design::fonts::FAMILY_MONO));
    let title = div().text_type(ladder.body).truncate().child(muted_tail(
        &summary.title,
        &format!(" #{}", summary.n),
        theme,
    ));
    let lead = match listed {
        Listed::Root => lead_icon("git-branch", ladder, theme),
        Listed::Repo => state_dot(summary.state, theme).into_any_element(),
    };
    Row::new(
        id(format!("forge-change-{}-{}", summary.repo, summary.n)),
        title,
    )
    .lead(lead)
    .caption(caption_line)
    .chevron()
}

/// A git time as `3 Oct 2026, 4:00 AM` (`parts.tsx` `when`).
pub(crate) fn when(seconds: i64) -> String {
    // a time before 1970 reads as 1970
    let at = u64::try_from(seconds).unwrap_or_default() * 1000;
    format!("{}, {}", design::day(at), design::clock(at))
}

/// One commit (`parts.tsx` `CommitRow`): its title, `<short> · <author> ·
/// <date, time>`, and `· N parents` for a merge.
pub(crate) fn commit_row(commit: &CommitInfo, ladder: &Ladder, theme: &Theme) -> Row {
    let short = design::short_hex(&commit.oid);
    let merge = match commit.parents.len() {
        0 | 1 => String::new(),
        parents => format!(" · {parents} parents"),
    };
    let title = match summary_of(commit) {
        summary if summary.is_empty() => short.clone(),
        summary => summary,
    };
    let author = String::from_utf8_lossy(&commit.author.name);
    let facts = format!("{short} · {author} · {}{merge}", when(commit.author.time));
    Row::titled(id(format!("forge-commit-{}", commit.oid)), title)
        .lead(lead_icon("git-commit-horizontal", ladder, theme))
        .caption(caption(facts, ladder, theme))
        .chevron()
}

// ------------------------------------------------------------------ the ref sheet

/// The ref sheet (`parts.tsx` `RefSheet`) for `menu`: Branches | Tags,
/// the filter, a row per ref with `default` on the default head and a
/// check on the picked one; a pick closes it.
pub(crate) fn ref_sheet(
    menu: Menu,
    title: &str,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let close =
        cx.listener(move |forge, _: &(), window, cx| forge.close_dropdown(menu, window, cx));
    let key = menu.key();
    let body = div()
        .id(id(format!("{key}-rows")))
        .size_full()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            id(format!("{key}-head")),
            title.to_owned(),
            ladder,
            theme,
        ))
        .child(ref_sheet_controls(menu, forge, cx, ladder, theme))
        .child(ref_sheet_list(menu, forge, cx, ladder, theme));
    design::sheet(id(format!("{key}-sheet")), title.to_owned(), ladder, theme)
        .detents([0.6, 0.92])
        .on_dismiss(close)
        .child(body)
        .into_any_element()
}

/// The sheet's Branches | Tags and its filter.
fn ref_sheet_controls(
    menu: Menu,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let key = menu.key();
    let choices = [("branches", "Branches"), ("tags", "Tags")]
        .map(|(slug, word)| (id(format!("{key}-{slug}")), SharedString::from(word)));
    let pick = cx.processor(move |forge, index: usize, _, cx| {
        forge.menu_tags = index == 1;
        forge.menu_step(0, cx);
    });
    let kinds = forge.menu_offers_tags(menu).then(|| {
        design::segmented(
            id(format!("{key}-kind")),
            words::BRANCHES_OR_TAGS,
            ladder,
            theme,
            Some(usize::from(forge.menu_tags)),
            choices,
            pick,
        )
    });
    let filter = Input::new(id(format!("{key}-filter")), &forge.menu_search, "Filter")
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .virtual_keyboard(VirtualKeyboard::Search)
        .placeholder("Filter")
        .on_change(cx.listener(|forge, _: &view::wire::TextChange, _, cx| forge.menu_step(0, cx)));
    div()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .p(design::space::BLOCK)
        .children(kinds)
        .child(design::field(filter, ladder, theme).lead("search"))
}

/// The refs the sheet lists, a row each, as one `Menu` composite the
/// arrows walk (`{key}-menu`); virtual, as a repository can hold many.
fn ref_sheet_list(
    menu: Menu,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let names = Rc::new(forge.menu_list(menu));
    let count = names.len();
    let active = forge.menu_cursor.min(count.saturating_sub(1));
    let pick = Rc::new(cx.processor(move |forge, name: Vec<u8>, window, cx| {
        forge.menu_pick(menu, name, window, cx)
    }));
    let paint = ref_rows(
        menu,
        names.clone(),
        (forge.default_head(), forge.menu_picked(menu)),
        active,
        pick.clone(),
        (*ladder, *theme),
    );
    let list = uniform_list(id(format!("{}-items", menu.key())), count, paint)
        .track_scroll(&forge.menu_scroll)
        .size_full();
    design::composite(id(format!("{}-menu", menu.key())), Role::Menu, "Refs")
        .active(active, count)
        .track_scroll(&forge.menu_scroll)
        .on_move(cx.processor(|forge, index: usize, _, cx| {
            forge.menu_cursor = index;
            cx.notify();
        }))
        .on_press(move |index, window, app| pick(names[index].clone(), window, app))
        .build()
        .flex_1()
        .min_h(px(0.))
        .child(list)
        .into_any_element()
}

/// Paints the ref sheet's rows in view: the name in the data face,
/// `default` on the default head, a check on the picked one.
fn ref_rows(
    menu: Menu,
    names: Rc<Vec<Vec<u8>>>,
    (head, picked): (Vec<u8>, Vec<u8>),
    active: usize,
    pick: Route<Vec<u8>>,
    (ladder, theme): (Ladder, Theme),
) -> impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static {
    move |range, _, _| {
        range
            .map(|index| {
                let name = &names[index];
                let mut row = Row::data(
                    id(format!("forge-ref-{}", crate::words::path_text(name))),
                    ref_label(name),
                );
                if *name == head {
                    let badge = design::badge(
                        id(format!("{}-default", menu.key())),
                        "default",
                        &ladder,
                        theme.muted,
                        theme.surface_raised,
                    );
                    row = row.tail(badge);
                }
                if *name == picked {
                    row = row
                        .tail(design::icon("check", ladder.icon_row).text_color(theme.foreground));
                }
                let (pick, name) = (pick.clone(), name.clone());
                let row = row.press(&ladder, &theme).on_click(
                    move |_: &ClickEvent, window: &mut Window, app: &mut App| {
                        pick(name.clone(), window, app)
                    },
                );
                design::item(row, Role::MenuItem, index == active).into_any_element()
            })
            .collect()
    }
}

// ------------------------------------------------------------------ the people sheet

/// What the people sheet is for: its id, title, the line under it, and
/// who it leaves out.
pub(crate) struct PeopleAsk<'a> {
    pub(crate) key: &'static str,
    pub(crate) title: &'static str,
    pub(crate) caption: Option<&'static str>,
    pub(crate) skip: &'a [forge::Principal],
}

/// The people sheet (`parts.tsx` `PeopleSheet`): the roster's people
/// whose name or number holds the filter, a row each (avatar, name,
/// `account N`); a pick closes it.
pub(crate) fn people_sheet(
    ask: PeopleAsk<'_>,
    pick: impl Fn(&mut Forge, forge::Principal, &mut Context<Forge>) + Clone + 'static,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let close = cx.listener(|forge, _: &(), _, cx| forge.close_sheet(cx));
    let filter = Input::new(
        id(format!("{}-filter", ask.key)),
        &forge.people_search,
        words::SEARCH_MEMBERS,
    )
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None)
    .virtual_keyboard(VirtualKeyboard::Search)
    .placeholder(words::SEARCH_MEMBERS);
    let head = design::sheet_title(id(format!("{}-head", ask.key)), ask.title, ladder, theme)
        .children(ask.caption.map(|said| caption(said, ladder, theme)));
    let body = div()
        .id(id(format!("{}-body", ask.key)))
        .size_full()
        .flex()
        .flex_col()
        .child(head)
        .child(
            div()
                .p(design::space::BLOCK)
                .child(design::field(filter, ladder, theme).lead("search")),
        )
        .child(people_rows(&ask, pick, forge, cx, ladder, theme));
    design::sheet(id(ask.key), ask.title, ladder, theme)
        .detents([0.6, 0.92])
        .on_dismiss(close)
        .child(body)
        .into_any_element()
}

fn people_rows(
    ask: &PeopleAsk<'_>,
    pick: impl Fn(&mut Forge, forge::Principal, &mut Context<Forge>) + Clone + 'static,
    forge: &Forge,
    cx: &mut Context<Forge>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let Some(names) = forge.names.ready() else {
        return quiet(
            id(format!("{}-reading", ask.key)),
            words::READING_ROSTER,
            ladder,
            theme,
        )
        .into_any_element();
    };
    let typed = forge.people_search.text().trim().to_lowercase();
    let people: Vec<forge::Principal> = names
        .people()
        .map(forge::Principal::Account)
        .filter(|person| !ask.skip.contains(person))
        .filter(|person| {
            let number = person.account().unwrap_or_default().to_string();
            let name = names.name(person).unwrap_or_default().to_lowercase();
            typed.is_empty() || number == typed || name.contains(&typed)
        })
        .collect();
    if people.is_empty() {
        return quiet(
            id(format!("{}-none", ask.key)),
            "No member reads like that.",
            ladder,
            theme,
        )
        .into_any_element();
    }
    let rows = people.into_iter().map(|person| {
        let name = names.member(&person);
        let number = person.account().unwrap_or_default();
        let pick = pick.clone();
        let chosen = person.clone();
        let press = cx.listener(move |forge, _: &ClickEvent, _, cx| {
            forge.close_sheet(cx);
            pick(forge, chosen.clone(), cx);
        });
        Row::titled(id(format!("{}-{number}", ask.key)), name.clone())
            .lead(avatar(
                &name,
                forge.picture(&person, cx),
                forge.is_agent(&person),
                ladder,
                theme,
            ))
            .caption(caption(format!("account {number}"), ladder, theme))
            .press(ladder, theme)
            .role(Role::Button)
            .focusable()
            .on_click(press)
    });
    div()
        .id(id(format!("{}-rows", ask.key)))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_size_reads_as_the_expo_app_says_it() {
        for (bytes, said) in [
            (38, "38 B"),
            (1023, "1023 B"),
            (6963, "6.8 KB"),
            (12 * 1024, "12 KB"),
            (1_468_006, "1.4 MB"),
        ] {
            assert_eq!(super::size_of(bytes), said);
        }
    }
}
