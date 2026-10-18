use super::*;

fn budget(chain: &MockChain) -> BudgetReport {
    let Reply::Budget(report) = chain.query(PROGRAM, &Query::Budget).unwrap() else {
        panic!("the budget answers as one")
    };
    report
}

// the open model call answered as `account` would, named by ccs as `label`, its 5-hour window at
// `used` until `reset`
fn answered(chain: &MockChain, label: Option<&str>, account: &str, used: &str, reset: &str) {
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
    let mut headers = vec![
        ("anthropic-organization-id", account),
        ("anthropic-ratelimit-unified-5h-utilization", used),
        ("anthropic-ratelimit-unified-5h-reset", reset),
        ("anthropic-ratelimit-unified-5h-status", "allowed"),
        ("anthropic-ratelimit-unified-status", "allowed"),
        ("anthropic-ratelimit-unified-fallback", "available"),
    ];
    headers.extend(label.map(|label| ("x-ccs-account", label)));
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
    answered(chain, None, "org-a", "0.2", "1000");
    assert_eq!(budget(chain).turns, 0);
    chain.at(4, 100_000);
    world.mention("m2", "again");
    chain.at(5, 120_000);
    answered(chain, None, "org-b", "0.5", "1000");
    chain.at(6, 150_000);
    world.mention("m3", "and again");
    chain.at(7, 180_000);
    answered(chain, None, "org-a", "0.4", "1000");
    let report = budget(chain);
    assert_eq!(report.accounts.len(), 2, "one row per account");
    let a = account(&report, "org-a");
    assert_eq!(
        (a.label.as_deref(), a.organization.as_deref()),
        (None, Some("org-a"))
    );
    assert_eq!(
        (a.status.as_deref(), a.fallback.as_deref()),
        (Some("allowed"), Some("available"))
    );
    assert_eq!((a.height, a.time), (7, 180_000));
    let [five]: [WindowOutlook; 1] = a.windows.clone().try_into().unwrap();
    assert_eq!(
        (
            five.name.as_str(),
            five.utilization.as_str(),
            five.used,
            five.reset
        ),
        ("5h", "0.4", 400_000, 1000)
    );
    // 0.2 → 0.4 from 60 s to 180 s: the last 0.6 takes 360 s more
    assert_eq!((five.since, five.runs_out), (60_000, Some(540_000)));
    let b = account(&report, "org-b");
    assert_eq!(b.windows[0].runs_out, None, "one answer is no pace");
    let [pace]: [ServicePace; 1] = report.services.try_into().unwrap();
    assert_eq!((pace.last_minute, pace.this_minute), (1, 1));
    chain.at(8, 1_000_000);
    let ended = budget(chain);
    assert_eq!(
        account(&ended, "org-a").windows[0].runs_out,
        None,
        "reset since"
    );
    assert_eq!(
        (ended.services[0].last_minute, ended.services[0].this_minute),
        (0, 0)
    );
    world.mention("m4", "a new window");
    chain.at(9, 1_010_000);
    answered(chain, None, "org-a", "0.1", "2000");
    let renewed = budget(chain);
    let five = &account(&renewed, "org-a").windows[0];
    assert_eq!(
        (five.used, five.since, five.runs_out),
        (100_000, 1_010_000, None)
    );
    assert_eq!(written(&Written::Budget.key()), Some(Written::Budget));
}

// ccs's status: one account read whole, one it could not read
fn status() -> Vec<u8> {
    let at = "2000-03-01T00:00:00Z";
    json!({
        "accounts": [
            {
                "label": "anthropic3-eph",
                "provider": "anthropic",
                "read_at": at,
                "five_hour": { "percent": 53.5, "resets_at": at },
                "weekly": { "percent": 12, "resets_at": "2000-03-01T02:00:00+02:00" },
                "models": [
                    { "model": "opus", "kind": "weekly", "percent": 40, "resets_at": at }
                ],
                "error": null
            },
            {
                "label": "jesse.claude.ext.5",
                "provider": "anthropic",
                "read_at": at,
                "five_hour": null,
                "weekly": null,
                "models": [],
                "error": "signed_out"
            }
        ]
    })
    .to_string()
    .into_bytes()
}

fn read_status(chain: &MockChain, height: u64, time: u64, answered: Answer) -> StatusRead {
    chain.at(height, time);
    let op = Op::ReadBudget {
        service: SERVICE.into(),
    };
    chain.submit(signed(ADA), PROGRAM, &op).unwrap();
    let (request, head) = open_request(chain);
    assert_eq!(
        (head.method, head.target.as_str(), head.headers.len()),
        (http::Method::Get, "/ccs/status", 0)
    );
    chain.at(height + 1, time + 10_000);
    answer(chain, request.id, answered);
    let [read]: [StatusRead; 1] = budget(chain).reads.try_into().unwrap();
    assert_eq!(
        (read.service.as_str(), read.height, read.time),
        (SERVICE, height + 1, time + 10_000)
    );
    read
}

#[test]
fn a_status_read_fills_every_account_and_a_404_is_only_unavailable() {
    let world = world();
    let chain = &world.chain;
    world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 50_000);
    answered(chain, None, "org-a", "0.2", "1000");
    let missing = response(404, json!({ "error": "not found" }));
    let read = read_status(chain, 4, 100_000, Answer::Output(missing));
    assert_eq!(read.outcome, StatusOutcome::Unavailable);
    assert_eq!(budget(chain).accounts.len(), 1, "a 404 writes no account");
    let lost = read_status(chain, 6, 120_000, Answer::Failed("no route".into()));
    assert!(matches!(lost.outcome, StatusOutcome::Failed(_)), "{lost:?}");
    let ok = http::Response {
        status: 200,
        headers: Vec::new(),
        body: status(),
    };
    let read = read_status(chain, 8, 190_000, Answer::Output(abi::encode(&ok)));
    assert_eq!(read.outcome, StatusOutcome::Read { accounts: 2 });
    let report = budget(chain);
    assert_eq!(
        report.services[0].this_minute, 0,
        "a status read is no model call"
    );
    assert_eq!(report.accounts.len(), 3);
    let eph = account(&report, "anthropic3-eph");
    assert_eq!(
        (
            eph.label.as_deref(),
            eph.organization.as_deref(),
            eph.height
        ),
        (Some("anthropic3-eph"), None, 9)
    );
    let read: Vec<_> = eph
        .windows
        .iter()
        .map(|w| (w.name.as_str(), w.utilization.as_str(), w.used, w.reset))
        .collect();
    assert_eq!(
        read,
        [
            ("5h", "0.535", 535_000, 951_868_800),
            ("7d", "0.12", 120_000, 951_868_800),
            ("opus weekly", "0.4", 400_000, 951_868_800)
        ]
    );
    let signed_out = account(&report, "jesse.claude.ext.5");
    assert_eq!(signed_out.error.as_deref(), Some("signed_out"));
    assert!(signed_out.windows.is_empty());
    // the account answers once ccs names it: its row is the one ccs's status filled, and the row
    // the model's id kept goes
    chain.at(10, 250_000);
    world.mention("m2", "again");
    chain.at(11, 300_000);
    answered(chain, Some("anthropic3-eph"), "org-a", "0.54", "951868800");
    let report = budget(chain);
    assert_eq!(report.accounts.len(), 2, "{:?}", report.accounts);
    let eph = account(&report, "anthropic3-eph");
    assert_eq!(
        (eph.organization.as_deref(), eph.error.as_deref()),
        (Some("org-a"), None)
    );
    let names: Vec<_> = eph.windows.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["5h", "7d", "opus weekly"]);
    let five = &eph.windows[0];
    // 0.535 → 0.54 from 200 s to 300 s: the last 0.46 takes 9 200 s more
    assert_eq!(
        (five.used, five.since, five.status.as_deref()),
        (540_000, 200_000, Some("allowed"))
    );
    assert_eq!(five.runs_out, Some(9_500_000));
    let status_key = crate::state::READS.key(&SERVICE.to_owned());
    assert_eq!(written(&status_key), Some(Written::Budget));
}
