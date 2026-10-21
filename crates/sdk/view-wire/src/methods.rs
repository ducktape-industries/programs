//! Every kind a view may ask its host for, with the request and reply each
//! carries. This is the ONE list: a view names a method by its type, the host
//! answers by the same type, and a kind that is not here is a compile error
//! on one side and [`refusal::UNKNOWN_REQUEST`] on the other. [`ALL`] is
//! what a host test checks its handlers against, and [`refusal`] lists every
//! code a host refuses one with.
//!
//! THE CODEC RULE. Two layers cross the view boundary and they want
//! opposite things. The tree a view draws (`Frame`, [`WidgetCommand`]) must
//! tolerate a hundred optional fields — that is MessagePack, in `codec`.
//! Everything a method carries is DATA: the bytes a program signs, stores
//! or answers with, where the same value must
//! be the same bytes and an unknown field is a fault. That is borsh, the
//! codec the program abi is written in, so a program's own request rides a
//! method with no second encoding around it. The rule is held by types, not
//! by review: [`Method`] is sealed, so a view cannot declare a kind or pick a
//! codec, and [`Program`]'s bounds are borsh, so a program that speaks
//! anything else does not have a method.
//!
//! ABSENT is `None`, never a refusal: a method whose thing may not exist replies `Option`, and a refusal means the ask itself failed.
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::WidgetCommand;
pub use describe::{Description, Field, Value};
pub use program::Program;

mod sealed {
    pub trait Sealed {}
}

/// One kind a view may ask for. Sealed: the methods are the ones in this
/// program.
pub trait Method: sealed::Sealed {
    const KIND: &'static str;
    /// The program a node method is addressed to, so a test host can key on
    /// it; `None` for the host's own methods.
    const TARGET: Option<&'static str> = None;
    type Request: std::fmt::Debug;
    type Reply;
    fn encode_request(request: &Self::Request) -> Vec<u8>;
    fn decode_request(bytes: &[u8]) -> Result<Self::Request, String>;
    fn encode_reply(reply: &Self::Reply) -> Vec<u8>;
    fn decode_reply(bytes: &[u8]) -> Result<Self::Reply, String>;
}

pub fn encode<T: BorshSerialize>(value: &T) -> Vec<u8> {
    borsh::to_vec(value).expect("borsh encodes an in-memory value")
}

pub fn decode<T: BorshDeserialize>(bytes: &[u8]) -> Result<T, String> {
    borsh::from_slice(bytes).map_err(|error| error.to_string())
}

macro_rules! method {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $request:ty, $reply:ty) => {
        $(#[$doc])*
        pub struct $name;
        impl sealed::Sealed for $name {}
        impl Method for $name {
            const KIND: &'static str = $kind;
            type Request = $request;
            type Reply = $reply;
            fn encode_request(request: &$request) -> Vec<u8> {
                encode(request)
            }
            fn decode_request(bytes: &[u8]) -> Result<$request, String> {
                decode(bytes)
            }
            fn encode_reply(reply: &$reply) -> Vec<u8> {
                encode(reply)
            }
            fn decode_reply(bytes: &[u8]) -> Result<$reply, String> {
                decode(bytes)
            }
        }
    };
}

// What [`each_listed`] hands every method the list below declares, by
// type: the golden's schema reads each one's request and reply through it.
#[cfg(feature = "schema")]
pub trait Listed {
    fn method<M: Method>(&mut self)
    where
        M::Request: borsh::BorshSchema,
        M::Reply: borsh::BorshSchema;
}

macro_rules! methods {
    (
        also: [$($also:expr),* $(,)?];
        $($(#[$doc:meta])* $name:ident, $kind:literal, $request:ty, $reply:ty;)*
    ) => {
        $(method!($(#[$doc])* $name, $kind, $request, $reply);)*

        /// Every kind, so a host can assert it answers each one.
        pub const ALL: &[&str] = &[$($also,)* $($kind),*];

        // Each method the list declares, in its order, to `listed`.
        #[cfg(feature = "schema")]
        pub fn each_listed(listed: &mut impl Listed) {
            $(listed.method::<$name>();)*
        }
    };
}

// ---------- the node ----------

// A node method is addressed to a [`Program`]: the program crate's own
// impl, on the type that is its `program::Program` (`Query<chat::Chat>`).

/// The envelope of a node method: the program addressed and the bytes it
/// gets, which the host signs into a frame without reading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Call {
    pub target: String,
    #[serde(with = "crate::codec::bin")]
    pub body: Vec<u8>,
}

fn decode_call<T: BorshDeserialize>(bytes: &[u8], target: &str) -> Result<T, String> {
    let call: Call = decode(bytes)?;
    if call.target != target {
        return Err(format!("expected target {target}, got {}", call.target));
    }
    decode(&call.body)
}

/// `program.query`: one query to `P`, answered with the bytes it `Respond`ed.
pub struct Query<P>(std::marker::PhantomData<P>);
impl<P: Program> sealed::Sealed for Query<P> {}
impl<P: Program> Method for Query<P> {
    const KIND: &'static str = "program.query";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = P::Query;
    type Reply = P::Reply;
    fn encode_request(request: &P::Query) -> Vec<u8> {
        encode(&Call {
            target: P::NAME.into(),
            body: encode(request),
        })
    }
    fn decode_request(bytes: &[u8]) -> Result<P::Query, String> {
        decode_call(bytes, P::NAME)
    }
    fn encode_reply(reply: &P::Reply) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<P::Reply, String> {
        decode(bytes)
    }
}

/// `op.submit`: one operation to `P`, signed with the seated key, asked of
/// the node as a preconfirm (node contract 6: the app's `submitted()` in
/// `src/runtime/kernel/node.rs` sends every frame with `Preconfirm::On`).
/// The reply is a [`Submitted`]: what the node predicts, and the frame's
/// identity for the view to settle it by. `Err` means nothing was pooled:
/// the bytes never left (`RPC_CLIENT`), the node refused the frame
/// outright (a bad signature, a stale anchor), or the host refused the
/// ask. This method never answers [`NODE_FAILED`]: an answer that went
/// missing after the send is [`Submitted::InDoubt`].
pub struct Submit<P>(std::marker::PhantomData<P>);

/// What `op.submit` answers. Every arm carries the frame's identity:
/// `frame` is the id a block carries it under (`chain.tx` looks it up by
/// hash), and `until` is the last height a block may carry it, the anchor
/// the host signed it against plus the node's anchor window (256 on core
/// `main`: `crates/kernel/host/src/lib.rs`). Past `until` with the frame in
/// no block, the write lapsed and a resend is safe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Submitted {
    /// The node pooled the frame and ran it over the preconfirmed layer:
    /// `output` is the program's own bytes from that run, and a `Query`
    /// reads the write as landed from now on. A prediction, not finality:
    /// the node re-runs the frame after each block until one carries it,
    /// and can still drop it (a block that lands first and makes it fail,
    /// a node restart) with no word to the view.
    Applied {
        frame: [u8; 32],
        until: u64,
        output: Vec<u8>,
    },
    /// The node pooled the frame and predicts the program refuses it
    /// (`refusal` is the program's own). Not final either: the pool admits
    /// a frame whatever the prediction says (core `crates/kernel/node`), so
    /// a block may still carry it and run it to another outcome; a manual
    /// resend before `until` could land it twice.
    Refused {
        frame: [u8; 32],
        until: u64,
        refusal: crate::Error,
    },
    /// The bytes were sent and no answer came back: the node may hold the
    /// frame, and may have preconfirmed it. The view judges by
    /// `chain.tx(frame)` per block until `until`, and words the doubt
    /// itself.
    InDoubt { frame: [u8; 32], until: u64 },
}
impl<P: Program> sealed::Sealed for Submit<P> {}
impl<P: Program> Method for Submit<P> {
    const KIND: &'static str = "op.submit";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = P::Op;
    type Reply = Submitted;
    fn encode_request(request: &P::Op) -> Vec<u8> {
        encode(&Call {
            target: P::NAME.into(),
            body: encode(request),
        })
    }
    fn decode_request(bytes: &[u8]) -> Result<P::Op, String> {
        decode_call(bytes, P::NAME)
    }
    fn encode_reply(reply: &Submitted) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<Submitted, String> {
        decode(bytes)
    }
}

/// One block's writes to a program, as `program.changes` carries them: the
/// block's height and every key it wrote under the program, as the node
/// publishes them. A view asks it which of its reads moved
/// ([`Change::touches`]) and re-reads those.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Change {
    pub height: u64,
    pub keys: Vec<Vec<u8>>,
}

impl Change {
    /// Whether this block can have changed the answer to `query`: it wrote
    /// a key of a table the query reads, as the program declares them
    /// ([`program::Reads`]). `P` is the program whose block this is, the
    /// one the `Changes<P>` subscription that carried it names: a query
    /// answered partly from another program's tables is touched by that
    /// program's blocks too. A query whose program declares nothing for it
    /// is touched by every block.
    pub fn touches<P: Program, Q: program::Reads + ?Sized>(&self, query: &Q) -> bool {
        query.touched_by(P::NAME, &self.keys)
    }
}

/// `program.changes`: a subscription to `P`, one item per block that wrote to
/// it, carrying the block's height and the keys it wrote ([`Change`]);
/// `None` when the node link was reopened and the view should re-read
/// everything. The request is `P`'s name, as the host reads it.
// The host also says `None` when the view comes back on screen or its app
// back in front: it follows the node only for a view on screen (a notifier:
// always) of an app in front.
pub struct Changes<P>(std::marker::PhantomData<P>);
impl<P: Program> sealed::Sealed for Changes<P> {}
impl<P: Program> Method for Changes<P> {
    const KIND: &'static str = "program.changes";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = ();
    type Reply = Option<Change>;
    fn encode_request(_: &()) -> Vec<u8> {
        encode(&P::NAME.to_owned())
    }
    fn decode_request(bytes: &[u8]) -> Result<(), String> {
        let name: String = decode(bytes)?;
        match name == P::NAME {
            true => Ok(()),
            false => Err(format!("expected program {}, got {name}", P::NAME)),
        }
    }
    fn encode_reply(reply: &Option<Change>) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<Option<Change>, String> {
        decode(bytes)
    }
}

// `program.query` to a program the view knows by name only when it runs (a
// chat widget's source): the envelope `Query<P>` sends, answered with the
// program's reply bytes as they are, which the view reads by the program's
// schema. The same kind and bytes on the wire as `Query<P>`.
pub struct NamedQuery;
impl sealed::Sealed for NamedQuery {}
impl Method for NamedQuery {
    const KIND: &'static str = "program.query";
    type Request = Call;
    type Reply = Vec<u8>;
    fn encode_request(request: &Call) -> Vec<u8> {
        encode(request)
    }
    fn decode_request(bytes: &[u8]) -> Result<Call, String> {
        decode(bytes)
    }
    fn encode_reply(reply: &Vec<u8>) -> Vec<u8> {
        reply.clone()
    }
    fn decode_reply(bytes: &[u8]) -> Result<Vec<u8>, String> {
        Ok(bytes.to_vec())
    }
}

// `program.changes` of a program named when the view runs: the request is
// its name, as `Changes<P>` sends it, and the items are `Changes<P>`'s.
pub struct NamedChanges;
impl sealed::Sealed for NamedChanges {}
impl Method for NamedChanges {
    const KIND: &'static str = "program.changes";
    type Request = String;
    type Reply = Option<Change>;
    fn encode_request(request: &String) -> Vec<u8> {
        encode(request)
    }
    fn decode_request(bytes: &[u8]) -> Result<String, String> {
        decode(bytes)
    }
    fn encode_reply(reply: &Option<Change>) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<Option<Change>, String> {
        decode(bytes)
    }
}

/// `program.watch`: a standing query to `P`, one item per answer the node pushes, `None` to read it again.
pub struct Watch<P>(std::marker::PhantomData<P>);
impl<P: Program> sealed::Sealed for Watch<P> {}
impl<P: Program> Method for Watch<P> {
    const KIND: &'static str = "program.watch";
    const TARGET: Option<&'static str> = Some(P::NAME);
    type Request = P::Query;
    type Reply = Option<P::Reply>;
    fn encode_request(request: &P::Query) -> Vec<u8> {
        encode(&Call {
            target: P::NAME.into(),
            body: encode(request),
        })
    }
    fn decode_request(bytes: &[u8]) -> Result<P::Query, String> {
        decode_call(bytes, P::NAME)
    }
    fn encode_reply(reply: &Option<P::Reply>) -> Vec<u8> {
        encode(reply)
    }
    fn decode_reply(bytes: &[u8]) -> Result<Option<P::Reply>, String> {
        decode(bytes)
    }
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct NodeStatus {
    pub chain_id: String,
    pub time: u64,
    pub block_time_ms: u64,
    pub epoch_length: u64,
    pub height: u64,
    pub tip: [u8; 32],
    pub root: [u8; 32],
    pub epoch: u64,
    pub identity: Vec<u8>,
    pub contract: u32,
}
/// Every member of the epoch as the connected node sees it: its applied
/// tip, and the members in key order. No member's `signed` exceeds `height`.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct NetworkStatus {
    pub height: u64,
    pub members: Vec<Peer>,
}
/// One member: the newest block the connected node applied that this key
/// sent a finalize vote for, as the node's consensus engine heard it. A
/// vote for a block not applied yet counts once it is. `None` for a
/// resident, for a validator not heard since the node started or began
/// validating, and for every member while the node is not itself seated as
/// a validator: it runs no engine, so it hears no votes.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Peer {
    pub key: Vec<u8>,
    pub signed: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct CreateInvite {
    pub ttl_days: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Invite {
    pub invite: String,
    pub notes: Vec<crate::Error>,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct BlockPage {
    pub before: Option<u64>,
    pub limit: u32,
    pub most: u32,
}
/// One finalized block, by height or by its id (the block digest).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum BlockRef {
    Height(u64),
    Id([u8; 32]),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct BlockAsk {
    pub by: BlockRef,
    pub most: u32,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Tx {
    pub hash: [u8; 32],
    pub signer: Vec<u8>,
    pub anchor: Tip,
    pub target: String,
    pub size: u32,
    pub payload: Option<Vec<u8>>,
    pub receipt: Option<Receipt>,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct BlockList {
    pub blocks: Vec<Block>,
    pub listed_from: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum TxRef {
    At { height: u64, position: u32 },
    Hash([u8; 32]),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct TxAsk {
    pub tx: TxRef,
    pub most: u32,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Located {
    pub height: u64,
    pub position: u32,
    pub time: u64,
    pub tx: Tx,
}
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Tip {
    pub height: u64,
    pub id: [u8; 32],
}
/// One run: the program's, how it ended, what it announced, and the runs
/// its messages caused, in order. A nested run's `Applied` stands only
/// where every run above it applied too: an ancestor's rejection undid it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Receipt {
    pub program: String,
    pub outcome: Outcome,
    pub events: Vec<Vec<u8>>,
    pub nested: Vec<Receipt>,
}
/// How a run ended: applied with the program's output, or rejected with
/// its refusal (`code` the refusal's reason token, `message` its sentence).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Outcome {
    Applied { output: Vec<u8> },
    Rejected(crate::Error),
}
/// A finalized block as the node's archive keeps it. `proposer` is the
/// validator key that led its round, where the node holds its certificate.
/// No state root or writes: the node keeps neither per height.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Block {
    pub height: u64,
    pub id: [u8; 32],
    pub parent: [u8; 32],
    pub time: u64,
    pub epoch: u64,
    pub proposer: Option<Vec<u8>>,
    pub txs: Vec<Tx>,
}
/// One finalized head, as `chain.heads` pushes it.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Head {
    pub height: u64,
    pub time: u64,
    pub id: [u8; 32],
}
// ---------- the host ----------

/// The session facts every view is handed: the connection, the
/// network (`<label>#<salt>`), the seated key (hex), the account it belongs
/// to (`None` while the host has not resolved one, or the key has none yet;
/// an item follows when that changes), and the read-only endpoint.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Session {
    pub connected: bool,
    pub chain_id: String,
    pub signer: String,
    pub account: Option<u64>,
    pub endpoint: String,
}
/// `host.float`'s request: this view's pane in a window of its own, the
/// view laid out `width` by `height` (the window's body, under its title
/// bar) and never under `min_width` by `min_height`, kept above other
/// windows when `on_top`. Sizes are logical pixels, the ones the view lays
/// out in. The person resizes the window down to the minimum, never under
/// it; a minimum of zero or over its side, or a side past the largest
/// window the host makes, is refused `malformed_request`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Float {
    pub width: u32,
    pub height: u32,
    pub min_width: u32,
    pub min_height: u32,
    pub on_top: bool,
}
/// Where this view's pane is: `floating` in a window `host.float` made for
/// it, and whether that window is kept above other windows.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Placement {
    pub floating: bool,
    pub on_top: bool,
}
/// A place of the host's own that a view asks it to bring up
/// ([`HostShow`]). What a place looks like is the host's to decide on its
/// platform: a sheet, a menu, a page. A place this host does not know does
/// not decode, so the request is refused `malformed_request`; a place it
/// knows and has nothing to show for (no reader's account on a host that
/// keeps none) is refused `unknown_request`, as a host that does not
/// answer `host.show` at all refuses it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Place {
    /// The reader's own account: who they are, on which network, and the
    /// way to their account, the app's settings and another network (the
    /// Expo app's account sheet, mobile `app/you.tsx`), asked for from the
    /// reader's avatar on a view's first page.
    Account,
    /// The system's page that allows this app the camera: macOS's privacy
    /// pane for it, the app's own settings on iOS and Android. Asked for
    /// where a refused camera piece's [`crate::MediaState::opens_settings`]
    /// says the host shows it; refused `unknown_request` where it has none.
    CameraSettings,
    /// The system's page that allows this app the microphone, as
    /// [`Place::CameraSettings`] is the camera's.
    MicrophoneSettings,
}
/// `host.widget`: a command on the mounted tree. The one method on the TREE
/// side of the codec rule — a [`WidgetCommand`] names typed element ids the
/// tree is drawn with — so it is the one method in the tree's MessagePack.
pub struct HostWidget;
impl sealed::Sealed for HostWidget {}
impl Method for HostWidget {
    const KIND: &'static str = "host.widget";
    type Request = WidgetCommand;
    type Reply = ();
    fn encode_request(request: &WidgetCommand) -> Vec<u8> {
        crate::encode(request)
    }
    fn decode_request(bytes: &[u8]) -> Result<WidgetCommand, String> {
        crate::decode(bytes)
    }
    fn encode_reply(_: &()) -> Vec<u8> {
        Vec::new()
    }
    fn decode_reply(_: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

// ---------- the device ----------

#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Clipboard {
    pub text: String,
}
/// What `file.pick` asks the person for: files on this device.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Pick {
    /// more than one file may be picked at once
    pub multiple: bool,
}
/// One file the person picked: its own name, never where it is on the
/// device; its size as picked; and the handle a [`BlobBody::Picked`] names
/// it by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Picked {
    pub name: String,
    pub size: u64,
    pub file: u64,
}
/// What `op.put` asks: blobs to put on the network as one bundle, in the
/// person's name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct PutBlobs {
    pub blobs: Vec<NewBlob>,
}
// One blob of a [`PutBlobs`]: its kind (the program that names it reads
// the kind, as files reads `chunk`) and its bytes, of any size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct NewBlob {
    pub kind: String,
    pub body: BlobBody,
}
/// Where a [`NewBlob`]'s bytes come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum BlobBody {
    /// bytes the view holds
    Bytes(Vec<u8>),
    Picked {
        file: u64,
        offset: u64,
        len: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Putting {
    Sent(u64),
    Held(Vec<[u8; 32]>),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct SaveFile {
    pub name: String,
    pub blobs: Vec<[u8; 32]>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Pasted {
    pub files: Vec<Picked>,
    pub text: String,
}
// What `file.read` asks: `len` bytes from `offset` of the file the view
// holds as `file`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct ReadFile {
    pub file: u64,
    pub offset: u64,
    pub len: u64,
}
/// One notice for the host to decide on: `notify.post`. The view asks; the
/// host logs it in its notification centre and decides whether a banner
/// reaches the screen (the person's per-view choice, focus, a burst limit).
/// `link` is a `duck://` link the centre opens when the notice is picked,
/// or empty.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub tag: String,
    pub link: String,
    /// What the notice is to a carrier: a [`NoticeKind::Notice`] is shown
    /// and logged; a [`NoticeKind::Call`] rings (a VoIP push on a phone, a
    /// banner with Answer and Decline on a desktop) until `notify.seen` of
    /// its tag or `until`.
    pub kind: NoticeKind,
    /// Unix seconds past which the notice is stale: a carrier drops it
    /// undelivered, and a ring stops. 0: never.
    pub until: u64,
    /// For a [`NoticeKind::Call`]: a `duck://` link the carrier opens when
    /// the person ends the call they answered from the system's own call
    /// screen (iOS CallKit's end button; later Android's ConnectionService).
    /// Never for a decline, nor for an end made in the app or by the
    /// program. What that hang-up means is the program's to decide, as
    /// `link` decides what Answer means. Empty: nothing is opened.
    pub end_link: String,
}
/// What a [`Notification`] is to the app and to a push server that carry
/// it: they know a call from a notice by this alone, never by its program.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum NoticeKind {
    /// Shown and logged.
    #[default]
    Notice,
    /// Someone is calling: it rings, its `link` answers, and its
    /// `end_link` is opened when the system's call screen ends it.
    Call,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
/// What `notify.needs` hands the host: every item the program has open for the reader now, in
/// place of the list it handed before; an empty list clears them.
pub struct Needs {
    pub items: Vec<Need>,
}
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
/// One open item of [`Needs`]: `key` the program's own stable id for it; `title` what is needed and
/// `body` one more line (or empty); `link` the `duck://` link where it is resolved; `opened_at` unix
/// seconds, chain time (`Notification::until`'s unit); `waits_on` the account waiting; `also_closes` others any of whom closes it.
pub struct Need {
    pub key: String,
    pub title: String,
    pub body: String,
    pub link: String,
    pub opened_at: u64,
    pub waits_on: Option<u64>,
    pub also_closes: Vec<u64>,
}
/// What the host did with a [`Notification`].
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Delivery {
    /// Logged, and a banner was raised.
    Banner,
    /// Logged in the centre only: no banner (not yet allowed, silenced,
    /// in front, over the burst limit, or banners are off).
    Logged,
    /// The person blocked this view's notices: dropped, not logged.
    Blocked,
}

/// One camera, as `camera.devices` lists it: `id` is what a camera
/// piece's `device` names it by, `name` what a picker shows, `default`
/// whether the system picks it when a piece names none, `facing` which way
/// it looks. An id is the host's, good while the camera stays plugged in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct CameraDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub facing: Facing,
}
/// Which way a camera looks, as the system says.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Facing {
    /// At the person in front of the screen: a laptop's built-in camera, a
    /// phone's selfie camera.
    Front,
    /// Away from the person: a phone's or tablet's main camera.
    Back,
    /// Not part of the machine: a USB or network camera, which looks
    /// wherever it is put. Also a camera the system says nothing of.
    External,
}
/// One microphone or speaker, as `microphone.devices` lists it: `id` is
/// what a microphone piece's `device` (an input) or `output` (an output)
/// names it by, `name` what a picker shows, `default` whether the system
/// picks it when a piece names none, `kind` what it is. An id is the
/// host's, good while the device stays plugged in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
    pub kind: AudioKind,
}
/// What an audio device is, as the system says: where an output plays, or
/// which set an input belongs to. A microphone piece's `monitor` howls
/// through a `Speaker`, and not through `Headphones`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum AudioKind {
    /// A loudspeaker: the computer's own, a phone's at its foot, a set of
    /// speakers plugged in. The microphone hears what it plays.
    Speaker,
    /// A phone's earpiece, held to the ear.
    Earpiece,
    /// Headphones or a headset, wired or Bluetooth; for an input, the
    /// headset's microphone.
    Headphones,
    /// Anything else, or a device the system says nothing of: a line in or
    /// out, a built-in or USB microphone.
    Other,
}
/// `microphone.devices`: the microphones (`inputs`) and the speakers
/// (`outputs`) the host can open.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct AudioDevices {
    pub inputs: Vec<AudioDevice>,
    pub outputs: Vec<AudioDevice>,
}

// ---------- live: tracks through the connected node, off consensus ----------

/// What a view publishes or subscribes to. `room` is the view's, one line
/// of plain text (no control character, bidi control or line separator);
/// the host stamps `<chain>/<program>/` in front, so a view names only its
/// own program's rooms, and the whole is at most 192 bytes. `name` is
/// `/`-separated, at most 128 bytes, no segment empty or `..`
/// (`presence/42`, `doc`). Either out of its rule is refused
/// `malformed_request`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct LiveTrack {
    pub room: String,
    pub name: String,
}

impl LiveTrack {
    /// Why the track is out of its rule, as far as it can be told without
    /// the chain and the program the host stamps in front of `room`; `Ok`
    /// when it holds. A host checks the stamped room's length on top.
    pub fn check(&self) -> Result<(), &'static str> {
        use program::live::{MAX_NAMESPACE, MAX_TRACK_NAME};
        let plain = |c: char| {
            !(c.is_control()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
                || matches!(c, '\u{2028}' | '\u{2029}'))
        };
        if self.room.is_empty() || self.room.len() > MAX_NAMESPACE || !self.room.chars().all(plain)
        {
            return Err("a live room is one line of plain text");
        }
        let segments_hold = self
            .name
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "..");
        if self.name.len() > MAX_TRACK_NAME || !segments_hold {
            return Err("a live track's name is `/`-separated segments, none empty or `..`");
        }
        Ok(())
    }
}

/// How a track's objects travel, chosen at `live.publish` for the track's
/// life. `Stream`: ordered and reliable within a group, groups independent,
/// the latest group replayed to a late subscriber. `Datagram`: unordered,
/// unreliable, the newest wins, the newest object replayed.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum LiveMode {
    Stream,
    Datagram,
}

/// `live.publish`'s request. `priority` is the track's, 0 highest, 128 the
/// default: the node sends a subscriber's higher-priority tracks first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct LivePublish {
    pub track: LiveTrack,
    pub mode: LiveMode,
    pub priority: u8,
}

/// Items of a `live.publish` subscription: the track is up while the
/// subscription lives; dropping it unpublishes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Publishing {
    /// The room's program has not granted this account the track yet: the
    /// node holds the ask and takes it on the block that writes the grant,
    /// and what the view sends onto the track meanwhile goes out then. Told
    /// each time the ask starts to wait: when first asked, and when a
    /// reconnect asks again while no row covers it.
    Waiting,
    /// How many are listening (every node counted), told each time it
    /// moves: a track no one subscribes to hears none. 0: stop producing
    /// what costs; a rise in stream mode: start a new group (a keyframe, a
    /// full state), so the newcomer has one to start from.
    Subscribers(u32),
    /// Stream mode: the host could not take an object of `group` (the
    /// track's queue to the node is full) and abandoned the group, never a
    /// single object: the node ends it short for every subscriber
    /// (`GroupEnd { complete: false }`), and what the view sends onto
    /// `group` from now on is dropped. Told once per group. Start a newer
    /// group with a full state (a keyframe); it starts clean. In datagram
    /// mode a full queue drops its oldest datagram, the newest wins, and
    /// nothing is told.
    Cut { group: u64 },
}

/// `live.send`'s request: one object. `group` must not decrease per track,
/// and an object of a newer group closes the one before it. In stream mode
/// `end` closes the group after this object, and an empty `body` with `end`
/// closes it adding none; `object` is ignored (objects are in send order).
/// In datagram mode `object` numbers the object within its group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct LiveSend {
    pub track: LiveTrack,
    pub group: u64,
    pub object: u32,
    #[serde(with = "crate::codec::bin")]
    pub body: Vec<u8>,
    pub end: bool,
}

/// `live.subscribe`'s request: the track, and where in it to start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct LiveSubscribe {
    pub track: LiveTrack,
    pub from: LiveStart,
}

/// Where a subscription starts in its track, as the node names it.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum LiveStart {
    /// The group the node holds first (a late joiner's state), then
    /// everything new.
    Latest,
    /// Only what is published from now on. In stream mode the group in
    /// flight is passed over, and each later one is heard from its first
    /// object; in datagram mode every datagram from now on.
    Next,
}

/// Items of a `live.subscribe` subscription.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum LiveItem {
    /// The room's program has not granted this account the track yet: the
    /// node holds the ask, and `Opened` follows on the block that writes
    /// the grant. Told each time the ask starts to wait, as
    /// [`Publishing::Waiting`] is.
    Waiting,
    /// The track as the node has it: who publishes it, how, and the group
    /// the node replays first. Comes again when the host opened the track
    /// anew (a reconnect, a publisher that published it again): what was
    /// built from the items before it starts over.
    Opened {
        by: u64,
        mode: LiveMode,
        latest: Option<u64>,
    },
    Object {
        group: u64,
        object: u32,
        #[serde(with = "crate::codec::bin")]
        body: Vec<u8>,
    },
    /// A stream-mode group closed (`end` on the publisher's side,
    /// `complete`) or was cut short.
    GroupEnd { group: u64, complete: bool },
}

/// Items of a `live.tracks` subscription.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Watching {
    /// The room's program has not granted this account watching the room
    /// yet: the node holds the ask, and the announces follow on the block
    /// that writes the grant. Told each time the ask starts to wait, as
    /// [`Publishing::Waiting`] is.
    Waiting,
    /// A track of the room came up or went down.
    Announce(LiveAnnounce),
}

/// One track of a room, up or down. A person's presence is their own track
/// being up.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct LiveAnnounce {
    pub name: String,
    pub by: u64,
    pub mode: LiveMode,
    pub up: bool,
}

/// What a code blob says of its view: `blob.manifest`'s answer. A program's
/// code carries its view in the `ducktape.view` section; a view-only
/// entry's blob is the view itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Manifested {
    /// the blob's body, in bytes
    pub size: u64,
    /// `None` for a program whose code carries no view
    pub view: Option<CarriedView>,
}
/// The view a blob carries, by size, and what its manifest declares.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct CarriedView {
    /// the view's bytes (the section's, or the whole blob's for a view-only
    /// entry)
    pub size: u64,
    /// `None` where this host cannot read it: no manifest section, a shape
    /// it does not know, or a capability it does not know.
    pub manifest: Option<crate::manifest::Manifest>,
}
// `host.methods`'s answer: the wire level this host reads
// ([`crate::WIRE_LEVEL`]) and every kind it answers. A host before J-085
// refuses `host.methods` [`refusal::UNKNOWN_REQUEST`], which a view reads
// as a host that says nothing of what it answers: it asks a kind, and
// falls back on `unknown_request`.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct Hosted {
    pub level: u32,
    pub kinds: Vec<String>,
}

impl Hosted {
    // Whether this host answers `kind` (`link.window`).
    pub fn has(&self, kind: &str) -> bool {
        self.kinds.iter().any(|known| known == kind)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct SocketOpen {
    pub socket: u64,
    pub url: String,
    pub protocols: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum SocketItem {
    Opened { protocol: Option<String> },
    Binary(#[serde(with = "crate::codec::bin")] Vec<u8>),
    Text(String),
    Closed { code: u16, reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub struct SocketSend {
    pub socket: u64,
    #[serde(with = "crate::codec::bin")]
    pub body: Vec<u8>,
}
methods! {
    also: ["program.query", "op.submit", "program.changes", HostWidget::KIND, "program.watch"];
    /// `chain.status`: the connected node's status.
    ChainStatus, "chain.status", (), NodeStatus;
    /// `chain.network`: every member as the connected node sees it.
    ChainNetwork, "chain.network", (), NetworkStatus;
    /// `invite.create`: mint one invite, once (never retried).
    InviteCreate, "invite.create", CreateInvite, Invite;
    /// `chain.blocks`: a page of finalized blocks, newest first.
    ChainBlocks, "chain.blocks", BlockPage, BlockList;
    /// `chain.block`: one finalized block; `None` where the node has none by
    /// that name.
    ChainBlock, "chain.block", BlockAsk, Option<Block>;
    ChainTx, "chain.tx", TxAsk, Option<Located>;
    /// `blob.get`: a blob by `sha256:<hex>` or `sha1:<hex>` id, unframed;
    /// `None` where the node holds no blob by that id.
    BlobGet, "blob.get", String, Option<Vec<u8>>;
    // `blob.manifest`: what the view in a code blob declares, by the
    // blob's `sha256:<hex>` or `sha1:<hex>` id: the host reads the manifest
    // as it would to seat the view, without fetching the bytes into the
    // view. Any blob may be named, as with `blob.get`. `None` where the
    // node holds no blob by that id.
    BlobManifest, "blob.manifest", String, Option<Manifested>;
    /// `host.session`: a subscription to [`Session`], an item per change.
    HostSession, "host.session", (), Session;
    /// `host.visible`: whether the view is on screen, an item per change.
    HostVisible, "host.visible", (), bool;
    /// `host.float`: this view's pane in a window of its own, laid out at
    /// the request's size and never under its minimum, kept above other
    /// windows where the platform can. Floating already: only `on_top` is
    /// applied. Honoured at a press or key in the view, as `link.open` is,
    /// else refused [`refusal::NEEDS_GESTURE`]. The reply says what the
    /// host could do: `on_top` is false where it keeps no window on top
    /// (Wayland, always). A host that cannot float a pane (a phone) answers
    /// none of `host.float`, `host.unfloat` and `host.placement`:
    /// [`refusal::UNKNOWN_REQUEST`].
    HostFloat, "host.float", Float, Placement;
    /// `host.unfloat`: the pane back onto the desk it left (the main
    /// window's when that one is gone). Nothing while not floating.
    HostUnfloat, "host.unfloat", (), ();
    /// `host.placement`: a subscription, an item per change of where this
    /// view's pane is ([`Placement`]); the first item is the state at
    /// subscribe. `host.float` itself, the host's own controls (keeping a
    /// window on top, putting a pane back) and the floated window's close
    /// each move it. Refused [`refusal::UNKNOWN_REQUEST`] at subscribe by a
    /// host that cannot float: a view that hears that hides its float
    /// control.
    HostPlacement, "host.placement", (), Placement;
    /// `host.badge`: the count of things unread in the view, shown on its
    /// tab (the app words it "N unread").
    HostBadge, "host.badge", i64, ();
    /// `link.open`: a `duck://` link, opened in the app, or an `https://`
    /// one, opened in the view that declares [`Capability::Web`] (as its
    /// `host.route`) when the app has one and a web view, else handed to
    /// the system browser. Any other scheme is refused
    /// (`malformed_request`). Only a person's press or key in the view
    /// opens one: a request without one is refused `needs_gesture`.
    LinkOpen, "link.open", String, ();
    /// `link.window`: a link as [`LinkOpen`] takes it, the same schemes
    /// and the same press rule (`needs_gesture`), but a `duck://` link's
    /// view opens in a window of its own; the window an earlier
    /// `link.window` opened for the same link comes forward instead. A
    /// host with one window (a phone) and an `https://` link open it as
    /// `link.open` does. A host older than this method refuses it
    /// `unknown_request`.
    LinkWindow, "link.window", String, ();
    /// `host.route`: a subscription, one item per `duck://` link opened into
    /// this view: the path after the view's own segment (`tx/<hash>` of
    /// `duck://<chain>/explorer/tx/<hash>`). The route is delivered DECODED:
    /// the link spells each segment percent-encoded (`ducklink`), the host
    /// decodes it and joins the segments with `/`, so `forge%3Aweb%3A3/42`
    /// arrives as `forge:web:3/42`. A segment is nonempty, not `.` or `..`,
    /// and holds no `/` and no control character; the whole route is at most
    /// 256 bytes. A link that mounted the view is its first item.
    ///
    /// The view that declares [`Capability::Web`] gets, besides, each http(s)
    /// link the app opens into it, as the WHOLE address
    /// (`https://example.com/a?b`), at most 2048 bytes, held to the web
    /// piece's rule ([`crate::Node::Web`]): `https://`, or `http://` on this
    /// machine, no user name or password. One waits per view: a second link
    /// before the view reads the first replaces it.
    HostRoute, "host.route", (), String;
    /// `host.showing`: the route this view shows now, in the grammar
    /// `host.route` hands it (`general` for a room `duck://<chain>/chat/general`
    /// opens); empty for none. The host keeps it while the view is on
    /// screen, for `host.screen`.
    HostShowing, "host.showing", String, ();
    /// `host.screen`: a subscription to the routes this program's views
    /// show on screen now (each one's `host.showing`, while it is on screen
    /// as `host.visible` says), sorted, one item per change, the first at
    /// once: what a notifier reads to leave alone what the person is
    /// looking at.
    HostScreen, "host.screen", (), Vec<String>;
    /// `host.id`: a fresh id under the named prefix.
    HostId, "host.id", String, String;
    /// `clock.ticks`: an item per period, in milliseconds.
    ClockTicks, "clock.ticks", i64, ();
    /// `host.log`: one line to the host's log.
    HostLog, "host.log", String, ();
    /// `clipboard.read`: the clipboard's text.
    ClipboardRead, "clipboard.read", (), Clipboard;
    /// `clipboard.write`: text onto the clipboard.
    ClipboardWrite, "clipboard.write", String, ();
    /// `notify.post`: hand the host a notice; it says what it did.
    NotifyPost, "notify.post", Notification, Delivery;
    /// `notify.seen`: the reader has seen what this view posted under a
    /// tag; the host marks this view's rows under it read and takes down
    /// its standing banner. Another view's rows are never touched.
    NotifySeen, "notify.seen", String, ();
    // `store.get`: the value this view keeps under a key on this device,
    // for the network and the key seated in hand (with no key seated, the
    // device's own); `None` where it keeps none. A view sees only its own
    // program's keys, only on the network it runs on and only for the key
    // it runs with: two accounts signed in on one device keep apart.
    StoreGet, "store.get", String, Option<Vec<u8>>;
    /// `store.set`: keep a value under a key, or drop it with `None`.
    StoreSet, "store.set", (String, Option<Vec<u8>>), ();
    /// `store.changes`: a subscription, one item per key another instance
    /// of this program (a pane's view, its notifier) set on this device for
    /// the network in hand, once the value is kept: a `store.get` asked
    /// after the item reads it. An instance never hears its own sets.
    StoreChanges, "store.changes", (), String;
    // `store.list`: the keys this view keeps under a prefix (empty: every
    // key), as `store.get` scopes them, in byte order. A host older than
    // this method refuses it `unknown_request`: a view reads that as
    // nothing kept and goes on with its defaults.
    StoreList, "store.list", String, Vec<String>;
    /// `chain.heads`: a subscription, one item per finalized block, oldest
    /// first, from the tip at subscribe on. Heights may skip where the host
    /// could not fill a gap (a reconnect, a node with no archive); a view
    /// that must see every block reads the gap with `chain.blocks`.
    ChainHeads, "chain.heads", (), Head;
    /// `program.describe`: an op as its program says a person reads it,
    /// from the `ducktape.describe` module in the program's current code;
    /// `None` where the code carries none or it cannot read these bytes.
    ProgramDescribe, "program.describe", (String, Vec<u8>), Option<Description>;
    /// `host.title`: the name of the view's window, shown by the host
    /// beside the program's name, on one line; empty clears it. At most 80
    /// chars; the host trims longer names and drops control characters,
    /// bidi controls and line or paragraph separators.
    HostTitle, "host.title", String, ();
    /// `host.show`: bring up one of the host's own places ([`Place`]).
    /// Only a person's press or key in the view brings one up, as with
    /// [`LinkOpen`]: a request without one is refused `needs_gesture`. A
    /// host older than this method refuses it `unknown_request`.
    HostShow, "host.show", Place, ();
    /// `live.publish`: a subscription that IS the published track; its
    /// items are [`Publishing`]: `Waiting` while the room's program has not
    /// granted the track, the subscriber count as it moves, and `Cut` when
    /// the host abandoned a stream group it could not queue. An object the
    /// host refused as `too_large` is an `Err` item that does not end it.
    /// It ends with a refusal
    /// (`not_granted`: the grant taken back; `publisher_gone`: the same
    /// account published the track again elsewhere; `duplicate_track`,
    /// `malformed_request`, `bad_proof`, `live_unavailable`). A reconnect
    /// publishes the track again on the new connection.
    LivePublishTrack, "live.publish", LivePublish, Publishing;
    /// `live.send`: one object onto a track this view publishes. Answered
    /// only when it names no track the view publishes
    /// (`live_not_published`) or does not decode: a 60 Hz sender pays no
    /// reply per frame. A refused object is told on the track's
    /// `live.publish` subscription.
    LiveSendObject, "live.send", LiveSend, ();
    /// `live.subscribe`: a subscription to a track; items [`LiveItem`]:
    /// `Waiting` while the room's program has not granted it, then
    /// [`LiveItem::Opened`] and what the track carries. Ends with a refusal
    /// (`track_ended`, `publisher_gone`, `unknown_track`; `not_granted`:
    /// this account's grant taken back; `malformed_request`, `bad_proof`,
    /// `live_unavailable`). A reconnect subscribes again from
    /// [`LiveStart::Latest`], and the next item is a fresh
    /// [`LiveItem::Opened`].
    LiveSubscribeTrack, "live.subscribe", LiveSubscribe, LiveItem;
    /// `live.tracks`: a subscription to a room's track list; items
    /// [`Watching`]: `Waiting` while the room's program has not granted
    /// watching the room, then an `Announce` per track up at subscribe
    /// (`up: true`) and one per track that comes or goes after, each a
    /// track this account's grant lets it subscribe to: a grant that moves
    /// announces the tracks it adds (`up: true`) and drops (`up: false`),
    /// so the list holds what a fresh `live.tracks` would. It ends with a
    /// refusal (`not_granted`: the grant taken back; `malformed_request`,
    /// `bad_proof`, `live_unavailable`).
    LiveTracks, "live.tracks", String, Watching;
    /// `camera.devices`: the cameras the host can open, as listed now
    /// (ask again after a piece reports one `Gone`). A host that captures
    /// nothing does not answer it: [`refusal::UNKNOWN_REQUEST`].
    CameraDevices, "camera.devices", (), Vec<CameraDevice>;
    /// `microphone.devices`: the microphones and speakers the host can
    /// open, as listed now. A host that captures nothing does not answer
    /// it: [`refusal::UNKNOWN_REQUEST`].
    MicrophoneDevices, "microphone.devices", (), AudioDevices;
    /// `microphone.chime`: the host's test chime (0.6 s, two notes) on a
    /// speaker, by an output id `microphone.devices` listed (`None`: the
    /// system's), answered once it has sounded. Apart from any microphone
    /// piece: the speaker test works muted. Refused
    /// [`refusal::HOST_FAULT`] when the speaker does not play; a host that
    /// plays nothing does not answer it: [`refusal::UNKNOWN_REQUEST`].
    MicrophoneChime, "microphone.chime", Option<String>, ();
    /// `file.pick`: the person picks files on this device in the system's
    /// own picker ([`Pick`]), answered with each one picked ([`Picked`]);
    /// cancelled, none. Only a press or key opens the picker: a request
    /// without one is refused `needs_gesture`. A host
    /// whose system has no picker, or a pick it cannot open (one that is
    /// not a file), is refused [`refusal::HOST_FAULT`], naming the file by
    /// its name alone.
    FilePick, "file.pick", Pick, Vec<Picked>;
    // `op.put`: the blobs of a [`PutBlobs`] put on the network as one
    // bundle in the person's name, answered once the network holds the
    // bundle (the blobs role's `Held`), so an op naming them may follow at
    // once. The answer is each blob's sha256 digest, in order, of the blob
    // framed with its kind (`<kind> <len>\0<body>`, as `program::blob_id`
    // makes it): `BlobId::Sha256(digest)` is the id an op names (files'
    // `Content::Chunks`), and `sha256:<hex>` the same id as `blob.get`
    // reads it.
    //
    // The host frames each blob and sends the bytes on the node's blob
    // upload: the bundle's op, signed by the person's key to the program
    // the blobs role binds, carries each blob's id, length and kind, never
    // its bytes. A blob may be of any size: the host moves its bytes to the
    // node in parts as the upload takes them, which the view never sees. A
    // [`BlobBody::Picked`] slice goes from the file to the node, never
    // through the view. The method is an op (`op`); its picked bodies name
    // handles only `file.pick` (`file`) makes. The host builds the op
    // itself, so a view can only ask it to keep bytes, and no confirm card
    // asks: as for an op to a program bound to no role, the person's press
    // that sent them is the yes. A held blob stays, and anyone may read it
    // by its id.
    //
    // Refused [`refusal::MALFORMED_REQUEST`]: no blob, a kind that is not
    // one word, or a slice past the file's size as picked.
    // [`refusal::HOST_FAULT`]: a picked file that cannot be read, or whose
    // size changed since the pick. [`refusal::SESSION_LOCKED`]: no key is
    // unlocked to sign it. `anchor`: no block carried the put by the last
    // its frame may land at. [`refusal::NODE_FAILED`]: the node stopped
    // telling its blocks or changes before the bundle was held. The
    // node's refusals and the blobs program's (a key with no account) pass
    // on as they came. To put again is safe: the same blobs are the same
    // ids, in a bundle of their own. The answer waits as long as the
    // network takes to hold the bundle.
    OpPut, "op.put", PutBlobs, Putting;
    FileRelease, "file.release", Vec<u64>, ();
    FileSave, "file.save", SaveFile, bool;
    ClipboardFiles, "clipboard.files", (), Pasted;
    // `host.methods`: what this host answers ([`Hosted`]), so a view asks
    // before it offers what needs a kind. Answered to every view, whatever
    // its manifest declares: it grants nothing.
    HostMethods, "host.methods", (), Hosted;
    // `file.read`: `len` bytes from `offset` of a file the view holds, by
    // the handle `file.pick`, `clipboard.files` or a drop gave it: any
    // file, of any kind and any size, read as it is, at any offset, as
    // much as the view asks. The answer is exactly those bytes. No press is
    // needed: the person's pick, paste or drop already chose the file.
    // Refused [`refusal::MALFORMED_REQUEST`]: a handle the view does not
    // hold, or a range past the file's size as held.
    // [`refusal::HOST_FAULT`]: the file cannot be read, or its size changed
    // since it was held.
    FileRead, "file.read", ReadFile, Vec<u8>;
    SocketConnect, "socket.open", SocketOpen, SocketItem;
    SocketSendMessage, "socket.send", SocketSend, ();
    HostRandom, "host.random", u32, Vec<u8>;
    StoreSecretGet, "store.secret_get", String, Option<Vec<u8>>;
    StoreSecretSet, "store.secret_set", (String, Option<Vec<u8>>), ();
    NotifyNeeds, "notify.needs", Needs, ();
}

// The `<capability>` half of every kind in [`ALL`]: what a view's manifest
// declares, and the only names it may. A method is reached only through the
// capability its kind starts with. Two capabilities gate a node, not a
// method: [`Capability::Web`] and [`Capability::Artifact`]; three gate a
// node and their methods: [`Capability::Camera`], [`Capability::Microphone`],
// [`Capability::Live`].
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    BorshSerialize,
    BorshDeserialize,
)]
#[cfg_attr(feature = "schema", derive(borsh::BorshSchema))]
pub enum Capability {
    Chain,
    Program,
    Op,
    Invite,
    Link,
    Blob,
    Host,
    Clock,
    Clipboard,
    Notify,
    Store,
    /// The view draws web pages ([`crate::Node::Web`]) and opens the
    /// http(s) links the app opens: the app hands such a link to the view
    /// that declares this, as its `host.route` (the whole address; see
    /// [`HostRoute`]), and to the system browser when none does or the app
    /// has no web view. When more than one installed view declares it, the
    /// first by program name gets the links. No method starts with it.
    Web,
    /// Tracks through the connected node, off consensus (`live.*`): the
    /// node lets an account publish or subscribe in a room only as the
    /// view's program granted it. Also gates the pieces that publish or
    /// play one: a camera or microphone piece's `publish`, and every
    /// [`crate::Node::Remote`].
    Live,
    /// The view places camera pieces ([`crate::Node::Camera`]) and lists
    /// the cameras (`camera.devices`). Apart from `microphone`, as the
    /// system asks for each apart: a voice-only view never gets a camera.
    Camera,
    /// The view places microphone pieces ([`crate::Node::Microphone`]),
    /// which also play on a speaker, lists the microphones and speakers
    /// (`microphone.devices`) and plays the test chime on a speaker
    /// (`microphone.chime`).
    Microphone,
    // The view asks the person for files on this device (`file.pick`):
    // what they pick, by name, and nothing else of the device's, and reads
    // what it holds of them (`file.read`). Putting them on the network is
    // an op (`op.put`), under [`Capability::Op`].
    File,
    // The view embeds artifacts ([`crate::Node::Artifact`]): pages kept on
    // the network at a `duck://` link, drawn by the app where it has a web
    // view and shown as their link where it has none. Unlike `web`, it takes
    // none of the links the app opens.
    Artifact,
    Socket,
}

impl Capability {
    pub const ALL: [Self; 18] = [
        Self::Chain,
        Self::Program,
        Self::Op,
        Self::Invite,
        Self::Link,
        Self::Blob,
        Self::Host,
        Self::Clock,
        Self::Clipboard,
        Self::Notify,
        Self::Store,
        Self::Web,
        Self::Live,
        Self::Camera,
        Self::Microphone,
        Self::File,
        Self::Artifact,
        Self::Socket,
    ];

    /// The name a manifest and a kind spell it with.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chain => "chain",
            Self::Program => "program",
            Self::Op => "op",
            Self::Invite => "invite",
            Self::Link => "link",
            Self::Blob => "blob",
            Self::Host => "host",
            Self::Clock => "clock",
            Self::Clipboard => "clipboard",
            Self::Notify => "notify",
            Self::Store => "store",
            Self::Web => "web",
            Self::Live => "live",
            Self::Camera => "camera",
            Self::Microphone => "microphone",
            Self::File => "file",
            Self::Artifact => "artifact",
            Self::Socket => "socket",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|known| known.as_str() == name)
    }

    /// A kind split into its capability and its verb: `chain.status` is
    /// `(Chain, "status")`. `None` for a kind no capability starts.
    pub fn of_kind(kind: &str) -> Option<(Self, &str)> {
        let (capability, verb) = kind.split_once('.')?;
        Some((Self::parse(capability)?, verb))
    }
}

/// Every code a host refuses a method with, beside the program codes in
/// [`crate::code`] that ride through as the node or the program wrote them.
/// A view branches on these; the strings are wire, so one never changes.
pub mod refusal {
    /// The kind is not in [`super::ALL`], or this host does not answer it.
    pub const UNKNOWN_REQUEST: &str = "unknown_request";
    /// The kind's capability is not in the view's manifest.
    pub const UNDECLARED_CAPABILITY: &str = "undeclared_capability";
    // No host since J-054 refuses with it. A host before J-054 refused a
    // node method naming a program its view's manifest did not list, and
    // `blob.manifest` naming code the programs role did not.
    pub const UNDECLARED_TARGET: &str = "undeclared_target";
    /// The op needed the person's confirmation on the host and did not get
    /// it: cancelled, or another confirmation for this view still waits.
    pub const CONSENT_REFUSED: &str = "consent_refused";
    /// The payload does not decode as the method's request, or says nothing
    /// the method can act on.
    pub const MALFORMED_REQUEST: &str = "malformed_request";
    /// A live object over the most its track's mode carries in one object
    /// on the node's wire; the track goes on.
    pub const TOO_LARGE: &str = "too_large";
    /// No node is connected.
    pub const NOT_CONNECTED: &str = "not_connected";
    /// The node connection changed while the request waited.
    pub const STALE_CONNECTION: &str = "stale_connection";
    pub const SESSION_LOCKED: &str = "session_locked";
    /// Nothing reached the node: safe to send again.
    pub const RPC_CLIENT: &str = "rpc_client";
    /// The node failed the request, or its answer went missing: it may have
    /// run. Never on `op.submit`, whose missing answer is
    /// [`Submitted::InDoubt`](super::Submitted::InDoubt).
    pub const NODE_FAILED: &str = "node_failed";
    /// The host itself failed (its disk, its config), not the request.
    pub const HOST_FAULT: &str = "host_fault";
    /// The widget command does not apply to the tree the view shows now.
    pub const INVALID_WIDGET_COMMAND: &str = "invalid_widget_command";
    /// The widget refused the command.
    pub const WIDGET_COMMAND_FAILED: &str = "widget_command_failed";
    /// The command's target left the tree before it ran.
    pub const WIDGET_UNMOUNTED: &str = "widget_unmounted";
    /// The method needs a person's input (a press or a key) in the view, and
    /// there was none, or it was spent on an earlier request.
    pub const NEEDS_GESTURE: &str = "needs_gesture";
    pub const INVITE_UNSUPPORTED: &str = "invite_unsupported";
    /// The host closed the request with no answer; written on the view's
    /// side, by the SDK.
    pub const REQUEST_CLOSED: &str = "request_closed";
    /// The connected node serves no live route, or the host could not open
    /// its live session with it.
    pub const LIVE_UNAVAILABLE: &str = "live_unavailable";
    /// A `live.send` onto a track this view does not publish (never, or no
    /// longer: its publication was refused or ended).
    pub const LIVE_NOT_PUBLISHED: &str = "live_not_published";
    /// The view's program took back the grant that let this account
    /// publish, subscribe to or watch this: the request's last item. An ask
    /// the program has not granted yet is not refused: it waits, and hears
    /// `Waiting` ([`super::Publishing::Waiting`]).
    pub const NOT_GRANTED: &str = "not_granted";
    /// The node did not take the proof the host signed with the session's
    /// key, and opened no live session: the request's last item.
    pub const BAD_PROOF: &str = "bad_proof";
    /// Another account publishes this track, or another window of this app
    /// does.
    pub const DUPLICATE_TRACK: &str = "duplicate_track";
    /// A subscription to a track no one publishes: subscribe again when
    /// `live.tracks` announces it.
    pub const UNKNOWN_TRACK: &str = "unknown_track";
    /// The publisher ended the track.
    pub const TRACK_ENDED: &str = "track_ended";
    /// The track has no publisher any more: its connection went away with
    /// the track up, the same account published it again elsewhere, or the
    /// room's program took the publisher's grant back.
    pub const PUBLISHER_GONE: &str = "publisher_gone";
    pub const UNDECLARED_HOST: &str = "undeclared_host";
    pub const NO_SECRET_STORE: &str = "no_secret_store";
    pub const SECRET_STORE_REFUSED: &str = "secret_store_refused";

    /// Every code above: what a view tells apart from a node's or a
    /// program's refusal (`design::failure` words these as the host's).
    pub const ALL: [&str; 29] = [
        UNKNOWN_REQUEST,
        UNDECLARED_CAPABILITY,
        UNDECLARED_TARGET,
        CONSENT_REFUSED,
        MALFORMED_REQUEST,
        TOO_LARGE,
        NOT_CONNECTED,
        STALE_CONNECTION,
        SESSION_LOCKED,
        RPC_CLIENT,
        NODE_FAILED,
        HOST_FAULT,
        INVALID_WIDGET_COMMAND,
        WIDGET_COMMAND_FAILED,
        WIDGET_UNMOUNTED,
        NEEDS_GESTURE,
        INVITE_UNSUPPORTED,
        REQUEST_CLOSED,
        LIVE_UNAVAILABLE,
        LIVE_NOT_PUBLISHED,
        NOT_GRANTED,
        BAD_PROOF,
        DUPLICATE_TRACK,
        UNKNOWN_TRACK,
        TRACK_ENDED,
        PUBLISHER_GONE,
        UNDECLARED_HOST,
        NO_SECRET_STORE,
        SECRET_STORE_REFUSED,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_exactly_the_prefixes_of_every_kind() {
        let mut prefixes: Vec<&str> = ALL
            .iter()
            .map(|kind| Capability::of_kind(kind).unwrap().0.as_str())
            .collect();
        prefixes.sort_unstable();
        prefixes.dedup();
        // `web` and `artifact` gate a node (`Node::Web`, `Node::Artifact`),
        // not a method
        let mut known: Vec<&str> = Capability::ALL
            .into_iter()
            .filter(|capability| !matches!(capability, Capability::Web | Capability::Artifact))
            .map(Capability::as_str)
            .collect();
        known.sort_unstable();
        assert_eq!(prefixes, known);
        assert_eq!(Capability::parse("link"), Some(Capability::Link));
        assert_eq!(Capability::parse("web"), Some(Capability::Web));
        assert_eq!(Capability::parse("artifact"), Some(Capability::Artifact));
        for name in ["rpc", "chat", "program.query", "", "Chain"] {
            assert_eq!(Capability::parse(name), None, "{name:?}");
        }
        assert_eq!(
            Capability::of_kind("chain.status"),
            Some((Capability::Chain, "status"))
        );
        assert_eq!(Capability::of_kind("chain"), None);
    }

    struct Binary;
    impl Program for Binary {
        const NAME: &'static str = "binary";
        type Op = ();
        type Query = (u64, String);
        type Reply = Vec<u32>;

        fn execute(_: &program::ExecCtx, _: Self::Op) -> Result<(), program::Error> {
            unreachable!()
        }

        fn query(_: &program::QueryCtx, _: Self::Query) -> Result<Self::Reply, program::Error> {
            unreachable!()
        }
    }

    #[test]
    fn program_methods_address_their_program_and_carry_its_bytes() {
        let request = (42, "query".to_owned());
        let bytes = Query::<Binary>::encode_request(&request);
        let call: Call = decode(&bytes).unwrap();
        assert_eq!(call.target, "binary");
        assert_eq!(call.body, encode(&request));
        assert_eq!(Query::<Binary>::decode_request(&bytes).unwrap(), request);
        let other = encode(&Call {
            target: "other".into(),
            body: encode(&request),
        });
        assert!(Query::<Binary>::decode_request(&other).is_err());
        let reply = vec![1, 2, 3];
        assert_eq!(
            Query::<Binary>::decode_reply(&Query::<Binary>::encode_reply(&reply)).unwrap(),
            reply
        );
    }

    /// A host reads the places it knows and refuses any other byte, so a
    /// place a newer view names is `malformed_request` on an older host,
    /// never some other place.
    #[test]
    fn a_place_this_host_does_not_know_does_not_decode() {
        let account = HostShow::encode_request(&Place::Account);
        assert_eq!(account, [0]);
        assert_eq!(HostShow::decode_request(&account), Ok(Place::Account));
        for hostile in [&[3][..], &[u8::MAX], &[], &[0, 0]] {
            assert!(HostShow::decode_request(hostile).is_err(), "{hostile:?}");
        }
    }

    #[test]
    fn every_kind_is_listed_once() {
        let mut kinds = ALL.to_vec();
        kinds.sort_unstable();
        kinds.dedup();
        assert_eq!(kinds.len(), ALL.len());
        assert!(ALL.iter().all(|kind| kind.split_once('.').is_some()));
    }

    /// The one place the refusal strings are spelled out: the host and the
    /// views name the consts, so a changed string would pass every other
    /// test and break each view built before it.
    #[test]
    fn the_refusal_codes_are_the_wire_strings() {
        use refusal::*;
        let codes = [
            (UNKNOWN_REQUEST, "unknown_request"),
            (UNDECLARED_CAPABILITY, "undeclared_capability"),
            (UNDECLARED_TARGET, "undeclared_target"),
            (CONSENT_REFUSED, "consent_refused"),
            (MALFORMED_REQUEST, "malformed_request"),
            (TOO_LARGE, "too_large"),
            (NOT_CONNECTED, "not_connected"),
            (STALE_CONNECTION, "stale_connection"),
            (SESSION_LOCKED, "session_locked"),
            (RPC_CLIENT, "rpc_client"),
            (NODE_FAILED, "node_failed"),
            (HOST_FAULT, "host_fault"),
            (INVALID_WIDGET_COMMAND, "invalid_widget_command"),
            (WIDGET_COMMAND_FAILED, "widget_command_failed"),
            (WIDGET_UNMOUNTED, "widget_unmounted"),
            (NEEDS_GESTURE, "needs_gesture"),
            (INVITE_UNSUPPORTED, "invite_unsupported"),
            (REQUEST_CLOSED, "request_closed"),
            (LIVE_UNAVAILABLE, "live_unavailable"),
            (LIVE_NOT_PUBLISHED, "live_not_published"),
            (NOT_GRANTED, "not_granted"),
            (BAD_PROOF, "bad_proof"),
            (DUPLICATE_TRACK, "duplicate_track"),
            (UNKNOWN_TRACK, "unknown_track"),
            (TRACK_ENDED, "track_ended"),
            (PUBLISHER_GONE, "publisher_gone"),
            (UNDECLARED_HOST, "undeclared_host"),
            (NO_SECRET_STORE, "no_secret_store"),
            (SECRET_STORE_REFUSED, "secret_store_refused"),
        ];
        for (code, wire) in codes {
            assert_eq!(code, wire);
        }
        // `refusal::ALL` lists every code, once
        assert_eq!(refusal::ALL, codes.map(|(_, wire)| wire));
    }
}
