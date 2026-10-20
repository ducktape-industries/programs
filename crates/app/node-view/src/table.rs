//! The members table: one row per key, validators then residents, each
//! with the blocks it led of the strip and its Height, Behind and Status
//! cells (`row.rs`), who operates it, and, for a reader who votes, the
//! motions on it; then a line on where those come from.
use abi::hex;
use view::prelude::*;
use view::{Div, Stateful};

use crate::queries::Node;
use crate::recent::{Recent, WINDOW};
use crate::row::{Row, Status};
use crate::{Nodes, Unheard, ui, voting, words};
use view::design::Ladder;

const KEY_W: Pixels = px(104.);
const ADDRESS_W: Pixels = px(196.);
const HEIGHT_W: Pixels = px(112.);
const BEHIND_W: Pixels = px(60.);
const STATUS_W: Pixels = px(136.);
const WORDS_W: f32 = 104. + 112. + 60. + 136. + 16.;
pub(crate) const INSET: f32 = 16.;
const MARK_W: f32 = 3.;
const GAPS: [f32; 3] = [3., 2., 1.];
const GUTTER: f32 = 16.;
const MONO_EM: f32 = 0.6;

fn width_beside_required_columns(view: &Nodes) -> f32 {
    let gutter = f32::from(design::size::SCROLLBAR);
    view.width - 2. * INSET - gutter - WORDS_W
}

fn address_fits(view: &Nodes) -> bool {
    width_beside_required_columns(view) >= f32::from(ADDRESS_W)
}

fn marks_w(gap: f32) -> f32 {
    WINDOW as f32 * MARK_W + (WINDOW - 1) as f32 * gap + 4.
}

fn column_w(gap: f32, label: f32) -> f32 {
    GUTTER + marks_w(gap).max(label)
}

fn strip_gap_beside_columns(view: &Nodes, label: f32) -> Option<f32> {
    let address = if address_fits(view) {
        f32::from(ADDRESS_W)
    } else {
        0.
    };
    let room = width_beside_required_columns(view) - address;
    GAPS.into_iter().find(|gap| column_w(*gap, label) <= room)
}

pub(crate) fn table(
    view: &Nodes,
    nodes: &[Node],
    cx: &Context<Nodes>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let head = view.status.ready().map_or(0, |status| status.height);
    let validators = nodes.iter().filter(|node| node.validator).count() as u64;
    let heard = view.heard();
    let answering = view.answering();
    let span = Recent::span(head);
    let label = format!("{} · {}", words::PROPOSED, words::span(&span));
    let label_w =
        (label.chars().count() as f32 * f32::from(design::text::CAPTION) * MONO_EM).ceil();
    let (address, gap) = (address_fits(view), strip_gap_beside_columns(view, label_w));
    let column = gap.map(|gap| px(column_w(gap, label_w)));
    let rows = |validator: bool| {
        nodes
            .iter()
            .enumerate()
            .filter(move |(_, node)| node.validator == validator)
            .map(move |(index, node)| {
                let cells = view.cells(node);
                let marks = strip(node, head, gap.unwrap_or(GAPS[0]), &view.recent, theme);
                line(
                    index,
                    node,
                    cells,
                    place(marks, column),
                    view,
                    theme,
                    ladder,
                )
                .children(under(index, node, view, cx, theme, ladder))
            })
    };
    let residents = nodes.len() as u64 - validators;
    let no_votes = view.unheard().map(|why| {
        let id = match why {
            Unheard::NotVoting => "nodes-no-votes",
            Unheard::NoNetwork => "nodes-no-network",
        };
        note(id, words::unheard(why).into(), theme)
    });
    let silent = (!answering).then(|| {
        let seconds = view.ticks - view.answered;
        note("nodes-last-answer", words::last_answer(seconds), theme)
    });
    div()
        .id("nodes-table")
        .w_full()
        .min_w(px(WORDS_W
            + if address { f32::from(ADDRESS_W) } else { 0. }
            + column.map_or(0., f32::from)))
        .flex()
        .flex_col()
        .children(no_votes)
        .children(silent)
        .child(columns(label, address, column, theme))
        .child(ui::section(
            "nodes-validators",
            format!("{} · {validators}", words::VALIDATORS),
            theme,
        ))
        .children(rows(true))
        .child(ui::section(
            "nodes-residents",
            format!("{} · {residents}", words::RESIDENTS),
            theme,
        ))
        .children(rows(false))
        .children(heard.is_some().then(|| {
            design::quiet(words::footnote(), theme)
                .id("nodes-footnote")
                .pt_2()
                .px_2()
        }))
}

fn note(id: &'static str, text: String, theme: &Theme) -> impl IntoElement {
    design::quiet(text, theme).id(id).pb_2().px_2()
}

fn columns(
    label: String,
    address: bool,
    column: Option<Pixels>,
    theme: &Theme,
) -> impl IntoElement {
    let name = |text: String, width: Pixels, right: bool| cell(width, right).child(text);
    let (beside, below) = place(label, column);
    let names = div()
        .flex()
        .items_center()
        .h(design::size::CONTROL)
        .child(name(words::KEY.into(), KEY_W, false))
        .children(address.then(|| name(words::ADDRESS.into(), ADDRESS_W, false)))
        .child(stretch(beside))
        .child(name(words::HEIGHT.into(), HEIGHT_W, true))
        .child(name(words::BEHIND.into(), BEHIND_W, true))
        .child(name(words::STATUS.into(), STATUS_W, false).pl_4());
    ruled("nodes-columns", names, below, theme)
        .font_family(design::fonts::FAMILY_MONO)
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
}

fn place<E: IntoElement>(content: E, column: Option<Pixels>) -> (Option<(Pixels, E)>, Option<Div>) {
    match column {
        Some(width) => (Some((width, content)), None),
        None => (None, Some(div().pb_2().child(content))),
    }
}

fn ruled(id: impl Into<ElementId>, cells: Div, below: Option<Div>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .px_2()
        .border_b_1()
        .border_color(theme.border)
        .child(cells)
        .children(below)
}

fn stretch(content: Option<(Pixels, impl IntoElement)>) -> Div {
    match content {
        Some((width, content)) => cell(width, false)
            .flex_1()
            .min_w(width)
            .pl(px(GUTTER))
            .child(content),
        None => div().flex_1(),
    }
}

fn cell(width: Pixels, right: bool) -> Div {
    let cell = div()
        .w(width)
        .flex_none()
        .min_w(px(0.))
        .flex()
        .items_center()
        .gap_1()
        .whitespace_nowrap()
        .overflow_hidden();
    match right {
        true => cell.justify_end(),
        false => cell,
    }
}

fn line(
    index: usize,
    node: &Node,
    cells: Row,
    (beside, below): (Option<(Pixels, Div)>, Option<Div>),
    view: &Nodes,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let this = view.is_this(node);
    let muted = |text: String| div().text_color(theme.muted).child(text);
    let address = address_fits(view);
    let key = design::mono(design::short_hex(&hex(&node.key)));
    let key = match this && !address {
        true => cell(KEY_W, false)
            .flex_col()
            .items_start()
            .gap_0()
            .child(key)
            .child(muted(words::THIS_NODE.into()).text_size(design::text::CAPTION)),
        false => cell(KEY_W, false).child(key),
    };
    let address = address.then(|| {
        cell(ADDRESS_W, false)
            .child(
                design::mono(node.shown_address().to_owned())
                    .text_color(theme.muted)
                    .truncate(),
            )
            .children(this.then(|| muted(words::THIS_NODE.into())))
    });
    let height = match cells.signed {
        Some(signed) => cell(HEIGHT_W, true)
            .child(design::mono(design::grouped(signed)))
            .child(muted(words::VOTED.into())),
        None => cell(HEIGHT_W, true).child(muted(words::NO_VALUE.into())),
    };
    let behind = match cells.behind {
        Some(behind) => design::mono(design::grouped(behind)),
        None => muted(words::NO_VALUE.into()),
    };
    let cells = div()
        .flex()
        .items_center()
        .min_h(design::size::CONTROL + design::space::SM)
        .child(key)
        .children(address)
        .child(stretch(beside))
        .child(height)
        .child(cell(BEHIND_W, true).child(behind))
        .child(cell(STATUS_W, false).pl_4().child(status(
            index,
            &cells.status,
            view.answering(),
            theme,
            ladder,
        )));
    let id = ElementId::Name(format!("nodes-row-{index}").into());
    ruled(id, cells, below, theme).text_size(design::text::SECONDARY)
}

fn under(
    index: usize,
    node: &Node,
    view: &Nodes,
    cx: &Context<Nodes>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Div> {
    let operator = node.operator.clone().map(|operator| {
        div()
            .id(ElementId::Name(format!("nodes-operator-{index}").into()))
            .text_color(theme.muted)
            .child(operator)
    });
    let motions = view.voting.votes().then(|| {
        voting::motions_on(node)
            .into_iter()
            .map(|(name, label, motion)| {
                let press = cx.listener(move |view: &mut Nodes, _: &ClickEvent, _, cx| {
                    view.press(motion.clone(), cx)
                });
                let id = ElementId::Name(format!("nodes-{name}-{index}").into());
                design::button(id, label, ladder, theme, press)
                    .kind(design::Kind::Quiet)
                    .enabled(view.voting.pressable())
            })
    });
    if operator.is_none() && motions.is_none() {
        return None;
    }
    Some(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .pb_1()
            .text_size(design::text::CAPTION)
            .children(operator)
            .child(div().flex_1())
            .children(motions.into_iter().flatten()),
    )
}

fn strip(node: &Node, head: u64, gap: f32, recent: &Recent, theme: &Theme) -> Div {
    if !node.validator {
        return div().text_color(theme.muted).child(words::DOESNT_PROPOSE);
    }
    let marks = Recent::span(head).map(|height| {
        let fill = match recent.led(&node.key, height) {
            Some(true) => theme.foreground,
            Some(false) => theme.border,
            None => theme.surface,
        };
        let mark = div().w(px(MARK_W)).h(px(14.)).bg(fill);
        match height == head {
            true => div()
                .p(px(1.))
                .border_1()
                .border_color(theme.foreground)
                .child(mark),
            false => mark,
        }
    });
    div().flex().items_center().gap(px(gap)).children(marks)
}

fn status(
    index: usize,
    status: &Status,
    answering: bool,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement {
    let id = ElementId::named_usize("nodes-status-word", index);
    let badge = |foreground, background| {
        let (foreground, background) = match answering {
            true => (foreground, background),
            false => (theme.muted, theme.surface_raised),
        };
        design::badge(id.clone(), status.word(), ladder, foreground, background).into_any_element()
    };
    let caption = |text: String| {
        div()
            .text_size(design::text::CAPTION)
            .text_color(theme.muted)
            .child(text)
            .into_any_element()
    };
    let parts: Vec<AnyElement> = match status {
        Status::InSync => vec![badge(theme.success, theme.success_soft)],
        Status::Behind(_) => vec![badge(theme.warning, theme.warning_soft)],
        Status::Blank => vec![
            div()
                .text_color(theme.muted)
                .child(status.word())
                .into_any_element(),
        ],
        Status::Quiet { since } => {
            let since = since.map(|since| caption(format!("since {}", design::grouped(since))));
            [badge(theme.warning, theme.warning_soft)]
                .into_iter()
                .chain(since)
                .collect()
        }
    };
    div().flex().items_center().gap_1().children(parts)
}
