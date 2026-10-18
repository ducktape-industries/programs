// Packfile reading and writing, shared by the server flows and the fetch response.

mod delta;
mod read;
mod write;

pub use read::read;
pub use write::{Deflate, PackWriter, write};

use crate::object::Kind;

pub(crate) const SIGNATURE: &[u8; 4] = b"PACK";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_objects: usize,
    pub max_delta_depth: usize,
    pub max_object_size: usize,
}

impl Limits {
    pub fn generous() -> Limits {
        Limits {
            max_objects: usize::MAX,
            max_delta_depth: 64,
            max_object_size: usize::MAX,
        }
    }
}

pub(crate) fn kind_code(kind: Kind) -> u8 {
    match kind {
        Kind::Commit => 1,
        Kind::Tree => 2,
        Kind::Blob => 3,
        Kind::Tag => 4,
    }
}

pub(crate) fn kind_from_code(code: u8) -> Option<Kind> {
    match code {
        1 => Some(Kind::Commit),
        2 => Some(Kind::Tree),
        3 => Some(Kind::Blob),
        4 => Some(Kind::Tag),
        _ => None,
    }
}
