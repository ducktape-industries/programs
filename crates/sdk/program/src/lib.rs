//! The program SDK. A program is a type implementing [`Program`] (its name,
//! the three types it speaks, the same trait a view names it by, and what it
//! does with each), and one [`export!`]; its entry points receive an [`ExecCtx`] or a [`QueryCtx`],
//! whose methods are the whole host surface: `env`, `sender` (the account a
//! write acts as, which the host resolved through the identity role), raw
//! state (`get`, `set`, `delete`, `scan`), blobs, `event`, `set_return_data`,
//! `sha256` and `verify`, and the two ways to another program: `emit` (a write
//! it runs in this frame, replying or not) and `query` (a read). On wasm32
//! they are host calls; natively the same contexts run over a [`MockHost`],
//! and several programs run together, a submission as one frame with its
//! messages and replies as the kernel runs it, on a [`MockChain`].
//! [`Env`] adds the origin checks (`signer`, `sending_program`, `sent_by`) and
//! `authority`, which admits only a message from the program bound to the
//! validators role.
//! A message emitted with [`Reply::Wanted`] comes back to its emitter as
//! [`Program::reply`], with its id and outcome; a program
//! that never wants a reply leaves `reply` at its default.
//!
//! ```ignore
//! use program::{Error, ExecCtx, Program, QueryCtx};
//!
//! pub struct Counter;
//!
//! impl Program for Counter {
//!     const NAME: &'static str = "counter";
//!     type Op = u64;
//!     type Query = ();
//!     type Reply = u64;
//!
//!     fn execute(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
//!         let n: u64 = ctx.record("n")?.unwrap_or(0);
//!         ctx.put("n", &(n + by));
//!         Ok(())
//!     }
//!
//!     fn query(ctx: &QueryCtx, (): ()) -> Result<u64, Error> {
//!         Ok(ctx.record("n")?.unwrap_or(0))
//!     }
//! }
//!
//! program::export!(Counter);
//! ```
//!
//! `store` is the optional typed layer on top (`Map`/`Set`/`Item`, pages).

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

pub use abi;
pub use abi::hook;
pub use abi::role::access::{Action, Need, Ownership, Party, Resource, Writers};
pub use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, Message, Root, Scheme,
};
pub use awaiting::{Awaiting, Ctx, all};
#[cfg(not(target_arch = "wasm32"))]
pub use chain::{MAX_DEPTH, MockChain, OpenRead, Roster};
pub use ctx::{ExecCtx, QueryCtx, Reply};
pub use kernel::{
    AccountNumber, Cause, EntryId, Env, Error, Foreign, FrameId, MessageId, NetworkId, Order,
    Origin, Outcome, Principal, ProgramId, Range, Roles, Ticket, code,
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
