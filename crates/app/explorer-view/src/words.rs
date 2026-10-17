//! The explorer's sentences, once: every sentence, problem and capability
//! line the desktop's pages and the phone's screens both show (the phone's
//! are Expo's: `src/network/words.ts` and the explorer screens, mobile dev
//! 266633c). Neither design types a sentence the other also types; a
//! heading or a button's label stays where it is drawn. Two homes sit
//! beside this one, each with the model it words: a form's titles, fields,
//! notes, problems and press words in `acting.rs`, and why a program page
//! lists no capability in `state.rs` (`Unlisted::sentence`).
use ::programs::{Change, NOTICE, Pending};
use view::methods::{Capability, Manifested};

use crate::decode::{ago, grouped, plural, scheme, short, size};
use crate::state::blob_kind;
use crate::{Network, Note};

pub(crate) const READING_CHAIN: &str = "Reading the chain…";
pub(crate) const READING_BLOCK: &str = "Reading the block…";
pub(crate) const READING_TX: &str = "Reading the transaction…";
pub(crate) const READING_ACCOUNTS: &str = "Reading accounts…";
pub(crate) const READING_ACCOUNT: &str = "Reading the account…";
pub(crate) const READING_ACTIVITY: &str = "Reading its activity…";
pub(crate) const READING_ROLE: &str = "Reading the programs role…";
pub(crate) const READING_CODE: &str = "Reading the code…";

/// The search field's name, and what it takes.
pub(crate) const SEARCH: &str = "Search the chain";
pub(crate) const SEARCH_HINT: &str = "Search by height, hash, account or program";

/// A figure not read yet.
pub(crate) const NO_VALUE: &str = "—";
/// A count not read yet.
pub(crate) const NO_COUNT: &str = "–";
/// A view-only entry: a view with no program behind it.
pub(crate) const VIEW_ONLY: &str = "view only";
/// What marks an agent among maintainers.
pub(crate) const AGENT: &str = "agent";
/// A program that runs with no params.
pub(crate) const NO_PARAMS: &str = "none";

pub(crate) const ALL_BLOCKS: &str = "All blocks →";
pub(crate) const ALL_TRANSACTIONS: &str = "All transactions →";

pub(crate) const NO_BLOCK_HERE: &str = "This node keeps no finalized block at this height.";
pub(crate) const NO_BLOCK_TRANSACTIONS: &str = "No transactions in this block.";
pub(crate) const NO_TRANSACTION: &str = "Transaction not found";
pub(crate) const NO_ACCOUNT_HERE: &str = "Identity holds no account by this number.";
pub(crate) const NO_ACTIVITY: &str = "No activity recorded.";
/// The line a list of transactions ends with while activity goes on past
/// the rows read: it reads the next page as it comes into view.
pub(crate) const READING_OLDER: &str = "Reading older transactions…";
/// The press under an account's newest activity that reads the page past it.
pub(crate) const SHOW_OLDER: &str = "Show older";
/// Over a list read on past its first page once rows were written above
/// it, and the press that reads it anew from them.
pub(crate) const NEWER: &str = "Newer transactions";
pub(crate) const SHOW_NEWEST: &str = "Show newest";
/// Under a program's transactions once they read back as far as they read
/// by themselves: the press that reads further.
pub(crate) const LOOK_FURTHER: &str = "Look further back";
pub(crate) const NO_PROGRAM_HERE: &str = "The programs role runs and lists nothing by this name.";
pub(crate) const RUNS_NOTHING: &str = "The programs role of this network runs nothing yet.";
pub(crate) const NOTHING_SCHEDULED: &str = "Nothing is scheduled against the programs role.";
pub(crate) const NOTHING_SCHEDULED_FOR_IT: &str = "Nothing is scheduled for it.";
pub(crate) const NO_MAINTAINER: &str =
    "No account maintains a program: only the validators' vote changes the programs role.";
/// Who changes a role's program, and a program no account maintains.
pub(crate) const VALIDATORS_VOTE: &str = "validators' vote";
pub(crate) const VOTE_ONLY: &str = "no account: the validators' vote only";
/// The accounts whose grant covers every program, named together.
pub(crate) const EVERY_PROGRAM: &str = "every-program maintainers";
/// A change the validators' vote makes.
pub(crate) const BY_VOTE: &str = "by vote";
/// The Programs page's control and sheet of what the reader may add: a
/// program by grant, a grant by vote.
pub(crate) const ADD: &str = "Add";
/// What a Copy put on the clipboard, as its note names it.
pub(crate) const CODE_ID: &str = "the code id";
pub(crate) const LINK: &str = "the link";
/// A grant form before its account is typed.
pub(crate) const TYPE_ACCOUNT: &str = "Type the account's number.";

/// What a programs role entry is: a program, or a view with none behind it.
pub(crate) fn entry_kind(listed: bool) -> &'static str {
    if listed { VIEW_ONLY } else { "program" }
}

/// How big the code is, and for a program whose code carries a view
/// (`listed`: a view-only entry, which is all view) how much of it the
/// view: `2.3 MB of code, 1.8 MB of it the view`.
pub(crate) fn code_size(blob: &Manifested, listed: bool) -> String {
    match &blob.view {
        Some(carried) if !listed => format!(
            "{} of code, {} of it the view",
            size(blob.size),
            size(carried.size)
        ),
        _ => size(blob.size),
    }
}

/// A block by its height: `Block 8,342`.
pub(crate) fn block(height: u64) -> String {
    format!("Block {}", grouped(height))
}

/// A run of empty blocks folded into one line: `8,343–8,400 · 58 empty
/// blocks`.
pub(crate) fn empty_run(oldest: u64, newest: u64) -> String {
    format!(
        "{}–{} · {} empty blocks",
        grouped(oldest),
        grouped(newest),
        grouped(newest - oldest + 1)
    )
}

/// A count of transactions, short: `12 tx`.
pub(crate) fn tx_count(transactions: u64) -> String {
    format!("{} tx", grouped(transactions))
}

/// How long ago a block or a transaction was: `3m ago`.
pub(crate) fn age(now: u64, time: u64) -> String {
    format!("{} ago", ago(now, time))
}

/// The narrowest window a view's manifest asks for: `480 px`.
pub(crate) fn min_width(pixels: u32) -> String {
    format!("{pixels} px")
}

/// The step to the block before: `← 8,341`; none before, the arrow alone.
pub(crate) fn step_back(previous: Option<u64>) -> String {
    format!("← {}", previous.map_or(String::new(), grouped))
}

/// The step to the block after: `8,343 →`.
pub(crate) fn step_on(next: u64) -> String {
    format!("{} →", grouped(next))
}

/// An account's transactions activity recorded: `12 transactions
/// recorded`.
pub(crate) fn recorded(transactions: u64) -> String {
    format!(
        "{} recorded",
        plural(transactions, "transaction", "transactions")
    )
}

/// The accounts list's count: `15 · tx recorded`.
pub(crate) fn tx_recorded(accounts: u64) -> String {
    format!("{} · tx recorded", grouped(accounts))
}

/// How long a maintainer's change waits, in blocks and, at a known block
/// time, about how many minutes.
pub(crate) fn maintainer_wait(minutes: Option<u64>) -> String {
    match minutes {
        Some(minutes) => {
            format!("changes by a maintainer wait {NOTICE} blocks (about {minutes} min)")
        }
        None => format!("changes by a maintainer wait {NOTICE} blocks"),
    }
}

/// A grant on a name that runs nothing yet: who may add it.
pub(crate) fn may_add(name: &str) -> String {
    format!("runs nothing yet: {name} may add it")
}

/// The reader, marked so among accounts: `Ada (you)`.
pub(crate) fn you(name: &str) -> String {
    format!("{name} (you)")
}

/// A maintainer by its name, the reader and an agent marked so (Expo
/// `holderName`, and `· agent` after it): `Ada (you) · agent`.
pub(crate) fn holder(name: &str, reader: bool, agent: bool) -> String {
    let name = if reader { you(name) } else { name.to_owned() };
    if agent {
        format!("{name} · {AGENT}")
    } else {
        name
    }
}

/// Who changes a name, as its row says (Expo `changedBy`): its
/// maintainers, else the validators' vote.
pub(crate) fn changed_by(maintainers: &[String]) -> String {
    if maintainers.is_empty() {
        first_up(VALIDATORS_VOTE)
    } else {
        format!("Maintainer {}", maintainers.join(", "))
    }
}

/// `words` with its first letter a capital, to open a line.
fn first_up(words: &str) -> String {
    let mut letters = words.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

/// A code id as the screens show it (Expo `words.ts` `codeWords`):
/// `sha256:1a2b3c4d…9f0e`.
pub(crate) fn code_words(code: &abi::BlobId) -> String {
    format!("{}:{}", blob_kind(code), short(code.digest()))
}

/// A scheduled change (Expo `words.ts` `scheduledWords`): a set against
/// what runs now, anything else by its verb.
pub(crate) fn scheduled_words(pending: &Pending, network: &Network) -> String {
    let at = grouped(pending.height);
    let Change::Set(entry) = &pending.change else {
        return format!(
            "{} {} at block {at}",
            pending.change.verb(),
            pending.change.program()
        );
    };
    let now = network
        .programs
        .iter()
        .find(|running| running.program == entry.program);
    let params = match now {
        None => "a new program",
        Some(now) if now.params == entry.params => "params unchanged",
        Some(_) => "params changed",
    };
    format!(
        "Set {} to code {} at block {at} · {params}",
        entry.program,
        code_words(&entry.code)
    )
}

pub(crate) fn no_block(height: u64) -> String {
    format!("No block {}", grouped(height))
}

pub(crate) fn no_transaction_at(height: u64, position: u32) -> String {
    format!(
        "This node keeps no transaction at position {position} of block {}.",
        grouped(height)
    )
}

pub(crate) fn no_account(number: u64) -> String {
    format!("No account #{number}")
}

/// A grant form's account, typed but held by no one.
pub(crate) fn no_account_numbered(number: u64) -> String {
    format!("No account {number} here.")
}

pub(crate) fn nothing_called(name: &str) -> String {
    format!("Nothing called {name}")
}

/// The link from a transaction to the block it is in.
pub(crate) fn in_block(height: u64) -> String {
    format!("In block {}", grouped(height))
}

/// A device's name where its key carries no label: `Device 2`.
pub(crate) fn device(index: usize) -> String {
    format!("Device {}", index + 1)
}

/// How far back a program's transactions were looked for, while activity
/// goes on past the rows read: `3 of the newest 150 transactions`, `None
/// in the newest 1,000 transactions`.
pub(crate) fn of_the_newest(matching: u64, read: u64) -> String {
    let read = plural(read, "transaction", "transactions");
    match matching {
        0 => format!("None in the newest {read}"),
        _ => format!("{} of the newest {read}", grouped(matching)),
    }
}

pub(crate) fn transactions_to(program: &str) -> String {
    format!("Transactions · {program}")
}

/// The node's block time: `Block every 1.0 s`.
pub(crate) fn block_every(block_time_ms: u64) -> String {
    format!("Block every {:.1} s", block_time_ms as f64 / 1000.)
}

/// How far the next epoch is: `Next in 7 blocks`.
pub(crate) fn next_in(blocks: u64) -> String {
    format!("Next in {}", plural(blocks, "block", "blocks"))
}

/// How far back the window reaches: `the last 1,000 blocks`.
pub(crate) fn last_blocks(blocks: u64) -> String {
    format!("the last {}", plural(blocks, "block", "blocks"))
}

/// The window's transactions: `2 transactions in the last 13 blocks`.
pub(crate) fn transactions_in(count: u64, blocks: u64) -> String {
    format!(
        "{} in {}",
        plural(count, "transaction", "transactions"),
        last_blocks(blocks)
    )
}

pub(crate) fn no_transactions_in(blocks: u64) -> String {
    format!("No transactions in {}.", last_blocks(blocks))
}

/// A program's way to its transactions, `sent` of them in the window's
/// `blocks`; with no block yet, no count of nothing.
pub(crate) fn to_transactions(sent: u64, blocks: u64) -> String {
    match blocks {
        0 => "Transactions →".into(),
        _ => format!("{} →", transactions_in(sent, blocks)),
    }
}

/// A block's proposer by its place in the validator set, `validator 1`;
/// a key the set no longer holds, `validator`.
pub(crate) fn validator(place: Option<usize>) -> String {
    match place {
        Some(place) => format!("validator {}", place + 1),
        None => "validator".into(),
    }
}

/// A validator's key: validators sign with ed25519.
pub(crate) fn validator_key(key: &[u8]) -> String {
    format!("ed25519 {}", short(key))
}

/// Which of `account`'s keys signed, its `at`th: `#8 device 1 · ed25519
/// 623b55d3…efd0`; a key with no label named as a device is, inside the
/// line (Expo `tx/[hash].tsx:166`).
pub(crate) fn key_line(account: &identity::Account, at: usize, signer: &[u8]) -> String {
    let key = &account.keys()[at];
    let label = key
        .label
        .clone()
        .unwrap_or_else(|| device(at).to_lowercase());
    format!(
        "#{} {label} · {} {}",
        account.number,
        scheme(key.scheme),
        short(signer)
    )
}

/// When a device last signed, by the time of its newest frame.
pub(crate) fn last_used(now: u64, last: Option<u64>) -> String {
    match last {
        Some(time) => format!("last used {} ago", ago(now, time)),
        None => "not used lately".into(),
    }
}

pub(crate) fn unindexed(from: u64) -> String {
    format!(
        "History before block {} is not indexed on this node.",
        grouped(from)
    )
}

/// The line under the page strip, in words.
pub(crate) fn note(note: &Note) -> String {
    match note {
        Note::NotFound(query) => format!("Nothing here is called “{query}”."),
        Note::NoSuchHash { listed_from, .. } => {
            let since = listed_from.filter(|from| *from > 0);
            match since {
                Some(from) => format!(
                    "No block or transaction since block {} has this hash. {}",
                    grouped(from),
                    unindexed(from)
                ),
                None => "No block or transaction has this hash.".into(),
            }
        }
        Note::Unlinked(route) => format!("This link names nothing the Explorer shows: {route}"),
        Note::Copied(what) => format!("Copied {what}."),
        Note::Sent(words) | Note::Refused(words) => words.clone(),
    }
}

/// What a capability lets a view do, in a line a person reads: the phone's
/// "uses camera" for a view. Every capability is worded here, so a new one
/// is worded before it ships.
pub(crate) fn allows(capability: Capability) -> &'static str {
    match capability {
        Capability::Chain => "reads the chain: blocks, transactions and the node's status",
        Capability::Program => "asks programs questions and follows what they change",
        Capability::Op => "sends operations to the chain in your name",
        Capability::Invite => "mints invites with the node's key",
        Capability::Link => "opens links: another view, or the browser",
        Capability::Blob => "reads blobs the node holds",
        Capability::Host => "knows the session: who you are, which chain, when it is on screen",
        Capability::Clock => "wakes on a timer",
        Capability::Clipboard => "reads and writes the clipboard",
        Capability::Notify => "posts notifications",
        Capability::Store => "keeps its own data on this device",
        Capability::Web => "shows web pages, and takes the web links the app opens",
        Capability::Live => {
            "sends and hears live streams in its program's rooms, as the program grants"
        }
        Capability::Camera => {
            "shows your camera's picture: the app draws it, the view never sees it"
        }
        Capability::Microphone => {
            "uses your microphone and speakers: the app records and plays, the view never hears it"
        }
        Capability::File => {
            "lets you pick files on this device: it sees their names and sizes, never where they are"
        }
    }
}
