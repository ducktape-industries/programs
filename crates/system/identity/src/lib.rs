//! The `identity` program: accounts, and who acts as each. Everything that
//! acts is an account: a user (the keys they hold), a program account (the
//! keys it or its manager adds, and the script that runs its actions) and
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
mod examples;
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
/// signature over the [`Admission`].
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

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Accounts and who acts as each: users and the keys they hold, program accounts (agents) with their manager, keys and script, and each program's own system account."
)]
pub enum Op {
    /// The identity role's op (`abi::role::identity::Op`), first: the
    /// system gives an admitted program its account.
    #[tool_op(description = "The system alone (the identity role): gives an admitted program its account. A program that has one keeps it.", example = examples::register)]
    RegisterProgram { program: ProgramId },
    /// A user's account, holding the signing key.
    #[tool_op(description = "Create a user's account for the signing key, which must hold no account yet; it becomes the account's first key. Returns the new account's number.", returns = AccountNumber, example = examples::create)]
    Create { name: String, scheme: Scheme },
    #[tool_op(description = "Create a program account of the Agent category that you manage, with no key and no script yet. Any account that acts may. Returns its number.", returns = AccountNumber, example = examples::create_agent)]
    CreateAgent { name: String },
    #[tool_op(description = "Add the signing key to consent.account: a key already on it signs the consent (Admission under ducktape:identity:consent). Or a program account's manager signs and the new key consents to itself. Never a system account's or a revoked one's. For an account that manages itself, only the validators' vote.", example = examples::add_key)]
    AddKey {
        scheme: Scheme,
        label: Option<String>,
        consent: Consent,
    },
    #[tool_op(description = "Remove a key: an account's own key removes itself or a junior one, never the last; a program account's manager removes any of its keys.", example = examples::remove_key)]
    RemoveKey {
        account: AccountNumber,
        key: Vec<u8>,
    },
    #[tool_op(description = "Rename an account. Whoever may write its key in the access program: a user or a system account itself, a program account's manager, and whom they list; not a revoked account.", example = examples::set_name)]
    SetName {
        account: AccountNumber,
        name: String,
    },
    #[tool_op(description = "Set an account's avatar (a blob) and bio. Whoever may rename it.", example = examples::set_profile)]
    SetProfile {
        account: AccountNumber,
        avatar: Option<BlobId>,
        bio: Option<String>,
    },
    /// A program account stops acting; its keys stay for `Resume`. Its
    /// manager's.
    #[tool_op(description = "Stop a program account acting; its keys stay for Resume. Its manager, or whom the manager lists. For an account that manages itself, only the validators' vote.", example = examples::suspend)]
    Suspend { account: AccountNumber },
    #[tool_op(description = "Let a suspended program account act again. Its manager, or whom the manager lists. For an account that manages itself, only the validators' vote.", example = examples::resume)]
    Resume { account: AccountNumber },
    /// A program account stops for good: its keys are dropped. Its
    /// manager's.
    #[tool_op(description = "Stop a program account for good: its keys are dropped. Its manager, or whom the manager lists. For an account that manages itself, only the validators' vote.", example = examples::revoke)]
    Revoke { account: AccountNumber },
    #[tool_op(description = "Set whom a program account answers: Anyone (the default), Owner (its manager only) or Listed (its manager and these accounts or programs). Whoever may write the account's key, not once revoked.", example = examples::set_ownership)]
    SetOwnership {
        account: AccountNumber,
        ownership: Ownership,
    },
    #[tool_op(description = "Name the program that runs a program account's actions (calls) and the config it reads (harness: the agent's config file in files), or null to take it back. Whoever may write the account's key, not once revoked.", example = examples::set_script)]
    SetScript {
        account: AccountNumber,
        script: Option<Script>,
    },
    #[tool_op(description = "The system alone (the identity role), as the kernel renames a program in place: the program's account moves from its old id to its new one, and so does its card's name while that is still the old id. A move already made answers ok.", example = examples::rename_program)]
    RenameProgram { from: ProgramId, to: ProgramId },
    #[tool_op(description = "Rewrite the program ids identity stores by the programs role's Renames at height: each script that calls a renamed program, and each renamed program an account answers, over the accounts first..=last (null: from the first, or to the last). Anyone: it reads the renames from the programs role, never from its sender, so running it again changes nothing.", example = examples::migrate)]
    Migrate {
        height: u64,
        first: Option<AccountNumber>,
        last: Option<AccountNumber>,
    },
    #[tool_op(description = "Move a program account to a new manager at once, and clear the list on its key in the access program. Its manager sends it; for an account that manages itself, only the validators' vote. Naming the account itself makes it manage itself: it then runs its own name, profile, script and whom it answers, and only the validators' vote suspends, resumes, revokes, adds a key to or moves it. Not a user's, a system account's or a revoked one's, and never to a revoked account.", example = examples::set_manager)]
    SetManager {
        account: AccountNumber,
        manager: AccountNumber,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reference {
    Account(AccountNumber),
    Key(Vec<u8>),
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(Identity)]
pub enum Query {
    /// The identity role's queries (`abi::role::identity::Query`), first
    /// and in its order: the account a key acts as (refused while it does
    /// not act), a program's account, one account's profile and every
    /// account's.
    #[ask(Reply::Number(Option<AccountNumber>))]
    #[tool_query(description = "The account a key acts as, or null; refused while that account does not act.", example = examples::of_key)]
    OfKey { key: Vec<u8> },
    #[ask(Reply::Number(Option<AccountNumber>))]
    #[tool_query(description = "A program's own account number, or null.", example = examples::of_program)]
    OfProgram { program: ProgramId },
    #[ask(Reply::Profile(Option<Profile>))]
    #[tool_query(description = "One account's number, name and kind (user, program account with its manager and standing, or system), or null.", example = examples::profile)]
    Profile { number: AccountNumber },
    #[ask(Reply::Profiles { profiles: Vec<Profile>, next: Option<AccountNumber> })]
    #[tool_query(description = "Every account's profile in number order, after a number, at most limit of them; next is where to go on.", example = examples::profiles)]
    Profiles {
        after: Option<AccountNumber>,
        limit: u32,
    },
    #[ask(Reply::Number(Option<AccountNumber>))]
    #[tool_query(description = "The account if program runs it (its script calls program), else null.", example = examples::runs)]
    Runs {
        program: ProgramId,
        account: AccountNumber,
    },
    #[ask(Reply::Account(Option<Account>))]
    #[tool_query(description = "One account's whole record: its card and who acts as it, keys included, or null.", example = examples::get)]
    Get { number: AccountNumber },
    #[ask(Reply::Generation(u64))]
    #[tool_query(description = "A key's generation: what a consent to it signs, so an old consent can't be replayed.", example = examples::generation)]
    Generation { key: Vec<u8> },
    #[ask(Reply::Resolved(Vec<Option<AccountNumber>>))]
    #[tool_query(description = "The account each reference (a number or a key) names, in order, null where none.", example = examples::resolve)]
    Resolve { references: Vec<Reference> },
    #[ask(Reply::Accounts(PageResponse<Account>))]
    #[tool_query(description = "Every account's whole record, a page at a time.", example = examples::list)]
    List { page: PageRequest },
    #[ask(Reply::Accounts(PageResponse<Account>))]
    #[tool_query(description = "The program accounts an account manages, a page at a time.", example = examples::managed)]
    Managed {
        by: AccountNumber,
        page: PageRequest,
    },
    #[ask(Reply::Answers(Option<Ownership>))]
    #[tool_query(description = "Whom a program account answers: Anyone until it or its manager narrows it with SetOwnership; null for an account that is no program account.", example = examples::answers)]
    Answers { account: AccountNumber },
    #[ask(Reply::Hears(bool))]
    #[tool_query(description = "Whether a program account takes a message from account from: its manager always, anyone else as Answers says, and a program's own account as that program too. The one rule every runtime that delivers to an agent asks.", example = examples::hears)]
    Hears {
        account: AccountNumber,
        from: AccountNumber,
    },
    #[ask(Reply::Narrowed(PageResponse<(AccountNumber, Ownership)>))]
    #[tool_query(description = "Every program account that narrowed whom it answers, with what Answers says for it, in number order a page at a time; an account not listed answers Anyone.", example = examples::narrowed)]
    Narrowed { page: PageRequest },
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
    Answers(Option<Ownership>),
    Hears(bool),
    Narrowed(PageResponse<(AccountNumber, Ownership)>),
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
        Op::RenameProgram { from, to } => (
            format!("Rename program · {from}"),
            vec![
                field("program", Value::Program(from.clone())),
                field("to", Value::Program(to.clone())),
            ],
        ),
        Op::Migrate {
            height,
            first,
            last,
        } => {
            let bound = |bound: &Option<AccountNumber>, open: &str| {
                bound.map_or_else(|| Value::text(open), Value::Account)
            };
            (
                "Migrate".into(),
                vec![
                    field("height", Value::Text(height.to_string())),
                    field("first", bound(first, "the first")),
                    field("last", bound(last, "the last")),
                ],
            )
        }
        Op::SetManager {
            account: number,
            manager,
        } => (
            "Set manager".into(),
            vec![account(number), field("manager", Value::Account(*manager))],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

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
            "RenameProgram",
            "Migrate",
            "SetManager",
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
            "Answers",
            "Hears",
            "Narrowed",
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
            "Answers",
            "Hears",
            "Narrowed",
        ]
    );
}
