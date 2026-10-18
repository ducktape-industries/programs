//! Typed storage over `program`'s contexts, optional: `const` [`Map`]/[`Set`]/
//! [`Item`] descriptors whose keys encode through [`KeyCodec`] and whose
//! methods take the context (`CHANNELS.get(ctx, &id)`, `CHANNELS.put(ctx,
//! &id, &row)`), and the bounded, resumable [`PageRequest`]/[`PageResponse`]. A read
//! takes `&QueryCtx`, so an `&ExecCtx` serves it too; a write takes
//! `&ExecCtx`.

mod key;
mod page;
mod table;

pub use key::KeyCodec;
pub use page::{Cursor, Listing, PageRequest, PageResponse};
pub use table::{Item, Kind, Map, Owned, Row, Set};
