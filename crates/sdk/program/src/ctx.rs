//! The two contexts a program's entry points receive. Every method is one
//! host call: on wasm32 through the `ducktape.*` imports, natively through
//! the [`MockHost`](crate::MockHost) the context was made over. The read
//! surface is defined once, on [`QueryCtx`]; an [`ExecCtx`] derefs to it and
//! adds the writes, so anything that reads takes `&QueryCtx` and accepts
//! either.

use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;

use abi::{
    Blob, BlobHeader, BlobId, CryptoOp, CryptoReply, Entry, HashKind, HostOp, HostReply, Message,
    Root, Scheme,
};
use borsh::{BorshDeserialize, BorshSerialize};

use crate::{AccountNumber, Env, Error, FrameId, MessageId, Principal, ProgramId, Range, Ticket};

pub(crate) struct Frame {
    env: RefCell<Rc<Env>>,
    #[cfg(not(target_arch = "wasm32"))]
    host: crate::MockHost,
}

impl Frame {
    pub(crate) fn env(&self) -> Rc<Env> {
        self.env.borrow().clone()
    }

    pub(crate) fn move_to(&self, env: Env) {
        *self.env.borrow_mut() = Rc::new(env);
    }

    pub(crate) fn host(&self, op: HostOp) -> HostReply {
        #[cfg(target_arch = "wasm32")]
        return ffi::host(&op);
        #[cfg(not(target_arch = "wasm32"))]
        return self.host.serve(&self.env(), op);
    }
}

/// A query's context: the env and the reads. It has no write methods.
pub struct QueryCtx {
    frame: Rc<Frame>,
}

/// An execute's (or init's) context: every read of [`QueryCtx`] and the
/// writes: state, blobs, and what leaves the program (`emit`, `event`,
/// `set_return_data`).
pub struct ExecCtx {
    reads: QueryCtx,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reply {
    /// The target's refusal fails this whole frame.
    None,
    /// The target's refusal undoes its writes and comes back as the
    /// reply; the reply's refusal fails the frame, its Ok absorbs it.
    Wanted,
}

impl Deref for ExecCtx {
    type Target = QueryCtx;

    fn deref(&self) -> &QueryCtx {
        &self.reads
    }
}

#[cfg(target_arch = "wasm32")]
mod ffi {
    use abi::{HostOp, HostReply};

    #[link(wasm_import_module = "ducktape")]
    unsafe extern "C" {
        fn host_call(ptr: u32, len: u32) -> u32;
        fn host_take(ptr: u32);
    }

    pub(super) fn host(op: &HostOp) -> HostReply {
        let request = abi::encode(op);
        let len = unsafe { host_call(request.as_ptr() as u32, request.len() as u32) };
        let mut reply = vec![0u8; len as usize];
        unsafe { host_take(reply.as_mut_ptr() as u32) };
        match abi::decode(&reply) {
            Ok(reply) => reply,
            Err(fault) => panic!("{fault}"),
        }
    }
}

fn protocol(expected: &str, got: HostReply) -> ! {
    panic!("host answered {got:?} where {expected} was expected")
}

#[cfg(target_arch = "wasm32")]
impl QueryCtx {
    pub(crate) fn new(env: Env) -> QueryCtx {
        let env = RefCell::new(Rc::new(env));
        QueryCtx {
            frame: Rc::new(Frame { env }),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl ExecCtx {
    pub(crate) fn new(env: Env) -> ExecCtx {
        ExecCtx {
            reads: QueryCtx::new(env),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl QueryCtx {
    pub(crate) fn over(host: crate::MockHost, env: Env) -> QueryCtx {
        let env = RefCell::new(Rc::new(env));
        QueryCtx {
            frame: Rc::new(Frame { env, host }),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ExecCtx {
    pub(crate) fn over(host: crate::MockHost, env: Env) -> ExecCtx {
        ExecCtx {
            reads: QueryCtx::over(host, env),
        }
    }
}

impl QueryCtx {
    /// Who called, at what height and time, on which chain.
    pub fn env(&self) -> Rc<Env> {
        self.frame.env()
    }

    pub(crate) fn frame(&self) -> &Rc<Frame> {
        &self.frame
    }

    fn host(&self, op: HostOp) -> HostReply {
        self.frame.host(op)
    }

    fn done(&self, op: HostOp) {
        match self.host(op) {
            HostReply::Done => {}
            other => protocol("done", other),
        }
    }

    /// This program's value at `key`, as this frame sees it: its own writes
    /// so far included. Keys are this program's alone; another program's
    /// state is read by [`query`](QueryCtx::query)ing it.
    pub fn get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(HostOp::Get(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    /// The entries `range` admits, in its order, as this frame sees them.
    /// `store`'s tables build the range (`prefix_of`, `below`, a page).
    pub fn scan(&self, range: Range) -> Vec<Entry> {
        match self.host(HostOp::Scan(range.into())) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    /// [`get`](QueryCtx::get) as of the last committed block: none of this
    /// block's writes, this frame's own included.
    pub fn committed_get(&self, key: impl AsRef<[u8]>) -> Option<Vec<u8>> {
        match self.host(HostOp::CommittedGet(key.as_ref().to_vec())) {
            HostReply::Value(value) => value,
            other => protocol("value", other),
        }
    }

    /// [`scan`](QueryCtx::scan) as of the last committed block.
    pub fn committed_scan(&self, range: Range) -> Vec<Entry> {
        match self.host(HostOp::CommittedScan(range.into())) {
            HostReply::Entries(entries) => entries,
            other => protocol("entries", other),
        }
    }

    /// [`get`](QueryCtx::get), the value decoded from borsh; a value that
    /// does not decode is an error, never a panic.
    pub fn record<T: BorshDeserialize>(&self, key: impl AsRef<[u8]>) -> Result<Option<T>, Error> {
        self.get(key)
            .map(|bytes| abi::decode(&bytes).map_err(crate::kernel::error_from))
            .transpose()
    }

    /// [`scan`](QueryCtx::scan), each value decoded from borsh beside its raw key.
    pub fn records<T: BorshDeserialize>(&self, range: Range) -> Result<Vec<(Vec<u8>, T)>, Error> {
        self.scan(range)
            .into_iter()
            .map(|entry| {
                Ok((
                    entry.key,
                    abi::decode(&entry.value).map_err(crate::kernel::error_from)?,
                ))
            })
            .collect()
    }

    /// A blob by id, whole: its kind and body; [`blob_read`](QueryCtx::blob_read)
    /// reads a slice.
    pub fn blob_get(&self, id: BlobId) -> Option<Blob> {
        match self.host(HostOp::BlobGet(id)) {
            HostReply::Blob(blob) => blob,
            other => protocol("blob", other),
        }
    }

    /// A blob's kind and length, without its body.
    pub fn blob_stat(&self, id: BlobId) -> Option<BlobHeader> {
        match self.host(HostOp::BlobStat(id)) {
            HostReply::BlobHeader(header) => header,
            other => protocol("blob header", other),
        }
    }

    /// Up to `len` bytes of a blob's body from `offset` (fewer at its end).
    pub fn blob_read(&self, id: BlobId, offset: u64, len: u64) -> Option<Vec<u8>> {
        match self.host(HostOp::BlobRead { id, offset, len }) {
            HostReply::Value(bytes) => bytes,
            other => protocol("value", other),
        }
    }

    /// `program`'s state root as of the last committed block; `None` for a
    /// program the chain does not run (and always natively).
    pub fn root(&self, program: impl Into<ProgramId>) -> Option<Root> {
        match self.host(HostOp::Root(program.into())) {
            HostReply::Root(root) => root,
            other => protocol("root", other),
        }
    }

    /// Another program's answer to `request`, raw. A query reads: it moves
    /// no state and runs in this frame.
    pub fn query_raw(
        &self,
        program: impl Into<ProgramId>,
        request: impl Into<Vec<u8>>,
    ) -> Result<Vec<u8>, Error> {
        match self.host(HostOp::Query {
            program: program.into(),
            request: request.into(),
        }) {
            HostReply::Query(answer) => answer.map_err(crate::kernel::error_from),
            other => protocol("query answer", other),
        }
    }

    pub fn resource(
        &self,
        program: impl Into<ProgramId>,
        key: impl Into<String>,
    ) -> Result<crate::Resource, Error> {
        match self.host(HostOp::Resource {
            program: program.into(),
            key: key.into(),
        }) {
            HostReply::Query(answer) => {
                let bytes = answer.map_err(crate::kernel::error_from)?;
                abi::decode(&bytes).map_err(crate::kernel::error_from)
            }
            other => protocol("resource answer", other),
        }
    }

    /// Another program's answer to `request`, borsh both ways.
    pub fn query<Q: BorshSerialize, R: BorshDeserialize>(
        &self,
        program: impl Into<ProgramId>,
        request: &Q,
    ) -> Result<R, Error> {
        abi::decode(&self.query_raw(program, abi::encode(request))?)
            .map_err(crate::kernel::error_from)
    }

    /// The SHA-256 of `bytes`, computed by the host.
    pub fn sha256(&self, bytes: impl Into<Vec<u8>>) -> [u8; 32] {
        match self.host(HostOp::Crypto(CryptoOp::Sha256(bytes.into()))) {
            HostReply::Crypto(CryptoReply::Digest(digest)) => digest,
            other => protocol("digest", other),
        }
    }

    /// Whether `signature` signs `message` under `namespace` for `key` in
    /// `scheme`. Natively the `MockHost`'s `verifier` decides, and with none
    /// set the check is refused.
    pub fn verify(
        &self,
        scheme: Scheme,
        key: impl Into<Vec<u8>>,
        namespace: impl Into<Vec<u8>>,
        message: impl Into<Vec<u8>>,
        signature: impl Into<Vec<u8>>,
    ) -> Result<bool, Error> {
        match self.host(HostOp::Crypto(CryptoOp::Verify {
            scheme,
            key: key.into(),
            namespace: namespace.into(),
            message: message.into(),
            signature: signature.into(),
        })) {
            HostReply::Crypto(CryptoReply::Verified(valid)) => Ok(valid),
            HostReply::Refused(refusal) => Err(crate::kernel::error_from(refusal)),
            other => protocol("verdict", other),
        }
    }

    pub(crate) fn respond(&self, bytes: impl Into<Vec<u8>>) {
        self.done(HostOp::Respond(bytes.into()))
    }
}

impl ExecCtx {
    /// Who this write acts as, as the host resolved it. A signed frame whose
    /// key holds no account is refused: a person writes through an account.
    pub fn sender(&self) -> Result<Principal, Error> {
        self.env().sender.clone().ok_or_else(|| {
            crate::unauthorized("a write acts as an account, and this frame holds none")
        })
    }

    /// The account this write acts as ([`sender`](ExecCtx::sender) as a
    /// number): a user's, a program account or a system account. The chain
    /// itself (`Root`) is refused: a row that names an account cannot name
    /// it.
    pub fn sender_account(&self) -> Result<AccountNumber, Error> {
        self.sender()?
            .account()
            .ok_or_else(|| crate::unauthorized("only an account does this, not the chain"))
    }

    /// Writes `value` at `key` in this program's state. A refused frame's
    /// writes are undone by the host, so a rule checks, then writes.
    pub fn set(&self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) {
        self.done(HostOp::Set {
            key: key.into(),
            value: value.into(),
        })
    }

    // Writes `value` at `key` as `set` does; with a kind, the write also claims the key: when it
    // creates the key, the account the frame acts as becomes its owner, an item of that kind,
    // and the ownership role guards every later write to it. A claim on a key that already
    // holds a value is a plain write.
    pub fn set_with(
        &self,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
        claim: Option<&str>,
    ) {
        let (key, value) = (key.into(), value.into());
        match claim {
            Some(kind) => self.done(HostOp::SetOwned {
                key,
                value,
                kind: kind.to_owned(),
            }),
            None => self.done(HostOp::Set { key, value }),
        }
    }

    /// Removes `key` from this program's state (a missing key is no error).
    pub fn delete(&self, key: impl Into<Vec<u8>>) {
        self.done(HostOp::Delete(key.into()))
    }

    /// [`set`](ExecCtx::set) the borsh of `record`; [`record`](QueryCtx::record) reads it back.
    pub fn put<T: BorshSerialize>(&self, key: impl Into<Vec<u8>>, record: &T) {
        self.set(key, abi::encode(record))
    }

    /// Stores `body` as a blob of `kind` (one word) and answers its id: the
    /// `hash` of `<kind> <len>\0<body>`, so the same bytes are one blob.
    pub fn blob_put(
        &self,
        hash: HashKind,
        kind: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Result<BlobId, Error> {
        match self.host(HostOp::BlobPut {
            hash,
            kind: kind.into(),
            body: body.into(),
        }) {
            HostReply::BlobId(id) => Ok(id),
            HostReply::Refused(refusal) => Err(crate::kernel::error_from(refusal)),
            other => protocol("blob id", other),
        }
    }

    /// Has `target` run `op` (its own `Op`, borsh) in this frame, once this
    /// handler returns Ok, as a message from this program. Messages run in
    /// the order emitted, each before the next, and `reply` says what the
    /// target's refusal does. [`query`](QueryCtx::query) is the read.
    pub fn emit<T: BorshSerialize>(
        &self,
        target: impl Into<ProgramId>,
        op: &T,
        reply: Reply,
    ) -> MessageId {
        self.emit_raw(target, abi::encode(op), reply)
    }

    /// [`emit`](ExecCtx::emit) with the message's bytes as they are.
    pub fn emit_raw(
        &self,
        target: impl Into<ProgramId>,
        payload: impl Into<Vec<u8>>,
        reply: Reply,
    ) -> MessageId {
        self.send(Message {
            target: target.into(),
            payload: payload.into(),
            reply: reply == Reply::Wanted,
            acting: None,
        })
    }

    pub fn emit_as<T: BorshSerialize>(
        &self,
        account: AccountNumber,
        target: impl Into<ProgramId>,
        op: &T,
        reply: Reply,
    ) -> MessageId {
        self.emit_raw_as(account, target, abi::encode(op), reply)
    }

    pub fn emit_raw_as(
        &self,
        account: AccountNumber,
        target: impl Into<ProgramId>,
        payload: impl Into<Vec<u8>>,
        reply: Reply,
    ) -> MessageId {
        self.send(Message {
            target: target.into(),
            payload: payload.into(),
            reply: reply == Reply::Wanted,
            acting: Some(account),
        })
    }

    fn send(&self, message: Message) -> MessageId {
        match self.host(HostOp::Emit(message)) {
            HostReply::Item(item) => item.into(),
            other => protocol("item", other),
        }
    }

    /// Adds `payload` to this call's receipt: a record for readers of the
    /// chain, not state any program reads back.
    pub fn event(&self, payload: impl Into<Vec<u8>>) {
        self.done(HostOp::Event(payload.into()))
    }

    /// The execute's return value (the last one set wins).
    pub fn set_return_data(&self, bytes: impl Into<Vec<u8>>) {
        self.done(HostOp::Output(bytes.into()))
    }

    pub fn defer(&self) -> Result<Ticket, Error> {
        match self.host(HostOp::Defer) {
            HostReply::Ticket(ticket) => Ok(ticket),
            HostReply::Refused(refusal) => Err(crate::kernel::error_from(refusal)),
            other => protocol("ticket", other),
        }
    }

    pub fn resolve(
        &self,
        ticket: Ticket,
        answer: Result<Vec<u8>, Error>,
    ) -> Result<MessageId, Error> {
        let answer = answer.map_err(crate::kernel::refusal_from);
        let resolve = abi::role::awaits::Op::Resolve { ticket, answer };
        let awaits = self.env().role(abi::role::AWAITS)?.clone();
        Ok(self.emit(awaits, &resolve, Reply::None))
    }

    pub fn resume(
        &self,
        frame: FrameId,
        ticket: Ticket,
        answer: Result<Vec<u8>, Error>,
    ) -> Result<(), Error> {
        let answer = answer.map_err(crate::kernel::refusal_from);
        match self.host(HostOp::Resume {
            frame,
            ticket,
            answer,
        }) {
            HostReply::Done => Ok(()),
            HostReply::Refused(refusal) => Err(crate::kernel::error_from(refusal)),
            other => protocol("done", other),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use abi::Message;

    use super::*;
    use crate::{MessageId, MockHost};

    #[test]
    fn emit_hands_the_host_the_message_and_names_it_in_this_frame() {
        let host = MockHost::default();
        let ctx = host.exec(MockHost::env("forge"));
        let first = ctx.emit_raw("chat", b"a".to_vec(), Reply::None);
        let second = ctx.emit_raw("chat", b"b".to_vec(), Reply::Wanted);
        let third = ctx.emit_raw_as(7, "chat", b"c".to_vec(), Reply::None);
        let id = |seq| MessageId {
            program: "forge".into(),
            seq,
        };
        assert_eq!((first, second, third), (id(0), id(1), id(2)));
        assert_eq!(
            host.take_emissions(),
            [
                Message {
                    target: "chat".into(),
                    payload: b"a".to_vec(),
                    reply: false,
                    acting: None,
                },
                Message {
                    target: "chat".into(),
                    payload: b"b".to_vec(),
                    reply: true,
                    acting: None,
                },
                Message {
                    target: "chat".into(),
                    payload: b"c".to_vec(),
                    reply: false,
                    acting: Some(7),
                },
            ]
        );
    }
}
