//! The model budget: each model account's usage windows, from its answers and from its service's
//! own status, how often harness called each service in the last whole minute, and when each
//! window runs out at the pace it climbed at.
use ::program::{Ctx, Error, ExecCtx, QueryCtx, all};
use abi::role::egress::{self, http};
use serde::Deserialize;

use crate::calls::refusal;
use crate::resume::header;
use crate::state::{BUDGETS, Budget, BudgetWindow, FLIGHTS, MODELS, PACES, Pace, READS, Seen};
use crate::{
    AccountBudget, BudgetReport, ServicePace, StatusOutcome, StatusRead, WHOLE_WINDOW as WHOLE,
    WindowOutlook,
};

const PREFIX: &str = "anthropic-ratelimit-unified-";
const UTILIZATION: &str = "-utilization";
// the account that answered, as Anthropic names it
pub(crate) const ORGANIZATION: &str = "anthropic-organization-id";
// ccs's own name for that account; sent on a request it pins the request to that account, with no
// fall-over, which is how harness picks the account each model call goes to (`routes.rs`)
pub(crate) const CCS_ACCOUNT: &str = "x-ccs-account";
// where ccs answers the windows of every account it holds
const STATUS: &str = "/ccs/status";
const MINUTE: u64 = 60_000;

// a utilization as the headers send it ("0.53"), in millionths; none for anything else
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

// a percent as ccs sends it ("53" or "53.5"), in millionths of the whole
fn from_percent(text: &str) -> Option<u64> {
    millionths(text).map(|hundredths| hundredths / 100)
}

// millionths written as the headers write a utilization ("0.535")
fn fraction(used: u64) -> String {
    let (whole, part) = (used / WHOLE, used % WHOLE);
    match part {
        0 => whole.to_string(),
        _ => format!("{whole}.{part:06}")
            .trim_end_matches('0')
            .to_owned(),
    }
}

fn digits(text: &str, from: usize, len: usize) -> Option<i64> {
    let part = text.get(from..from + len)?;
    match part.bytes().all(|byte| byte.is_ascii_digit()) {
        true => part.parse().ok(),
        false => None,
    }
}

fn days_in(year: i64, month: i64) -> i64 {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

// days from 1970-01-01 to a date of the proleptic Gregorian calendar
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

// an RFC 3339 time ("2026-10-18T23:00:00Z", "2026-10-18T23:00:00.5+02:00") in whole seconds
// since the epoch; none for anything else or for a time before the epoch
pub(crate) fn epoch_seconds(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    let at = |index: usize, byte: u8| bytes.get(index) == Some(&byte);
    let parted = at(4, b'-') && at(7, b'-') && at(13, b':') && at(16, b':');
    if !parted || !matches!(bytes.get(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let (year, month, day) = (
        digits(text, 0, 4)?,
        digits(text, 5, 2)?,
        digits(text, 8, 2)?,
    );
    let (hour, minute) = (digits(text, 11, 2)?, digits(text, 14, 2)?);
    let second = digits(text, 17, 2)?;
    let dated = (1..=12).contains(&month) && (1..=days_in(year, month)).contains(&day);
    if !dated || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = text.get(19..)?;
    if let Some(fraction) = rest.strip_prefix('.') {
        let read = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if read == 0 {
            return None;
        }
        rest = &fraction[read..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first() {
                Some(b'+') => 1,
                Some(b'-') => -1,
                _ => return None,
            };
            let (hours, minutes) = (digits(rest, 1, 2)?, digits(rest, 4, 2)?);
            if rest.len() != 6 || !rest[3..].starts_with(':') || hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3_600 + minutes * 60)
        }
    };
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset;
    u64::try_from(seconds).ok()
}

// every window an answer names whose utilization and reset both read, by name
fn windows(response: &http::Response, time: u64) -> Vec<BudgetWindow> {
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

// the windows a reading names over the ones the row held, by name: each keeps where harness first
// saw it until its reset passes, and its status while the reading gives none; the windows the
// reading does not name stay as they were
fn merged(before: Option<&Budget>, mut windows: Vec<BudgetWindow>, time: u64) -> Vec<BudgetWindow> {
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

// an answer from `service`: one more call this minute, and the windows it names on its account's
// row, kept under ccs's name for the account when the answer gives one, else under the model's
pub(crate) fn seen(ctx: &ExecCtx, service: &str, response: &http::Response) {
    let env = ctx.env();
    paced(ctx, service, env.time);
    let windows = windows(response, env.time);
    if windows.is_empty() {
        return;
    }
    let organization = header(response, ORGANIZATION).filter(|id| !id.is_empty());
    let label = header(response, CCS_ACCOUNT).filter(|label| !label.is_empty());
    let account = label.or(organization).unwrap_or_default();
    let key = (service.to_owned(), account.to_owned());
    // a row that no longer reads is written anew
    let before = BUDGETS.get(ctx, &key).ok().flatten();
    // the row the account had before ccs named it goes: one account, one row
    if let (Some(_), Some(organization)) = (label, organization) {
        let unnamed = (service.to_owned(), organization.to_owned());
        if unnamed != key {
            BUDGETS.remove(ctx, &unnamed);
        }
    }
    let row = Budget {
        label: label.map(str::to_owned),
        organization: organization.map(str::to_owned),
        status: header(response, &format!("{PREFIX}status")).map(str::to_owned),
        fallback: header(response, &format!("{PREFIX}fallback")).map(str::to_owned),
        error: None,
        windows: merged(before.as_ref(), windows, env.time),
        height: env.height,
        time: env.time,
    };
    BUDGETS.put(ctx, &key, &row);
}

// ccs's status of every account it holds, idle ones too, read when someone asks (chief's
// heartbeat, the strip's Refresh). A service with no status (404) is noted as such, quietly.
pub(crate) async fn read(ctx: Ctx, service: String) -> Result<(), Error> {
    read_one(&ctx, service).await;
    Ok(())
}

// every service a model is reached through, each read as `read` reads one, at once (J-090)
pub(crate) async fn read_all(ctx: Ctx) -> Result<(), Error> {
    let mut services: Vec<String> = Vec::new();
    for (id, model) in MODELS.all(&ctx)? {
        for service in crate::routes::services_of(&ctx, &id, &model.service)? {
            if !services.contains(&service) {
                services.push(service);
            }
        }
    }
    all(services.into_iter().map(|service| read_one(&ctx, service))).await;
    Ok(())
}

async fn read_one(ctx: &Ctx, service: String) {
    let head = http::Head {
        method: http::Method::Get,
        target: STATUS.to_owned(),
        headers: Vec::new(),
    };
    let request = egress::Request::Http {
        service: service.clone(),
        head,
        body: Vec::new(),
    };
    let answer = crate::turns::request(ctx, request, egress::Delivery::Whole, None).await;
    let outcome = match answer {
        Err(error) => StatusOutcome::Failed(refusal(&error)),
        Ok(bytes) => match abi::decode::<http::Response>(&bytes) {
            Ok(response) => status_read(ctx, &service, &response),
            Err(_) => StatusOutcome::Failed("the egress answer is not an HTTP response".into()),
        },
    };
    let env = ctx.env();
    let read = StatusRead {
        service,
        height: env.height,
        time: env.time,
        outcome,
    };
    READS.put(ctx, &read.service, &read);
}

#[derive(Deserialize)]
struct Status {
    accounts: Vec<Reading>,
}

#[derive(Deserialize)]
struct Reading {
    label: String,
    five_hour: Option<Usage>,
    weekly: Option<Usage>,
    models: Option<Vec<ModelUsage>>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct Usage {
    percent: Option<serde_json::Number>,
    resets_at: Option<String>,
}

#[derive(Deserialize)]
struct ModelUsage {
    model: String,
    kind: Option<String>,
    #[serde(flatten)]
    usage: Usage,
}

fn status_read(ctx: &ExecCtx, service: &str, response: &http::Response) -> StatusOutcome {
    match response.status {
        200 => {}
        404 => return StatusOutcome::Unavailable,
        status => return StatusOutcome::Failed(format!("the service answered HTTP {status}")),
    }
    let Ok(status) = serde_json::from_slice::<Status>(&response.body) else {
        return StatusOutcome::Failed("the answer is not ccs's status".into());
    };
    let env = ctx.env();
    for reading in &status.accounts {
        let key = (service.to_owned(), reading.label.clone());
        let before = BUDGETS.get(ctx, &key).ok().flatten();
        let windows = merged(before.as_ref(), read_windows(reading, env.time), env.time);
        let row = Budget {
            label: Some(reading.label.clone()),
            error: reading.error.clone(),
            windows,
            height: env.height,
            time: env.time,
            ..before.unwrap_or_default()
        };
        BUDGETS.put(ctx, &key, &row);
        crate::routes::read(ctx, service, &reading.label, &row);
    }
    StatusOutcome::Read {
        accounts: status.accounts.len() as u64,
    }
}

// the windows ccs read for an account, named as the headers name them (5h, 7d), and each model's
// own window by its model and kind
fn read_windows(reading: &Reading, time: u64) -> Vec<BudgetWindow> {
    let named = [("5h", &reading.five_hour), ("7d", &reading.weekly)];
    let mut found: Vec<BudgetWindow> = named
        .into_iter()
        .filter_map(|(name, usage)| window_of(name.to_owned(), usage.as_ref()?, time))
        .collect();
    for model in reading.models.iter().flatten() {
        let name = match model.kind.as_deref() {
            Some(kind) if !kind.is_empty() => format!("{} {kind}", model.model),
            _ => model.model.clone(),
        };
        if found.iter().any(|window| window.name == name) {
            continue;
        }
        found.extend(window_of(name, &model.usage, time));
    }
    found
}

fn window_of(name: String, usage: &Usage, time: u64) -> Option<BudgetWindow> {
    let used = from_percent(&usage.percent.as_ref()?.to_string())?;
    let reset = epoch_seconds(usage.resets_at.as_deref()?)?;
    Some(BudgetWindow {
        name,
        utilization: fraction(used),
        used,
        reset,
        status: None,
        since: Seen { used, time },
    })
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

// the calls in the last whole minute before `time`, and in the minute `time` is in so far
fn calls(pace: &Pace, time: u64) -> (u64, u64) {
    let minute = time / MINUTE;
    match minute.checked_sub(pace.minute) {
        Some(0) => (pace.before, pace.calls),
        Some(1) => (pace.calls, 0),
        _ => (0, 0),
    }
}

// when `window` reaches all of its budget at the pace it climbed at from where harness first saw
// it to `at`, the last answer's time; none while it has not climbed
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
            let (last_minute, this_minute) = calls(&pace, time);
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
    let reads = READS.all(ctx)?.into_iter().map(|(_, read)| read).collect();
    Ok(BudgetReport {
        time,
        turns: FLIGHTS.all(ctx)?.len() as u64,
        services,
        accounts,
        reads,
    })
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
    fn a_percent_reads_in_millionths_and_writes_back_as_a_fraction() {
        let read = [
            ("53", Some(530_000)),
            ("53.5", Some(535_000)),
            ("100", Some(WHOLE)),
            ("0", Some(0)),
            ("5.3e1", None),
            ("-1", None),
        ];
        for (text, used) in read {
            assert_eq!(from_percent(text), used, "{text}");
        }
        let written = [(535_000, "0.535"), (WHOLE, "1"), (0, "0"), (1, "0.000001")];
        for (used, text) in written {
            assert_eq!(fraction(used), text, "{used}");
        }
    }

    #[test]
    fn an_rfc_3339_time_reads_in_seconds_since_the_epoch() {
        let read = [
            ("1970-01-01T00:00:00Z", Some(0)),
            ("2000-03-01T00:00:00Z", Some(951_868_800)),
            ("2000-03-01t00:00:00z", Some(951_868_800)),
            ("2000-03-01 02:00:00+02:00", Some(951_868_800)),
            ("2000-02-29T23:00:00.999-01:00", Some(951_868_800)),
            ("2000-02-30T00:00:00Z", None),
            ("1900-02-29T00:00:00Z", None),
            ("2000-03-01T24:00:00Z", None),
            ("2000-03-01T00:00:00", None),
            ("2000-03-01T00:00:00.Z", None),
            ("2000-03-01T00:00:00+0200", None),
            ("1969-12-31T23:59:59Z", None),
            ("tomorrow", None),
        ];
        for (text, seconds) in read {
            assert_eq!(epoch_seconds(text), seconds, "{text}");
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
        let found = merged(None, windows(&heard, 9), 9);
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
        let kept = merged(Some(&row), vec![read.clone()], 5_000);
        assert_eq!(kept.len(), 2, "the window the reading left out stays");
        assert_eq!((kept[0].since, kept[0].used), (first, 490_000));
        assert_eq!(kept[0].status.as_deref(), Some("allowed"));
        let renewed = merged(Some(&row), vec![read], 10_000);
        assert_eq!(renewed[0].since, now, "its reset passed");
    }

    #[test]
    fn a_window_runs_out_at_the_pace_it_climbed_at() {
        let since = Seen {
            used: 200_000,
            time: 1_000,
        };
        // 0.2 → 0.6 in 4 000 ms: the last 0.4 takes 4 000 ms more
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
        assert_eq!(calls(&pace, 10 * MINUTE + 5), (7, 4));
        assert_eq!(calls(&pace, 11 * MINUTE), (4, 0));
        assert_eq!(calls(&pace, 12 * MINUTE), (0, 0));
        assert_eq!(calls(&pace, 9 * MINUTE), (0, 0));
    }

    #[test]
    fn ccs_live_status_reads_into_windows() {
        // GET /ccs/status on duckpond's anthropic service, as ccs 41cfbbd answered it
        let live = r#"{"accounts":[{"error":null,"five_hour":{"percent":100.0,"resets_at":"2026-10-18T22:19:59.703348+00:00"},"label":"anthropic3-eph","models":[{"kind":"weekly_scoped","model":"Fable","percent":0.0,"resets_at":"2026-10-21T11:00:00+00:00"}],"provider":"claude","read_at":"2026-10-18T21:24:39.83298049Z","weekly":{"percent":57.0,"resets_at":"2026-10-21T10:59:59.703379+00:00"}}]}"#;
        let status: Status = serde_json::from_str(live).unwrap();
        let read: Vec<_> = read_windows(&status.accounts[0], 9)
            .into_iter()
            .map(|w| (w.name, w.utilization, w.used, w.reset))
            .collect();
        let named = |name: &str, fraction: &str, used, reset| {
            (name.to_owned(), fraction.to_owned(), used, reset)
        };
        let windows = [
            named("5h", "1", WHOLE, 1792361999),
            named("7d", "0.57", 570_000, 1792580399),
            named("Fable weekly_scoped", "0", 0, 1792580400),
        ];
        assert_eq!(read, windows);
    }
}
