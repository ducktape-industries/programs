//! The kinds of item the ownership program records, as the programs' owned tables claim them (a
//! test holds the two together), and what a person calls each.

// One kind of owned item: its name in ownership, the program that keeps it, and its words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub name: &'static str,
    pub program: &'static str,
    pub label: &'static str,
}

const fn of(name: &'static str, program: &'static str, label: &'static str) -> Kind {
    Kind {
        name,
        program,
        label,
    }
}

pub static KINDS: [Kind; 10] = [
    of("chat.channel", "chat", "Chat channels"),
    of("chat.message", "chat", "Chat messages"),
    of("forge.repo", "forge", "Forge repositories"),
    of("forge.change", "forge", "Forge changes"),
    of("forge.review", "forge", "Forge reviews"),
    of("harness.model", "harness", "Harness models"),
    of(
        "harness.agent-conversation",
        "harness",
        "Agents' conversations",
    ),
    of("egress.service", "egress", "Egress services"),
    of("files.path", "files", "Files and folders"),
    of("files.pin", "files", "Pinned snapshots"),
];

// The programs that keep owned items, each once, in the order `KINDS` names them.
pub static PROGRAMS: [&str; 5] = ["chat", "forge", "harness", "egress", "files"];

pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|kind| kind.name == name)
}
