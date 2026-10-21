//! The errors a program hands back: a [`code`] token, each with one message.

use crate::{Error, code};
use borsh::BorshDeserialize;

pub fn invalid(message: impl Into<String>) -> Error {
    Error::new(code::INVALID_INPUT, message)
}

pub fn not_found(message: impl Into<String>) -> Error {
    Error::new(code::NOT_FOUND, message)
}

pub fn already_exists(message: impl Into<String>) -> Error {
    Error::new(code::ALREADY_EXISTS, message)
}

pub fn wrong_state(message: impl Into<String>) -> Error {
    Error::new(code::WRONG_STATE, message)
}

pub fn unauthorized(message: impl Into<String>) -> Error {
    Error::new(code::UNAUTHORIZED, message)
}

pub fn capacity(message: impl Into<String>) -> Error {
    Error::new(code::CAPACITY, message)
}

pub fn stale(message: impl Into<String>) -> Error {
    Error::new(code::STALE, message)
}

/// A sibling `program` answered `asked` with a reply of another shape than
/// the one its role or its types promise: an operator's problem.
pub fn unexpected_reply(program: &str, asked: &str, reply: &impl std::fmt::Debug) -> Error {
    Error::new(
        code::UNEXPECTED_REPLY,
        format!("{program} answered {asked} with {reply:?}"),
    )
}

/// Stored state that does not decode: an operator's problem, never a panic.
pub fn corrupt(table: &str, key: &[u8], what: impl std::fmt::Display) -> Error {
    Error::new(code::CORRUPT, format!("{table}[{}]: {what}", abi::hex(key)))
}

/// A program's `Op`/`Query` that does not decode, refused as invalid input
/// naming the program and the shape.
pub fn decoded<T: BorshDeserialize>(program: &str, shape: &str, bytes: &[u8]) -> Result<T, Error> {
    abi::decode(bytes).map_err(|fault| {
        invalid(format!(
            "{program}: {shape} did not decode: {}",
            fault.sentence
        ))
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use borsh::{BorshDeserialize, BorshSerialize};

    use super::*;

    #[derive(BorshSerialize)]
    enum Before {
        Open { name: String },
        Grant { who: u64 },
        Revoke { who: u64 },
        Push { repo: String, at: u64 },
        Merge(u64),
    }

    #[derive(Debug, PartialEq, BorshDeserialize)]
    #[borsh(use_discriminant = true)]
    #[repr(u8)]
    enum After {
        Open { name: String },
        Push { repo: String, at: u64 } = 3,
        Merge(u64),
    }

    fn read(op: &Before) -> Result<After, Error> {
        decoded::<After>("forge", "Op", &borsh::to_vec(op).unwrap())
    }

    #[test]
    fn an_op_sent_before_a_retirement_reads_the_same_and_a_retired_one_is_refused_as_invalid() {
        let open = read(&Before::Open { name: "a".into() }).unwrap();
        assert_eq!(open, After::Open { name: "a".into() });
        let push = Before::Push {
            repo: "r".into(),
            at: 7,
        };
        let pushed = After::Push {
            repo: "r".into(),
            at: 7,
        };
        assert_eq!(read(&push).unwrap(), pushed);
        assert_eq!(read(&Before::Merge(9)).unwrap(), After::Merge(9));
        for retired in [Before::Grant { who: 1 }, Before::Revoke { who: 1 }] {
            let refused = read(&retired).unwrap_err();
            assert_eq!(refused.code, code::INVALID_INPUT);
            assert!(
                refused.message.starts_with("forge: Op did not decode: "),
                "{refused}"
            );
        }
    }
}
