//! Every sentence and status word the members screens show, once: the desk
//! and the phone both read them from here, and neither types one. Expo
//! types them inline in `app/(tabs)/members/index.tsx` and `[number].tsx`;
//! each cites its line. Where the two trees say a thing differently, each
//! word is named for where it shows.
use view::design;

pub(crate) const MEMBERS: &str = "Members";
pub(crate) const FILTER: &str = "Filter";
pub(crate) const CLOSE_FILTER: &str = "Close filter";
pub(crate) const FILTER_LABEL: &str = "Filter members";
pub(crate) const FILTER_HINT: &str = "Filter by name or number";
pub(crate) const READING_ROSTER: &str = "Reading the roster…";
pub(crate) const NO_ACCOUNTS: &str = "No accounts";
pub(crate) const NO_ACCOUNTS_WHY: &str =
    "The identity program of this network holds no accounts yet.";
pub(crate) const NOTHING_MATCHES: &str = "Nothing matches";
pub(crate) const ALL: &str = "All";
pub(crate) const YOU: &str = "you";
pub(crate) const ACTIVE: &str = "active";
pub(crate) const USER: &str = "User";
pub(crate) const AGENT: &str = "Agent";
pub(crate) const MANAGED_BY: &str = "managed by";
pub(crate) const PROGRAM: &str = "Program";
pub(crate) const VALIDATOR: &str = "Validator";
pub(crate) const RESIDENT: &str = "Resident";
pub(crate) const OPEN_DM: &str = "Open DM";
pub(crate) const EXPLORER: &str = "Explorer";
pub(crate) const DEVICES: &str = "Devices";
pub(crate) const MANAGES: &str = "Manages";
pub(crate) const RECENT_ACTIVITY: &str = "Recent activity";
pub(crate) const NO_DEVICES: &str = "No devices";
pub(crate) const READING_ACTIVITY: &str = "Reading its activity…";
pub(crate) const NO_ACTIVITY: &str = "No activity recorded.";
pub(crate) const NO_ACCOUNT_WHY: &str = "Identity holds no account by this number.";
pub(crate) const RESIZE_LIST: &str = "Resize the member list";
pub(crate) const CHOOSE_MEMBER: &str = "Choose a member";
pub(crate) const CLOSE: &str = "Close";
pub(crate) const NEWEST_FIRST: &str = "newest first";
pub(crate) const MANAGED_IN_SETTINGS: &str =
    "Suspend, revoke, rename and keys live in Account → Agents, for the manager.";

pub(crate) fn no_match(needle: &str, only: Option<&str>) -> String {
    match (needle, only) {
        ("", Some(group)) => format!("No {} on this network yet.", group.to_lowercase()),
        (needle, _) => format!("No account reads like “{needle}”."),
    }
}

pub(crate) fn accounts(count: usize) -> String {
    design::plural(count as u64, "account", "accounts")
}

pub(crate) fn account_title(number: u64) -> String {
    format!("Account {number}")
}

pub(crate) fn no_account(number: u64) -> String {
    format!("No account #{number}")
}

pub(crate) fn block(place: &str) -> String {
    format!("block {place}")
}

pub(crate) fn open_in_explorer(frame: &str) -> String {
    format!("Open {frame} in Explorer")
}

pub(crate) fn account(number: u64) -> String {
    format!("account {number}")
}

pub(crate) fn devices(count: usize) -> String {
    design::plural(count as u64, "device", "devices")
}

pub(crate) fn agent(number: u64) -> String {
    format!("{} · {AGENT}", account(number))
}

pub(crate) fn device(index: usize) -> String {
    format!("Device {}", index + 1)
}

pub(crate) fn sized(program: &str, size: u64) -> String {
    format!("{program} · {}", design::plural(size, "byte", "bytes"))
}

pub(crate) fn added(at: u64) -> String {
    format!("added {}", design::day(at))
}
