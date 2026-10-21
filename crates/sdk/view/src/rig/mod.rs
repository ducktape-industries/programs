//! A fake node for one view: the real programs on a native chain
//! ([`program::MockChain`]), run as the kernel runs them, behind every node
//! method the app forwards. [`run`] puts it behind the node's own HTTP
//! surface and opens the view in the real app (`cargo run -p <view>
//! --example live`); [`Network::back`] puts the same chain behind a test's
//! [`FakeHost`], so a test seeds state through real ops instead of
//! hand-writing replies.
//!
//! The chain is founded with the boot set seated (`identity`, `valset`,
//! `programs`, the owner's kept system-program exception) and a dev
//! key holding an account through identity's own `Create`; identity checks
//! a consent's signature as a node does ([`agent`](Network::agent)); every
//! [`submit`](Network::submit) is one block of the archive, and the keys it
//! wrote are what `program.changes` carries. One entry,
//! [`answer`](Network::answer), serves both fronts at the view-wire level:
//! a typed override first ([`handle`](Network::handle),
//! [`refuse`](Network::refuse), FakeHost's signatures), else the chain.
//!
//! Left as constants, on purpose: one validator (the dev key), epoch 0, a
//! zero state root, no program upgrade, and blobs put held at once
//! (`op.put`, answered on a test's front only: the live window's node takes
//! no blob upload).

mod node;
mod pack;
mod run;
mod wire;

pub use run::run;

pub use crate::host::Error;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use abi::role::validators::{Genesis as Validators, Member};
use borsh::{BorshDeserialize, BorshSerialize};
use commonware_cryptography::{Signer as _, ed25519};
use program::abi;
use program::{AccountNumber, ExecCtx, MockChain, MockHost, Origin, Program, ProgramId, hook};
use programs::{Entry, Genesis as Roster, View};
use view_wire::methods::{self, Method};

use crate::testing::FakeHost;

/// What the status says a block takes; the app polls heads at half of it.
pub const BLOCK_TIME_MS: u64 = 2000;
/// The chain's name, and its id (its genesis yields no salt).
pub const CHAIN: &str = "live";
/// The seed the dev key is made from; the app's `--live` takes the same.
pub const DEV_SEED: u64 = 1;
const ANCHOR_WINDOW: u64 = 256;

type Override = Box<dyn FnMut(&[u8]) -> Result<Vec<u8>, Error>>;
type Writes = Vec<(Vec<u8>, Option<Vec<u8>>)>;
struct Hook {
    name: ProgramId,
    host: MockHost,
    entry: HookEntry,
}
type HookEntry = fn(&ExecCtx, hook::Record) -> Result<(), Error>;

struct State {
    chain: MockChain,
    seats: Vec<(ProgramId, MockHost)>,
    hooks: Vec<Hook>,
    keys: BTreeMap<AccountNumber, ed25519::PrivateKey>,
    names: BTreeMap<String, AccountNumber>,
    me: AccountNumber,
    validator: Vec<u8>,
    founded: u64,
    pinned: Option<u64>,
    blocks: Vec<methods::Block>,
    changes: Vec<(String, wire::Change)>,
    subscribers: Vec<(String, std::sync::mpsc::Sender<Vec<u8>>)>,
    spent: BTreeSet<[u8; 32]>,
    codes: BTreeMap<ProgramId, (abi::BlobId, Vec<u8>)>,
    bare: Option<(String, abi::BlobId, Vec<u8>)>,
    overrides: HashMap<(String, Option<String>), Override>,
    invites: u64,
    founded_roster: bool,
    device: Vec<(String, Vec<u8>)>,
    pick: Vec<(u64, Vec<u8>)>,
    handles: u64,
    params: BTreeMap<ProgramId, Vec<u8>>,
}

/// The fake node: shared, single-threaded.
#[derive(Clone)]
pub struct Network(Rc<RefCell<State>>);

impl Default for Network {
    fn default() -> Self {
        Self::new()
    }
}

/// The time now, unix ms.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or_default()
}

fn digest(parts: &[&[u8]]) -> [u8; 32] {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn key_from_seed(seed: u64) -> ed25519::PrivateKey {
    ed25519::PrivateKey::from_seed(seed)
}

fn verify(scheme: abi::Scheme, key: &[u8], namespace: &[u8], message: &[u8], proof: &[u8]) -> bool {
    scheme == abi::Scheme::Ed25519
        && wire::KeyScheme::Ed25519.verify(key, namespace, message, proof)
}

impl Network {
    /// A chain with the boot set seated and the dev key holding account
    /// `me`, at its genesis block.
    pub fn new() -> Self {
        let founded = now_ms();
        let genesis = methods::Block {
            height: 1,
            id: digest(&[b"live-block", &1u64.to_le_bytes()]),
            parent: [0; 32],
            time: founded,
            epoch: 0,
            proposer: None,
            txs: Vec::new(),
        };
        let net = Network(Rc::new(RefCell::new(State {
            chain: MockChain::default(),
            seats: Vec::new(),
            hooks: Vec::new(),
            keys: BTreeMap::new(),
            names: BTreeMap::new(),
            me: 0,
            validator: key_from_seed(DEV_SEED).public_key().as_ref().to_vec(),
            founded,
            pinned: None,
            blocks: vec![genesis],
            changes: Vec::new(),
            subscribers: Vec::new(),
            spent: BTreeSet::new(),
            codes: BTreeMap::new(),
            bare: None,
            overrides: HashMap::new(),
            invites: 0,
            founded_roster: false,
            device: Vec::new(),
            pick: Vec::new(),
            handles: 0,
            params: BTreeMap::new(),
        })));
        let identity = net.seat::<identity::Identity>(identity::PROGRAM);
        identity.borrow_mut().verifier = Some(Box::new(verify));
        net.seat::<valset::Valset>(valset::PROGRAM);
        net.seat::<programs::Programs>(programs::PROGRAM);
        net.seat::<blobs::Blobs>(blobs::PROGRAM);
        net.seat::<access::Access>(access::PROGRAM);
        let dev = key_from_seed(DEV_SEED);
        net.0
            .borrow()
            .chain
            .init(
                valset::PROGRAM,
                &Validators {
                    validators: vec![Member {
                        key: dev.public_key().as_ref().to_vec(),
                        address: Some("127.0.0.1:1".into()),
                        halo: "dev.example.org:9100".into(),
                    }],
                    member_cap: 8,
                },
            )
            .expect("valset founds with the dev validator");
        let me = net.account(dev, "dev");
        net.0.borrow_mut().me = me;
        net.take_changes();
        net
    }

    /// Seats `M` as `program` and gives it its account, as admission does;
    /// its host is returned for a seed to read or write directly.
    pub fn seat<M: Program>(&self, program: &str) -> MockHost {
        let mut state = self.0.borrow_mut();
        let host = state.chain.seat::<M>(program);
        state.seats.push((program.to_owned(), host.clone()));
        state
            .chain
            .submit(
                Origin::Root,
                identity::PROGRAM,
                &identity::Op::RegisterProgram {
                    program: program.to_owned(),
                },
            )
            .expect("identity registers a seated program");
        host
    }

    pub fn params(&self, program: &str, params: Vec<u8>) {
        self.0
            .borrow_mut()
            .params
            .insert(program.to_owned(), params);
    }

    /// Seats `M` as `program` and lists it among the roster's hooks: each
    /// frame after is told to it, as the kernel tells a hook. Like every
    /// [`seat`](Self::seat), it comes before the roster is founded
    /// ([`back`](Self::back), [`run`]): the roster lists the hooks seated by
    /// then.
    pub fn hook<M: Program>(&self, program: &str) -> MockHost {
        let host = self.seat::<M>(program);
        self.0.borrow_mut().hooks.push(Hook {
            name: program.to_owned(),
            host: host.clone(),
            entry: ::program::hook::<M>,
        });
        host
    }

    /// Account `name`, made through identity's `Create` on its first
    /// mention, held by a key of its own.
    pub fn person(&self, name: &str) -> AccountNumber {
        if let Some(number) = self.0.borrow().names.get(name) {
            return *number;
        }
        let seed = u64::from_le_bytes(
            digest(&[b"live-person", name.as_bytes()])[..8]
                .try_into()
                .unwrap(),
        );
        self.account(key_from_seed(seed), name)
    }

    fn account(&self, key: ed25519::PrivateKey, name: &str) -> AccountNumber {
        let receipt = self.run(
            Origin::Signed(key.public_key().as_ref().to_vec()),
            identity::PROGRAM.to_owned(),
            methods::encode(&identity::Op::Create {
                name: name.to_owned(),
                scheme: abi::Scheme::Ed25519,
            }),
        );
        let methods::Outcome::Applied { output } = receipt.outcome else {
            panic!("identity refuses the account {name}: {:?}", receipt.outcome);
        };
        let number: AccountNumber = methods::decode(&output).expect("an account number");
        let mut state = self.0.borrow_mut();
        state.keys.insert(number, key);
        state.names.insert(name.to_owned(), number);
        number
    }

    /// Agent `name`, made by `manager` through identity's `CreateAgent` on
    /// its first mention, holding a key of its own through `AddKey`: its
    /// consent signed by that key, as a node's or a bot's is.
    pub fn agent(&self, manager: AccountNumber, name: &str) -> Result<AccountNumber, Error> {
        if let Some(number) = self.0.borrow().names.get(name) {
            return Ok(*number);
        }
        let made = self.submit(
            manager,
            identity::PROGRAM,
            &identity::Op::CreateAgent {
                name: name.to_owned(),
            },
        )?;
        let number: AccountNumber = methods::decode(&made).expect("an account number");
        let seed = u64::from_le_bytes(
            digest(&[b"live-agent", name.as_bytes()])[..8]
                .try_into()
                .unwrap(),
        );
        let key = key_from_seed(seed);
        let public = key.public_key().as_ref().to_vec();
        let expires_at = now_ms() + 15 * 60 * 1000;
        let admission = identity::Admission {
            network: self.0.borrow().chain.env(identity::PROGRAM).chain_id,
            scheme: abi::Scheme::Ed25519,
            key: public.clone(),
            generation: 0,
            account: number,
            expires_at,
        };
        let proof = key.sign(identity::CONSENT_NAMESPACE, &admission.preimage());
        let consent = identity::Consent {
            key: public,
            account: number,
            expires_at,
            proof: proof.as_ref().to_vec(),
        };
        let add = identity::Op::AddKey {
            scheme: abi::Scheme::Ed25519,
            label: Some(name.to_owned()),
            consent,
        };
        self.submit(manager, identity::PROGRAM, &add)?;
        let mut state = self.0.borrow_mut();
        state.keys.insert(number, key);
        state.names.insert(name.to_owned(), number);
        Ok(number)
    }

    pub fn hold(&self, kind: &str, body: &[u8]) -> Result<abi::BlobId, Error> {
        let framed = wire::framed(kind, body);
        let entry = blobs::Entry::of(&framed)
            .ok_or_else(|| Error::new(program::code::INVALID_INPUT, "a blob kind is one word"))?;
        let id = entry.id;
        let blob = program::Blob {
            kind: kind.to_owned(),
            body: body.to_vec(),
        };
        self.host(blobs::PROGRAM)
            .borrow_mut()
            .blobs
            .insert(id, blob);
        let me = self.me();
        let put = blobs::Op::Put { blobs: vec![entry] };
        let bundle: u64 =
            methods::decode(&self.submit(me, blobs::PROGRAM, &put)?).expect("a bundle number");
        self.submit(me, blobs::PROGRAM, &blobs::Op::Receipt { bundle })?;
        Ok(id)
    }

    /// The dev account: the one the app signs in as.
    pub fn me(&self) -> AccountNumber {
        self.0.borrow().me
    }

    /// The public key `who` signs with.
    pub fn key_of(&self, who: AccountNumber) -> Vec<u8> {
        self.0
            .borrow()
            .keys
            .get(&who)
            .unwrap_or_else(|| panic!("account {who} holds no key here"))
            .public_key()
            .as_ref()
            .to_vec()
    }

    /// Pins the next block's time (unix ms); unpinned, a block is at the
    /// later of now and the block before.
    pub fn at(&self, time_ms: u64) {
        self.0.borrow_mut().pinned = Some(time_ms);
    }

    /// `op` to `program`, signed by `who`, as one block: the program's
    /// output, or the refusal that rejected it.
    pub fn submit<O: BorshSerialize>(
        &self,
        who: AccountNumber,
        program: &str,
        op: &O,
    ) -> Result<Vec<u8>, Error> {
        self.applied(self.run(
            Origin::Signed(self.key_of(who)),
            program.to_owned(),
            methods::encode(op),
        ))
    }

    /// `op` to `program` from the system itself (`Origin::Root`), as one
    /// block: what genesis and the programs role do.
    pub fn system<O: BorshSerialize>(&self, program: &str, op: &O) -> Result<Vec<u8>, Error> {
        self.applied(self.run(Origin::Root, program.to_owned(), methods::encode(op)))
    }

    /// `program`'s `init` with `params`, as admission runs it (no block).
    pub fn init<P: BorshSerialize>(&self, program: &str, params: &P) -> Result<(), Error> {
        self.0.borrow().chain.init(program, params)
    }

    fn applied(&self, receipt: methods::Receipt) -> Result<Vec<u8>, Error> {
        match receipt.outcome {
            methods::Outcome::Applied { output } => Ok(output),
            methods::Outcome::Rejected(refusal) => Err(refusal),
        }
    }

    /// `program`'s answer to `request`, borsh both ways.
    pub fn query<Q: BorshSerialize, R: BorshDeserialize>(
        &self,
        program: &str,
        request: &Q,
    ) -> Result<R, Error> {
        self.0.borrow().chain.query(program, request)
    }

    /// The host `program` is seated over.
    pub fn host(&self, program: &str) -> MockHost {
        self.0
            .borrow()
            .seats
            .iter()
            .find(|(name, _)| name == program)
            .map(|(_, host)| host.clone())
            .unwrap_or_else(|| panic!("{program} is not seated"))
    }

    /// Every seated program, in seating order.
    pub fn seats(&self) -> Vec<ProgramId> {
        self.0
            .borrow()
            .seats
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Answers each ask of `C` with what `handler` returns, in place of
    /// the chain.
    pub fn handle<C: Method>(
        &self,
        mut handler: impl FnMut(C::Request) -> Result<C::Reply, Error> + 'static,
    ) {
        self.0.borrow_mut().overrides.insert(
            (C::KIND.to_owned(), C::TARGET.map(str::to_owned)),
            Box::new(move |payload| {
                let request = C::decode_request(payload).map_err(crate::host::malformed)?;
                handler(request).map(|reply| C::encode_reply(&reply))
            }),
        );
    }

    /// Refuses every ask of `C`.
    pub fn refuse<C: Method>(&self, code: &str, message: &str) {
        let refusal = Error::new(code, message);
        self.0.borrow_mut().overrides.insert(
            (C::KIND.to_owned(), C::TARGET.map(str::to_owned)),
            Box::new(move |_| Err(refusal.clone())),
        );
    }

    /// The session the app hands the view: signed in as `me`.
    pub fn session(&self) -> methods::Session {
        let me = self.me();
        methods::Session {
            connected: true,
            chain_id: CHAIN.into(),
            signer: abi::hex(&self.key_of(me)),
            account: Some(me),
            endpoint: "http://127.0.0.1:0".into(),
        }
    }

    /// The chain behind `host`: every node method a view asks that the
    /// test did not answer itself is answered here, and each submission's
    /// writes reach the view's `program.changes` subscriptions after the
    /// frame. A typed `handle` on `host` for one program still wins.
    pub fn back(&self, host: &FakeHost) {
        self.found_roster();
        self.take_changes();
        for kind in [
            "program.query",
            "op.submit",
            "chain.status",
            "chain.network",
            "chain.blocks",
            "chain.block",
            "chain.tx",
            "blob.get",
            "blob.manifest",
            "invite.create",
            "program.describe",
            "file.pick",
            "file.release",
            "file.read",
            "op.put",
        ] {
            let net = self.clone();
            host.answer_with(kind, None, move |payload| {
                net.answer(kind, target_of(kind, payload).as_deref(), payload)
            });
        }
        let net = self.clone();
        host.after_frame(move |host, _, _| {
            for (program, change) in net.take_changes() {
                let keys = change.writes.into_iter().map(|(key, _)| key).collect();
                let item = Some(methods::Change {
                    height: change.height,
                    keys,
                });
                host.send_raw("program.changes", Some(&program), methods::encode(&item));
            }
        });
    }

    /// Puts `body` as a blob of `kind`, as the node's blob upload does:
    /// held, so every program reads it.
    pub fn put_blob(&self, kind: &str, body: Vec<u8>) -> Result<abi::BlobId, Error> {
        self.0.borrow().chain.put_blob(kind, body)
    }

    /// The files a person picks on this device, by name, in order:
    /// `file.pick` answers them (the first alone to a pick not `multiple`;
    /// none, a cancelled pick).
    pub fn device(&self, files: &[(&str, &[u8])]) {
        self.0.borrow_mut().device = files
            .iter()
            .map(|(name, bytes)| ((*name).to_owned(), bytes.to_vec()))
            .collect();
    }

    fn picked(&self, file: u64, offset: u64, len: u64) -> Option<Vec<u8>> {
        let state = self.0.borrow();
        let (_, bytes) = state.pick.iter().find(|(handle, _)| *handle == file)?;
        let start = usize::try_from(offset).ok()?;
        let end = start.checked_add(usize::try_from(len).ok()?)?;
        bytes.get(start..end).map(<[u8]>::to_vec)
    }

    /// One node method at the view-wire level: an override, else the chain.
    /// A submission runs as the dev account, the one a test's session
    /// signs in as.
    pub fn answer(
        &self,
        kind: &str,
        target: Option<&str>,
        payload: &[u8],
    ) -> Result<Vec<u8>, Error> {
        self.answer_as(self.key_of(self.me()), kind, target, payload)
    }

    pub(crate) fn answer_as(
        &self,
        signer: Vec<u8>,
        kind: &str,
        target: Option<&str>,
        payload: &[u8],
    ) -> Result<Vec<u8>, Error> {
        let exact = (kind.to_owned(), target.map(str::to_owned));
        let any = (kind.to_owned(), None);
        let mut state = self.0.borrow_mut();
        let key = [exact, any]
            .into_iter()
            .find(|key| state.overrides.contains_key(key));
        if let Some(handler) = key.and_then(|key| state.overrides.get_mut(&key)) {
            return handler(payload);
        }
        drop(state);
        let malformed = |error: &str| crate::host::malformed(error.to_owned());
        match kind {
            "program.query" => {
                let call: methods::Call =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                self.0.borrow().chain.query_bytes(&call.target, &call.body)
            }
            "op.submit" => {
                let call: methods::Call =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let receipt = self.run(Origin::Signed(signer), call.target, call.body);
                let state = self.0.borrow();
                let tx = state.blocks.last().and_then(|block| block.txs.last());
                let tx = tx.expect("the block just landed holds its tx");
                let (frame, until) = (tx.hash, tx.anchor.height + ANCHOR_WINDOW);
                Ok(methods::encode(&match receipt.outcome {
                    methods::Outcome::Applied { output } => methods::Submitted::Applied {
                        frame,
                        until,
                        output,
                    },
                    methods::Outcome::Rejected(refusal) => methods::Submitted::Refused {
                        frame,
                        until,
                        refusal,
                    },
                }))
            }
            "chain.status" => Ok(methods::encode(&self.status())),
            "chain.network" => {
                let members = match self.query(valset::PROGRAM, &valset::Query::Members)? {
                    valset::Reply::Members(members) => members,
                    other => {
                        return Err(program::unexpected_reply(
                            valset::PROGRAM,
                            "Members",
                            &other,
                        ));
                    }
                };
                let height = self.height();
                Ok(methods::encode(&methods::NetworkStatus {
                    height,
                    members: members
                        .into_iter()
                        .map(|member| methods::Peer {
                            key: member.key,
                            signed: Some(height),
                        })
                        .collect(),
                }))
            }
            "chain.blocks" => {
                let page: methods::BlockPage =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let state = self.0.borrow();
                let before = page.before.unwrap_or(u64::MAX);
                let blocks = state
                    .blocks
                    .iter()
                    .rev()
                    .filter(|block| block.height < before)
                    .take(page.limit as usize)
                    .map(|block| listed(block, page.most))
                    .collect();
                let listed_from = state.blocks.first().map(|block| block.height);
                Ok(methods::encode(&methods::BlockList {
                    blocks,
                    listed_from,
                }))
            }
            "chain.block" => {
                let ask: methods::BlockAsk =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let state = self.0.borrow();
                let block = state.blocks.iter().find(|block| match &ask.by {
                    methods::BlockRef::Height(height) => block.height == *height,
                    methods::BlockRef::Id(id) => block.id == *id,
                });
                Ok(methods::encode(&block.map(|block| listed(block, ask.most))))
            }
            "chain.tx" => {
                let ask: methods::TxAsk =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let state = self.0.borrow();
                let found = state.blocks.iter().find_map(|block| {
                    let position = match &ask.tx {
                        methods::TxRef::At { height, position } => {
                            (block.height == *height).then_some(*position)?
                        }
                        methods::TxRef::Hash(hash) => {
                            block.txs.iter().position(|tx| tx.hash == *hash)? as u32
                        }
                    };
                    let tx = block.txs.get(position as usize)?;
                    Some(methods::Located {
                        height: block.height,
                        position,
                        time: block.time,
                        tx: capped(tx, ask.most),
                    })
                });
                Ok(methods::encode(&found))
            }
            "file.pick" => {
                let ask: methods::Pick =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let mut state = self.0.borrow_mut();
                let most = if ask.multiple { usize::MAX } else { 1 };
                let files: Vec<_> = state.device.iter().take(most).cloned().collect();
                let first = state.handles + 1;
                if !files.is_empty() {
                    state.handles += files.len() as u64;
                    let bodies = files.iter().map(|(_, bytes)| bytes.clone());
                    state.pick.extend((first..).zip(bodies));
                }
                let picked: Vec<methods::Picked> = (files.into_iter().zip(first..))
                    .map(|((name, bytes), file)| methods::Picked {
                        name,
                        size: bytes.len() as u64,
                        file,
                    })
                    .collect();
                Ok(methods::encode(&picked))
            }
            "file.read" => {
                let ask: methods::ReadFile =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let bytes = self.picked(ask.file, ask.offset, ask.len).ok_or_else(|| {
                    Error::new(
                        methods::refusal::MALFORMED_REQUEST,
                        "no file the view holds holds that range",
                    )
                })?;
                Ok(methods::encode(&bytes))
            }
            "file.release" => {
                let handles: Vec<u64> =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                let mut state = self.0.borrow_mut();
                state.pick.retain(|(handle, _)| !handles.contains(handle));
                Ok(methods::encode(&()))
            }
            "op.put" => {
                let malformed =
                    |sentence: &str| Error::new(methods::refusal::MALFORMED_REQUEST, sentence);
                let ask: methods::PutBlobs =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                if ask.blobs.is_empty() {
                    return Err(malformed("an op.put names at least one blob"));
                }
                let mut bodies = Vec::with_capacity(ask.blobs.len());
                for blob in ask.blobs {
                    let body = match blob.body {
                        methods::BlobBody::Bytes(bytes) => bytes,
                        methods::BlobBody::Picked { file, offset, len } => self
                            .picked(file, offset, len)
                            .ok_or_else(|| malformed("no file the view holds holds that slice"))?,
                    };
                    bodies.push((blob.kind, body));
                }
                let mut ids = Vec::with_capacity(bodies.len());
                for (kind, body) in bodies {
                    let abi::BlobId::Sha256(digest) = self.put_blob(&kind, body)? else {
                        unreachable!("a blob is put by its sha256")
                    };
                    ids.push(digest);
                }
                Ok(methods::encode(&methods::Putting::Held(ids)))
            }
            "blob.get" => {
                let body = self
                    .blob(&blob_id(payload)?)
                    .map(|framed| unframe(&framed).unwrap_or_default().to_vec());
                Ok(methods::encode(&body))
            }
            "blob.manifest" => {
                use view_wire::manifest::{MANIFEST_SECTION, Manifest};
                let blob = self.blob(&blob_id(payload)?).map(|framed| {
                    let bytes = unframe(&framed).unwrap_or_default();
                    let view = pack::section(bytes, "ducktape.view")
                        .or_else(|| pack::section(bytes, MANIFEST_SECTION).map(|_| bytes))
                        .map(|view| methods::CarriedView {
                            size: view.len() as u64,
                            manifest: pack::section(view, MANIFEST_SECTION)
                                .and_then(|text| Manifest::parse(std::str::from_utf8(text).ok()?)),
                        });
                    methods::Manifested {
                        size: bytes.len() as u64,
                        view,
                    }
                });
                Ok(methods::encode(&blob))
            }
            "invite.create" => {
                let invite: methods::CreateInvite =
                    methods::decode(payload).map_err(|error| malformed(&error))?;
                if invite.ttl_days == 0 {
                    return Err(malformed("ttl_days must be a positive integer"));
                }
                let mut state = self.0.borrow_mut();
                state.invites += 1;
                Ok(methods::encode(&methods::Invite {
                    invite: format!("live-invite-{}", state.invites),
                    notes: Vec::new(),
                }))
            }
            "program.describe" => Ok(methods::encode(&None::<methods::Description>)),
            other => Err(Error::new(
                methods::refusal::UNKNOWN_REQUEST,
                format!("the fake node does not answer `{other}`"),
            )),
        }
    }

    /// `chain.status` as the archive stands.
    pub fn status(&self) -> methods::NodeStatus {
        let state = self.0.borrow();
        let tip = state.blocks.last().expect("the genesis block");
        methods::NodeStatus {
            chain_id: CHAIN.into(),
            time: state.founded,
            block_time_ms: BLOCK_TIME_MS,
            epoch_length: 100,
            height: tip.height,
            tip: tip.id,
            root: [0; 32],
            epoch: 0,
            identity: state.validator.clone(),
            contract: wire::NODE_CONTRACT,
        }
    }

    pub fn height(&self) -> u64 {
        self.0.borrow().blocks.len() as u64
    }

    pub(crate) fn genesis(&self) -> [u8; 32] {
        self.0.borrow().blocks[0].id
    }

    pub(crate) fn roles(&self) -> abi::Roles {
        self.0.borrow().chain.env(identity::PROGRAM).roles
    }

    pub(crate) fn fresh(&self, body: &wire::Body) -> Result<(), abi::Refusal> {
        let state = self.0.borrow();
        let next = state.blocks.len() as u64 + 1;
        let anchor = body.anchor;
        let in_window = anchor.height < next && next - anchor.height <= ANCHOR_WINDOW;
        let names_a_block = state
            .blocks
            .iter()
            .any(|block| block.height == anchor.height && block.id == anchor.id);
        if !(in_window && names_a_block) {
            return Err(abi::Refusal::new(
                abi::reason::ANCHOR,
                format!(
                    "the frame's anchor is not one of the {ANCHOR_WINDOW} blocks before block {next}"
                ),
            ));
        }
        if state.spent.contains(&body.id()) {
            return Err(abi::Refusal::new(
                abi::reason::DUPLICATE,
                "a block already carried this frame",
            ));
        }
        Ok(())
    }

    pub(crate) fn spend(&self, id: [u8; 32]) {
        self.0.borrow_mut().spent.insert(id);
    }

    pub(crate) fn blob(&self, id: &abi::BlobId) -> Option<Vec<u8>> {
        let state = self.0.borrow();
        if let Some((_, framed)) = state.codes.values().find(|(code, _)| code == id) {
            return Some(framed.clone());
        }
        if let Some((_, bare, framed)) = &state.bare
            && bare == id
        {
            return Some(framed.clone());
        }
        state.seats.iter().find_map(|(_, host)| {
            let host = host.borrow();
            let blob = host.blobs.get(id)?;
            Some(wire::framed(&blob.kind, &blob.body))
        })
    }

    pub(crate) fn code(&self, program: &str, body: &[u8]) {
        let framed = wire::framed(programs::CODE_KIND, body);
        let id = wire::blob_id(&framed);
        self.0
            .borrow_mut()
            .codes
            .insert(program.to_owned(), (id, framed));
    }

    pub(crate) fn code_bare(&self, name: &str, body: &[u8]) {
        let framed = wire::framed("view", body);
        let id = wire::blob_id(&framed);
        self.0.borrow_mut().bare = Some((name.to_owned(), id, framed));
    }

    pub(crate) fn found_roster(&self) {
        let mut state = self.0.borrow_mut();
        if std::mem::replace(&mut state.founded_roster, true) {
            return;
        }
        let programs = state
            .seats
            .iter()
            .map(|(name, _)| Entry {
                program: name.clone(),
                code: state
                    .codes
                    .get(name)
                    .map(|(id, _)| *id)
                    .unwrap_or(abi::BlobId::Sha256(digest(&[
                        b"live-code",
                        name.as_bytes(),
                    ]))),
                params: state.params.get(name).cloned().unwrap_or_default(),
            })
            .collect();
        let views = state
            .bare
            .iter()
            .map(|(name, id, _)| View {
                name: name.clone(),
                view: *id,
            })
            .collect();
        state
            .chain
            .init(
                programs::PROGRAM,
                &Roster {
                    programs,
                    views,
                    hooks: state.hooks.iter().map(|hook| hook.name.clone()).collect(),
                    roles: state.chain.env(programs::PROGRAM).roles,
                },
            )
            .expect("the programs role founds with the seated programs");
    }

    pub(crate) fn subscribe(&self, program: &str, sender: std::sync::mpsc::Sender<Vec<u8>>) {
        self.0
            .borrow_mut()
            .subscribers
            .push((program.to_owned(), sender));
    }

    pub(crate) fn take_changes(&self) -> Vec<(String, wire::Change)> {
        std::mem::take(&mut self.0.borrow_mut().changes)
    }

    pub(crate) fn run(&self, origin: Origin, target: String, payload: Vec<u8>) -> methods::Receipt {
        let (signer, signed) = match &origin {
            Origin::Signed(key) => (key.clone(), true),
            Origin::Program(_) | Origin::Root => (Vec::new(), false),
        };
        let mut state = self.0.borrow_mut();
        let height = state.blocks.len() as u64 + 1;
        let last = state
            .blocks
            .last()
            .map_or(state.founded, |block| block.time);
        let time = state
            .pinned
            .take()
            .unwrap_or_else(|| now_ms().max(last + 1));
        state.chain.at(height, time);
        let before: Vec<BTreeMap<Vec<u8>, Vec<u8>>> = state
            .seats
            .iter()
            .map(|(_, host)| host.borrow().state.clone())
            .collect();
        let account = match &origin {
            Origin::Signed(key) => acting(&state.chain, key),
            Origin::Program(_) | Origin::Root => None,
        };
        let ran = state.chain.submit_raw(origin, target.clone(), &payload);
        let events = state
            .seats
            .iter()
            .find(|(name, _)| *name == target)
            .map(|(_, host)| std::mem::take(&mut host.borrow_mut().events))
            .unwrap_or_default();
        let record = hook::Record {
            height,
            position: 0,
            signer: signer.clone(),
            account,
            target: target.clone(),
            applied: ran.is_ok(),
        };
        for hook in state.hooks.iter().filter(|_| signed) {
            tell_hook_and_rollback_refusal(&state.chain, hook, record.clone());
        }
        let outcome = match ran {
            Ok(output) => methods::Outcome::Applied { output },
            Err(refusal) => methods::Outcome::Rejected(refusal),
        };
        let parent = state.blocks.last().expect("the genesis block");
        let anchor = methods::Tip {
            height: parent.height,
            id: parent.id,
        };
        let parent = parent.id;
        let tx = methods::Tx {
            hash: digest(&[b"live-tx", &height.to_le_bytes()]),
            signer: signer.clone(),
            anchor,
            target: target.clone(),
            size: payload.len() as u32,
            payload: Some(payload),
            receipt: Some(methods::Receipt {
                program: target,
                outcome: outcome.clone(),
                events: events.clone(),
                nested: Vec::new(),
            }),
        };
        let proposer = state.validator.clone();
        state.blocks.push(methods::Block {
            height,
            id: digest(&[b"live-block", &height.to_le_bytes()]),
            parent,
            time,
            epoch: 0,
            proposer: Some(proposer),
            txs: vec![tx.clone()],
        });
        let mut changed = Vec::new();
        for ((name, host), before) in state.seats.iter().zip(before) {
            let after = &host.borrow().state;
            let writes: Writes = before
                .keys()
                .chain(after.keys())
                .filter(|key| before.get(*key) != after.get(*key))
                .map(|key| (key.clone(), after.get(key).cloned()))
                .collect();
            if !writes.is_empty() {
                changed.push((
                    name.clone(),
                    wire::Change {
                        height,
                        root: abi::Root([0; 32]),
                        writes,
                    },
                ));
            }
        }
        state.changes.extend(changed);
        tx.receipt.expect("the receipt just made")
    }

    pub(crate) fn flush(&self) {
        let mut state = self.0.borrow_mut();
        for (program, change) in std::mem::take(&mut state.changes) {
            let item = abi::encode(&change);
            state
                .subscribers
                .retain(|(name, sender)| *name != program || sender.send(item.clone()).is_ok());
        }
    }
}

fn acting(chain: &MockChain, key: &[u8]) -> Option<AccountNumber> {
    let asked = abi::role::identity::Query::Account(key.to_vec());
    match chain.query(identity::PROGRAM, &asked) {
        Ok(abi::role::identity::Reply::Account(account)) => account,
        Ok(_) | Err(_) => None,
    }
}

fn tell_hook_and_rollback_refusal(chain: &MockChain, hook: &Hook, record: hook::Record) {
    let Hook { name, host, entry } = hook;
    let kept = host.borrow().state.clone();
    let height = record.height;
    let told = entry(&host.exec(chain.env(name)), record);
    let sent = !host.take_emissions().is_empty();
    let refused = match told {
        Err(refusal) => refusal.to_string(),
        Ok(()) if sent => "a hook sends no messages".to_owned(),
        Ok(()) => return,
    };
    eprintln!("hook {name} refused the frame at {height}: {refused}");
    host.borrow_mut().state = kept;
}

fn listed(block: &methods::Block, most: u32) -> methods::Block {
    methods::Block {
        txs: block.txs.iter().map(|tx| capped(tx, most)).collect(),
        ..block.clone()
    }
}

fn capped(tx: &methods::Tx, most: u32) -> methods::Tx {
    let within = tx.size <= most;
    methods::Tx {
        payload: tx.payload.clone().filter(|_| within),
        ..tx.clone()
    }
}

fn target_of(kind: &str, payload: &[u8]) -> Option<String> {
    match kind {
        "program.query" | "op.submit" => methods::decode::<methods::Call>(payload)
            .ok()
            .map(|call| call.target),
        _ => None,
    }
}

fn blob_id(payload: &[u8]) -> Result<abi::BlobId, Error> {
    let malformed = |error: &str| crate::host::malformed(error.to_owned());
    let id: String = methods::decode(payload).map_err(|error| malformed(&error))?;
    let (kind, hex) = id
        .split_once(':')
        .ok_or_else(|| malformed("id is `sha256:<hex>` or `sha1:<hex>`"))?;
    let digest = hex_decode(hex).map_err(|error| malformed(&error))?;
    Ok(match (kind, digest.len()) {
        ("sha256", 32) => abi::BlobId::Sha256(digest.try_into().unwrap()),
        ("sha1", 20) => abi::BlobId::Sha1(digest.try_into().unwrap()),
        _ => return Err(malformed("id is `sha256:<hex>` or `sha1:<hex>`")),
    })
}

pub(crate) fn unframe(framed: &[u8]) -> Option<&[u8]> {
    let nul = framed.iter().position(|byte| *byte == 0)?;
    Some(&framed[nul + 1..])
}

fn hex_decode(value: &str) -> Result<Vec<u8>, String> {
    if !value.bytes().all(|byte| byte.is_ascii_hexdigit()) || !value.len().is_multiple_of(2) {
        return Err("not hex".into());
    }
    (0..value.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&value[at..at + 2], 16).map_err(|error| error.to_string()))
        .collect()
}

#[cfg(test)]
mod tests;
