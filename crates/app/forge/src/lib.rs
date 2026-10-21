//! forge: a git server as a ducktape program, `gitcore` (objects, packs,
//! walks, diffs, the wire) over `store`. The [`Forge`] program runs natively
//! over `program::MockHost` (tests, fixtures); the `program` feature adds its
//! wasm exports.
//!
//! A write is an [`Op`] run as a [`Principal`] (the account the host
//! resolved, [`ExecCtx::sender_account`](program::ExecCtx::sender_account):
//! a key's, a program's own or the agent a program sends it for, never the
//! chain's), a read a [`Query`] answered by a [`Reply`]. The layout, in
//! reading order:
//!
//! - `contract.rs`, `read_contract.rs`, `review_contract.rs`: the wire
//! - `program.rs`: [`Forge`], the program: the account resolved, then one
//!   match over every op and one over every query
//! - `state.rs`: every table and index, declared once
//! - `ops.rs`: the repository ops; `changes.rs` the change ops
//! - `queries.rs`, `reads.rs`, `diffs.rs`, `change_queries.rs`: the answers
//!   to its repository, object and change questions
//! - `objects.rs`: git objects over the store's blobs
//! - `discussion.rs`: what forge asks of and posts into chat
//! - `description.rs`: [`describe()`], an op in a person's words

// The wire, as a view and a git client see it.
mod adopt;
mod contract;
mod read_contract;
mod review_contract;

mod change_queries;
mod changes;
mod checks;
mod description;
mod diffs;
mod discussion;
mod examples;
mod objects;
mod ops;
mod queries;
mod reads;
mod state;
mod watchers;

mod program;
mod queue;

pub use contract::*;
pub use description::describe;
pub use discussion::NOTICE;
pub use objects::{blob_id_of, hash_of};
pub use ops::PROGRAM;
pub use program::Forge;

/// Every table forge keeps, as a block's writes name them: what a follower
/// of `program.changes` plays back in a test to say which a block touched.
pub mod tables {
    pub use crate::state::{
        ACTIVITY, ARCHIVED, ARCHIVING, AUTHORED, AUTHORSHIP, BOUNDS, CHANGES, CUSTODY, INVOLVED,
        LOGGING, MESSAGES, NUMBERS, PREVIEWS, PROTECTED, QUEUE, QUEUED, REFS, REPOS, REQUIRED,
        REVIEWS, STATUSES, WATCHERS, WRITES,
    };
}

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "Create",
            "Configure",
            "Grant",
            "Revoke",
            "Push",
            "Merge",
            "ChangeOpen",
            "ChangeEdit",
            "ChangeClose",
            "ReviewSubmit",
            "Stage",
            "SquashMerge",
            "Rebase",
            "ChangeRepoint",
            "DeleteBranch",
            "Protect",
            "Watch",
            "ArchiveEnded",
            "Preview",
            "Status",
            "RequireChecks",
            "Enqueue",
            "Dequeue",
            "AdoptOwners",
            "Archive",
            "Dispatch",
        ]
    );
}

#[test]
fn watched_variants_only_append() {
    assert_eq!(
        describe::variants::<Watched>(),
        [
            "Moved",
            "Opened",
            "Previewed",
            "Closed",
            "Merged",
            "Reviewed",
            "LeftQueue",
            "Dispatched",
        ]
    );
}
