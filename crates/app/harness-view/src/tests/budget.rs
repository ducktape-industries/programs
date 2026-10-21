//! The budget strip: one row per account from its freshest read, each window's bar with its share
//! and the time to its reset, the account's state, and the budget read again on every budget write.
use super::*;
use harness::{AccountBudget, BudgetReport, ServicePace, WindowOutlook};

use crate::ui::budget::{AccountState, Level, account_state, answering, freshest, level};
use crate::words::span;

const MINUTE: u64 = 60_000;

fn outlook(name: &str, used: u64, reset: u64, runs_out: Option<u64>) -> WindowOutlook {
    WindowOutlook {
        name: name.into(),
        utilization: format!("{}", used as f64 / 1e6),
        used,
        reset,
        status: Some("allowed".into()),
        since: NOW - 600_000,
        runs_out,
    }
}

fn row(service: &str, account: &str, time: u64, windows: Vec<WindowOutlook>) -> AccountBudget {
    AccountBudget {
        service: service.into(),
        account: account.into(),
        label: Some(account.into()),
        organization: None,
        status: Some("allowed".into()),
        fallback: None,
        error: None,
        height: 9000,
        time,
        windows,
    }
}

fn report(accounts: Vec<AccountBudget>) -> BudgetReport {
    BudgetReport {
        time: NOW,
        turns: 3,
        services: vec![ServicePace {
            service: "models".into(),
            last_minute: 12,
            this_minute: 2,
        }],
        accounts,
        reads: Vec::new(),
    }
}

fn one(windows: Vec<WindowOutlook>) -> BudgetReport {
    report(vec![row("models", "work", NOW, windows)])
}

fn at(minutes: u64) -> u64 {
    (NOW + minutes * MINUTE) / 1000
}

fn fill(cx: &TestAppContext, account: &str, window: &str) -> Option<Hsla> {
    cx.style(&format!("budget-{account}-{window}-fill"))
        .background
        .as_ref()
        .and_then(|fill| fill.color())
        .and_then(|colour| colour.as_solid())
}

#[test]
fn a_window_turns_amber_at_80_and_red_at_95() {
    let read = [
        (0, Level::Calm),
        (799_999, Level::Calm),
        (800_000, Level::Amber),
        (949_999, Level::Amber),
        (950_000, Level::Red),
        (1_200_000, Level::Red),
    ];
    for (used, shown) in read {
        assert_eq!(level(used), shown, "{used}");
    }
}

#[test]
fn a_time_to_reset_reads_in_its_two_largest_units() {
    let read = [
        (0, "0m"),
        (59_999, "0m"),
        (12 * MINUTE, "12m"),
        (217 * MINUTE, "3h 37m"),
        ((4 * 24 * 60 + 14 * 60 + 5) * MINUTE, "4d 14h"),
    ];
    for (millis, shown) in read {
        assert_eq!(span(millis), shown, "{millis}");
    }
}

#[test]
fn each_window_shows_its_share_its_time_to_reset_and_a_short_run_out() {
    let net = Net {
        budget: one(vec![
            outlook("5h", 530_000, at(217), Some(NOW + 30 * MINUTE)),
            outlook("7d", 850_000, at(6_600), Some(NOW + 9_000 * MINUTE)),
            outlook("Fable weekly_scoped", 960_000, at(60), None),
            outlook("2d", 990_000, NOW / 1000 - 60, None),
        ]),
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    let calls = "Model budget · 1 account · 12 calls in the last minute · 3 turns in flight";
    for text in [
        calls,
        "work",
        "via models",
        "session",
        "53%",
        "3h 37m",
        "out in 30m",
        "weekly",
        "85%",
        "4d 14h",
        "Fable",
        "96%",
        "1h 0m",
        "2d",
        "0%",
        "reset",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    let out_after_reset = cx.texts().iter().filter(|t| t.starts_with("out ")).count();
    assert_eq!(
        out_after_reset,
        1,
        "only the 5h runs out first: {:?}",
        cx.texts()
    );
    let theme = view::Theme::default();
    assert_eq!(fill(&cx, "work", "5h"), Some(theme.accent));
    assert_eq!(fill(&cx, "work", "7d"), Some(theme.warning));
    assert_eq!(fill(&cx, "work", "Fable weekly_scoped"), Some(theme.danger));
    assert_eq!(
        fill(&cx, "work", "2d"),
        Some(theme.accent),
        "reset since: empty"
    );
}

#[test]
fn an_account_several_services_report_shows_once_from_its_freshest_read() {
    let stale = row(
        "anthropic",
        "work",
        NOW - 600 * MINUTE,
        vec![outlook("5h", 270_000, at(100), None)],
    );
    let fresh = row(
        "models",
        "work",
        NOW - MINUTE,
        vec![outlook("5h", 640_000, at(100), None)],
    );
    let other = row(
        "anthropic",
        "home",
        NOW - 600 * MINUTE,
        vec![outlook("5h", 110_000, at(100), None)],
    );
    let budget = report(vec![stale, other, fresh]);
    let kept: Vec<(&str, &str)> = freshest(&budget.accounts)
        .iter()
        .map(|row| (row.account.as_str(), row.service.as_str()))
        .collect();
    assert_eq!(kept, [("work", "models"), ("home", "anthropic")]);
    let net = Net {
        budget,
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    for text in ["64%", "11%", "via models", "via anthropic", "2 accounts"] {
        let shown = cx.texts().iter().any(|t| t.contains(text));
        assert!(shown, "{text}: {:?}", cx.texts());
    }
    assert!(
        !cx.has_text("27%"),
        "the stale read is left out: {:?}",
        cx.texts()
    );
}

#[test]
fn an_account_is_spent_while_a_window_is_full_and_active_when_it_answered_last() {
    let fine = row(
        "models",
        "work",
        NOW,
        vec![outlook("5h", 100_000, at(60), None)],
    );
    assert_eq!(account_state(&fine, true, NOW), AccountState::Active);
    assert_eq!(account_state(&fine, false, NOW), AccountState::Usable);
    let full = row(
        "models",
        "work",
        NOW,
        vec![outlook("7d", 1_000_000, at(60), None)],
    );
    assert_eq!(account_state(&full, true, NOW), AccountState::Spent);
    let renewed = NOW + 61 * MINUTE;
    assert_eq!(account_state(&full, false, renewed), AccountState::Usable);
    let mut refused = row("models", "work", NOW, vec![outlook("5h", 0, at(60), None)]);
    refused.status = Some("rejected".into());
    assert_eq!(account_state(&refused, false, NOW), AccountState::Spent);
    assert_eq!(
        account_state(&refused, false, renewed),
        AccountState::Usable
    );
}

#[test]
fn each_row_says_whether_its_account_is_active_usable_or_spent() {
    let budget = report(vec![
        row(
            "models",
            "zk",
            NOW,
            vec![outlook("5h", 200_000, at(60), None)],
        ),
        row(
            "models",
            "spare",
            NOW - MINUTE,
            vec![outlook("7d", 10_000, at(60), None)],
        ),
        row(
            "models",
            "out",
            NOW - 2 * MINUTE,
            vec![outlook("7d", 1_000_000, at(60), None)],
        ),
    ]);
    let net = Net {
        budget,
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    for text in ["Active", "Usable", "Spent"] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
}

#[test]
fn each_service_names_its_own_active_account() {
    let budget = report(vec![
        row(
            "zk-claude",
            "zk",
            NOW,
            vec![outlook("5h", 200_000, at(60), None)],
        ),
        row(
            "models",
            "work",
            NOW - MINUTE,
            vec![outlook("5h", 300_000, at(60), None)],
        ),
        row(
            "models",
            "spare",
            NOW - 2 * MINUTE,
            vec![outlook("7d", 10_000, at(60), None)],
        ),
        row(
            "models",
            "out",
            NOW + MINUTE,
            vec![outlook("7d", 1_000_000, at(60), None)],
        ),
    ]);
    let kept = freshest(&budget.accounts);
    assert_eq!(answering(&kept, NOW), ["zk", "work"]);
    let net = Net {
        budget,
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    let shown = cx.texts();
    let count = |word: &str| shown.iter().filter(|text| *text == word).count();
    assert_eq!(count("Active"), 2, "{shown:?}");
    assert_eq!(count("Usable"), 1, "{shown:?}");
    assert_eq!(count("Spent"), 1, "{shown:?}");
}

#[test]
fn a_budget_write_reads_the_budget_again_on_the_phone_too() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    let none = "No model answer has named its usage windows yet.";
    assert!(cx.has_text(none), "{:?}", cx.texts());
    cx.simulate_touch(true);
    cx.run_until_parked();
    assert!(cx.has_text(none), "{:?}", cx.texts());
    shared.borrow_mut().budget = one(vec![outlook("5h", 970_000, at(60), None)]);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    changes.send(Some(Change {
        height: 10,
        keys: vec![Written::Budget.key()],
    }));
    cx.run_until_parked();
    assert!(cx.has_text("97%"), "{:?}", cx.texts());
    assert_eq!(fill(&cx, "work", "5h"), Some(view::Theme::default().danger));
}

#[test]
fn the_strip_names_no_service_error_and_asks_its_service_for_nothing() {
    let mut budget = one(Vec::new());
    budget.accounts[0].fallback = Some("available".into());
    budget.accounts[0].error = Some("signed_out".into());
    let net = Net {
        budget,
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    let shown = cx.texts();
    assert!(cx.has_text("work"), "{shown:?}");
    let named_the_error = shown.iter().any(|text| text.contains("signed out"));
    assert!(!named_the_error, "{shown:?}");
    assert!(!cx.has_text("Refresh"), "{shown:?}");
    assert!(cx.host().requests::<Submit<harness::Harness>>().is_empty());
}
