//! The one place the kernel's names become the program SDK's.
//!
//! `abi` is a byte-for-byte copy of the kernel's contract and keeps the
//! kernel's names. A program author reads these instead; every type here has
//! the same borsh layout as its kernel twin (borsh encodes no names), so the
//! conversions below move fields and never touch bytes.
//!
//! | kernel (`abi`)                           | program SDK (`program`)                   |
//! |------------------------------------------|-------------------------------------------|
//! | `Refusal { reason, sentence }`, `reason` | [`Error`] `{ code, message }`, [`code`]   |
//! | `Origin::{External, Program, System}`    | [`Origin`]`::{Signed, Program, Root}`     |
//! | `Env { network, me, .. }`                | [`Env`] `{ chain_id, program, .. }`       |
//! | `Roles`, `ProgramId`                     | [`Roles`], [`ProgramId`] (the same types) |
//! | `ItemRef { source, item }`               | [`MessageId`] `{ program, seq }`          |
//! | `Cause::{Direct, Message, Completion, Call, Scheduled}` | [`Cause`]`::{Direct, Message, Reply, Call, Scheduled}` |
//! | `Outcome` (carries a `Refusal`)          | [`Outcome`] (carries an [`Error`])        |
//! | `Scan { lo, hi, reverse, limit }`        | [`Range`] `{ start, end, order, limit }`  |

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use abi::role::awaits::{FrameId, Ticket};
pub use abi::role::scheduler::EntryId;
pub use abi::{ProgramId, Roles};
pub use error::{Error, code};

/// The kernel's refusal as the SDK's [`Error`]: the same two strings.
pub fn error_from(refusal: abi::Refusal) -> Error {
    Error::new(refusal.reason, refusal.sentence)
}

/// The SDK's [`Error`] as the kernel's refusal.
pub fn refusal_from(error: Error) -> abi::Refusal {
    abi::Refusal::new(error.code, error.message)
}

/// Who called: a signed transaction (the signer's key), another program, or
/// the chain itself (genesis and system-internal calls).
#[derive(Clone, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub enum Origin {
    Signed(Vec<u8>),
    Program(ProgramId),
    Root,
}

impl From<abi::Origin> for Origin {
    fn from(o: abi::Origin) -> Self {
        match o {
            abi::Origin::External(key) => Origin::Signed(key),
            abi::Origin::Program(id) => Origin::Program(id),
            abi::Origin::System => Origin::Root,
        }
    }
}

impl From<Origin> for abi::Origin {
    fn from(o: Origin) -> Self {
        match o {
            Origin::Signed(key) => abi::Origin::External(key),
            Origin::Program(id) => abi::Origin::Program(id),
            Origin::Root => abi::Origin::System,
        }
    }
}

/// An account's number: the identity role's (`abi::role::identity`).
pub type AccountNumber = abi::role::identity::AccountNumber;

pub type NetworkId = abi::NetworkId;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(deny_unknown_fields)
)]
pub struct Foreign {
    pub network: NetworkId,
    pub account: AccountNumber,
}

impl core::fmt::Display for Foreign {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "account {} of network {}",
            self.account,
            abi::hex(&self.network)
        )
    }
}

impl From<abi::Foreign> for Foreign {
    fn from(f: abi::Foreign) -> Self {
        Foreign {
            network: f.network,
            account: f.account,
        }
    }
}

impl From<Foreign> for abi::Foreign {
    fn from(f: Foreign) -> Self {
        abi::Foreign {
            network: f.network,
            account: f.account,
        }
    }
}

/// There is no default principal: "nobody" is `Option<Principal>::None`,
/// never the most trusted variant.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case", deny_unknown_fields)
)]
pub enum Principal {
    /// a user, a program account or a system account: an identity account.
    Account(AccountNumber),
    /// genesis and system-internal writes.
    Root,
    Foreign(Foreign),
}

impl Principal {
    /// The account this principal is, if it is one.
    pub fn account(&self) -> Option<AccountNumber> {
        match self {
            Principal::Account(account) => Some(*account),
            Principal::Root | Principal::Foreign(_) => None,
        }
    }

    /// The principal a reader writes as, from the account their seated key
    /// holds. None while it holds none: every view gates its writes on this,
    /// the way [`ExecCtx::sender`](crate::ExecCtx::sender) refuses them.
    pub fn writer(account: Option<AccountNumber>) -> Option<Principal> {
        account.map(Principal::Account)
    }

    /// A principal typed by a person: an account number, `acct:<n>` too.
    pub fn parse(text: &str) -> Option<Principal> {
        let text = text.trim();
        let number = text.strip_prefix("acct:").unwrap_or(text);
        number.parse().ok().map(Principal::Account)
    }
}

impl From<abi::Principal> for Principal {
    fn from(p: abi::Principal) -> Self {
        match p {
            abi::Principal::Account(number) => Principal::Account(number),
            abi::Principal::System => Principal::Root,
            abi::Principal::Foreign(foreign) => Principal::Foreign(foreign.into()),
        }
    }
}

impl From<Principal> for abi::Principal {
    fn from(p: Principal) -> Self {
        match p {
            Principal::Account(number) => abi::Principal::Account(number),
            Principal::Root => abi::Principal::System,
            Principal::Foreign(foreign) => abi::Principal::Foreign(foreign.into()),
        }
    }
}

/// A message a program emitted: the emitter and the message's number in
/// its frame.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
)]
pub struct MessageId {
    pub program: ProgramId,
    pub seq: u64,
}

impl From<abi::ItemRef> for MessageId {
    fn from(i: abi::ItemRef) -> Self {
        MessageId {
            program: i.source,
            seq: i.item,
        }
    }
}

impl From<MessageId> for abi::ItemRef {
    fn from(m: MessageId) -> Self {
        abi::ItemRef {
            source: m.program,
            item: m.seq,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Outcome {
    Applied { output: Vec<u8> },
    Rejected(Error),
}

impl From<abi::Outcome> for Outcome {
    fn from(o: abi::Outcome) -> Self {
        match o {
            abi::Outcome::Applied { output } => Outcome::Applied { output },
            abi::Outcome::Rejected(r) => Outcome::Rejected(error_from(r)),
        }
    }
}

impl From<Outcome> for abi::Outcome {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Applied { output } => abi::Outcome::Applied { output },
            Outcome::Rejected(e) => abi::Outcome::Rejected(refusal_from(e)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Cause {
    Direct,
    Message(MessageId),
    Reply { id: MessageId, outcome: Outcome },
    Call { ticket: Ticket },
    Scheduled { entry: EntryId },
}

impl From<abi::Cause> for Cause {
    fn from(c: abi::Cause) -> Self {
        match c {
            abi::Cause::Direct => Cause::Direct,
            abi::Cause::Message(item) => Cause::Message(item.into()),
            abi::Cause::Completion { item, outcome } => Cause::Reply {
                id: item.into(),
                outcome: outcome.into(),
            },
            abi::Cause::Call { ticket } => Cause::Call { ticket },
            abi::Cause::Scheduled { entry } => Cause::Scheduled { entry },
        }
    }
}

impl From<Cause> for abi::Cause {
    fn from(c: Cause) -> Self {
        match c {
            Cause::Direct => abi::Cause::Direct,
            Cause::Message(id) => abi::Cause::Message(id.into()),
            Cause::Reply { id, outcome } => abi::Cause::Completion {
                item: id.into(),
                outcome: outcome.into(),
            },
            Cause::Call { ticket } => abi::Cause::Call { ticket },
            Cause::Scheduled { entry } => abi::Cause::Scheduled { entry },
        }
    }
}

/// What a call runs in: the chain, the block, this program, who called and why.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Env {
    pub chain_id: Vec<u8>,
    pub height: u64,
    pub time: u64,
    /// This program's own id.
    pub program: ProgramId,
    pub origin: Origin,
    /// Who the frame acts as: `None` for a query, and for a frame whose
    /// key or program holds no account.
    pub sender: Option<Principal>,
    pub roles: Roles,
    pub cause: Cause,
}

impl Env {
    pub fn role(&self, role: &str) -> Result<&ProgramId, Error> {
        self.roles.bound(role).map_err(error_from)
    }
}

pub fn access_checker(roles: &Roles) -> Option<&ProgramId> {
    roles.get(abi::role::ACCESS)
}

impl From<abi::Env> for Env {
    fn from(e: abi::Env) -> Self {
        Env {
            chain_id: e.network,
            height: e.height,
            time: e.time,
            program: e.me,
            origin: e.origin.into(),
            sender: e.sender.map(Into::into),
            roles: e.roles,
            cause: e.cause.into(),
        }
    }
}

impl From<Env> for abi::Env {
    fn from(e: Env) -> Self {
        abi::Env {
            network: e.chain_id,
            height: e.height,
            time: e.time,
            me: e.program,
            origin: e.origin.into(),
            sender: e.sender.map(Into::into),
            roles: e.roles,
            cause: e.cause.into(),
        }
    }
}

/// A [`Range`]'s direction; the kernel's `reverse` flag (`false`, `true`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Order {
    #[default]
    Ascending,
    Descending,
}

/// The keys from `start` (inclusive) to `end` (exclusive; `None`: to the
/// last key), in `order`, at most `limit` of them.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Range {
    pub start: Vec<u8>,
    pub end: Option<Vec<u8>>,
    pub order: Order,
    pub limit: Option<u64>,
}

impl Range {
    pub fn new(start: impl Into<Vec<u8>>, end: Option<Vec<u8>>) -> Self {
        Range {
            start: start.into(),
            end,
            order: Order::Ascending,
            limit: None,
        }
    }

    pub fn prefix(prefix: impl AsRef<[u8]>) -> Self {
        let prefix = prefix.as_ref();
        Range::new(prefix.to_vec(), abi::prefix_end(prefix))
    }

    pub fn after(mut self, key: impl AsRef<[u8]>) -> Self {
        let mut start = key.as_ref().to_vec();
        start.push(0);
        self.start = start;
        self
    }

    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn reverse(mut self) -> Self {
        self.order = Order::Descending;
        self
    }

    pub fn admits(&self, key: &[u8]) -> bool {
        let above_start = key >= self.start.as_slice();
        let below_end = self.end.as_ref().is_none_or(|end| key < end.as_slice());
        above_start && below_end
    }
}

impl From<Range> for abi::Scan {
    fn from(r: Range) -> Self {
        abi::Scan {
            lo: r.start,
            hi: r.end,
            reverse: r.order == Order::Descending,
            limit: r.limit,
        }
    }
}

impl From<abi::Scan> for Range {
    fn from(s: abi::Scan) -> Self {
        Range {
            start: s.lo,
            end: s.hi,
            order: if s.reverse {
                Order::Descending
            } else {
                Order::Ascending
            },
            limit: s.limit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sdk_type_is_its_kernel_twins_bytes() {
        let replies = [
            Outcome::Rejected(Error::new("x", "y")),
            Outcome::Applied { output: vec![4] },
        ];
        let causes = replies
            .into_iter()
            .map(|outcome| Cause::Reply {
                id: MessageId {
                    program: "a".into(),
                    seq: 3,
                },
                outcome,
            })
            .chain([
                Cause::Direct,
                Cause::Message(MessageId {
                    program: "c".into(),
                    seq: 1,
                }),
            ]);
        let origins = [
            Origin::Signed(vec![1]),
            Origin::Program("b".into()),
            Origin::Root,
        ];
        let foreign = Principal::Foreign(Foreign {
            network: [5; 32],
            account: 18,
        });
        let senders = [
            Some(Principal::Account(4)),
            Some(Principal::Root),
            None,
            Some(foreign),
        ];
        let calls = origins.into_iter().cycle().zip(senders.into_iter().cycle());
        for (cause, (origin, sender)) in causes.zip(calls) {
            let env = Env {
                chain_id: b"n".to_vec(),
                height: 7,
                time: 9,
                program: "a".into(),
                origin,
                sender,
                roles: crate::MockHost::roles(),
                cause,
            };
            let kernel: abi::Env = env.clone().into();
            assert_eq!(abi::encode(&env), abi::encode(&kernel));
            assert_eq!(abi::decode::<Env>(&abi::encode(&kernel)).unwrap(), env);
            assert_eq!(Env::from(kernel), env);
        }
        let error = Error::new(code::STALE, "behind");
        let refusal = refusal_from(error.clone());
        assert_eq!(abi::encode(&error), abi::encode(&refusal));
        assert_eq!(error_from(refusal), error);
        for range in [
            Range::prefix(b"t/").after(b"t/7").reverse().limit(2),
            Range::new(b"a".to_vec(), None),
        ] {
            let scan: abi::Scan = range.clone().into();
            assert_eq!(abi::encode(&range), abi::encode(&scan));
            assert_eq!(Range::from(scan), range);
        }
        let range = Range::prefix(b"t/").after(b"t/7");
        assert!(!range.admits(b"t/7"));
        assert!(range.admits(b"t/8"));
        assert!(!range.admits(b"u"));
    }

    #[test]
    fn a_principal_reads_back_from_input() {
        assert_eq!(Principal::parse(" 7 "), Some(Principal::Account(7)));
        assert_eq!(Principal::parse("acct:7"), Some(Principal::Account(7)));
        for nothing in ["acct:x", "user:ab01", "ab01", "", "someone"] {
            assert_eq!(Principal::parse(nothing), None, "{nothing}");
        }
        assert_eq!(Principal::writer(Some(3)), Some(Principal::Account(3)));
        assert_eq!(Principal::writer(None), None);
    }

    #[test]
    fn codes_are_the_kernels_reasons() {
        use abi::reason as r;
        let pairs = [
            (code::UNKNOWN_PROGRAM, r::UNKNOWN_PROGRAM),
            (code::TRAP, r::TRAP),
            (code::PROTOCOL, r::PROTOCOL),
            (code::ANCHOR, r::ANCHOR),
            (code::DUPLICATE, r::DUPLICATE),
            (code::NOT_FOUND, r::NOT_FOUND),
            (code::ALREADY_EXISTS, r::ALREADY_EXISTS),
            (code::STALE, r::STALE),
            (code::WRONG_STATE, r::WRONG_STATE),
            (code::INVALID_INPUT, r::INVALID_INPUT),
            (code::CAPACITY, r::CAPACITY),
            (code::NOT_YET, r::NOT_YET),
            (code::EXHAUSTED, r::EXHAUSTED),
            (code::UNAUTHORIZED, r::UNAUTHORIZED),
            (code::UNSUPPORTED, r::UNSUPPORTED),
            (code::CORRUPT, r::CORRUPT),
            (code::UNEXPECTED_REPLY, r::UNEXPECTED_REPLY),
        ];
        for (code, reason) in pairs {
            assert_eq!(code, reason);
        }
        let count = |text: &str| text.matches("pub const").count();
        let metadata = std::process::Command::new(env!("CARGO"))
            .args(["metadata", "--format-version", "1", "--manifest-path"])
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .output()
            .expect("cargo metadata runs");
        assert!(metadata.status.success(), "{metadata:?}");
        let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
        let abi = metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|package| package["name"] == "abi")
            .expect("the SDK links abi");
        let manifest = std::path::Path::new(abi["manifest_path"].as_str().unwrap());
        let reasons = std::fs::read_to_string(manifest.with_file_name("src/lib.rs")).unwrap();
        let reasons = &reasons[reasons.find("pub mod reason").unwrap()..];
        let reasons = &reasons[..reasons.find("\n}").unwrap()];
        let codes = include_str!("../../error/src/lib.rs");
        let codes = &codes[codes.find("pub mod code").unwrap()..];
        assert_eq!(count(reasons), pairs.len());
        assert_eq!(count(codes), pairs.len());
    }
}
