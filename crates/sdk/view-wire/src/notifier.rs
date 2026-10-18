//! The calling convention between the host and a NOTIFIER: the small
//! headless module a program ships beside its view (the `ducktape.notify`
//! section of its code) and the host runs once per network, whether or not
//! a pane shows the view. It draws nothing and hears no input: it follows
//! what it names, asks what it asks, and posts what its program decides is
//! a notice. A plain core wasm module, bytes through its own memory, as a
//! view's ([`crate::abi`]):
//!
//! ```text
//! import ducktape_view.panicked(ptr: u32, len: u32)
//! export alloc(len: u32) -> u32
//! export init()
//! export tick(ptr: u32, len: u32) -> u64
//! ```
//!
//! `tick` takes the borsh of `Vec<`[`Answer`]`>` (the answers and stream
//! items owed to it, in order) and answers the borsh of a [`Turn`], packed
//! as a view's answer is ([`crate::abi::pack`]). Borsh both ways, the codec
//! a method's data is in: a notifier links neither the tree nor the events
//! a view's frame codec carries. There is no snapshot: what a notifier keeps
//! across runs it keeps in its store (`store.set`), and a new deployment
//! starts it again.
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{Error, Request};

/// One answer the host owes a notifier: to request `id`, or one item of
/// the subscription `id` (`done` ends it), as a view's `Event::Response`.
#[derive(Clone, Debug, PartialEq, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Answer {
    pub id: u64,
    pub result: Result<Vec<u8>, Error>,
    pub done: bool,
}

/// What one tick of a notifier produced: what it asked, what it stopped
/// waiting on, and whether it ran out of budget with work still ready (a
/// view's `Frame` with no tree).
#[derive(Clone, Debug, Default, PartialEq, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Turn {
    pub requests: Vec<Request>,
    pub cancels: Vec<u64>,
    pub busy: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_and_its_answers_round_trip() {
        let turn = Turn {
            requests: vec![Request {
                id: 3,
                kind: "notify.post".into(),
                payload: vec![1, 2],
            }],
            cancels: vec![2],
            busy: true,
        };
        let answers = vec![
            Answer {
                id: 3,
                result: Ok(vec![0]),
                done: true,
            },
            Answer {
                id: 4,
                result: Err(Error::new("not_connected", "no node")),
                done: true,
            },
        ];
        assert_eq!(
            borsh::from_slice::<Turn>(&borsh::to_vec(&turn).unwrap()).unwrap(),
            turn
        );
        assert_eq!(
            borsh::from_slice::<Vec<Answer>>(&borsh::to_vec(&answers).unwrap()).unwrap(),
            answers
        );
    }
}
