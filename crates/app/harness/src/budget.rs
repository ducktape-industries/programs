//! The model budget: each model account's usage windows, from its answers, how often harness
//! called each service in the last whole minute, and when each window runs out at the pace it
//! climbed at.
use ::program::{Error, ExecCtx, QueryCtx, invalid};
use abi::role::egress::http;
use model_wire::budget as harness_words;

use crate::resume::header;
use crate::state::{
    BUDGET_FACTS, BUDGETS, Budget, BudgetFacts, BudgetWindow, FLIGHTS, PACES, Pace, Seen,
};
use crate::{
    AccountBudget, BudgetFact, BudgetFactsReport, BudgetReport, ServicePace, WHOLE_WINDOW as WHOLE,
    WindowOutlook,
};

const PREFIX: &str = "anthropic-ratelimit-unified-";
const UTILIZATION: &str = "-utilization";
pub(crate) const ORGANIZATION: &str = "anthropic-organization-id";
const MINUTE: u64 = 60_000;

pub(crate) fn millionths(text: &str) -> Option<u64> {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) if !fraction.is_empty() => (whole, fraction),
        Some(_) => return None,
        None => (text, ""),
    };
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    let mut used = whole.parse::<u64>().ok()?.checked_mul(WHOLE)?;
    let mut unit = WHOLE;
    for byte in fraction.bytes() {
        unit /= 10;
        used += u64::from(byte - b'0') * unit;
    }
    Some(used)
}

pub(crate) fn status_reads_retired() -> Error {
    invalid(
        "reading a service's status is retired: each model answer names its account's usage windows, and an idle account's windows are its service's own page",
    )
}

fn named<'a>(response: &'a http::Response, name: &str) -> Option<&'a str> {
    header(response, name).filter(|value| !value.is_empty())
}

fn harness_windows(response: &http::Response, time: u64) -> Vec<BudgetWindow> {
    let budget = harness_words::Budget::read(&response.headers);
    let windows = budget.windows.into_iter().filter_map(|window| {
        let used = millionths(&window.used)?;
        Some(BudgetWindow {
            name: window.name,
            utilization: window.used,
            used,
            reset: window.reset,
            status: window.status,
            since: Seen { used, time },
        })
    });
    windows.collect()
}

fn anthropic_windows(response: &http::Response, time: u64) -> Vec<BudgetWindow> {
    let mut found: Vec<BudgetWindow> = Vec::new();
    for (key, value) in &response.headers {
        let key = key.to_ascii_lowercase();
        let named = key.strip_prefix(PREFIX);
        let Some(name) = named.and_then(|rest| rest.strip_suffix(UTILIZATION)) else {
            continue;
        };
        let utilization = value.trim();
        let Some(used) = millionths(utilization) else {
            continue;
        };
        let reset = header(response, &format!("{PREFIX}{name}-reset"));
        let Some(reset) = reset.and_then(|value| value.parse().ok()) else {
            continue;
        };
        if name.is_empty() || found.iter().any(|window| window.name == name) {
            continue;
        }
        let status = header(response, &format!("{PREFIX}{name}-status"));
        found.push(BudgetWindow {
            name: name.to_owned(),
            utilization: utilization.to_owned(),
            used,
            reset,
            status: status.map(str::to_owned),
            since: Seen { used, time },
        });
    }
    found
}

fn windows(response: &http::Response, time: u64) -> Vec<BudgetWindow> {
    let mut found = harness_windows(response, time);
    for window in anthropic_windows(response, time) {
        let named_already = found.iter().any(|held| held.name == window.name);
        if !named_already {
            found.push(window);
        }
    }
    found
}

fn merge_budget_windows(
    before: Option<&Budget>,
    mut windows: Vec<BudgetWindow>,
    time: u64,
) -> Vec<BudgetWindow> {
    let held = before.map_or(&[][..], |row| &row.windows[..]);
    for window in &mut windows {
        let Some(last) = held.iter().find(|last| last.name == window.name) else {
            continue;
        };
        if time < last.reset.saturating_mul(1000) {
            window.since = last.since;
        }
        if window.status.is_none() {
            window.status = last.status.clone();
        }
    }
    for last in held {
        if !windows.iter().any(|window| window.name == last.name) {
            windows.push(last.clone());
        }
    }
    windows.sort_by(|a, b| a.name.cmp(&b.name));
    windows
}

pub(crate) fn learned(
    held: BudgetFacts,
    provider: &str,
    identity: Option<&str>,
    time: u64,
) -> BudgetFacts {
    let provider = Some(provider.to_owned());
    let Some(identity) = identity else {
        return BudgetFacts { provider, ..held };
    };
    let swapped = held
        .identity
        .as_deref()
        .is_some_and(|before| before != identity);
    let identity = Some(identity.to_owned());
    match swapped {
        true => BudgetFacts {
            provider,
            identity,
            previous: held.identity,
            changed: Some(time),
        },
        false => BudgetFacts {
            provider,
            identity,
            ..held
        },
    }
}

pub(crate) fn seen(ctx: &ExecCtx, service: &str, provider: &str, response: &http::Response) {
    let env = ctx.env();
    paced(ctx, service, env.time);
    let windows = windows(response, env.time);
    if windows.is_empty() {
        return;
    }
    let organization = named(response, ORGANIZATION);
    let label = named(response, harness_words::ACCOUNT);
    let account = label.or(organization).unwrap_or_default();
    let key = (service.to_owned(), account.to_owned());
    let before = BUDGETS.get(ctx, &key).ok().flatten();
    if let (Some(_), Some(organization)) = (label, organization) {
        let unnamed = (service.to_owned(), organization.to_owned());
        if unnamed != key {
            BUDGETS.remove(ctx, &unnamed);
            BUDGET_FACTS.remove(ctx, &unnamed);
        }
    }
    let anthropic_status = format!("{PREFIX}status");
    let status =
        named(response, harness_words::STATUS).or_else(|| named(response, &anthropic_status));
    let row = Budget {
        label: label.map(str::to_owned),
        organization: organization.map(str::to_owned),
        status: status.map(str::to_owned),
        fallback: header(response, &format!("{PREFIX}fallback")).map(str::to_owned),
        error: None,
        windows: merge_budget_windows(before.as_ref(), windows, env.time),
        height: env.height,
        time: env.time,
    };
    BUDGETS.put(ctx, &key, &row);
    let identity = named(response, harness_words::IDENTITY);
    let held = BUDGET_FACTS
        .get(ctx, &key)
        .ok()
        .flatten()
        .unwrap_or_default();
    BUDGET_FACTS.put(ctx, &key, &learned(held, provider, identity, env.time));
}

fn paced(ctx: &ExecCtx, service: &str, time: u64) {
    let minute = time / MINUTE;
    let service = service.to_owned();
    let pace = match PACES.get(ctx, &service).ok().flatten() {
        Some(pace) if pace.minute == minute => Pace {
            calls: pace.calls + 1,
            ..pace
        },
        Some(pace) if pace.minute + 1 == minute => Pace {
            minute,
            calls: 1,
            before: pace.calls,
        },
        _ => Pace {
            minute,
            calls: 1,
            before: 0,
        },
    };
    PACES.put(ctx, &service, &pace);
}

fn calls_in_adjacent_minutes(pace: &Pace, time: u64) -> (u64, u64) {
    let minute = time / MINUTE;
    match minute.checked_sub(pace.minute) {
        Some(0) => (pace.before, pace.calls),
        Some(1) => (pace.calls, 0),
        _ => (0, 0),
    }
}

pub(crate) fn runs_out(window: &BudgetWindow, at: u64) -> Option<u64> {
    if window.used >= WHOLE {
        return Some(at);
    }
    let climbed = window.used.checked_sub(window.since.used)?;
    let took = at.checked_sub(window.since.time)?;
    if climbed == 0 || took == 0 {
        return None;
    }
    let more = u128::from(WHOLE - window.used) * u128::from(took) / u128::from(climbed);
    u64::try_from(more).ok().map(|more| at.saturating_add(more))
}

pub(crate) fn report(ctx: &QueryCtx) -> Result<BudgetReport, Error> {
    let time = ctx.env().time;
    let services = PACES
        .all(ctx)?
        .into_iter()
        .map(|(service, pace)| {
            let (last_minute, this_minute) = calls_in_adjacent_minutes(&pace, time);
            ServicePace {
                service,
                last_minute,
                this_minute,
            }
        })
        .collect();
    let accounts = BUDGETS
        .all(ctx)?
        .into_iter()
        .map(|((service, account), row)| outlook(service, account, row, time))
        .collect();
    Ok(BudgetReport {
        time,
        turns: FLIGHTS.all(ctx)?.len() as u64,
        services,
        accounts,
        reads: Vec::new(),
    })
}

pub(crate) fn facts(ctx: &QueryCtx) -> Result<BudgetFactsReport, Error> {
    let budget = report(ctx)?;
    let facts = BUDGET_FACTS
        .all(ctx)?
        .into_iter()
        .map(|((service, account), held)| BudgetFact {
            service,
            account,
            provider: held.provider,
            identity: held.identity,
            previous: held.previous,
            changed: held.changed,
        })
        .collect();
    Ok(BudgetFactsReport { budget, facts })
}

fn outlook(service: String, account: String, row: Budget, time: u64) -> AccountBudget {
    let windows = row
        .windows
        .iter()
        .map(|window| {
            let ended = time >= window.reset.saturating_mul(1000);
            WindowOutlook {
                name: window.name.clone(),
                utilization: window.utilization.clone(),
                used: window.used,
                reset: window.reset,
                status: window.status.clone(),
                since: window.since.time,
                runs_out: runs_out(window, row.time).filter(|_| !ended),
            }
        })
        .collect();
    AccountBudget {
        service,
        account,
        label: row.label,
        organization: row.organization,
        status: row.status,
        fallback: row.fallback,
        error: row.error,
        height: row.height,
        time: row.time,
        windows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(headers: &[(&str, &str)]) -> http::Response {
        http::Response {
            status: 200,
            headers: headers
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            body: Vec::new(),
        }
    }

    fn window(used: u64, since: Seen) -> BudgetWindow {
        BudgetWindow {
            name: "5h".into(),
            utilization: String::new(),
            used,
            reset: 0,
            status: None,
            since,
        }
    }

    #[test]
    fn a_utilization_reads_in_millionths_and_nothing_else_does() {
        let read = [
            ("0.53", Some(530_000)),
            ("1", Some(WHOLE)),
            ("1.0", Some(WHOLE)),
            ("0.1234567", Some(123_456)),
            ("0", Some(0)),
            ("", None),
            (".5", None),
            ("1.", None),
            ("-0.5", None),
            ("5e-1", None),
            ("0.5.1", None),
            ("half", None),
        ];
        for (text, used) in read {
            assert_eq!(millionths(text), used, "{text}");
        }
    }

    #[test]
    fn every_named_window_reads_and_a_malformed_one_is_left_out() {
        let heard = answer(&[
            ("anthropic-ratelimit-unified-5h-utilization", "0.53"),
            ("anthropic-ratelimit-unified-5h-reset", "1792362000"),
            ("anthropic-ratelimit-unified-5h-status", "allowed"),
            ("Anthropic-Ratelimit-Unified-7d-Utilization", " 0.44 "),
            ("anthropic-ratelimit-unified-7d-reset", "1792580400"),
            ("anthropic-ratelimit-unified-reset", "1792362000"),
            ("anthropic-ratelimit-unified-1d-utilization", "lots"),
            ("anthropic-ratelimit-unified-1d-reset", "1792362000"),
            ("anthropic-ratelimit-unified-2d-utilization", "0.1"),
            ("anthropic-ratelimit-unified-2d-reset", "soon"),
        ]);
        let found = merge_budget_windows(None, windows(&heard, 9), 9);
        let read: Vec<_> = found
            .iter()
            .map(|w| (w.name.as_str(), w.used, w.reset, w.status.as_deref()))
            .collect();
        assert_eq!(
            read,
            [
                ("5h", 530_000, 1_792_362_000, Some("allowed")),
                ("7d", 440_000, 1_792_580_400, None)
            ]
        );
        assert_eq!(found[1].utilization, "0.44");
        assert_eq!(
            found[0].since,
            Seen {
                used: 530_000,
                time: 9
            }
        );
        assert!(windows(&answer(&[]), 9).is_empty());
    }

    #[test]
    fn the_harness_words_name_each_window_and_the_providers_own_fill_in_only_what_they_leave_out() {
        let heard = answer(&[
            ("x-harness-window-5h-used", "0.25"),
            ("x-harness-window-5h-reset", "1792362000"),
            ("x-harness-window-5h-status", "allowed"),
            ("anthropic-ratelimit-unified-5h-utilization", "0.9"),
            ("anthropic-ratelimit-unified-5h-reset", "1792362000"),
            ("anthropic-ratelimit-unified-7d-utilization", "0.44"),
            ("anthropic-ratelimit-unified-7d-reset", "1792580400"),
            ("x-harness-window-1d-used", "half"),
            ("x-harness-window-1d-reset", "1792362000"),
        ]);
        let found = merge_budget_windows(None, windows(&heard, 9), 9);
        let read: Vec<_> = found
            .iter()
            .map(|w| {
                let shown = (w.utilization.as_str(), w.used);
                (w.name.as_str(), shown, w.status.as_deref())
            })
            .collect();
        assert_eq!(
            read,
            [
                ("5h", ("0.25", 250_000), Some("allowed")),
                ("7d", ("0.44", 440_000), None)
            ]
        );
    }

    #[test]
    fn a_window_keeps_where_it_was_first_seen_until_its_reset_passes() {
        let first = Seen {
            used: 500_000,
            time: 1_000,
        };
        let held = BudgetWindow {
            reset: 10,
            status: Some("allowed".into()),
            ..window(500_000, first)
        };
        let row = Budget {
            windows: vec![
                held.clone(),
                BudgetWindow {
                    name: "7d".into(),
                    ..held.clone()
                },
            ],
            ..Budget::default()
        };
        let now = Seen {
            used: 490_000,
            time: 5_000,
        };
        let read = BudgetWindow {
            reset: 11,
            ..window(490_000, now)
        };
        let kept = merge_budget_windows(Some(&row), vec![read.clone()], 5_000);
        assert_eq!(kept.len(), 2, "the window the reading left out stays");
        assert_eq!((kept[0].since, kept[0].used), (first, 490_000));
        assert_eq!(kept[0].status.as_deref(), Some("allowed"));
        let renewed = merge_budget_windows(Some(&row), vec![read], 10_000);
        assert_eq!(renewed[0].since, now, "its reset passed");
    }

    #[test]
    fn a_window_runs_out_at_the_pace_it_climbed_at() {
        let since = Seen {
            used: 200_000,
            time: 1_000,
        };
        assert_eq!(runs_out(&window(600_000, since), 5_000), Some(9_000));
        assert_eq!(runs_out(&window(200_000, since), 5_000), None, "flat");
        assert_eq!(runs_out(&window(600_000, since), 1_000), None, "no time");
        assert_eq!(runs_out(&window(WHOLE, since), 5_000), Some(5_000));
        let fell = Seen {
            used: 700_000,
            ..since
        };
        assert_eq!(runs_out(&window(600_000, fell), 5_000), None);
    }

    #[test]
    fn calls_count_by_whole_minute() {
        let pace = Pace {
            minute: 10,
            calls: 4,
            before: 7,
        };
        assert_eq!(calls_in_adjacent_minutes(&pace, 10 * MINUTE + 5), (7, 4));
        assert_eq!(calls_in_adjacent_minutes(&pace, 11 * MINUTE), (4, 0));
        assert_eq!(calls_in_adjacent_minutes(&pace, 12 * MINUTE), (0, 0));
        assert_eq!(calls_in_adjacent_minutes(&pace, 9 * MINUTE), (0, 0));
    }
}
