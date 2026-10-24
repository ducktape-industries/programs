//! Exact-commit check and patch-review receipts, without scheduling, carry or automatic retry.
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use ::program::AccountNumber;
pub use forge::CheckState;
pub use program::ForgeReach;
pub use store::{PageRequest, PageResponse};

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

use crate::state::{PATCHES, REACH, REPORTS};

pub const PROGRAM: &str = "forge-reach";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Report {
    pub reporter: AccountNumber,
    pub context: String,
    pub state: CheckState,
    pub url: Option<String>,
    pub height: u64,
    pub reach: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Rerun {
    pub repo: String,
    pub n: u64,
    pub commit: String,
    pub context: String,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Exact-commit check and patch-review receipts, stored by reporting account without scheduling or result carry."
)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Op {
    #[tool_op(description = "Record your check result at an exact commit, context and declared read paths. Anyone may; it is stored under the sender account. Repeating it replaces that account's result. No result is forwarded, carried or used to schedule work.", example = crate::examples::report)]
    Report {
        repo: String,
        commit: String,
        context: String,
        state: CheckState,
        url: Option<String>,
        reach: Vec<String>,
        change: Option<u64>,
    } = 1,
    #[tool_op(description = "Record your patch-review result at an exact commit and context. Anyone may; it replaces the same account's tree result at that identity and never carries to another commit.", example = crate::examples::report_patch)]
    ReportPatch {
        repo: String,
        n: u64,
        commit: String,
        context: String,
        state: CheckState,
        url: Option<String>,
    },
    #[tool_op(description = "Retired result-carry operation. Always refuses; this terminal wire tag remains reserved.", example = crate::examples::retell)]
    Retell { repo: String, n: u64 },
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(ForgeReach)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Query {
    #[ask(Reply::Reports(PageResponse<Report>))]
    #[reads(REPORTS, REACH, PATCHES)]
    #[tool_query(description = "Every result reported for a commit, by reporter and context, each with the paths its check read: the results of a tree first, then the results of a change's patch (ReportPatch), each with an empty reach, which no Report has.", example = crate::examples::reports)]
    Reports {
        repo: String,
        commit: String,
        page: PageRequest,
    },
    #[ask(Reply::Wanted(PageResponse<Rerun>))]
    #[tool_query(description = "Retired automatic re-request query. Always refuses; this terminal wire tag remains reserved.", example = crate::examples::wanted)]
    Wanted {
        reporter: AccountNumber,
        page: PageRequest,
    } = 2,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Reply {
    Reports(PageResponse<Report>),
    Wanted(PageResponse<Rerun>) = 2,
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Report {
            repo,
            commit,
            context,
            state,
            reach,
            ..
        } => (
            format!("Report {context} {} in {repo}", state.word()),
            vec![
                field("commit", Value::text(commit)),
                field("reach", Value::count(reach.len() as u64)),
            ],
        ),
        Op::ReportPatch {
            repo,
            n,
            commit,
            context,
            state,
            ..
        } => (
            format!("Report {context} {} of {repo}#{n}", state.word()),
            vec![field("commit", Value::text(commit))],
        ),
        Op::Retell { .. } => ("Refuse retired result carry".to_owned(), Vec::new()),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["-", "Report", "ReportPatch", "Retell"]
    );
}
