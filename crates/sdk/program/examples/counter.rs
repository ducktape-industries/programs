//! A program written with `program` alone: a counter anyone may add to.

use program::{Error, ExecCtx, Program, QueryCtx, invalid};

pub struct Counter;

impl Program for Counter {
    const NAME: &'static str = "counter";
    type Op = u64;
    type Query = ();
    type Reply = u64;

    fn execute(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
        let n = count(ctx)?
            .checked_add(by)
            .ok_or_else(|| invalid("the count would overflow"))?;
        ctx.put("n", &n);
        ctx.set_return_data(abi_bytes(n));
        Ok(())
    }

    fn query(ctx: &QueryCtx, (): ()) -> Result<u64, Error> {
        count(ctx)
    }
}

fn count(ctx: &QueryCtx) -> Result<u64, Error> {
    Ok(ctx.record("n")?.unwrap_or(0))
}

fn abi_bytes(n: u64) -> Vec<u8> {
    program::abi::encode(&n)
}

program::export!(Counter);

#[cfg(test)]
mod tests {
    use program::{Env, MockHost, code};

    use super::*;

    fn signed_by_account_one() -> Env {
        MockHost::env(Counter::NAME).signed([1; 32], Some(1))
    }

    #[test]
    fn adds_and_answers_and_a_refusal_writes_nothing() {
        let host = MockHost::default();
        Counter::execute(&host.exec(signed_by_account_one()), 2).unwrap();
        Counter::execute(&host.exec(signed_by_account_one()), 3).unwrap();
        assert_eq!(
            Counter::query(&host.query(signed_by_account_one()), ()).unwrap(),
            5
        );
        assert_eq!(host.take_output(), program::abi::encode(&5u64));

        let overflow =
            host.refused(|| Counter::execute(&host.exec(signed_by_account_one()), u64::MAX));
        assert_eq!(overflow.code, code::INVALID_INPUT);

        program::execute::<Counter>(
            &host.exec(signed_by_account_one()),
            &program::abi::encode(&1u64),
        )
        .unwrap();
        program::query::<Counter>(
            &host.query(signed_by_account_one()),
            &program::abi::encode(&()),
        )
        .unwrap();
        assert_eq!(host.take_response(), program::abi::encode(&6u64));
        let garbage =
            program::execute::<Counter>(&host.exec(signed_by_account_one()), &[1]).unwrap_err();
        assert_eq!(garbage.code, code::INVALID_INPUT);
    }
}
