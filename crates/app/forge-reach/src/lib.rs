//! The `forge-reach` program: a check's result carried to a change's Preview when nothing it read
//! changed. A reporter names the paths its check read; on each Preview forge makes, forge-reach
//! compares those paths there and reports forge a status of its own (`rules.rs`).
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

use crate::state::{HEADS, REACH, REPORTS};

pub const PROGRAM: &str = "forge-reach";

pub fn watching() -> Vec<u8> {
    vec![0]
}

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
pub struct Carried {
    pub context: String,
    pub gated: String,
    pub reported: CheckState,
    pub state: CheckState,
    pub url: Option<String>,
    pub differs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct PreviewVerdict {
    pub n: u64,
    pub head: String,
    pub merge: String,
    pub carried: Vec<Carried>,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "A check's result carried to a change's Preview when nothing it read changed. A reporter (a lane, a CI runner) reports its result for a commit with the paths its check read; on each Preview forge makes, forge-reach compares those paths between the reported commit and the Preview, and reports forge the status `<reporter>/<context>` on the Preview: the reported state when every path is the same, Pending when one differs. A branch requires it by naming forge-reach's account as RequireChecks' reporter."
)]
pub enum Op {
    #[tool_op(description = "What forge tells a watcher; forge alone sends it. A Previewed event makes forge-reach report, on the new Preview, every result reported for the change's head or for the Preview itself, and for a reporter with none there its newest result on an earlier head or Preview of the change whose patch is the same.", example = crate::examples::watched)]
    Watched(forge::Watched),
    #[tool_op(description = "Report your check's result for a commit (its full hex id) in a context, with the paths in the repository the check read (\"\" is the whole tree) and a url to its log; saying it again replaces it. With change, forge-reach keeps the commit as one of that change's heads and reports on its latest Preview at once; without it, on the latest Preview of every open change whose head or Preview commit is that commit. Anyone may; it is recorded as yours, and the status it carries is `<your account>/<context>`.", example = crate::examples::report)]
    Report {
        repo: String,
        commit: String,
        context: String,
        state: CheckState,
        url: Option<String>,
        reach: Vec<String>,
        change: Option<u64>,
    },
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
pub enum Query {
    #[ask(Reply::Reports(PageResponse<Report>))]
    #[reads(REPORTS, REACH)]
    #[tool_query(description = "Every result reported for a commit, by reporter and context, each with the paths its check read.", example = crate::examples::reports)]
    Reports {
        repo: String,
        commit: String,
        page: PageRequest,
    },
    #[ask(Reply::Verdict(Option<PreviewVerdict>))]
    #[reads(REPORTS, REACH, HEADS)]
    #[reads(forge::Forge: forge::tables::PREVIEWS, forge::tables::REPOS)]
    #[tool_query(description = "How each result reported for change n's head, or on an earlier head of it for a reporter with none on the head, stands on its latest Preview: carried with its state, or Pending with the paths that differ there (a reached path, or a path the patch of an earlier head and the Preview's patch change differently). Null while the change has no Preview.", example = crate::examples::verdict)]
    Verdict { repo: String, n: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Reports(PageResponse<Report>),
    Verdict(Option<PreviewVerdict>),
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Watched(forge::Watched::Previewed { repo, n, .. }) => {
            (format!("Hear change {repo}#{n} previewed"), Vec::new())
        }
        Op::Watched(_) => ("Hear forge".to_owned(), Vec::new()),
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
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Watched", "Report"]);
}
