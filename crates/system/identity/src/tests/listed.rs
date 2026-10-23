// view::listed: a revoked program account is the one account no list shows, and every view that
// reads a list of accounts filters it through listed (J-381).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::{Category, Kind, Standing, view};

const READS: [&str; 4] = [
    "identity::ask::List {",
    "identity::ask::Managed {",
    "ask::Accounts {",
    "harness::ask::Cards {",
];

const FILTERS: [&str; 2] = ["view::listed(", "view::standing_listed("];

const LOOKUPS: [(&str, &str); 3] = [
    (
        "app/files-view/src/queries.rs",
        "names the owner a file names",
    ),
    (
        "app/node-view/src/queries.rs",
        "names the operator a validator names",
    ),
    (
        "app/harness-view/src/read.rs",
        "the agent view's groups fold revoked agents (ui/mod.rs)",
    ),
];

fn program(standing: Standing) -> Kind {
    Kind::Program {
        manager: 1,
        category: Category::Agent,
        standing,
        script: None,
    }
}

#[test]
fn only_a_revoked_program_account_goes_unlisted() {
    assert!(view::listed(&Kind::User));
    assert!(view::listed(&Kind::System("forge".into())));
    assert!(view::listed(&program(Standing::Active)));
    assert!(view::listed(&program(Standing::Suspended)));
    assert!(!view::listed(&program(Standing::Revoked)));
    assert!(!view::standing_listed(Standing::Revoked));
    assert_eq!(
        view::named("scout", &program(Standing::Revoked)),
        "scout (revoked)"
    );
    assert_eq!(view::named("ada", &Kind::User), "ada");
}

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let test = name == "tests" || name == "tests.rs";
        if test {
            continue;
        }
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn every_view_reading_a_list_of_accounts_filters_it() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for group in ["app", "sdk"] {
        let members = std::fs::read_dir(crates.join(group)).expect("the crates of a group");
        for member in members.flatten() {
            sources(&member.path().join("src"), &mut files);
        }
    }
    let mut unfiltered = Vec::new();
    let mut reading = BTreeSet::new();
    for file in files {
        let source = std::fs::read_to_string(&file).expect("a source reads");
        let code: Vec<&str> = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        let text = code.join("\n");
        let at = file
            .strip_prefix(&crates)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned();
        let reads = READS.iter().any(|asked| text.contains(asked));
        let filters = FILTERS.iter().any(|filter| text.contains(filter));
        let exempt = LOOKUPS.iter().any(|(path, _)| *path == at);
        let skipped = reads && !filters && !exempt;
        if skipped {
            unfiltered.push(at.clone());
        }
        if reads {
            reading.insert(at);
        }
    }
    assert!(
        unfiltered.is_empty(),
        "these read a list of accounts and never call identity::view::listed: {unfiltered:?}"
    );
    let stale: Vec<_> = LOOKUPS
        .iter()
        .filter(|(path, _)| !reading.contains(*path))
        .collect();
    assert!(
        stale.is_empty(),
        "these lookups read no list any more: {stale:?}"
    );
}
