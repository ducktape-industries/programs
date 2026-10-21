//! Every table actions keeps, declared once: runs, checks and the indexes over them, the open
//! change each source ref belongs to, the runners and the counters.
use store::{Item, Map, Set};

use crate::{AccountNumber, Asks, CheckRow, Need, OutputsRow, RunRow, RunnerRow};

pub const RUNS: Map<u64, RunRow> = Map::new("r/");
pub const CHECKS: Map<u64, CheckRow> = Map::new("c/");
// Index: each check by its repository and commit.
pub const BY_COMMIT: Set<(String, String, u64)> = Set::new("k/");
// Index: each check of a run, its reruns included.
pub const BY_RUN: Set<(u64, u64)> = Set::new("b/");
// Index: the runs whose open checks the next run on their line supersedes, by repository and
// line: a push's branch, or a change's Preview ref (`refs/changes/<n>/merge`).
pub const LINES: Set<(String, Vec<u8>, u64)> = Set::new("l/");
// Index: the checks no runner has claimed.
pub const QUEUED: Set<u64> = Set::new("q/");
// The open change whose source a repository's ref is.
pub const SOURCES: Map<(String, Vec<u8>), u64> = Map::new("o/");
// The Previews actions asked forge for, by the message's seq, until forge answers in the
// frame that asked.
pub const PREVIEWING: Map<u64, (String, u64)> = Map::new("p/");
// The offers actions sent runners, by the message's seq, each naming its check and runner, until
// the runner answers in the frame that offered.
pub const OFFERING: Map<u64, (u64, String)> = Map::new("f/");
pub const RUNNERS: Map<AccountNumber, RunnerRow> = Map::new("u/");
pub const LAST_RUN: Item<u64> = Item::new("last-run");
pub const LAST_CHECK: Item<u64> = Item::new("last-check");
// What each check's job asks of its pod, when its workflow states anything.
pub const ASKS: Map<u64, Asks> = Map::new("asks/");
// The jobs each check's job needs, when it names any.
pub const NEEDS: Map<u64, Vec<String>> = Map::new("needs/");
// The accounts a check's workflow names as its job's runners, when it names any: only they claim
// it or end it.
pub const NAMED: Map<u64, Vec<AccountNumber>> = Map::new("named/");
// For a check its needs held back, the passed check of each job it needs, once all have passed.
pub const MET: Map<u64, Vec<Need>> = Map::new("met/");
// What each check produced, once its runner said.
pub const OUTPUTS: Map<u64, OutputsRow> = Map::new("outputs/");
// Index: each run by its repository and event (the event's borsh), so a branch's pushes, a
// change's Previews or the runs triggered by hand list in one query. Runs recorded before this
// index was kept are not in it.
pub const BY_EVENT: Set<(String, Vec<u8>, u64)> = Set::new("e/");
pub const KEEP_STARTED: Set<u64> = Set::new("keep/");
