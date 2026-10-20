//! The explorer's sentences, once: every sentence both designs show (the phone's are Expo's,
//! `src/network/words.ts`, mobile dev 266633c). A form's words live in `acting.rs`, and why a
//! program page lists no capability in `state.rs` (`Unlisted::sentence`).
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

pub(crate) const SEARCH: &str = "Search the chain";
pub(crate) const SEARCH_HINT: &str = "Search by height, hash, account or program";

pub(crate) const NO_VALUE: &str = "—";
pub(crate) const NO_COUNT: &str = "–";
pub(crate) const VIEW_ONLY: &str = "view only";
pub(crate) const NO_PARAMS: &str = "none";

pub(crate) fn built_from(source: &::programs::Provenance) -> String {
    use ::programs::Provenance;
    match source {
        Provenance::Commit(source) => {
            let commit = source.commit.get(..7).unwrap_or(&source.commit);
            format!("{}@{commit}", source.repo)
        }
        Provenance::Unstamped => "its code names no commit".into(),
        Provenance::Unread(why) => format!("not read: {why}"),
    }
}

pub(crate) const ALL_BLOCKS: &str = "All blocks →";
pub(crate) const ALL_TRANSACTIONS: &str = "All transactions →";

pub(crate) const NO_BLOCK_HERE: &str = "This node keeps no finalized block at this height.";
pub(crate) const NO_BLOCK_TRANSACTIONS: &str = "No transactions in this block.";
pub(crate) const NO_TRANSACTION: &str = "Transaction not found";
pub(crate) const NO_ACCOUNT_HERE: &str = "Identity holds no account by this number.";
pub(crate) const NO_ACTIVITY: &str = "No activity recorded.";
/// The press under an account's newest activity that reads the page past it.
pub(crate) const SHOW_OLDER: &str = "Show older";
pub(crate) const NEWER: &str = "Newer transactions";
pub(crate) const SHOW_NEWEST: &str = "Show newest";
// Under the transactions while the window does not reach the chain's start: where older ones are.
pub(crate) const OLDER_TRANSACTIONS: &str = "Older transactions: open a block or search a hash.";
pub(crate) const NO_PROGRAM_HERE: &str = "The programs role runs and lists nothing by this name.";
pub(crate) const RUNS_NOTHING: &str = "The programs role of this network runs nothing yet.";
pub(crate) const NOTHING_SCHEDULED: &str = "Nothing is scheduled against the programs role.";
pub(crate) const NOTHING_SCHEDULED_FOR_IT: &str = "Nothing is scheduled for it.";
pub(crate) const OPEN_TO_ACCOUNTS: &str = "Any account changes a program that fills no role. Only the validators' vote changes a role's program.";
pub(crate) const VALIDATORS_VOTE: &str = "validators' vote";
pub(crate) const ANY_ACCOUNT: &str = "any account";
pub(crate) const BY_VOTE: &str = "by vote";
pub(crate) const ADD: &str = "Add";
pub(crate) const CODE_ID: &str = "the code id";
pub(crate) const LINK: &str = "the link";

pub(crate) fn entry_kind(listed: bool) -> &'static str {
    if listed { VIEW_ONLY } else { "program" }
}

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

pub(crate) fn block(height: u64) -> String {
    format!("Block {}", grouped(height))
}

pub(crate) fn empty_run(oldest: u64, newest: u64) -> String {
    format!(
        "{}–{} · {} empty blocks",
        grouped(oldest),
        grouped(newest),
        grouped(newest - oldest + 1)
    )
}

pub(crate) fn tx_count(transactions: u64) -> String {
    format!("{} tx", grouped(transactions))
}

pub(crate) fn age(now: u64, time: u64) -> String {
    format!("{} ago", ago(now, time))
}

pub(crate) fn min_width(pixels: u32) -> String {
    format!("{pixels} px")
}

pub(crate) fn step_back(previous: Option<u64>) -> String {
    format!("← {}", previous.map_or(String::new(), grouped))
}

pub(crate) fn step_on(next: u64) -> String {
    format!("{} →", grouped(next))
}

pub(crate) fn recorded(transactions: u64) -> String {
    format!(
        "{} recorded",
        plural(transactions, "transaction", "transactions")
    )
}

pub(crate) fn tx_recorded(accounts: u64) -> String {
    format!("{} · tx recorded", grouped(accounts))
}

pub(crate) fn account_wait(minutes: Option<u64>) -> String {
    match minutes {
        Some(minutes) => {
            format!("changes by an account wait {NOTICE} blocks (about {minutes} min)")
        }
        None => format!("changes by an account wait {NOTICE} blocks"),
    }
}

pub(crate) fn changed_by(fills_a_role: bool) -> String {
    match fills_a_role {
        true => first_up(VALIDATORS_VOTE),
        false => first_up(ANY_ACCOUNT),
    }
}

fn first_up(words: &str) -> String {
    let mut letters = words.chars();
    letters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(letters).collect()
    })
}

pub(crate) fn code_words(code: &abi::BlobId) -> String {
    format!("{}:{}", blob_kind(code), short(code.digest()))
}

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

pub(crate) fn nothing_called(name: &str) -> String {
    format!("Nothing called {name}")
}

pub(crate) fn in_block(height: u64) -> String {
    format!("In block {}", grouped(height))
}

pub(crate) fn device(index: usize) -> String {
    format!("Device {}", index + 1)
}

pub(crate) fn transactions_to(program: &str) -> String {
    format!("Transactions · {program}")
}

pub(crate) fn block_every(block_time_ms: u64) -> String {
    format!("Block every {:.1} s", block_time_ms as f64 / 1000.)
}

pub(crate) fn next_in(blocks: u64) -> String {
    format!("Next in {}", plural(blocks, "block", "blocks"))
}

pub(crate) fn last_blocks(blocks: u64) -> String {
    format!("the last {}", plural(blocks, "block", "blocks"))
}

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

pub(crate) fn to_transactions(sent: u64, blocks: u64) -> String {
    match blocks {
        0 => "Transactions →".into(),
        _ => format!("{} →", transactions_in(sent, blocks)),
    }
}

pub(crate) fn validator(place: Option<usize>) -> String {
    match place {
        Some(place) => format!("validator {}", place + 1),
        None => "validator".into(),
    }
}

pub(crate) fn validator_key(key: &[u8]) -> String {
    format!("ed25519 {}", short(key))
}

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
        Capability::Artifact => "shows pages kept on the network, which the app draws",
    }
}
