//! The `halo` program: the lanes halo carries between members, as chain state. Each lane is a
//! name with a wire id, the class its streams are served at most, whether it is served, and
//! whether the founding pinned it (no vote pauses or closes a pinned lane). Names come from
//! the founding and from the validators' vote; the program knows none itself. An id is handed
//! out once, in order from 0, and never again.
//! Types, rules and [`Halo`] always build; `program` adds the wasm exports.
mod program;
mod rules;
#[cfg(test)]
mod tests;
mod tools;

pub use abi::role::halo::{Class, Founded, Genesis, Lane, LaneId, MAX_NAME, Op, Query, Reply};
pub use program::Halo;

pub const PROGRAM: &str = "halo";

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let cap = |cap: &Class| field("cap", Value::text(format!("{cap:?}")));
    let (title, fields) = match op {
        Op::Open { name, cap: capped } => (format!("Open lane · {name}"), vec![cap(capped)]),
        Op::Set {
            name,
            cap: capped,
            enabled,
        } => (
            format!("Set lane · {name}"),
            vec![
                cap(capped),
                field("served", Value::text(if *enabled { "yes" } else { "no" })),
            ],
        ),
        Op::Close { name } => (format!("Close lane · {name}"), vec![]),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Open", "Set", "Close"]);
}
