//! A policy, a party and a default's scope, in the words a person reads.
use std::collections::BTreeMap;

use ownership::{Party, Scope, Writers};

use crate::kind;

// Who may write the items on a policy with `write`, beside its `owner`: "Anyone", "Only you",
// "You, Ada and board" to the owner, and to a `reader` who is someone else (or nobody seated)
// the same with the owner named: "Only Maya".
pub fn writers(
    write: &Writers,
    owner: &Party,
    reader: Option<&Party>,
    names: &BTreeMap<u64, String>,
) -> String {
    let theirs = reader == Some(owner);
    let listed: Vec<String> = match write {
        Writers::Anyone => return "Anyone".to_owned(),
        Writers::Owner => Vec::new(),
        Writers::Listed(parties) => parties
            .iter()
            .filter(|listed| *listed != owner)
            .map(|listed| party(listed, names))
            .collect(),
    };
    let named = party(owner, names);
    let (first, alone) = match theirs {
        true => ("You", "you"),
        false => (named.as_str(), named.as_str()),
    };
    match listed.split_last() {
        None => format!("Only {alone}"),
        Some((last, [])) => format!("{first} and {last}"),
        Some((last, rest)) => format!("{first}, {} and {last}", rest.join(", ")),
    }
}

// An account by its name (by its number where `names` lacks it), a program by its id.
pub fn party(party: &Party, names: &BTreeMap<u64, String>) -> String {
    match party {
        Party::Account(number) => names
            .get(number)
            .cloned()
            .unwrap_or_else(|| format!("account {number}")),
        Party::Program(program) => program.clone(),
    }
}

// Where a default applies: "Everything", "Everything in chat", "Chat messages".
pub fn scope(scope: &Scope) -> String {
    match scope {
        Scope::Global => "Everything".to_owned(),
        Scope::Program(program) => format!("Everything in {program}"),
        Scope::Kind(name) => kind(name).map_or_else(|| name.clone(), |kind| kind.label.to_owned()),
    }
}
