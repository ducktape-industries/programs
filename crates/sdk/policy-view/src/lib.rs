//! What a view shows of the ownership program's policies, once for every view that links it: the
//! kinds a default names (`kinds.rs`), policies, parties and scopes in words (`words.rs`), one
//! owner's side read whole (`reads.rs`), and "Who can edit" on one item with its sheets (`item.rs`).
pub mod item;
mod kinds;
mod reads;
mod words;

#[cfg(test)]
mod tests;

pub use item::{Editing, Item, Lens, Sheet, Standing};
pub use kinds::{KINDS, Kind, PROGRAMS, kind};
pub use reads::{Holdings, holdings, items};
pub use words::{party, scope, writers};
