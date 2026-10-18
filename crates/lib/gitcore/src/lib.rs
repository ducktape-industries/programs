// gitcore: git objects, packs, walks, diffs, the 3-way merge a squash takes and the server side of the wire protocol, over one host-supplied store. What a program needs to verify a push, answer a fetch and squash a change.

#![forbid(unsafe_code)]

pub mod diff;
mod error;
pub mod merge;
mod object;
mod oid;
pub mod pack;
pub mod server;
mod store;
pub mod walk;
pub mod wire;

pub use error::{Error, Result};
pub use object::{Commit, Kind, Mode, Object, Signature, Tag, Tree, TreeEntry};
pub use oid::{Hash, Oid, oid_of};
pub use pack::Limits;
pub use store::{MemoryObjects, Objects, WriteObjects};
