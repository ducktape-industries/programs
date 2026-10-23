//! The node daemon's `/v1` wire, mirrored field for field from the app's
//! copy (`crates/noded-client/src/wire.rs` in the app, itself a copy of
//! `crates/noded/src/wire.rs` and `crates/kernel/node/src/frame.rs` in
//! ducktape): borsh in and out, and the signed frame every write and query
//! carries. A field that moves there moves here.

use abi::{BlobId, ProgramId, Roles, Root};
use borsh::{BorshDeserialize, BorshSerialize};
use program::abi;

pub const NODE_CONTRACT: u32 = 9;
pub const FRAME_NAMESPACE: &[u8] = b"ducktape:frame";

pub mod route {
    pub const STATUS: &str = "/v1/status";
    pub const SUBMIT: &str = "/v1/submit";
    pub const QUERY: &str = "/v1/query";
    pub const BLOB_GET: &str = "/v1/blob/get";
    pub const CHANGES: &str = "/v1/changes";
    pub const BLOCKS: &str = "/v1/blocks";
    pub const BLOCK: &str = "/v1/block";
    pub const TX: &str = "/v1/tx";
    pub const BLOCK_FACTS: &str = "/v1/blocks/facts";
    pub const TXS: &str = "/v1/txs";
    pub const NETWORK: &str = "/v1/network";
    /// JSON, the one route the app reaches through `ducktape-rpc`.
    pub const INVITE: &str = "/v1/invite";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Layer {
    Confirmed,
    Preconfirmed,
}

/// The signer's scheme, as a frame body encodes it: core's own type, as
/// the app's copy uses it.
pub use keyscheme::KeyScheme;

#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Tip {
    pub height: u64,
    pub id: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Body {
    pub scheme: KeyScheme,
    pub signer: Vec<u8>,
    pub network: Vec<u8>,
    pub anchor: Tip,
    pub salt: Option<u64>,
    pub target: String,
    pub payload: Vec<u8>,
}

impl Body {
    /// What the signer signs under [`FRAME_NAMESPACE`].
    pub fn preimage(&self) -> Vec<u8> {
        abi::encode(self)
    }

    pub fn id(&self) -> [u8; 32] {
        use sha2::Digest as _;
        let submission = (
            &self.signer,
            self.anchor,
            self.salt,
            &self.target,
            &self.payload,
        );
        sha2::Sha256::digest(abi::encode(&submission)).into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Frame {
    pub body: Body,
    pub proof: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Status {
    pub network: String,
    /// The chain's founding time (its genesis block's), unix ms.
    pub time: u64,
    pub block_time_ms: u64,
    pub epoch_length: u64,
    pub height: u64,
    pub tip: [u8; 32],
    pub root: Root,
    pub epoch: u64,
    pub identity: Vec<u8>,
    pub contract: u32,
    pub genesis: [u8; 32],
    pub roles: Roles,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Network {
    pub height: u64,
    pub members: Vec<PeerStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct PeerStatus {
    pub key: Vec<u8>,
    pub signed: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Query {
    pub layer: Layer,
    pub frame: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Receipt {
    pub program: ProgramId,
    pub outcome: abi::Outcome,
    pub events: Vec<Vec<u8>>,
    pub nested: Vec<Receipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Change {
    pub height: u64,
    pub root: Root,
    pub writes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Blocks {
    pub before: Option<u64>,
    pub limit: u32,
    pub most: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum BlockRef {
    Height(u64),
    Id([u8; 32]),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct BlockAsk {
    pub by: BlockRef,
    pub most: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Tx {
    pub hash: [u8; 32],
    pub signer: Vec<u8>,
    pub anchor: Tip,
    pub target: String,
    pub size: u32,
    pub payload: Option<Vec<u8>>,
    pub receipt: Option<Receipt>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Page {
    pub blocks: Vec<Finalized>,
    pub listed_from: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TxRef {
    At { height: u64, position: u32 },
    Hash([u8; 32]),
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TxAsk {
    pub tx: TxRef,
    pub most: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Located {
    pub height: u64,
    pub position: u32,
    pub time: u64,
    pub tx: Tx,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Finalized {
    pub height: u64,
    pub id: [u8; 32],
    pub parent: [u8; 32],
    pub time: u64,
    pub epoch: u64,
    pub proposer: Option<Vec<u8>>,
    pub txs: Vec<Tx>,
}

/// A blob as the node frames it, `kind len\0body`: the bytes a blob id
/// hashes and `/v1/blob/get` answers.
pub fn framed(kind: &str, body: &[u8]) -> Vec<u8> {
    let mut framed = format!("{kind} {}\0", body.len()).into_bytes();
    framed.extend_from_slice(body);
    framed
}

pub fn blob_id(framed: &[u8]) -> BlobId {
    use sha2::Digest as _;
    BlobId::Sha256(sha2::Sha256::digest(framed).into())
}

/// The block's view-wire shape and the archive's are the same fields; a
/// receipt's outcome carries the refusal as the program wrote it on one
/// side and as a view reads it on the other.
pub fn finalized(block: &view_wire::methods::Block) -> Finalized {
    Finalized {
        height: block.height,
        id: block.id,
        parent: block.parent,
        time: block.time,
        epoch: block.epoch,
        proposer: block.proposer.clone(),
        txs: block.txs.iter().map(tx).collect(),
    }
}

pub fn tx(tx: &view_wire::methods::Tx) -> Tx {
    Tx {
        hash: tx.hash,
        signer: tx.signer.clone(),
        anchor: Tip {
            height: tx.anchor.height,
            id: tx.anchor.id,
        },
        target: tx.target.clone(),
        size: tx.size,
        payload: tx.payload.clone(),
        receipt: tx.receipt.as_ref().map(receipt),
    }
}

pub fn receipt(receipt: &view_wire::methods::Receipt) -> Receipt {
    use view_wire::methods::Outcome;
    Receipt {
        program: receipt.program.clone(),
        outcome: match &receipt.outcome {
            Outcome::Applied { output } => abi::Outcome::Applied {
                output: output.clone(),
            },
            Outcome::Rejected(refusal) => {
                abi::Outcome::Rejected(abi::Refusal::new(&refusal.code, &refusal.message))
            }
        },
        events: receipt.events.clone(),
        nested: receipt.nested.iter().map(self::receipt).collect(),
    }
}
