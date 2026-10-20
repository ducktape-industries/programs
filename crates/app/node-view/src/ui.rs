//! The Nodes sheet: the network's name and this node's pulse, its numbers
//! (height, last block, block time, the epoch), then the members table
//! (`table.rs`) in one of its four states, and for a reader who votes the
//! vote (`motions.rs`). `render` reads the state and changes nothing.
//! Under a finger the root draws the phone's screens (`phone/`) instead.
use abi::hex;
use view::prelude::*;
use view::{Div, FontWeight};

use crate::words::{self, EpochPlace};
use crate::{Nodes, motions, table};
use valset::Motion;
use view::design::Ladder;

pub(crate) mod phone;

pub(crate) fn render(view: &Nodes, cx: &mut Context<Nodes>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    div()
        .id("nodes")
        .flex()
        .flex_col()
        .gap_3()
        .p(px(table::INSET))
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(head(view, cx, &theme, ladder))
        .child(numbers(view, cx, &theme, ladder))
        .children(motions::confirm(view, cx, &theme, ladder))
        .children(motions::outcome(view, cx, &theme, ladder))
        .child(
            div()
                .id("nodes-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_scroll()
                .child(body(view, cx, &theme, ladder))
                .children(motions::sections(view, cx, &theme, ladder)),
        )
}

fn head(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let status = view.status.ready();
    let title = status.map_or(words::NODES.into(), |status| status.chain_id.clone());
    let door = view.voting.votes().then(|| {
        let press = |open: bool| {
            cx.listener(move |view: &mut Nodes, _: &ClickEvent, _, cx| {
                view.press(Motion::Door { open }, cx)
            })
        };
        div()
            .id("nodes-door")
            .flex()
            .gap_1()
            .child(
                design::button(
                    "nodes-door-open",
                    words::door(true).label,
                    ladder,
                    theme,
                    press(true),
                )
                .kind(design::Kind::Outline)
                .enabled(view.voting.pressable()),
            )
            .child(
                design::button(
                    "nodes-door-close",
                    words::door(false).label,
                    ladder,
                    theme,
                    press(false),
                )
                .kind(design::Kind::Outline)
                .enabled(view.voting.pressable()),
            )
    });
    div()
        .id("nodes-head")
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(design::heading("nodes-title", title, 1, theme).flex_1())
        .children(status.map(|status| pulse(view.answering(), status, theme)))
        .children(door)
}

fn pulse(answering: bool, status: &NodeStatus, theme: &Theme) -> impl IntoElement {
    let color = match answering {
        true => theme.success,
        false => theme.danger,
    };
    div()
        .id("nodes-pulse")
        .flex()
        .items_center()
        .gap_2()
        .text_size(design::text::SECONDARY)
        .child(div().size_2().rounded_full().bg(color))
        .child(words::pulse(answering))
        .child(
            div()
                .text_color(theme.muted)
                .child(format!("· {}", words::THIS_NODE)),
        )
        .child(design::mono(design::short_hex(&hex(&status.identity))).text_color(theme.muted))
}

fn numbers(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let status = match &view.status {
        Loadable::Ready(status) | Loadable::Reloading(status, _) => status,
        Loadable::Failed(refusal) => {
            let retry =
                cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.retry_status(cx));
            return design::refused(
                "nodes-status",
                design::sentence(refusal),
                ladder,
                theme,
                retry,
            )
            .into_any_element();
        }
        Loadable::Idle | Loadable::Loading(_) => {
            return design::quiet(words::READING_STATUS, theme)
                .id("nodes-status-loading")
                .into_any_element();
        }
    };
    let last = words::block_age(view.ticks - view.moved);
    let block_time = format!("{:.1} s", status.block_time_ms as f64 / 1000.);
    div()
        .id("nodes-status")
        .flex()
        .flex_wrap()
        .items_end()
        .gap_x_6()
        .gap_y_3()
        .pb_3()
        .border_b_1()
        .border_color(theme.border)
        .child(fact(
            "height",
            words::HEIGHT,
            design::grouped(status.height),
            true,
            theme,
        ))
        .child(fact("last-block", "Last block", last, false, theme))
        .child(fact(
            "block-time",
            words::BLOCK_TIME,
            block_time,
            false,
            theme,
        ))
        .children(epoch(status, theme).into_iter().flatten())
        .into_any_element()
}

fn fact(key: &str, label: &str, value: String, mono: bool, theme: &Theme) -> impl IntoElement {
    let value = match mono {
        true => design::mono(value).text_size(design::text::SECTION),
        false => div().text_size(design::text::SECTION).child(value),
    };
    div()
        .id(ElementId::Name(format!("nodes-status-{key}").into()))
        .flex()
        .flex_col()
        .gap_1()
        .child(quiet_label(label, theme))
        .child(value)
}

fn quiet_label(label: impl Into<SharedString>, theme: &Theme) -> Div {
    div()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(label.into())
}

fn epoch(status: &NodeStatus, theme: &Theme) -> Option<[AnyElement; 2]> {
    let epoch = EpochPlace::of(status)?;
    let share = epoch.into as f32 / epoch.length as f32;
    const BAR: f32 = 180.;
    let label = epoch.place();
    let progress = div()
        .id("nodes-status-epoch")
        .flex()
        .flex_col()
        .gap_2()
        .child(quiet_label(label, theme))
        .child(
            div()
                .w(px(BAR))
                .h(px(2.))
                .bg(theme.border)
                .child(div().h_full().w(px(BAR * share)).bg(theme.foreground)),
        )
        .into_any_element();
    let next = quiet_label(epoch.next(), theme)
        .id("nodes-status-next-epoch")
        .ml_auto()
        .into_any_element();
    Some([progress, next])
}

fn body(view: &Nodes, cx: &mut Context<Nodes>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    match &*view.nodes {
        Loadable::Idle | Loadable::Loading(_) => design::quiet(words::READING_MEMBERS, theme)
            .id("nodes-loading")
            .into_any_element(),
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|view: &mut Nodes, _: &ClickEvent, _, cx| view.read(cx));
            design::refused("nodes", design::sentence(refusal), ladder, theme, retry)
                .into_any_element()
        }
        Loadable::Ready(nodes) | Loadable::Reloading(nodes, _) if nodes.is_empty() => {
            design::empty_state(
                "nodes-empty",
                "server",
                words::NO_MEMBERS,
                words::NO_MEMBERS_WHY,
                ladder,
                theme,
            )
            .into_any_element()
        }
        Loadable::Ready(_) | Loadable::Reloading(_, _) if view.status.ready().is_none() => {
            div().into_any_element()
        }
        Loadable::Ready(nodes) | Loadable::Reloading(nodes, _) => {
            table::table(view, nodes, cx, theme, ladder).into_any_element()
        }
    }
}

pub(crate) fn section(id: &'static str, label: String, theme: &Theme) -> impl IntoElement {
    div()
        .id(id)
        .role(Role::Heading)
        .aria_level(2)
        .h(design::size::CONTROL)
        .flex()
        .items_end()
        .pb_1()
        .px_2()
        .border_b_1()
        .border_color(theme.border)
        .text_size(design::text::SECONDARY)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted)
        .child(label)
}
