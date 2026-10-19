// Shared helpers for the integration tests: fixture loading and small object builders.

#![allow(dead_code, unused_macros, unused_imports)]

use gitcore::{
    Commit, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tree, TreeEntry,
};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/",
            $name
        )) as &[u8]
    };
}
pub(crate) use fixture;

pub fn oid_text(text: &[u8], hash: Hash) -> Oid {
    let trimmed = String::from_utf8_lossy(text);
    Oid::from_hex(hash, trimmed.trim()).expect("fixture oid")
}

pub fn oid_list(text: &[u8], hash: Hash) -> Vec<Oid> {
    String::from_utf8_lossy(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Oid::from_hex(hash, line.trim()).expect("fixture oid"))
        .collect()
}

pub fn sha1(hex: &str) -> Oid {
    Oid::from_hex(Hash::Sha1, hex).expect("hex")
}

pub fn signature(time: i64) -> Signature {
    Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time,
        offset_minutes: 0,
    }
}

pub fn blob(store: &mut MemoryObjects, content: &[u8]) -> Oid {
    store.put(Kind::Blob, content).expect("put blob")
}

pub fn tree(store: &mut MemoryObjects, entries: &[(&str, Mode, Oid)]) -> Oid {
    let tree = Tree {
        entries: entries
            .iter()
            .map(|(name, mode, id)| TreeEntry {
                mode: *mode,
                name: name.as_bytes().to_vec(),
                id: *id,
            })
            .collect(),
    };
    store.put(Kind::Tree, &tree.serialize()).expect("put tree")
}

pub fn commit(
    store: &mut MemoryObjects,
    tree: Oid,
    parents: &[Oid],
    time: i64,
    message: &str,
) -> Oid {
    let commit = Commit {
        tree,
        parents: parents.to_vec(),
        author: signature(time),
        committer: signature(time),
        extra: Vec::new(),
        message: message.as_bytes().to_vec(),
    };
    store
        .put(Kind::Commit, &commit.serialize())
        .expect("put commit")
}

pub fn file_tree(store: &mut MemoryObjects, files: &[(&str, &str)]) -> Oid {
    let entries: Vec<(&str, Mode, Oid)> = files
        .iter()
        .map(|(name, content)| (*name, Mode::Regular, blob(store, content.as_bytes())))
        .collect();
    tree(store, &entries)
}

/// No base held: a pack read that needs one refuses.
pub fn no_base(_: &Oid) -> gitcore::Result<Option<Object>> {
    Ok(None)
}

// The `git` on PATH: a test that checks our bytes with git fails without one.
pub fn git_binary() -> std::path::PathBuf {
    let git = std::path::PathBuf::from("git");
    let found = std::process::Command::new(&git).arg("--version").output();
    assert!(
        found.is_ok_and(|output| output.status.success()),
        "git not on PATH"
    );
    git
}

/// A fresh directory under the system temp dir, named for this process and thread.
pub fn temp_dir(prefix: &str) -> std::path::PathBuf {
    let unique = format!(
        "{prefix}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
