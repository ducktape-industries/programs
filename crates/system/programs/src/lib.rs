//! The `programs` program: the roster of programs a network runs and the changes scheduled to
//! it, by the validators' vote or by the account that holds a name (`Query::Owner`), whose change
//! lands at least [`NOTICE`] blocks after it is made. What the kernel asks of it: the README.
mod examples;
mod program;
mod rules;
#[cfg(test)]
mod tests;

pub use program::Programs;
pub use store::{PageRequest, PageResponse};

use ::program::{AccountNumber, ProgramId, Roles};
use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub use ::program::source::Source;
pub use abi::role::programs::{Entry, Genesis, View};

pub const PROGRAM: &str = "programs";

pub const CODE_KIND: &str = "program";

pub const NOTICE: u64 = 60;

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Pending {
    pub height: u64,
    pub change: Change,
    pub by: Option<AccountNumber>,
}

// Who holds a name, the program's or the view's under it: who, beside the vote, changes it.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Holder {
    Vote,
    Account(AccountNumber),
    // several accounts, any of which acts as the holder
    Accounts(Vec<AccountNumber>),
}

impl Holder {
    pub fn lists(&self, account: AccountNumber) -> bool {
        match self {
            Holder::Vote => false,
            Holder::Account(held) => *held == account,
            Holder::Accounts(held) => held.contains(&account),
        }
    }
}

pub fn role_bound(roles: &Roles, program: &str) -> bool {
    roles.iter().any(|(_, filled)| filled == program)
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Change {
    Set(Entry),
    Remove(ProgramId),
    /// A view with no program behind it, listed under its name.
    SetView(View),
    RemoveView(ProgramId),
    SetHook(ProgramId),
    RemoveHook(ProgramId),
    Publish {
        entry: Entry,
        roles: Vec<String>,
    },
    Rename {
        from: ProgramId,
        to: ProgramId,
    },
}

impl Change {
    /// What the change does, in words: `Set`, `Remove`, `Set view`, `Remove view`.
    pub fn verb(&self) -> &'static str {
        match self {
            Change::Set(_) => "Set",
            Change::Remove(_) => "Remove",
            Change::SetView(_) => "Set view",
            Change::RemoveView(_) => "Remove view",
            Change::SetHook(_) => "Set hook",
            Change::RemoveHook(_) => "Remove hook",
            Change::Publish { .. } => "Publish",
            Change::Rename { .. } => "Rename",
        }
    }

    /// The code a set lands; a removal has none.
    pub fn code(&self) -> Option<abi::BlobId> {
        match self {
            Change::Set(entry) | Change::Publish { entry, .. } => Some(entry.code),
            Change::SetView(view) => Some(view.view),
            Change::Remove(_)
            | Change::RemoveView(_)
            | Change::SetHook(_)
            | Change::RemoveHook(_)
            | Change::Rename { .. } => None,
        }
    }

    pub fn program(&self) -> &str {
        match self {
            Change::Set(entry) | Change::Publish { entry, .. } => &entry.program,
            Change::SetView(view) => &view.name,
            Change::Remove(program)
            | Change::RemoveView(program)
            | Change::SetHook(program)
            | Change::RemoveHook(program) => program,
            Change::Rename { from, .. } => from,
        }
    }

    pub fn lands(&self) -> Option<&str> {
        match self {
            Change::Set(entry) | Change::Publish { entry, .. } => Some(&entry.program),
            Change::SetView(view) => Some(&view.name),
            Change::Rename { to, .. } => Some(to),
            Change::Remove(_)
            | Change::RemoveView(_)
            | Change::SetHook(_)
            | Change::RemoveHook(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Scheduled {
    pub height: u64,
    pub change: Change,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "The roster of programs and views a network runs, and the changes scheduled to it: by the validators' vote, or by the account that holds a name, at least 60 blocks ahead."
)]
pub enum Op {
    #[tool_op(description = "Schedule a change for a later height: Set or Remove a program (a Set's code a held blob of kind program), SetView or RemoveView a view with no program behind it, Rename a program or view to a new owner/name id with its state, account, roles, hook and holder; SetHook, RemoveHook and a Publish that declares roles are the validators' vote's. An account's change lands at least 60 blocks after the block that carries it, only for a name it holds or a free one, which it then holds (a new id is owner/name, and no two ids write the same with / as -); a program that fills a role changes only by the vote.", example = crate::examples::schedule)]
    Schedule(Scheduled),
    #[tool_op(description = "Take back the change scheduled for program at height, before it lands: an account's change by the name's holder, the vote's by the vote.", example = crate::examples::cancel)]
    Cancel { height: u64, program: ProgramId },
    #[tool_op(description = "Hand a name on: to an account, to several accounts any of which acts as its holder, or to the validators' vote. Its holder or the vote.", example = crate::examples::own)]
    Own { program: ProgramId, holder: Holder },
    #[tool_op(description = "Schedule a change as Schedule does, refused unless expected names what is under its name both just before its height and once every change already waiting for that name has landed: a code, or null for nothing. A deploy's rollback code is its expected, so two deploys of one program never both land.", example = crate::examples::schedule_over)]
    ScheduleOver {
        scheduled: Scheduled,
        expected: Option<abi::BlobId>,
    },
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
#[ask(Programs)]
#[borsh(use_discriminant = true)]
#[repr(u8)]
pub enum Query {
    #[ask(Reply::Programs(Vec<Entry>))]
    #[tool_query(description = "The programs that run at block height, each with its code and params: 0 is genesis, not now.", example = crate::examples::at)]
    At(u64),
    // The changes still to land, in height order: those above the query's own height. One at or
    // below it has landed, whether or not an op has folded it since.
    #[ask(Reply::Scheduled(PageResponse<Pending>))]
    #[tool_query(description = "The changes still to land, in height order, each with the account that scheduled it (null for the vote), a page at a time.", example = crate::examples::scheduled)]
    Scheduled { page: PageRequest },
    #[ask(Reply::Program { height: u64, entry: Option<Entry> })]
    #[tool_query(description = "One program that runs now, with its code and params, or null; and the query's height.", example = crate::examples::program)]
    Program(ProgramId),
    #[ask(Reply::Roles(abi::Roles))]
    #[tool_query(description = "Which program fills each role now.", example = crate::examples::roles)]
    Roles = 4,
    #[ask(Reply::Hooks(Vec<ProgramId>))]
    #[tool_query(description = "The hooks at block height.", example = crate::examples::hooks)]
    Hooks(u64),
    /// What runs and what is listed now: the programs and the view-only
    /// entries at the query's own height, by name, and that height. A change
    /// lands at its height whether or not an op has run here since.
    #[ask(Reply::Roster { height: u64, programs: Vec<Entry>, views: Vec<View> })]
    #[tool_query(description = "Every program that runs and every view-only entry now, by name, with the query's height.", example = crate::examples::roster)]
    Roster,
    // What each program that runs now was built from, as its code says, and the query's own
    // height.
    #[ask(Reply::Sources { height: u64, programs: Vec<Built> })]
    #[tool_query(description = "What each program that runs now was built from, as its code's ducktape.source section says: a repository and commit, Unstamped, or why it did not read.", example = crate::examples::sources)]
    Sources,
    // Who holds a name at the query's own height: its record, else the vote for a name that runs,
    // is listed or has a change waiting, else none (free: an account's first set claims it).
    #[ask(Reply::Owner { height: u64, holder: Option<Holder> })]
    #[tool_query(description = "Who holds a name: an account, several accounts or the validators' vote; null while it is free, and then the first account to set it holds it.", example = crate::examples::owner)]
    Owner(ProgramId),
    // The programs that run at block `at_height`, as the kernel's `At` answers: 0 is genesis, not
    // now. `Roster` answers at the query's own height.
    #[ask(Reply::Programs(Vec<Entry>))]
    #[tool_query(description = "The programs that run at block at_height, as the kernel's At answers: 0 is genesis, not now.", example = crate::examples::programs_at)]
    ProgramsAt { at_height: u64 },
    // The view-only entries listed at block `at_height`, by name.
    #[ask(Reply::Views(Vec<View>))]
    #[tool_query(description = "The view-only entries listed at block at_height, by name.", example = crate::examples::views_at)]
    ViewsAt { at_height: u64 },
    // The hooks at block `at_height`, as the kernel's `Hooks` answers.
    #[ask(Reply::Hooks(Vec<ProgramId>))]
    #[tool_query(description = "The hooks at block at_height, as the kernel's Hooks answers.", example = crate::examples::hooks_at)]
    HooksAt { at_height: u64 },
    #[ask(Reply::Renames(Vec<(ProgramId, ProgramId)>))]
    #[tool_query(description = "The renames that hold at block height: each (from, to) landed by then whose new id the roster lists there and whose old id it does not. The kernel renames by it, and each program's Migrate rewrites its stored ids by it.", example = crate::examples::renames)]
    Renames(u64) = 12,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Programs(Vec<Entry>),
    Scheduled(PageResponse<Pending>),
    Program {
        height: u64,
        entry: Option<Entry>,
    },
    Views(Vec<View>),
    Roles(abi::Roles),
    Hooks(Vec<ProgramId>),
    Roster {
        height: u64,
        programs: Vec<Entry>,
        views: Vec<View>,
    },
    Sources {
        height: u64,
        programs: Vec<Built>,
    },
    Owner {
        height: u64,
        holder: Option<Holder>,
    },
    Renames(Vec<(ProgramId, ProgramId)>),
}

// A program that runs, its code, and what that code says it was built from.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Built {
    pub program: ProgramId,
    pub code: abi::BlobId,
    pub source: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Provenance {
    // the repository and commit the code's `ducktape.source` section names
    Commit(Source),
    // code with no such section: packed before packers stamped one, or from a checkout with
    // changes git sees
    Unstamped,
    // why the code was not read: its blob is not held here, or its section does not decode
    Unread(String),
}

/// An op as a person reads it: a title and its fields. The source of the
/// `ducktape.describe` module this program ships (`make wasm-describes`).
fn scheduled_fields(scheduled: &Scheduled) -> (String, Vec<describe::Field>) {
    use describe::{Value, field};
    let Scheduled { height, change } = scheduled;
    let mut fields = vec![
        field("change", Value::text(change.verb())),
        field("program", Value::Program(change.program().into())),
        field("height", Value::Text(height.to_string())),
    ];
    fields.extend(
        change
            .code()
            .map(|code| field("code", Value::Hash(code.digest().to_vec()))),
    );
    match change {
        Change::Set(entry) => fields.push(field("params", Value::bytes(&entry.params))),
        Change::Publish { entry, roles } => {
            fields.push(field("params", Value::bytes(&entry.params)));
            fields.push(field(
                "roles",
                Value::List(roles.iter().map(Value::text).collect()),
            ));
        }
        Change::Rename { to, .. } => fields.push(field("to", Value::Program(to.clone()))),
        Change::Remove(_)
        | Change::SetView(_)
        | Change::RemoveView(_)
        | Change::SetHook(_)
        | Change::RemoveHook(_) => {}
    }
    (format!("Schedule · {}", change.program()), fields)
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let height = |height: &u64| field("height", Value::Text(height.to_string()));
    let (title, fields) = match op {
        Op::Schedule(scheduled) => scheduled_fields(scheduled),
        Op::ScheduleOver {
            scheduled,
            expected,
        } => {
            let (title, mut fields) = scheduled_fields(scheduled);
            let expected = match expected {
                Some(code) => Value::Hash(code.digest().to_vec()),
                None => Value::text("nothing"),
            };
            fields.push(field("expected", expected));
            (title, fields)
        }
        Op::Cancel {
            height: at,
            program,
        } => (
            format!("Cancel · {program}"),
            vec![
                field("program", Value::Program(program.clone())),
                height(at),
            ],
        ),
        Op::Own { program, holder } => {
            let holder = match holder {
                Holder::Vote => Value::text("the validators' vote"),
                Holder::Account(account) => Value::Account(*account),
                Holder::Accounts(accounts) => {
                    Value::List(accounts.iter().copied().map(Value::Account).collect())
                }
            };
            (
                format!("Own · {program}"),
                vec![
                    field("program", Value::Program(program.clone())),
                    field("holder", holder),
                ],
            )
        }
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(
        describe::variants::<Op>(),
        ["Schedule", "Cancel", "Own", "ScheduleOver"]
    );
}

#[test]
fn change_variants_only_append() {
    assert_eq!(
        describe::variants::<Change>(),
        [
            "Set",
            "Remove",
            "SetView",
            "RemoveView",
            "SetHook",
            "RemoveHook",
            "Publish",
            "Rename"
        ]
    );
}
