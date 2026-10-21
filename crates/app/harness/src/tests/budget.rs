//! The budget row each model answer writes, read from the answer's own words for its account and
//! its usage windows, with each service's pace and when each window runs out.
use super::*;

fn budget(chain: &MockChain) -> BudgetReport {
    let Reply::Budget(report) = chain.query(PROGRAM, &Query::Budget).unwrap() else {
        panic!("the budget answers as one")
    };
    report
}

fn answered(chain: &MockChain, account: &str, used: &str, reset: &str) {
    let (request, _) = open_request(chain);
    let body = json!({
        "id": "msg_1",
        "type": "message",
        "role": "assistant",
        "model": "claude-test",
        "content": [{ "type": "text", "text": "hello back" }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    });
    let headers = [
        ("x-harness-account", account),
        ("x-harness-window-5h-used", used),
        ("x-harness-window-5h-reset", reset),
        ("x-harness-window-5h-status", "allowed"),
        ("x-harness-status", "allowed"),
    ];
    let response = http::Response {
        status: 200,
        headers: headers
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
        body: body.to_string().into_bytes(),
    };
    answer(chain, request.id, Answer::Output(abi::encode(&response)));
}

fn account<'a>(report: &'a BudgetReport, account: &str) -> &'a AccountBudget {
    let found = report.accounts.iter().find(|row| row.account == account);
    found.expect("the account has a row")
}

#[test]
fn each_account_keeps_one_row_with_its_pace_and_when_it_runs_out() {
    let world = world();
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    let learned = budget(chain);
    assert!(learned.accounts.is_empty(), "an answer naming no window");
    let [pace]: [ServicePace; 1] = learned.services.try_into().unwrap();
    assert_eq!(
        (pace.service.as_str(), pace.last_minute, pace.this_minute),
        (SERVICE, 0, 1)
    );
    assert_eq!(learned.turns, 1, "the turn's model call is out");
    chain.at(3, 60_000);
    answered(chain, "a", "0.2", "1000");
    assert_eq!(budget(chain).turns, 0);
    chain.at(4, 100_000);
    world.mention("m2", "again");
    chain.at(5, 120_000);
    answered(chain, "b", "0.5", "1000");
    chain.at(6, 150_000);
    world.mention("m3", "and again");
    chain.at(7, 180_000);
    answered(chain, "a", "0.4", "1000");
    let report = budget(chain);
    assert_eq!(report.accounts.len(), 2, "one row per account");
    assert!(
        report.reads.is_empty(),
        "no status read: {:?}",
        report.reads
    );
    let a = account(&report, "a");
    assert_eq!(
        (a.label.as_deref(), a.organization.as_deref()),
        (Some("a"), None)
    );
    assert_eq!(
        (a.status.as_deref(), a.fallback.as_deref()),
        (Some("allowed"), None)
    );
    assert_eq!((a.height, a.time), (7, 180_000));
    let [five]: [WindowOutlook; 1] = a.windows.clone().try_into().unwrap();
    assert_eq!(
        (
            five.name.as_str(),
            five.utilization.as_str(),
            five.used,
            five.reset,
            five.status.as_deref()
        ),
        ("5h", "0.4", 400_000, 1000, Some("allowed"))
    );
    assert_eq!((five.since, five.runs_out), (60_000, Some(540_000)));
    let b = account(&report, "b");
    assert_eq!(b.windows[0].runs_out, None, "one answer is no pace");
    let [pace]: [ServicePace; 1] = report.services.try_into().unwrap();
    assert_eq!((pace.last_minute, pace.this_minute), (1, 1));
    chain.at(8, 1_000_000);
    let ended = budget(chain);
    assert_eq!(
        account(&ended, "a").windows[0].runs_out,
        None,
        "reset since"
    );
    assert_eq!(
        (ended.services[0].last_minute, ended.services[0].this_minute),
        (0, 0)
    );
    world.mention("m4", "a new window");
    chain.at(9, 1_010_000);
    answered(chain, "a", "0.1", "2000");
    let renewed = budget(chain);
    let five = &account(&renewed, "a").windows[0];
    assert_eq!(
        (five.used, five.since, five.runs_out),
        (100_000, 1_010_000, None)
    );
    assert_eq!(written(&Written::Budget.key()), Some(Written::Budget));
}
