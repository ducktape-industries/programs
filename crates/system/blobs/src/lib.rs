//! The `blobs` program: an account puts a bundle of blobs by id, length and kind, a current
//! validator's node key receipts the bundle once its node holds every blob, and a quorum of
//! receipts marks it held. Types, rules and [`Blobs`] always build; `program` adds the exports.
mod program;
mod rules;
#[cfg(test)]
mod tests;
mod tools;

pub use blob::contract::{Bundle, Custody, Entry, Holding, Op, Query, Reply, quorum};
pub use program::Blobs;

pub const PROGRAM: &str = "blobs";

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Put { blobs } => (
            format!("Put · {} blobs", blobs.len()),
            blobs
                .iter()
                .map(|entry| field(&entry.kind, Value::Hash(entry.id.digest().to_vec())))
                .collect(),
        ),
        Op::Receipt { bundle } => (
            format!("Receipt · bundle {bundle}"),
            vec![field("bundle", Value::Text(bundle.to_string()))],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Put", "Receipt"]);
}
