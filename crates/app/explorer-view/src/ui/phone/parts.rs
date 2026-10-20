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

const CAPTION_LINE: Pixels = px(16.);
const SMALL_LINE: Pixels = px(19.);
const NOTICE_ACTION_H: Pixels = px(30.);
const NOTICE_REACH: Pixels = px(8.);
const MORE_REACH: Pixels = px(12.);
const LINK_REACH: Pixels = px(18.);
const KEY_W: Pixels = px(96.);
const OFF_OPACITY: f32 = 0.5;
const MARK: Pixels = px(8.);
const TARGET_TEXT: Pixels = px(12.);

pub(super) type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

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

#[derive(Clone, Copy)]
pub(super) enum Tone {
    Quiet,
    Success,
    Danger,
}

impl Tone {
    pub(super) fn of_run(applied: bool) -> Tone {
        if applied { Tone::Success } else { Tone::Danger }
    }

    pub(super) fn ink(self, theme: &Theme) -> Hsla {
        match self {
            Tone::Quiet => theme.muted,
            Tone::Success => theme.success,
            Tone::Danger => theme.danger,
        }
    }

    pub(super) fn ground(self, theme: &Theme) -> Hsla {
        match self {
            Tone::Quiet => theme.surface,
            Tone::Success => theme.success_soft,
            Tone::Danger => theme.danger_soft,
        }
    }
}

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

pub(super) struct Act {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) press: Press,
}

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

pub(super) fn refused(sentence: &str, cx: Cx, k: &Look) -> AnyElement {
    let retry = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.read_all(cx));
    design::refused("explorer", sentence.to_owned(), &k.ladder, &k.theme, retry).into_any_element()
}

pub(super) fn quiet(id: &'static str, text: impl Into<SharedString>, k: &Look) -> AnyElement {
    design::quiet(text, &k.theme)
        .id(id)
        .text_type(k.ladder.secondary)
        .p(space::BLOCK)
        .into_any_element()
}

pub(super) fn empty(
    id: &'static str,
    icon: &str,
    title: impl Into<SharedString>,
    body: impl Into<SharedString>,
    k: &Look,
) -> AnyElement {
    design::empty_state(id, icon, title, body, &k.ladder, &k.theme).into_any_element()
}

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

pub(super) fn aside(text: impl Into<SharedString>) -> AnyElement {
    div().truncate().child(text.into()).into_any_element()
}

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

pub(super) struct Fact {
    pub(super) key: &'static str,
    pub(super) label: &'static str,
    pub(super) value: String,
    pub(super) caption: Option<String>,
}

pub(super) fn facts(id: &'static str, facts: Vec<Fact>, k: &Look) -> Stateful<Div> {
    div().id(id).flex_none().flex().flex_wrap().children(
        facts
            .into_iter()
            .enumerate()
            .map(|(index, fact)| figure(id, index, fact, k)),
    )
}

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

pub(super) fn data(text: impl Into<SharedString>) -> Div {
    div()
        .font_family(design::fonts::FAMILY_MONO)
        .child(text.into())
}

pub(super) fn time_and_age(time: u64, now: u64, k: &Look) -> Div {
    div()
        .flex()
        .flex_wrap()
        .child(data(format!("{} ", date(time))))
        .child(data(words::age(now, time)).text_color(k.theme.faint))
}

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

pub(super) fn list_row(id: impl Into<ElementId>, k: &Look) -> Stateful<Div> {
    design::row(id, &k.ladder, &k.theme).min_h(k.ladder.row_tall)
}

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

pub(super) fn title_and_caption(title: impl IntoElement, caption: impl IntoElement) -> Div {
    design::row_words(title).child(caption)
}

pub(super) fn caption(text: impl Into<SharedString>, k: &Look) -> Div {
    design::row_caption(text, &k.ladder, &k.theme)
}

pub(super) fn chevron(k: &Look) -> impl IntoElement {
    design::chevron(&k.ladder, &k.theme)
}

pub(super) fn tail(text: impl Into<SharedString>, k: &Look) -> Div {
    mono(text)
        .text_type(k.ladder.caption)
        .text_color(k.theme.muted)
}

pub(super) fn column(id: &'static str, rows: Vec<Stateful<Div>>) -> Stateful<Div> {
    div().id(id).flex_none().flex().flex_col().children(rows)
}

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
    avatar.when(off, |avatar| avatar.opacity(OFF_OPACITY))
}

pub(super) fn module_face(name: &str, size: Face, k: &Look) -> Div {
    design::avatar(name, size, &k.theme)
        .rounded(px(0.))
        .font_family(design::fonts::FAMILY_MONO)
}

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

fn mark(applied: Option<bool>, k: &Look) -> Div {
    design::lead(
        div().children(
            applied.map(|applied| div().size(MARK).bg(Tone::of_run(applied).ink(&k.theme))),
        ),
    )
}

fn signer_words(view: &Explorer, key: &[u8]) -> String {
    match view.holder(key) {
        Some((account, _)) => format!("{} #{}", account.card.name, account.number),
        None => short(key),
    }
}

pub(super) fn title_of(tx: &TxRow) -> String {
    match tx.op() {
        Some(op) => op.title.clone(),
        None => sized(&tx.target, u64::from(tx.size)),
    }
}

#[derive(Clone, Copy)]
pub(super) enum Who {
    Signer,
    Block,
}

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
