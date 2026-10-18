//! The `identity` program: accounts, and who acts as each. Everything that
//! acts is an account: a user (the keys they hold), a program account (the
//! keys its manager, a user, adds, and the script that runs its actions) and
//! a system account (the one the kernel registers as it admits a program). Who edits an
//! account is its key's list in the access program, owned by the user, the program account's
//! manager or the system account itself. The types, rules and [`Identity`] program are
//! always built; a view links them with `program` off (`view.rs` is what a
//! view shows of an account). The `program` feature adds its wasm exports.
//!
//! It fills the kernel's identity role (`abi::role::identity`): the role's
//! op, queries and replies are the first variants of [`Op`], [`Query`] and
//! [`Reply`]. Who may do what, op by op: `docs/roles.md` at the repository
//! root.
mod program;
mod rules;
#[cfg(test)]
mod tests;
pub mod view;

pub use ::program::{AccountNumber, Ownership, Party};
pub use abi::role::identity::{Category, Kind, Profile, Script, Standing};
pub use program::Identity;
pub use rules::account_key;

use ::program::{BlobId, ProgramId, Scheme};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};
pub use store::{PageRequest, PageResponse};

/// The name this program runs under.
pub const PROGRAM: &str = "identity";
/// What a key signs to consent to a key joining an account ([`Admission`]).
pub const CONSENT_NAMESPACE: &[u8] = b"ducktape:identity:consent";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Key {
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub label: Option<String>,
    pub added_at: u64,
}

/// One account: what it shows ([`Card`]) and who acts as it ([`Control`]).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Account {
    pub number: AccountNumber,
    pub card: Card,
    pub control: Control,
}

/// What an account shows of itself.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Card {
    pub name: String,
    pub avatar: Option<BlobId>,
    pub bio: Option<String>,
    pub updated_at: u64,
}

/// Who acts as an account, and with which keys. A system account has no
/// keys to hold; a revoked program account has none left ([`Life`]).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Control {
    /// A user: their own keys, which they add and remove.
    User { keys: Vec<Key> },
    /// A program account: its `manager`, a user, adds its keys, declares
    /// its `category`, sets its `script`, suspends, resumes and revokes it.
    Program {
        manager: AccountNumber,
        category: Category,
        life: Life,
        script: Option<Script>,
    },
    /// A program's own account: the program alone acts as it.
    System { program: ProgramId },
}

/// Whether a program account acts. Suspended keeps its keys, so resuming
/// issues none again; revoked is final and keeps none.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Life {
    Active { keys: Vec<Key> },
    Suspended { keys: Vec<Key> },
    Revoked,
}

impl Account {
    /// The keys that act as this account: none for a system account or a
    /// revoked program account.
    pub fn keys(&self) -> &[Key] {
        match &self.control {
            Control::User { keys } => keys,
            Control::Program {
                life: Life::Active { keys } | Life::Suspended { keys },
                ..
            } => keys,
            Control::Program {
                life: Life::Revoked,
                ..
            }
            | Control::System { .. } => &[],
        }
    }

    pub fn holds(&self, key: &[u8]) -> bool {
        self.keys().iter().any(|held| held.key == key)
    }

    /// What this account is, as the identity role says it.
    pub fn kind(&self) -> Kind {
        match &self.control {
            Control::User { .. } => Kind::User,
            Control::Program {
                manager,
                category,
                life,
                script,
            } => Kind::Program {
                manager: *manager,
                category: *category,
                standing: match life {
                    Life::Active { .. } => Standing::Active,
                    Life::Suspended { .. } => Standing::Suspended,
                    Life::Revoked => Standing::Revoked,
                },
                script: script.clone(),
            },
            Control::System { program } => Kind::System(program.clone()),
        }
    }

    /// How others show this account.
    pub fn profile(&self) -> Profile {
        Profile {
            number: self.number,
            name: self.card.name.clone(),
            kind: self.kind(),
        }
    }
}

/// A key's consent to a new key joining an account: `proof` is `key`'s
/// signature over the [`Admission`]. For a user, `key` is one already on
/// the account and the new key signs the frame; for a program account,
/// `key` is the new key itself and the manager signs the frame.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Consent {
    pub key: Vec<u8>,
    pub account: AccountNumber,
    pub expires_at: u64,
    pub proof: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Admission {
    pub network: Vec<u8>,
    pub scheme: Scheme,
    pub key: Vec<u8>,
    pub generation: u64,
    pub account: AccountNumber,
    pub expires_at: u64,
}

impl Admission {
    pub fn preimage(&self) -> Vec<u8> {
        abi::encode(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    /// The identity role's op (`abi::role::identity::Op`), first: the
    /// system gives an admitted program its account.
    RegisterProgram {
        program: ProgramId,
    },
    /// A user's account, holding the signing key.
    Create {
        name: String,
        scheme: Scheme,
    },
    /// A program account of the agent category that the signer's account
    /// manages; it holds no key and no script yet.
    CreateAgent {
        name: String,
    },
    AddKey {
        scheme: Scheme,
        label: Option<String>,
        consent: Consent,
    },
    RemoveKey {
        account: AccountNumber,
        key: Vec<u8>,
    },
    SetName {
        account: AccountNumber,
        name: String,
    },
    SetProfile {
        account: AccountNumber,
        avatar: Option<BlobId>,
        bio: Option<String>,
    },
    /// A program account stops acting; its keys stay for `Resume`. Its
    /// manager's.
    Suspend {
        account: AccountNumber,
    },
    Resume {
        account: AccountNumber,
    },
    /// A program account stops for good: its keys are dropped. Its
    /// manager's.
    Revoke {
        account: AccountNumber,
    },
    SetOwnership {
        account: AccountNumber,
        ownership: Ownership,
    },
    SetScript {
        account: AccountNumber,
        script: Option<Script>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reference {
    Account(AccountNumber),
    Key(Vec<u8>),
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask,
)]
#[ask(Identity)]
pub enum Query {
    /// The identity role's queries (`abi::role::identity::Query`), first
    /// and in its order: the account a key acts as (refused while it does
    /// not act), a program's account, one account's profile and every
    /// account's.
    #[ask(Reply::Number(Option<AccountNumber>))]
    OfKey { key: Vec<u8> },
    #[ask(Reply::Number(Option<AccountNumber>))]
    OfProgram { program: ProgramId },
    #[ask(Reply::Profile(Option<Profile>))]
    Profile { number: AccountNumber },
    #[ask(Reply::Profiles { profiles: Vec<Profile>, next: Option<AccountNumber> })]
    Profiles {
        after: Option<AccountNumber>,
        limit: u32,
    },
    #[ask(Reply::Number(Option<AccountNumber>))]
    Runs {
        program: ProgramId,
        account: AccountNumber,
    },
    #[ask(Reply::Account(Option<Account>))]
    Get { number: AccountNumber },
    #[ask(Reply::Generation(u64))]
    Generation { key: Vec<u8> },
    #[ask(Reply::Resolved(Vec<Option<AccountNumber>>))]
    Resolve { references: Vec<Reference> },
    #[ask(Reply::Accounts(PageResponse<Account>))]
    List { page: PageRequest },
    #[ask(Reply::Accounts(PageResponse<Account>))]
    Managed {
        by: AccountNumber,
        page: PageRequest,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    /// The identity role's replies (`abi::role::identity::Reply`), first
    /// and in its order.
    Number(Option<AccountNumber>),
    Profile(Option<Profile>),
    Profiles {
        profiles: Vec<Profile>,
        next: Option<AccountNumber>,
    },
    Account(Option<Account>),
    Generation(u64),
    Resolved(Vec<Option<AccountNumber>>),
    Accounts(PageResponse<Account>),
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let account = |number: &AccountNumber| field("account", Value::Account(*number));
    let optional = |text: &Option<String>| Value::Text(text.clone().unwrap_or_else(|| "—".into()));
    let answers = |ownership: &Ownership| match ownership {
        Ownership::Owner => Value::text("its manager"),
        Ownership::Listed(parties) => Value::List(
            std::iter::once(Value::text("its manager"))
                .chain(parties.iter().map(|party| match party {
                    Party::Account(number) => Value::Account(*number),
                    Party::Program(program) => Value::Program(program.clone()),
                }))
                .collect(),
        ),
        Ownership::Anyone => Value::text("anyone"),
    };
    let scheme = |scheme: &abi::Scheme| {
        field(
            "scheme",
            Value::text(match scheme {
                abi::Scheme::Ed25519 => "Ed25519",
                abi::Scheme::Secp256k1 => "secp256k1",
                abi::Scheme::Secp256r1 => "secp256r1",
                abi::Scheme::Bls12381 => "BLS12-381",
            }),
        )
    };
    let (title, fields) = match op {
        Op::RegisterProgram { program } => (
            format!("Register program · {program}"),
            vec![field("program", Value::Program(program.clone()))],
        ),
        Op::Create { name, scheme: s } => (
            format!("Create · {name}"),
            vec![field("name", Value::text(name)), scheme(s)],
        ),
        Op::CreateAgent { name } => (
            format!("Create agent · {name}"),
            vec![field("name", Value::text(name))],
        ),
        Op::AddKey {
            scheme: s,
            label,
            consent,
        } => (
            "Add key".into(),
            vec![
                scheme(s),
                field("label", optional(label)),
                field("key", Value::Key(consent.key.clone())),
                account(&consent.account),
                field("expires at", Value::Time(consent.expires_at)),
            ],
        ),
        Op::RemoveKey {
            account: number,
            key,
        } => (
            "Remove key".into(),
            vec![account(number), field("key", Value::Key(key.clone()))],
        ),
        Op::SetName {
            account: number,
            name,
        } => (
            format!("Set name · {name}"),
            vec![account(number), field("name", Value::text(name))],
        ),
        Op::SetProfile {
            account: number,
            avatar,
            bio,
        } => (
            "Set profile".into(),
            vec![
                account(number),
                field(
                    "avatar",
                    avatar.map_or_else(
                        || Value::text("—"),
                        |blob| Value::Hash(blob.digest().to_vec()),
                    ),
                ),
                field("bio", optional(bio)),
            ],
        ),
        Op::Suspend { account: number } => ("Suspend".into(), vec![account(number)]),
        Op::Resume { account: number } => ("Resume".into(), vec![account(number)]),
        Op::Revoke { account: number } => ("Revoke".into(), vec![account(number)]),
        Op::SetOwnership {
            account: number,
            ownership,
        } => (
            "Set who it answers".into(),
            vec![account(number), field("answers", answers(ownership))],
        ),
        Op::SetScript {
            account: number,
            script,
        } => (
            "Set script".into(),
            match script {
                Some(script) => vec![
                    account(number),
                    field("calls", Value::Program(script.calls.clone())),
                    field("config", Value::text(&script.config)),
                ],
                None => vec![account(number), field("script", Value::text("—"))],
            },
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

/// Old op bytes are described with the current code (`describe`): the op
/// enum only grows at its end. Append a new variant here; never reorder.
#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        [
            "RegisterProgram",
            "Create",
            "CreateAgent",
            "AddKey",
            "RemoveKey",
            "SetName",
            "SetProfile",
            "Suspend",
            "Resume",
            "Revoke",
            "SetOwnership",
            "SetScript",
        ]
    );
}

#[test]
fn query_and_reply_variants_only_append() {
    assert_eq!(
        describe::variants::<Query>(),
        [
            "OfKey",
            "OfProgram",
            "Profile",
            "Profiles",
            "Runs",
            "Get",
            "Generation",
            "Resolve",
            "List",
            "Managed",
        ]
    );
    assert_eq!(
        describe::variants::<Reply>(),
        [
            "Number",
            "Profile",
            "Profiles",
            "Account",
            "Generation",
            "Resolved",
            "Accounts",
        ]
    );
}
