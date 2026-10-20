use super::*;
use harness::{AccountBudget, BudgetReport, ServicePace, StatusOutcome, StatusRead, WindowOutlook};

use crate::ui::budget::{Level, level};

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

fn report(windows: Vec<WindowOutlook>) -> BudgetReport {
    BudgetReport {
        time: NOW,
        turns: 3,
        services: vec![ServicePace {
            service: "anthropic".into(),
            last_minute: 12,
            this_minute: 2,
        }],
        accounts: vec![AccountBudget {
            service: "anthropic".into(),
            account: "org-a".into(),
            label: Some("1".into()),
            organization: Some("org-a".into()),
            status: Some("allowed".into()),
            fallback: None,
            error: None,
            height: 9000,
            time: NOW,
            windows,
        }],
        reads: Vec::new(),
    }
}

fn fill(cx: &TestAppContext, window: &str) -> Option<Hsla> {
    cx.style(&format!("budget-anthropic-org-a-{window}-fill"))
        .background
        .as_ref()
        .and_then(|fill| fill.color())
        .and_then(|colour| colour.as_solid())
}

fn starts(cx: &TestAppContext, text: &str) -> bool {
    cx.texts().iter().any(|shown| shown.starts_with(text))
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
fn the_strip_shows_each_window_its_reset_and_when_it_runs_out() {
    let hour = NOW / 1000 + 3_600;
    let net = Net {
        budget: report(vec![
            outlook("5h", 530_000, hour, Some(NOW + 1_800_000)),
            outlook("7d", 850_000, hour, Some(NOW + 7_200_000)),
            outlook("1d", 960_000, hour, None),
            outlook("2d", 990_000, NOW / 1000 - 60, None),
        ]),
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    let calls = "Model budget · 12 calls in the last minute · 3 turns in flight";
    for text in [
        calls,
        "Account 1 · anthropic",
        "Status allowed",
        "No climb seen yet",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    for text in [
        "53% of the 5-hour window · resets ",
        "85% of the weekly window · resets ",
        "96% of the 1d window · resets ",
        "2d window · reset at ",
        "At this pace it runs out at ",
        "At this pace it lasts until it resets",
    ] {
        assert!(starts(&cx, text), "{text}: {:?}", cx.texts());
    }
    let theme = view::Theme::default();
    assert_eq!(fill(&cx, "5h"), Some(theme.accent));
    assert_eq!(fill(&cx, "7d"), Some(theme.warning));
    assert_eq!(fill(&cx, "1d"), Some(theme.danger));
    assert_eq!(fill(&cx, "2d"), Some(theme.accent), "reset since: empty");
}

#[test]
fn a_budget_write_reads_the_budget_again_on_the_phone_too() {
    let (mut cx, shared) = seated(Net::default(), EDDY);
    let none = "No model answer has named its usage windows yet.";
    assert!(cx.has_text(none), "{:?}", cx.texts());
    cx.simulate_touch(true);
    cx.run_until_parked();
    assert!(cx.has_text(none), "{:?}", cx.texts());
    let hour = NOW / 1000 + 3_600;
    shared.borrow_mut().budget = report(vec![outlook("5h", 970_000, hour, None)]);
    let changes = cx.host().stream::<Changes<harness::Harness>>();
    changes.send(Some(Change {
        height: 10,
        keys: vec![Written::Budget.key()],
    }));
    cx.run_until_parked();
    assert!(starts(&cx, "97% of the 5-hour window"), "{:?}", cx.texts());
    assert_eq!(fill(&cx, "5h"), Some(view::Theme::default().danger));
}

fn read(outcome: StatusOutcome) -> StatusRead {
    StatusRead {
        service: "anthropic".into(),
        height: 9001,
        time: NOW,
        outcome,
    }
}

#[test]
fn a_status_with_none_to_read_is_said_quietly_and_an_account_ccs_could_not_read_says_so() {
    let mut budget = report(Vec::new());
    budget.reads = vec![read(StatusOutcome::Unavailable)];
    budget.accounts[0].fallback = Some("available".into());
    budget.accounts[0].error = Some("signed_out".into());
    let net = Net {
        budget,
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    for text in [
        "anthropic status unavailable",
        "Status allowed · fallback available",
        "ccs could not read this account: signed out",
    ] {
        assert!(cx.has_text(text), "{text}: {:?}", cx.texts());
    }
    let mut budget = report(Vec::new());
    budget.reads = vec![read(StatusOutcome::Read { accounts: 2 })];
    let net = Net {
        budget,
        ..Net::default()
    };
    let (cx, _) = seated(net, EDDY);
    assert!(starts(&cx, "anthropic status read at "), "{:?}", cx.texts());
}

#[test]
fn a_refresh_press_reads_each_services_status_and_nothing_reads_it_unasked() {
    let (mut cx, _) = seated(
        Net {
            budget: report(Vec::new()),
            ..Net::default()
        },
        EDDY,
    );
    cx.host()
        .handle::<Submit<harness::Harness>>(|_| Ok(applied(Vec::new())));
    assert!(cx.host().requests::<Submit<harness::Harness>>().is_empty());
    cx.simulate_click("budget-refresh");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<harness::Harness>>(),
        [harness::Op::ReadBudget {
            service: "anthropic".into()
        }]
    );
    let (cx, _) = seated(Net::default(), EDDY);
    assert!(
        !cx.has_text("Refresh"),
        "no service named yet: {:?}",
        cx.texts()
    );
}
