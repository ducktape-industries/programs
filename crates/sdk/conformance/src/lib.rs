//! Role conformance: proof that a program fills a role the kernel calls
//! (`abi::role::{programs, validators, identity, awaits, halo, scheduler}`).
//!
//! Each role is a program here with a `Fixture` trait and a `run` that
//! checks the role's contract, one named function per rule. The suite
//! speaks the role's bytes alone: it encodes `abi::role` values, hands
//! them to the program's own decoding (`program::execute`, `program::query`)
//! over a [`MockHost`], and decodes the answer as the role's reply, so a
//! program whose variants drift from the role's tags fails here as it would
//! under the kernel. What the role leaves to the program (how an account
//! comes to hold a key, how a validator is seated, how code is scheduled)
//! the fixture does with the program's own ops.
//!
//! ```ignore
//! #[test]
//! fn fills_the_identity_role() {
//!     conformance::identity::run(&MyFixture);
//! }
//! ```

use borsh::{BorshDeserialize, BorshSerialize};
use program::{Env, Error, MockHost, Origin, Principal, Program};

pub mod awaits;
pub mod halo;
pub mod identity;
pub mod programs;
pub mod scheduler;
pub mod validators;

/// An env for `program` at `height`, as the kernel would send it.
pub fn env(program: &str, height: u64, origin: Origin, sender: Option<Principal>) -> Env {
    Env {
        chain_id: b"conformance".to_vec(),
        height,
        origin,
        sender,
        ..MockHost::env(program)
    }
}

/// The env of a founding or a kernel call: the chain itself.
pub fn root(program: &str, height: u64) -> Env {
    env(program, height, Origin::Root, Some(Principal::Root))
}

fn init<M: Program>(host: &MockHost, program: &str, genesis: &impl BorshSerialize) {
    M::init(&host.exec(root(program, 0)), &abi::encode(genesis))
        .unwrap_or_else(|e| panic!("{program}: init refused the role's genesis: {e:?}"));
}

fn execute<M: Program>(host: &MockHost, env: Env, op: &impl BorshSerialize) -> Result<(), Error> {
    program::execute::<M>(&host.exec(env), &abi::encode(op))
}

fn ask<M: Program, R: BorshDeserialize + std::fmt::Debug>(
    host: &MockHost,
    program: &str,
    height: u64,
    query: &(impl BorshSerialize + std::fmt::Debug),
) -> Result<R, Error> {
    let ctx = host.query(env(program, height, Origin::Root, None));
    program::query::<M>(&ctx, &abi::encode(query))?;
    let bytes = host.take_response();
    Ok(abi::decode(&bytes).unwrap_or_else(|e| {
        panic!("{program}: the answer to {query:?} is not the role's reply: {e}")
    }))
}

#[track_caller]
fn same_bytes<T: BorshSerialize + BorshDeserialize>(
    program: &str,
    what: &str,
    role: &(impl BorshSerialize + std::fmt::Debug),
) {
    let bytes = abi::encode(role);
    let ours: T = abi::decode(&bytes).unwrap_or_else(|e| {
        panic!("{program}: the role's {role:?} does not decode as the program's {what}: {e}")
    });
    assert_eq!(
        abi::encode(&ours),
        bytes,
        "{program}: the role's {role:?} is not the program's {what} byte for byte: \
         each role variant is the program's at the role's tag, with the same fields"
    );
}
