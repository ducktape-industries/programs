//! What blobs tells an agent of its ops and queries. Both enums are blob's contract, defined in
//! the blob crate, so markers carry each variant's words and a working value of it.
use ::program::tools::{Described, Tool, example};
use abi::BlobId;

use crate::{Entry, Op, Query};

pub(crate) struct Ops;

pub(crate) struct Queries;

fn op(variant: &'static str, description: &'static str, value: Op) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: "",
    }
}

fn query(variant: &'static str, description: &'static str, value: Query) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: variant,
    }
}

fn blob() -> BlobId {
    BlobId::Sha256([0; 32])
}

impl Described for Ops {
    const ABOUT: &'static str = "Custody of blobs: an account puts a numbered bundle of blobs by id, length and kind, each current validator's node key receipts it once its node holds every blob, and a quorum of receipts marks the bundle held.";

    fn tools() -> Vec<Tool> {
        vec![
            op(
                "Put",
                "Put a bundle of one or more blobs, each by id, length in bytes and kind (one word, such as program or chunk), as the sender. Returns the bundle's number; its custody starts receipted by no one.",
                Op::Put {
                    blobs: vec![Entry {
                        id: blob(),
                        len: 1024,
                        kind: "chunk".into(),
                    }],
                },
            ),
            op(
                "Receipt",
                "A current validator's node key says its node holds every blob of bundle, once per key; the receipt that brings the current validators' receipts to a quorum marks the bundle held from this height.",
                Op::Receipt { bundle: 0 },
            ),
        ]
    }
}

impl Described for Queries {
    fn tools() -> Vec<Tool> {
        vec![
            query(
                "Held",
                "Where a blob is held: the first held bundle that names it, with its length, kind and the height it was held since; null while no bundle naming it is held.",
                Query::Held { blob: blob() },
            ),
            query(
                "Bundle",
                "Bundle n: who put it and at what height, its blobs, and its custody (the keys that receipted it, or the height it was held since); null for no such bundle.",
                Query::Bundle { n: 0 },
            ),
        ]
    }
}
