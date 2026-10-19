//! What a view shows of the ownership program's policies, once for every view that links it: the
//! kinds of item a default names (`kinds.rs`), a policy, a party and a scope in words (`words.rs`),
//! and one owner's side read whole (`reads.rs`).
mod kinds;
mod reads;
mod words;

#[cfg(test)]
mod tests;

pub use kinds::{KINDS, Kind, PROGRAMS, kind};
pub use reads::{Holdings, holdings, items};
pub use words::{party, scope, writers};
