//! The words every view says of a host's answer: a refusal, and a write
//! that did not land. One table, so no view words them its own way (the
//! Expo app's `src/write/words.ts:72-85`).

use crate::host::Error;
use crate::methods::{Submitted, refusal};

/// A refusal as a person reads it: the program's or the node's own
/// sentence, but a program the network does not run comes back as its bare
/// name (`unknown_program` carries only that), so it is said in full
/// (`crates/mobile-core/src/lib.rs:104-118`): "This network runs no program
/// called activity."
pub fn sentence(error: &Error) -> String {
    match error.code.as_str() {
        ::error::code::UNKNOWN_PROGRAM => {
            format!("This network runs no program called {}.", error.message)
        }
        _ => error.message.clone(),
    }
}

/// What a write that did not land says, or `None` when it landed
/// (`src/write/words.ts:77-85`): refused, in the program's or the node's
/// words, or by the person (a cancelled confirm, `consent_refused`); in
/// doubt, since it may have landed; not sent, because no node was
/// reached, no node is connected, or the host refused it before the send.
/// A view shows it where the write was asked for.
pub fn failure(answer: &Result<Submitted, Error>) -> Option<String> {
    match answer {
        Ok(Submitted::Applied { .. }) => None,
        Ok(Submitted::Refused { refusal, .. }) => Some(did_not_go_through(refusal)),
        Ok(Submitted::InDoubt { .. }) => Some(IN_DOUBT.into()),
        Err(error) => Some(unsent(error)),
    }
}

const IN_DOUBT: &str =
    "This may have gone through: no answer came back. What you see is read again from the node.";

fn did_not_go_through(error: &Error) -> String {
    format!("That didn’t go through: {}", sentence(error))
}

fn unsent(error: &Error) -> String {
    match error.code.as_str() {
        refusal::CONSENT_REFUSED => did_not_go_through(error),
        refusal::RPC_CLIENT => {
            "Not sent: the node can’t be reached. Check the connection, then try again.".into()
        }
        refusal::NOT_CONNECTED => "Not sent: not connected to a node.".into(),
        refusal::NODE_FAILED | refusal::REQUEST_CLOSED => IN_DOUBT.into(),
        code if refusal::ALL.contains(&code) => format!("Not sent: {}", sentence(error)),
        _ => did_not_go_through(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(code: &str, message: &str) -> Error {
        Error::new(code, message)
    }

    #[test]
    fn a_refusal_says_its_own_sentence_but_a_missing_program_in_full() {
        assert_eq!(
            sentence(&error("not_found", "No such room.")),
            "No such room."
        );
        assert_eq!(
            sentence(&error("unknown_program", "activity")),
            "This network runs no program called activity."
        );
    }

    #[test]
    fn a_write_that_did_not_land_says_why_by_what_is_known_of_it() {
        let frame = [0; 32];
        assert_eq!(
            failure(&Ok(Submitted::Applied {
                frame,
                until: 9,
                output: Vec::new(),
            })),
            None
        );
        let said = |answer| failure(&answer).unwrap();
        assert_eq!(
            said(Ok(Submitted::Refused {
                frame,
                until: 9,
                refusal: error("wrong_state", "The room is closed."),
            })),
            "That didn’t go through: The room is closed."
        );
        assert_eq!(
            said(Ok(Submitted::InDoubt { frame, until: 9 })),
            "This may have gone through: no answer came back. What you see is read again from the node."
        );
        assert_eq!(
            said(Err(error(refusal::RPC_CLIENT, "connection refused"))),
            "Not sent: the node can’t be reached. Check the connection, then try again."
        );
        assert_eq!(
            said(Err(error(refusal::NOT_CONNECTED, "no node"))),
            "Not sent: not connected to a node."
        );
        assert_eq!(
            said(Err(error(refusal::SESSION_LOCKED, "Unlock a key first."))),
            "Not sent: Unlock a key first."
        );
        assert_eq!(
            said(Err(error(
                refusal::CONSENT_REFUSED,
                "the person did not confirm"
            ))),
            "That didn’t go through: the person did not confirm"
        );
        assert_eq!(
            said(Err(error(refusal::REQUEST_CLOSED, "closed"))),
            "This may have gone through: no answer came back. What you see is read again from the node."
        );
        assert_eq!(
            said(Err(error("unknown_program", "activity"))),
            "That didn’t go through: This network runs no program called activity."
        );
    }
}
