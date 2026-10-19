//! Values of any program as JSON, by the borsh schema its code carries: the `borsh-json` crate,
//! which the `network_*` tools use. Its tests here read every program's interface through it, and
//! write back an example of every op and query.
#[cfg(test)]
mod tests;

pub(crate) use borsh_json::{Fault, spoken, unquoted};
#[cfg(test)]
use borsh_json::{decode, encode, shape};
