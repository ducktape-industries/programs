// The single error type every git function returns.

use crate::object::Kind;
use crate::oid::{Hash, Oid};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Truncated,
    BadHeader,
    UnknownObjectKind,
    BadHex,
    BadOidLength,
    WrongHash { expected: Hash, actual: Hash },
    Collision,
    MissingBase(Oid),
    MissingObject(Oid),
    WrongKind { id: Oid, expected: Kind },
    Cycle,
    BadPktLine,
    BadPackHeader,
    UnsupportedPackVersion(u32),
    BadDelta,
    Inflate,
    BadChecksum,
    UnsortedTree,
    BadMode,
    BadSignature,
    BadCommit,
    BadTag,
    CapReached,
    Unsupported,
    TooManyObjects,
    ObjectTooLarge,
    DeltaTooDeep,
    BadRequest,
    UnknownCommand,
    NotAdvertised(Oid),
    Storage,
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Truncated => f.write_str("truncated input"),
            Error::BadHeader => f.write_str("bad object header: supply an object header with its kind and byte length"),
            Error::UnknownObjectKind => f.write_str("unknown object kind: use a commit, tree, blob or tag object"),
            Error::BadHex => f.write_str("bad hex: use pairs of hex digits"),
            Error::BadOidLength => f.write_str("bad object id length"),
            Error::WrongHash { expected, actual } => write!(
                f,
                "wrong hash algorithm: expected {}, got {}",
                expected.name(),
                actual.name()
            ),
            Error::Collision => f.write_str("sha1 collision detected"),
            Error::MissingBase(id) => write!(f, "missing delta base {id}: include the delta base in the pack or upload it first"),
            Error::MissingObject(id) => write!(f, "missing object {id}"),
            Error::WrongKind { id, expected } => write!(f, "{id} is not a {}", expected.as_str()),
            Error::Cycle => f.write_str("cycle"),
            Error::BadPktLine => f.write_str("bad pkt-line"),
            Error::BadPackHeader => f.write_str("bad pack header"),
            Error::UnsupportedPackVersion(v) => write!(f, "unsupported pack version {v}: send a pack using version 2 or 3"),
            Error::BadDelta => f.write_str("bad delta"),
            Error::Inflate => f.write_str("inflate failed"),
            Error::BadChecksum => f.write_str("bad pack checksum: rebuild or retransmit the pack with its correct checksum"),
            Error::UnsortedTree => f.write_str("unsorted tree: sort the tree entries in Git order before encoding"),
            Error::BadMode => f.write_str("bad tree entry mode"),
            Error::BadSignature => f.write_str("bad signature line: encode the signature line as name, email, timestamp and timezone"),
            Error::BadCommit => f.write_str("bad commit: supply a commit with valid tree, parent and signature headers"),
            Error::BadTag => f.write_str("bad tag: supply a tag with valid object, type and tag headers"),
            Error::CapReached => f.write_str("walk cap reached"),
            Error::Unsupported => f.write_str("unsupported"),
            Error::TooManyObjects => f.write_str("too many objects"),
            Error::ObjectTooLarge => f.write_str("object too large"),
            Error::DeltaTooDeep => f.write_str("delta chain too deep: repack with a shallower delta chain"),
            Error::BadRequest => f.write_str("bad request"),
            Error::UnknownCommand => f.write_str("unknown command"),
            Error::NotAdvertised(id) => write!(f, "not our ref {id}: fetch the advertised refs and choose one of their object ids"),
            Error::Storage => f.write_str("storage failure"),
        }
    }
}
