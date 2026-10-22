//! The model budget in harness's own words, on every answer the adapter gives beside the provider's
//! own headers: each window's share used (a fraction), its reset (seconds since the epoch) and
//! status, the account's status and id, the login a gate names and the accounts it serves now.
pub const WINDOW: &str = "x-harness-window-";
pub const STATUS: &str = "x-harness-status";
pub const ACCOUNT: &str = "x-harness-account";
pub const IDENTITY: &str = "x-harness-identity";
pub const POOL: &str = "x-harness-pool";
const USED: &str = "-used";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    pub name: String,
    pub used: String,
    pub reset: u64,
    pub status: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub windows: Vec<Window>,
    pub status: Option<String>,
    pub account: Option<String>,
    pub identity: Option<String>,
    pub pool: Option<Vec<String>>,
}

pub(crate) fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let found = headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name));
    found.map(|(_, value)| value.trim())
}

fn members(text: &str) -> Vec<String> {
    let named = text
        .split(',')
        .map(str::trim)
        .filter(|label| !label.is_empty());
    named.map(str::to_owned).collect()
}

impl Budget {
    pub fn headers(&self) -> Vec<(String, String)> {
        let mut headers = Vec::new();
        for window in &self.windows {
            let named = |part: &str| format!("{WINDOW}{}-{part}", window.name);
            headers.push((named("used"), window.used.clone()));
            headers.push((named("reset"), window.reset.to_string()));
            if let Some(status) = &window.status {
                headers.push((named("status"), status.clone()));
            }
        }
        let status = self
            .status
            .iter()
            .map(|status| (STATUS.to_owned(), status.clone()));
        headers.extend(status);
        let account = self.account.iter();
        headers.extend(account.map(|account| (ACCOUNT.to_owned(), account.clone())));
        let identity = self.identity.iter();
        headers.extend(identity.map(|identity| (IDENTITY.to_owned(), identity.clone())));
        let pool = self.pool.iter();
        headers.extend(pool.map(|pool| (POOL.to_owned(), pool.join(", "))));
        headers
    }

    pub fn read(headers: &[(String, String)]) -> Budget {
        let mut windows: Vec<Window> = Vec::new();
        for (key, value) in headers {
            let key = key.to_ascii_lowercase();
            let named = key.strip_prefix(WINDOW);
            let Some(name) = named.and_then(|rest| rest.strip_suffix(USED)) else {
                continue;
            };
            let reset = header(headers, &format!("{WINDOW}{name}-reset"));
            let Some(reset) = reset.and_then(|reset| reset.parse().ok()) else {
                continue;
            };
            if name.is_empty() || windows.iter().any(|window| window.name == name) {
                continue;
            }
            let status = header(headers, &format!("{WINDOW}{name}-status"));
            windows.push(Window {
                name: name.to_owned(),
                used: value.trim().to_owned(),
                reset,
                status: status.map(str::to_owned),
            });
        }
        Budget {
            windows,
            status: header(headers, STATUS).map(str::to_owned),
            account: header(headers, ACCOUNT).map(str::to_owned),
            identity: header(headers, IDENTITY).map(str::to_owned),
            pool: header(headers, POOL).map(members),
        }
    }
}

pub fn named(minutes: u64) -> String {
    match minutes {
        0 => "0m".to_owned(),
        minutes if minutes.is_multiple_of(24 * 60) => format!("{}d", minutes / (24 * 60)),
        minutes if minutes.is_multiple_of(60) => format!("{}h", minutes / 60),
        minutes => format!("{minutes}m"),
    }
}

pub fn percent(text: &str) -> Option<String> {
    let (whole, part) = match text.split_once('.') {
        Some((whole, part)) if !part.is_empty() => (whole, part),
        Some(_) => return None,
        None => (text, ""),
    };
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(part) {
        return None;
    }
    let all = format!("{}{part}", whole.trim_start_matches('0'));
    let scale = part.len() + 2;
    let (int, fraction) = match all.len().checked_sub(scale) {
        Some(at) if at > 0 => (all[..at].to_owned(), all[at..].to_owned()),
        _ => ("0".to_owned(), format!("{all:0>scale$}")),
    };
    let fraction = fraction.trim_end_matches('0');
    Some(match fraction {
        "" => int,
        fraction => format!("{int}.{fraction}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        let each = list.iter();
        each.map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    #[test]
    fn the_pool_names_each_account_a_gate_serves_and_no_word_reads_as_none() {
        let served = Budget {
            pool: Some(vec!["zk-dev-2".into(), "zk-dev-3".into()]),
            ..Budget::default()
        };
        let headers = served.headers();
        assert_eq!(header(&headers, POOL), Some("zk-dev-2, zk-dev-3"));
        assert_eq!(Budget::read(&headers), served);
        let spaced = pairs(&[("X-Harness-Pool", " zk-dev-2 ,, zk-dev-3 ")]);
        assert_eq!(Budget::read(&spaced).pool, served.pool);
        let none_served = Budget::read(&pairs(&[(POOL, "")])).pool;
        assert_eq!(none_served, Some(Vec::new()), "a gate that serves no one");
        assert_eq!(Budget::read(&[]).pool, None, "a gate that names no pool");
    }
}
