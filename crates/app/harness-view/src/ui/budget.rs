//! The model budget atop the agents: each model account's usage windows as bars, amber from 80%
//! and red from 95%, with when each resets and runs out, and a press that reads ccs's status.
use std::collections::BTreeSet;

use harness::{AccountBudget, BudgetReport, WHOLE_WINDOW, WindowOutlook};
use view::design::{self, Ladder, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::caption;
use crate::Agents;
use crate::words::{
    budget_account, budget_calls, budget_error, budget_outlook, budget_read, budget_state,
    budget_used,
};

const BAR: f32 = 240.;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Level {
    Calm,
    Amber,
    Red,
}

pub(crate) fn level(used: u64) -> Level {
    match used {
        950_000.. => Level::Red,
        800_000.. => Level::Amber,
        _ => Level::Calm,
    }
}

pub(crate) fn ended(window: &WindowOutlook, now: u64) -> bool {
    now >= window.reset.saturating_mul(1000)
}

pub(crate) fn services(report: &BudgetReport) -> BTreeSet<String> {
    let paced = report.services.iter().map(|pace| &pace.service);
    let held = report.accounts.iter().map(|row| &row.service);
    let read = report.reads.iter().map(|read| &read.service);
    paced.chain(held).chain(read).cloned().collect()
}

pub(crate) fn strip(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Stateful<Div>> {
    let report = view.budget.ready()?;
    let now = view
        .roster
        .ready()
        .map_or(report.time, |roster| roster.now.max(report.time));
    let label = match view.reading.is_some() {
        true => "Refreshing…",
        false => "Refresh",
    };
    let refresh = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.read_status(cx));
    let refresh = (!services(report).is_empty()).then(|| {
        design::button("budget-refresh", label, ladder, theme, refresh).kind(design::Kind::Outline)
    });
    let head = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(space::SM)
        .child(caption(budget_calls(report), theme))
        .children(refresh);
    let strip = div()
        .id("budget")
        .flex()
        .flex_col()
        .gap(space::SM)
        .when(ladder.touch, |strip| strip.p(space::BLOCK))
        .child(head)
        .children(
            report
                .reads
                .iter()
                .map(|read| caption(budget_read(read, now), theme)),
        );
    let none = "No model answer has named its usage windows yet.";
    Some(match report.accounts.is_empty() {
        true => strip.child(caption(none, theme)),
        false => strip.child(
            div()
                .flex()
                .flex_wrap()
                .gap(space::LG)
                .children(report.accounts.iter().map(|row| account(row, now, theme))),
        ),
    })
}

fn account(row: &AccountBudget, now: u64, theme: &Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(budget_account(row)),
        )
        .children(budget_state(row).map(|words| caption(words, theme)))
        .children(budget_error(row).map(|words| caption(words, theme)))
        .children(
            row.windows
                .iter()
                .map(|window| bar(row, window, now, theme)),
        )
}

fn bar(row: &AccountBudget, window: &WindowOutlook, now: u64, theme: &Theme) -> Div {
    let used = match ended(window, now) {
        true => 0,
        false => window.used,
    };
    let colour = match level(used) {
        Level::Calm => theme.accent,
        Level::Amber => theme.warning,
        Level::Red => theme.danger,
    };
    let full = used.min(WHOLE_WINDOW) as f32 / WHOLE_WINDOW as f32;
    let key = format!("budget-{}-{}-{}", row.service, row.account, window.name);
    let track = div()
        .flex_none()
        .w(px(BAR))
        .h(px(6.))
        .rounded_full()
        .bg(theme.border)
        .child(
            div()
                .id(SharedString::from(format!("{key}-fill")))
                .w(px(BAR * full))
                .h_full()
                .rounded_full()
                .bg(colour),
        );
    div()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .child(track)
        .child(caption(budget_used(window, now), theme))
        .children(budget_outlook(window, now).map(|words| caption(words, theme)))
}
