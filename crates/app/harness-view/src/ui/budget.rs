//! The model budget atop the agents: one row per model account from its freshest read, grouped by its
//! provider (learned, else its service's route), with the login its gate names, a bar per usage
//! window (amber from 80%, red from 95%) with the time to its reset, and whether it can answer.
use harness::{AccountBudget, BudgetFact, ProviderRouteRow, WHOLE_WINDOW, WindowOutlook};
use view::design::{Ladder, space, text};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::caption;
use crate::Agents;
use crate::words::{
    budget_calls, budget_left, budget_name, budget_percent, budget_provider, budget_runs_out,
    budget_swapped, budget_via, budget_window_label,
};

const NAME: f32 = 200.;
const STATE: f32 = 72.;
const BAR: f32 = 240.;
pub(crate) const SESSION_WEEKLY_SCOPED: f32 = 3.;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccountState {
    Active,
    Usable,
    Spent,
}

pub(crate) fn ended(window: &WindowOutlook, now: u64) -> bool {
    now >= window.reset.saturating_mul(1000)
}

pub(crate) fn shown_used(window: &WindowOutlook, now: u64) -> u64 {
    match ended(window, now) {
        true => 0,
        false => window.used,
    }
}

pub(crate) fn freshest(accounts: &[AccountBudget]) -> Vec<&AccountBudget> {
    let mut newest: Vec<&AccountBudget> = accounts.iter().collect();
    newest.sort_by_key(|row| std::cmp::Reverse(row.time));
    let mut kept: Vec<&AccountBudget> = Vec::new();
    for row in newest {
        if !kept.iter().any(|held| held.account == row.account) {
            kept.push(row);
        }
    }
    kept
}

pub(crate) fn spent(row: &AccountBudget, now: u64) -> bool {
    let window_spent = row.windows.iter().any(|window| {
        let refused = window.status.as_deref() == Some("rejected");
        !ended(window, now) && (window.used >= WHOLE_WINDOW || refused)
    });
    let renewed_since = row.windows.iter().any(|window| {
        let reset = window.reset.saturating_mul(1000);
        reset > row.time && reset <= now
    });
    let refused = row.status.as_deref() == Some("rejected");
    window_spent || (refused && !renewed_since)
}

pub(crate) fn account_state(row: &AccountBudget, answering: bool, now: u64) -> AccountState {
    match (spent(row, now), answering) {
        (true, _) => AccountState::Spent,
        (false, true) => AccountState::Active,
        (false, false) => AccountState::Usable,
    }
}

pub(crate) fn answering<'a>(rows: &[&'a AccountBudget], now: u64) -> Vec<&'a str> {
    let mut services: Vec<&str> = Vec::new();
    let mut active = Vec::new();
    for row in rows {
        let passed_over = services.contains(&row.service.as_str()) || spent(row, now);
        if passed_over {
            continue;
        }
        services.push(row.service.as_str());
        active.push(row.account.as_str());
    }
    active
}

pub(crate) fn fact<'a>(facts: &'a [BudgetFact], row: &AccountBudget) -> Option<&'a BudgetFact> {
    let mut found = facts.iter();
    found.find(|fact| fact.service == row.service && fact.account == row.account)
}

pub(crate) type Group<'a> = (Option<String>, Vec<&'a AccountBudget>);

pub(crate) fn provider_of(
    row: &AccountBudget,
    facts: &[BudgetFact],
    routes: &[ProviderRouteRow],
) -> Option<String> {
    let learned = fact(facts, row).and_then(|fact| fact.provider.clone());
    learned.or_else(|| {
        let routed = routes.iter().find(|route| {
            let mut services = route.services.iter();
            services.any(|named| named.service == row.service)
        });
        routed.map(|route| route.provider.clone())
    })
}

pub(crate) fn grouped<'a>(
    rows: &[&'a AccountBudget],
    facts: &[BudgetFact],
    routes: &[ProviderRouteRow],
) -> Vec<Group<'a>> {
    let mut groups: Vec<Group<'a>> = Vec::new();
    for row in rows {
        let provider = provider_of(row, facts, routes);
        match groups.iter_mut().find(|(held, _)| *held == provider) {
            Some((_, members)) => members.push(row),
            None => groups.push((provider, vec![row])),
        }
    }
    groups.sort_by_key(|(provider, _)| (provider.is_none(), provider.clone()));
    for (_, members) in &mut groups {
        members.sort_by(|a, b| a.account.cmp(&b.account));
    }
    groups
}

pub(crate) fn ordered(row: &AccountBudget) -> Vec<&WindowOutlook> {
    let mut windows: Vec<&WindowOutlook> = row.windows.iter().collect();
    windows.sort_by_key(|window| match window.name.as_str() {
        "5h" => 0,
        "7d" => 1,
        _ => 2,
    });
    windows
}

pub(crate) fn strip(view: &Agents, theme: &Theme, ladder: &Ladder) -> Option<Stateful<Div>> {
    let read = view.budget.ready()?;
    let report = &read.report.budget;
    let now = view
        .roster
        .ready()
        .map_or(report.time, |roster| roster.now.max(report.time));
    let rows = freshest(&report.accounts);
    let strip = div()
        .id("budget")
        .flex()
        .flex_col()
        .gap(space::SM)
        .when(ladder.touch, |strip| strip.p(space::BLOCK))
        .child(caption(budget_calls(report, rows.len()), theme));
    let none = "No model answer has named its usage windows yet.";
    Some(match rows.is_empty() {
        true => strip.child(caption(none, theme)),
        false => strip.children(groups(&rows, read, now, theme, ladder)),
    })
}

pub(crate) fn full(windows: f32) -> Pixels {
    let bars = px(BAR) * windows + space::LG * (windows - 1.);
    px(NAME + STATE) + space::LG * 2. + bars
}

fn widest(rows: &[&AccountBudget]) -> Pixels {
    let windows = rows.iter().map(|row| row.windows.len()).max().unwrap_or(1);
    full(windows.max(1) as f32)
}

fn groups(
    rows: &[&AccountBudget],
    read: &crate::read::Budgeted,
    now: u64,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<Div> {
    let facts = &read.report.facts;
    let active = answering(rows, now);
    let widest = widest(rows);
    let found = grouped(rows, facts, &read.routes);
    let shown = found.into_iter().map(|(provider, members)| {
        let named = provider.as_deref().unwrap_or("other");
        let head = div()
            .id(SharedString::from(format!("budget-provider-{named}")))
            .flex()
            .items_baseline()
            .gap(space::SM)
            .pt(space::XS)
            .pb(space::XS)
            .role(Role::Heading)
            .aria_level(3)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(budget_provider(provider.as_deref())),
            )
            .child(caption(members.len().to_string(), theme));
        let rows = members.into_iter().map(|row| {
            let answers = active.contains(&row.account.as_str());
            let state = account_state(row, answers, now);
            account(row, fact(facts, row), state, now, theme, ladder)
        });
        let group = div()
            .flex()
            .flex_col()
            .when(!ladder.touch, |group| group.w_full().max_w(widest));
        group.child(head).child(
            div()
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(theme.border)
                .children(rows),
        )
    });
    shown.collect()
}

fn account(
    row: &AccountBudget,
    fact: Option<&BudgetFact>,
    state: AccountState,
    now: u64,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let key = format!("budget-{}", row.account);
    let who = div()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(budget_name(row)),
        )
        .child(caption(budget_via(row, fact), theme).truncate())
        .children(
            fact.and_then(|fact| budget_swapped(fact, now))
                .map(|words| caption(words, theme).text_color(theme.warning).truncate()),
        );
    let windows = div().flex_1().min_w_0().flex().gap(space::LG).children(
        ordered(row)
            .into_iter()
            .map(|window| bar(&key, window, now, theme)),
    );
    let badge = badge(&key, state, theme, ladder);
    let line = div()
        .id(SharedString::from(key))
        .py(space::SM)
        .border_b_1()
        .border_color(theme.border);
    match ladder.touch {
        false => line
            .flex()
            .items_center()
            .gap(space::LG)
            .child(who.flex_none().w(px(NAME)))
            .child(windows)
            .child(
                div()
                    .flex_none()
                    .w(px(STATE))
                    .flex()
                    .justify_end()
                    .child(badge),
            ),
        true => line
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(space::SM)
                    .child(who.flex_1())
                    .child(badge),
            )
            .child(windows),
    }
}

fn badge(key: &str, state: AccountState, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let (label, foreground, background) = match state {
        AccountState::Active => ("Active", theme.success, theme.success_soft),
        AccountState::Usable => ("Usable", theme.muted, theme.surface_raised),
        AccountState::Spent => ("Spent", theme.danger, theme.danger_soft),
    };
    design::badge(
        SharedString::from(format!("{key}-state")),
        label,
        ladder,
        foreground,
        background,
    )
}

fn grown(part: Div, share: f32) -> Div {
    let mut part = part;
    part.style().flex_grow = Some(share);
    part
}

fn bar(key: &str, window: &WindowOutlook, now: u64, theme: &Theme) -> Div {
    let used = shown_used(window, now);
    let colour = match level(used) {
        Level::Calm => theme.accent,
        Level::Amber => theme.warning,
        Level::Red => theme.danger,
    };
    let full = used.min(WHOLE_WINDOW) as f32 / WHOLE_WINDOW as f32;
    let fill = grown(div().h_full().rounded_full().bg(colour), full)
        .id(SharedString::from(format!("{key}-{}-fill", window.name)));
    let track = div()
        .w_full()
        .h(px(4.))
        .flex()
        .rounded_full()
        .bg(theme.border)
        .child(fill)
        .child(grown(div(), 1. - full));
    let named = div()
        .flex()
        .justify_between()
        .gap(space::XS)
        .text_size(text::CAPTION)
        .child(
            div()
                .min_w_0()
                .truncate()
                .child(budget_window_label(&window.name)),
        )
        .child(div().flex_none().child(budget_percent(window, now)));
    let below = div()
        .flex()
        .flex_wrap()
        .justify_between()
        .gap(space::XS)
        .child(caption(budget_left(window, now), theme))
        .children(budget_runs_out(window, now).map(|words| {
            div()
                .text_size(text::CAPTION)
                .text_color(theme.warning)
                .child(words)
        }));
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(space::HAIR)
        .child(named)
        .child(track)
        .child(below)
}
