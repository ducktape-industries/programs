//! Every table actions keeps, declared once: runs, checks and the indexes over them, the open
//! change each source ref belongs to, the runners and the counters.
use store::{Item, Map, Set};

use crate::{AccountNumber, CheckRow, RunRow, RunnerRow};

pub const RUNS: Map<u64, RunRow> = Map::new("r/").shared();
pub const CHECKS: Map<u64, CheckRow> = Map::new("c/").shared();
// Index: each check by its repository and commit.
pub const BY_COMMIT: Set<(String, String, u64)> = Set::new("k/").shared();
// Index: each check of a run, its reruns included.
pub const BY_RUN: Set<(u64, u64)> = Set::new("b/").shared();
// Index: the checks no runner has claimed.
pub const QUEUED: Set<u64> = Set::new("q/").shared();
// The open change whose source a repository's ref is.
pub const SOURCES: Map<(String, Vec<u8>), u64> = Map::new("o/").shared();
pub const RUNNERS: Map<AccountNumber, RunnerRow> = Map::new("u/").shared();
pub const LAST_RUN: Item<u64> = Item::new("last-run").shared();
pub const LAST_CHECK: Item<u64> = Item::new("last-check").shared();

// Every table here is general data: only this program's code governs it.
pub(crate) const TABLES: &[::program::Table] = &[
    RUNS.table(),
    CHECKS.table(),
    BY_COMMIT.table(),
    BY_RUN.table(),
    QUEUED.table(),
    SOURCES.table(),
    RUNNERS.table(),
    LAST_RUN.table(),
    LAST_CHECK.table(),
];
