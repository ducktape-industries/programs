//! Expo `src/network/explorer/parts.tsx` and the `src/kit` pieces the
//! explorer's screens share: the rows (a block, a run of empty blocks, a
//! transaction), `Notice`, `Quiet`, `EmptyState`, `SectionHead`, `Facts`,
//! `KeyValue`, `Row`, `Button small`, `Avatar`, `Linked` and `CopyLink`,
//! at the Expo kit's sizes. Each draws what it is handed; a press goes
//! through the model's actions (`go`, `copy`, `read_all`).
//!
//! The Expo line numbers below are mobile dev 266633c.
use view::design::{Face, mono, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::chain::{Line, lines};
use crate::decode::{ago, clip, date, grouped, short, sized};
use crate::words;
use crate::{BlockRow, Cx, Explorer, Note, Route, TxRow};

use super::{Look, back};

/// A caption's line (Expo `src/design/theme.ts:47`): the box a section
/// head's link draws in.
const CAPTION_LINE: Pixels = px(16.);
/// A small text's line (Expo `src/design/theme.ts:44`): the box a link
/// draws in.
const SMALL_LINE: Pixels = px(19.);
/// A notice's action: secondary's 18 line (`theme.ts:46`) and `gap.xs`
/// over and under (Expo `src/kit/Notice.tsx:60`).
const NOTICE_ACTION_H: Pixels = px(30.);
/// How far a notice's action presses past its words (Expo
/// `src/kit/Notice.tsx:60` `hitSlop={8}`).
const NOTICE_REACH: Pixels = px(8.);
/// How far a section head's link presses past its words (Expo
/// `src/network/explorer/pages.tsx:41` `hitSlop={12}`).
const MORE_REACH: Pixels = px(12.);
/// How far a link in a value presses past its words each side: Expo's
/// `Linked` sets none, and a one-digit height (8 wide) then presses the
/// ladder's 44, our rule.
const LINK_REACH: Pixels = px(18.);
/// A key's column in a key/value row (Expo `src/kit/Facts.tsx:64`).
const KEY_W: Pixels = px(96.);
/// The face of an account that does not act, at half strength (Expo
/// `src/kit/Avatar.tsx:38`).
const OFF_OPACITY: f32 = 0.5;
/// A transaction's outcome mark, each way (Expo
/// `src/network/explorer/parts.tsx:139`).
const MARK: Pixels = px(8.);
/// A transaction row's program, before its title (Expo
/// `src/network/explorer/parts.tsx:151`).
const TARGET_TEXT: Pixels = px(12.);

/// A press handed to a piece that draws it.
pub(super) type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

// ---------- pages ----------

// SDK gap: Page, the scroller with its foot clear, the SDK builds it later
/// A page's scroller (Expo `pages.tsx` `Page`): what is under the title
/// row and the notice, its foot `gap.block` clear.
pub(super) fn scroller(id: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .pb(space::BLOCK)
}

/// A page over the root (Expo `Header back`): its title row with Back,
/// and the notice under it.
pub(super) fn over(
    id: &'static str,
    title: impl Into<SharedString>,
    view: &Explorer,
    cx: Cx,
    k: &Look,
) -> design::Page {
    let nav = design::Nav::back(k.depth, back(cx));
    let page = design::page(id, title, nav, &k.ladder, &k.theme);
    match note(view, cx, k) {
        Some(note) => page.child(note),
        None => page,
    }
}

/// A page a link opened that shows nothing (Expo's `Unlinked` and
/// `ByHash`): its title row with Back, which dismisses the note the link
/// left, over the empty state saying why.
pub(super) fn gone(
    id: &'static str,
    title: &'static str,
    icon: &str,
    heading: &'static str,
    note: &Note,
    cx: Cx,
    k: &Look,
) -> AnyElement {
    let dismiss = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.dismiss_note(cx));
    // over the stack's top page
    let nav = design::Nav::back(k.depth + 1, dismiss);
    design::page(id, title, nav, &k.ladder, &k.theme)
        .child(empty(
            "explorer-gone-why",
            icon,
            heading,
            words::note(note),
            k,
        ))
        .into_any_element()
}

/// Copy link in a page's title row (Expo `CopyLink`): absent while the
/// session names no chain to mint the link in.
pub(super) fn copy_link(view: &Explorer, route: &Route, cx: Cx, k: &Look) -> Option<AnyElement> {
    let link = view.link(route)?;
    let copy = cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
        view.copy(link.clone(), words::LINK, cx)
    });
    Some(
        design::icon_button(
            "explorer-copy-link",
            design::icon("link", k.ladder.icon),
            "Copy link",
            &k.ladder,
            &k.theme,
            copy,
        )
        .into_any_element(),
    )
}

// SDK gap: a press over words reaching a set way (React Native's
// `hitSlop`), the SDK builds it later
/// A press box over words `height` tall (React Native's `hitSlop`):
/// `side` past each end, and over and under `side` or to the ladder's 44,
/// whichever is further. Not [`design::hit`]: that reaches from the
/// parent's width, and a view cannot measure its words.
pub(super) fn reach(
    id: impl Into<ElementId>,
    height: Pixels,
    side: Pixels,
    k: &Look,
) -> Stateful<Div> {
    let to_hit = (f32::from(k.ladder.hit) - f32::from(height)) / 2.;
    let y = px(to_hit.max(f32::from(side)));
    div()
        .id(id)
        .absolute()
        .top(-y)
        .bottom(-y)
        .left(-side)
        .right(-side)
}

// ---------- states ----------

/// A notice's or a badge's tone (Expo `Notice kind`, `Badge tone`).
#[derive(Clone, Copy)]
pub(super) enum Tone {
    Quiet,
    Success,
    Danger,
}

impl Tone {
    /// How a run went: success where it applied, danger where it was
    /// refused.
    pub(super) fn of_run(applied: bool) -> Tone {
        if applied { Tone::Success } else { Tone::Danger }
    }

    /// The tone's words and marks.
    pub(super) fn ink(self, theme: &Theme) -> Hsla {
        match self {
            Tone::Quiet => theme.muted,
            Tone::Success => theme.success,
            Tone::Danger => theme.danger,
        }
    }

    /// The ground the tone's words sit on.
    pub(super) fn ground(self, theme: &Theme) -> Hsla {
        match self {
            Tone::Quiet => theme.surface,
            Tone::Success => theme.success_soft,
            Tone::Danger => theme.danger_soft,
        }
    }
}

/// What a search, a link, a copy or a write came to, under the title row
/// (Expo `Notice` with Dismiss): a write that went through in success,
/// one that did not in danger, the rest quiet.
pub(super) fn note(view: &Explorer, cx: Cx, k: &Look) -> Option<Stateful<Div>> {
    let note = view.note.as_ref()?;
    let tone = match note {
        Note::Sent(_) => Tone::Success,
        Note::Refused(_) => Tone::Danger,
        Note::NotFound(_) | Note::NoSuchHash { .. } | Note::Unlinked(_) | Note::Copied(_) => {
            Tone::Quiet
        }
    };
    let dismiss = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.dismiss_note(cx));
    let dismiss = Act {
        id: "explorer-note-dismiss",
        label: "Dismiss",
        press: Box::new(dismiss),
    };
    Some(notice(
        "explorer-note",
        words::note(note),
        tone,
        Some(dismiss),
        k,
    ))
}

/// A notice's one action (Expo `Notice action`).
pub(super) struct Act {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) press: Press,
}

// SDK gap: Notice with its action, the SDK builds it later
/// A strip above content (Expo `Notice`): its words in the tone on the
/// tone's soft ground, `gap.md` over and under, the gutter in; one action
/// at the right, its press reaching past it to `ladder.hit`.
pub(super) fn notice(
    id: &'static str,
    text: impl Into<SharedString>,
    tone: Tone,
    action: Option<Act>,
    k: &Look,
) -> Stateful<Div> {
    let (theme, ladder) = (&k.theme, &k.ladder);
    let (ground, ink) = (tone.ground(theme), tone.ink(theme));
    let action = action.map(|Act { id, label, press }| {
        div()
            .relative()
            .flex_none()
            .h(NOTICE_ACTION_H)
            .flex()
            .items_center()
            .pl(space::SM)
            .text_type(ladder.secondary)
            .font_weight(FontWeight::MEDIUM)
            .text_color(ink)
            .child(label)
            .child(
                reach(id, NOTICE_ACTION_H, NOTICE_REACH, k)
                    .role(Role::Button)
                    .aria_label(label)
                    .focusable()
                    .on_click(press),
            )
    });
    div()
        .id(id)
        .flex_none()
        .flex()
        .items_center()
        .gap(space::MD)
        .py(space::MD)
        .px(space::BLOCK)
        .bg(ground)
        .when(matches!(tone, Tone::Quiet), |strip| {
            strip.border_b_1().border_color(theme.border)
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_type(ladder.secondary)
                .text_color(ink)
                .child(text.into()),
        )
        .children(action)
}

/// A refused read (Expo `Refused`): its sentence (a host refusal's is
/// `design::sentence`), and Retry, which reads the view's data again.
pub(super) fn refused(sentence: &str, cx: Cx, k: &Look) -> AnyElement {
    let retry = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.read_all(cx));
    design::refused("explorer", sentence.to_owned(), &k.ladder, &k.theme, retry).into_any_element()
}

// SDK gap: Quiet at the ladder's sizes, the SDK builds it later
/// One muted status line, the gutter in (Expo `Quiet`).
pub(super) fn quiet(id: &'static str, text: impl Into<SharedString>, k: &Look) -> AnyElement {
    design::quiet(text, &k.theme)
        .id(id)
        .text_type(k.ladder.secondary)
        .p(space::BLOCK)
        .into_any_element()
}

/// Nothing to show (Expo `EmptyState`): its icon, what is missing, why.
pub(super) fn empty(
    id: &'static str,
    icon: &str,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    k: &Look,
) -> AnyElement {
    design::empty_state(id, icon, title, body, &k.ladder, &k.theme).into_any_element()
}

// ---------- sections and facts ----------

/// A section's head (Expo `SectionHead`): its label, and what sits beside
/// it (a count, a sentence, a link).
pub(super) fn section(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    aside: Option<AnyElement>,
    k: &Look,
) -> Stateful<Div> {
    design::section_head(id, label, &k.ladder, &k.theme)
        .flex_none()
        .children(aside)
}

/// Words beside a section head's label, cut short where they run out.
pub(super) fn aside(text: impl Into<SharedString>) -> AnyElement {
    div().truncate().child(text.into()).into_any_element()
}

/// A section head's link (words::ALL_BLOCKS, Expo `pages.tsx` `More`): caption
/// words whose press reaches past them to `ladder.hit`.
pub(super) fn more_link(
    id: &'static str,
    label: &'static str,
    route: Route,
    cx: Cx,
    k: &Look,
) -> AnyElement {
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    div()
        .relative()
        .flex_none()
        .h(CAPTION_LINE)
        .flex()
        .items_center()
        .child(label)
        .child(
            reach(id, CAPTION_LINE, MORE_REACH, k)
                .role(Role::Link)
                .aria_label(label)
                .focusable()
                .on_click(go),
        )
        .into_any_element()
}

/// One figure of [`facts`]: an eyebrow, a value, a note under it.
pub(super) struct Fact {
    /// its element's key under the facts' own
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) value: String,
    pub(super) caption: Option<String>,
}

// SDK gap: Facts, the SDK builds it later
/// Two columns of figures (Expo `Facts`): half the width each, hairlines
/// between.
pub(super) fn facts(id: &'static str, facts: Vec<Fact>, k: &Look) -> Stateful<Div> {
    div().id(id).flex_none().flex().flex_wrap().children(
        facts
            .into_iter()
            .enumerate()
            .map(|(index, fact)| figure(id, index, fact, k)),
    )
}

/// One figure: its eyebrow in the data face, uppercase, its value in the
/// heading role, its note; the left column draws the hairline between.
fn figure(id: &str, index: usize, fact: Fact, k: &Look) -> Stateful<Div> {
    let (theme, ladder) = (&k.theme, &k.ladder);
    let left = index.is_multiple_of(2);
    div()
        .id(SharedString::from(format!("{id}-{}", fact.key)))
        .w_1_2()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .py(space::LG)
        .px(space::BLOCK)
        .border_b_1()
        .when(left, |figure| figure.border_r_1())
        .border_color(theme.border)
        .child(
            design::eyebrow(fact.label, ladder, theme)
                .font_weight(FontWeight::MEDIUM)
                .mb(space::XXS),
        )
        .child(
            div()
                .text_type(ladder.heading)
                .font_weight(FontWeight::MEDIUM)
                .child(fact.value),
        )
        .children(fact.caption.map(|caption| {
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(caption)
        }))
}

// SDK gap: KeyValue, the SDK builds it later
/// A label and its value (Expo `KeyValue`): the label small and muted in
/// its column, the value small, wrapping, level with it.
pub(super) fn key_value(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    value: impl IntoElement,
    k: &Look,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id.into())
        .flex_none()
        .min_h(k.ladder.control)
        .flex()
        .items_start()
        .gap(space::LG)
        .py(space::MD)
        .px(space::BLOCK)
        .border_b_1()
        .border_color(k.theme.border)
        .child(
            div()
                .w(KEY_W)
                .flex_none()
                .text_type(k.ladder.small)
                .text_color(k.theme.muted)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_type(k.ladder.small)
                .child(value),
        )
}

// SDK gap: wrapping mono text at the ladder's sizes, the SDK builds it later
/// Text in the data face that wraps where it must: a 64-digit hash.
pub(super) fn data(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .child(text.into())
}

/// A date in the data face, and how long ago it was, faint (Expo's Time
/// rows).
pub(super) fn time_and_age(time: u64, now: u64, k: &Look) -> Div {
    div()
        .flex()
        .flex_wrap()
        .child(data(format!("{} ", date(time))))
        .child(data(words::age(now, time)).text_color(k.theme.faint))
}

// SDK gap: Linked, the SDK builds it later
/// A link inside a page (Expo `Linked`): small, foreground, underlined; its
/// press reaches past its line to `ladder.hit`. A caller wanting the data
/// face sets it on what this returns.
pub(super) fn linked(
    id: impl Into<SharedString>,
    text: impl Into<SharedString>,
    route: Route,
    cx: Cx,
    k: &Look,
) -> Div {
    let text = text.into();
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    div()
        .relative()
        .flex_none()
        .h(SMALL_LINE)
        .flex()
        .items_center()
        .text_type(k.ladder.small)
        .text_color(k.theme.foreground)
        .underline()
        .whitespace_nowrap()
        .child(text.clone())
        .child(
            reach(id.into(), SMALL_LINE, LINK_REACH, k)
                .role(Role::Link)
                .aria_label(text)
                .focusable()
                .on_click(go),
        )
}

// ---------- rows ----------

/// A list row with a caption under its title (Expo `Row`): the kit's frame
/// at `rowTall`.
pub(super) fn list_row(id: impl Into<ElementId>, k: &Look) -> Stateful<Div> {
    design::row(id, &k.ladder, &k.theme).min_h(k.ladder.row_tall)
}

/// A row whose press opens `route` (Expo `Row onPress`): a button named
/// `label`, the whole row the press, dimmed while held.
pub(super) fn opens(
    row: Stateful<Div>,
    label: impl Into<SharedString>,
    route: Route,
    cx: Cx,
    k: &Look,
) -> Stateful<Div> {
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    design::pressed(row, &k.ladder)
        .role(Role::Button)
        .aria_label(label.into())
        .focusable()
        .on_click(go)
}

/// A row's title over its caption.
pub(super) fn title_and_caption(title: impl IntoElement, caption: impl IntoElement) -> Div {
    design::row_words(title).child(caption)
}

/// A caption under a row's title, muted, cut short where it runs out.
pub(super) fn caption(text: impl Into<SharedString>, k: &Look) -> Div {
    design::row_caption(text, &k.ladder, &k.theme)
}

/// The chevron a row that opens a page ends with (Expo `Row chevron`).
pub(super) fn chevron(k: &Look) -> impl IntoElement {
    design::chevron(&k.ladder, &k.theme)
}

// SDK gap: the Row's tail in the data face, the SDK builds it later
/// What ends a row (Expo `Row tail`): a caption in the data face, muted.
pub(super) fn tail(text: impl Into<SharedString>, k: &Look) -> Div {
    mono(text)
        .text_type(k.ladder.caption)
        .text_color(k.theme.muted)
}

/// A list drawn whole in a page that scrolls.
pub(super) fn column(id: &'static str, rows: Vec<Stateful<Div>>) -> Stateful<Div> {
    div().id(id).flex_none().flex().flex_col().children(rows)
}

/// A small button (Expo `Button small`): `Quiet` or `Outline`; with no
/// press it is disabled.
pub(super) fn small(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    kind: design::Kind,
    press: Option<Press>,
    k: &Look,
) -> AnyElement {
    let enabled = press.is_some();
    let press = press.unwrap_or_else(|| Box::new(|_: &ClickEvent, _: &mut Window, _: &mut App| {}));
    design::button(
        ElementId::Name(id.into()),
        label,
        &k.ladder,
        &k.theme,
        press,
    )
    .kind(kind)
    .small()
    .enabled(enabled)
    .into_any_element()
}

// An account's face (Expo `Avatar` with `words.ts` `faceOf`): its
// `picture` once read, else its initial; a system program's is square with
// a mono letter, a managed account's in the agent tint (a picture ringed in
// it), a suspended or revoked one at half strength.
pub(super) fn face(
    account: &identity::Account,
    picture: Option<std::sync::Arc<view::Image>>,
    size: Face,
    k: &Look,
) -> Div {
    let kind = account.kind();
    let off = design::note(&kind).is_some();
    let name = &account.card.name;
    let ringed = picture.is_some();
    let avatar = match kind {
        identity::Kind::System(_) => module_face(name, size, k),
        identity::Kind::Program { .. } => design::avatar_with(name, picture, size, &k.theme)
            .bg(k.theme.agent_soft)
            .text_color(k.theme.agent)
            .when(ringed, |face| face.border_2().border_color(k.theme.agent)),
        identity::Kind::User => design::avatar_with(name, picture, size, &k.theme),
    };
    // SDK gap: Avatar `off`, the SDK builds it later
    avatar.when(off, |avatar| avatar.opacity(OFF_OPACITY))
}

// SDK gap: Avatar module, the SDK builds it later
/// A program's face: square, a mono letter (Expo `Avatar module`).
pub(super) fn module_face(name: &str, size: Face, k: &Look) -> Div {
    design::avatar(name, size, &k.theme)
        .rounded(px(0.))
        .font_family(design::fonts::FAMILY_MONO)
}

// ---------- the explorer's rows ----------

/// Up to `most` lines of the window, newest first (Expo `BlockLines`): a
/// block, or a run of empty ones folded.
pub(super) fn block_lines(view: &Explorer, most: usize, cx: Cx, k: &Look) -> Vec<Stateful<Div>> {
    let now = view.chain.now();
    lines(&view.chain.blocks, most)
        .into_iter()
        .map(|line| match line {
            Line::Block(block) => block_line(block, now, cx, k),
            Line::Empty { newest, oldest } => empty_run(newest, oldest, k),
        })
        .collect()
}

/// A block (Expo `BlockLine`): its height, medium when it carries
/// transactions and faint when not, its short id, its count and age.
fn block_line(block: &BlockRow, now: u64, cx: Cx, k: &Look) -> Stateful<Div> {
    let height = block.height;
    let carries = block.txs > 0;
    let row = list_row(SharedString::from(format!("explorer-block-{height}")), k);
    opens(row, format!("Block {height}"), Route::Block(height), cx, k)
        .child(title_and_caption(
            mono(grouped(height))
                .text_type(k.ladder.body)
                .when(carries, |height| height.font_weight(FontWeight::MEDIUM))
                .text_color(if carries {
                    k.theme.foreground
                } else {
                    k.theme.faint
                }),
            tail(short(&block.id), k),
        ))
        .child(tail(
            format!(
                "{} · {}",
                words::tx_count(block.txs as u64),
                ago(now, block.time)
            ),
            k,
        ))
        .child(chevron(k))
}

/// A run of empty blocks folded into one quiet line (Expo `EmptyRun`).
fn empty_run(newest: u64, oldest: u64, k: &Look) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("explorer-empty-{newest}")))
        .flex_none()
        .min_h(k.ladder.row)
        .flex()
        .items_center()
        .px(space::BLOCK)
        .border_b_1()
        .border_color(k.theme.border)
        .child(
            mono(words::empty_run(oldest, newest))
                .text_type(k.ladder.secondary)
                .text_color(k.theme.faint),
        )
}

/// How a run ended, as the mark in a row's lead (Expo `Mark`): success,
/// danger, an empty lead where the node kept no receipt.
fn mark(applied: Option<bool>, k: &Look) -> Div {
    design::lead(
        div().children(
            applied.map(|applied| div().size(MARK).bg(Tone::of_run(applied).ink(&k.theme))),
        ),
    )
}

/// Who signed: the account holding the key as `name #n`, or the key
/// (Expo `signerWords`).
fn signer_words(view: &Explorer, key: &[u8]) -> String {
    match view.holder(key) {
        Some((account, _)) => format!("{} #{}", account.card.name, account.number),
        None => short(key),
    }
}

/// A row's title (Expo `titleOf`): its op's, once its program has
/// described it, else its program and size.
pub(super) fn title_of(tx: &TxRow) -> String {
    match tx.op() {
        Some(op) => op.title.clone(),
        None => sized(&tx.target, u64::from(tx.size)),
    }
}

/// What a transaction row says after its hash (Expo `TxLine who`).
#[derive(Clone, Copy)]
pub(super) enum Who {
    /// who signed it
    Signer,
    /// its block, on the signer's own page
    Block,
}

/// A transaction (Expo `TxLine`): its mark, its program and its op's
/// title, its hash and its signer or block, its age. Drawing it asks its
/// program to describe it, once.
pub(super) fn tx_line(view: &Explorer, tx: &TxRow, who: Who, cx: Cx, k: &Look) -> Stateful<Div> {
    view.describe(tx, cx);
    let (height, position) = (tx.height, tx.index);
    let applied = tx
        .receipt
        .as_ref()
        .map(|run| matches!(run.outcome, Outcome::Applied { .. }));
    let label = match applied {
        Some(applied) => activity::view::label(&tx.target, applied, &grouped(height), position),
        None => format!("{}, block {} · {position}", tx.target, grouped(height)),
    };
    // one line, as Expo's nested spans: the program in the data face, then
    // the title, cut short where it runs out
    let title = div()
        .flex()
        .items_baseline()
        .min_w_0()
        .text_type(k.ladder.body)
        .child(
            mono(format!("{} ", tx.target))
                .flex_none()
                .text_size(TARGET_TEXT)
                .text_color(k.theme.muted),
        )
        .child(div().min_w_0().truncate().child(clip(&title_of(tx))));
    let place = div()
        .flex()
        .items_baseline()
        .min_w_0()
        .text_type(k.ladder.caption)
        .text_color(k.theme.muted)
        .child(mono(short(&tx.hash)).flex_none())
        .child(div().min_w_0().truncate().child(match who {
            Who::Signer => format!(" · {}", signer_words(view, &tx.signer)),
            Who::Block => format!(" · block {}", grouped(height)),
        }));
    let row = list_row(
        SharedString::from(format!("explorer-tx-{height}-{position}")),
        k,
    );
    opens(row, label, Route::Tx { height, position }, cx, k)
        .child(mark(applied, k))
        .child(title_and_caption(title, place))
        .child(tail(ago(view.chain.now(), tx.time), k))
        .child(chevron(k))
}
