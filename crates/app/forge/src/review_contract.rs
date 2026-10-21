//! Immutable batched reviews, mutable changes, and the shared per-repo item counter.
use crate::read_contract::Revision;
use ::program::Principal;
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub enum ChangeState {
    Open,
    Closed,
    Merged,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Verdict {
    Approve,
    RequestChanges,
    Comment,
}
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub enum Side {
    Old,
    New,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct LineComment {
    pub path: Vec<u8>,
    pub side: Side,
    pub line: u64,
    pub body: String,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ReviewDraft {
    pub commit_oid: String,
    pub base_oid: Option<String>,
    pub verdict: Verdict,
    pub body: String,
    pub comments: Vec<LineComment>,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Review {
    pub id: u64,
    pub author: Principal,
    pub height: u64,
    pub time: u64,
    pub draft: ReviewDraft,
    /// Look up this chat root by MessageById; all line discussions are chat replies.
    pub message_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Change {
    pub n: u64,
    pub from: Revision,
    pub into: Vec<u8>,
    pub title: String,
    pub body: String,
    pub author: Principal,
    pub state: ChangeState,
    pub reviewers: Vec<Principal>,
    pub created_height: u64,
    pub updated_height: u64,
    pub created_time: u64,
    pub updated_time: u64,
    pub review_count: u64,
    pub comment_count: u64,
    pub verdicts: ReviewCounts,
    pub merge_oid: Option<String>,
    /// Who closed the change; set once, by the close op.
    pub closed_by: Option<Principal>,
    /// Who merged the change; set once, by a merge linked to it.
    pub merged_by: Option<Principal>,
    /// The source and target heads the merge joined; set once, by a merge
    /// linked to it. A merged change reads these, not its live refs.
    pub merged_heads: Option<MergedHeads>,
    pub channel: String,
    pub system_seq: u64,
}
/// A merged change's two heads as the merge found them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct MergedHeads {
    pub source: String,
    pub target: String,
}
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema,
)]
pub struct ReviewCounts {
    pub approve: u64,
    pub request_changes: u64,
    pub comment: u64,
}
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    PartialOrd,
    Ord,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub struct ChangeFilter {
    pub state: Option<ChangeState>,
    pub author: Option<Principal>,
    pub involves: Option<Principal>,
}
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct ChangeSummary {
    pub repo: String,
    pub n: u64,
    pub from: Revision,
    pub into: Vec<u8>,
    pub title: String,
    pub author: Principal,
    pub state: ChangeState,
    pub updated_height: u64,
    pub review_count: u64,
    pub comment_count: u64,
    pub verdicts: ReviewCounts,
}
