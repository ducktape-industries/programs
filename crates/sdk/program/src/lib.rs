//! The program SDK, its contexts, awaiting calls and native test chain.
pub mod awaiting;
#[cfg(not(target_arch = "wasm32"))]
mod chain;
mod ctx;
mod identity;
pub mod kernel;
pub mod live;
#[cfg(not(target_arch = "wasm32"))]
mod mock;
mod origin;
mod program;
pub mod refuse;
mod renames;
pub mod schema;
pub mod section;
pub mod source;
pub mod tools;
pub mod view_sign;

pub use abi;
pub use abi::hook;
pub use abi::role::access::{Action, Need, Ownership, Party, Resource, Writers};
pub use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, Message, Root, Scheme,
};
pub use awaiting::{Awaiting, Ctx, all};
#[cfg(not(target_arch = "wasm32"))]
pub use chain::{MAX_DEPTH, MockChain, OpenPreparation, OpenRead, Roster};
pub use ctx::{ExecCtx, QueryCtx, Reply};
pub use kernel::{
    AccountNumber, Cause, EntryId, Env, Error, Foreign, FrameId, MessageId, NetworkId, Order,
    Origin, Outcome, Principal, ProgramId, Range, Roles, Ticket, access_checker, code,
};
#[cfg(not(target_arch = "wasm32"))]
pub use mock::{
    Called, Emitted, MockHost, MockState, Renamer, Resumed, Sibling, Touch, Verifier, Work,
    blob_id, identity_role,
};
#[cfg(target_arch = "wasm32")]
pub use program::exports;
pub use program::{
    Ask, Program, Reads, access, execute, hook, hooks, is_id, is_name, query, resource, slug,
};
pub use program_derive::{Ask, Described};
pub use refuse::{
    already_exists, capacity, corrupt, decoded, invalid, not_found, stale, unauthorized,
    unexpected_reply, wrong_state,
};
pub use renames::Renames;
