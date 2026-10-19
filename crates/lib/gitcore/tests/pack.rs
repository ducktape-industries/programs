// Pack tests: real git packs with ofs/ref/thin deltas, sha256 packs, write→read round trip, and git validating our packs.

#[path = "common/mod.rs"]
mod common;

use common::{fixture, git_binary, no_base, oid_list, oid_text, temp_dir};
use gitcore::pack::{self, PackWriter};
use gitcore::{Commit, Error, Hash, Kind, Limits, MemoryObjects, Object, Objects, Oid};
use std::collections::BTreeSet;

fn ids(objects: &[(Oid, Object)]) -> BTreeSet<Oid> {
    objects.iter().map(|(id, _)| *id).collect()
}

fn load_base() -> MemoryObjects {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let objects = pack::read(
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    for (id, object) in objects {
        assert_eq!(store.put(object.kind, &object.body).unwrap(), id);
    }
    store
}

#[test]
fn base_pack_reads_to_the_oids_git_listed() {
    let objects = pack::read(
        fixture!("base.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let expected: BTreeSet<Oid> = oid_list(fixture!("base.oids"), Hash::Sha1)
        .into_iter()
        .collect();
    assert_eq!(ids(&objects), expected);
    let base = oid_text(fixture!("base.oid"), Hash::Sha1);
    let commit = objects.iter().find(|(id, _)| *id == base).unwrap();
    assert_eq!(commit.1.kind, Kind::Commit);
    assert!(
        Commit::parse(&commit.1.body, Hash::Sha1)
            .unwrap()
            .parents
            .is_empty()
    );
}

#[test]
fn ofs_delta_pack_reads_to_the_oids_git_listed() {
    let objects = pack::read(
        fixture!("ofs.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let expected: BTreeSet<Oid> = oid_list(fixture!("delta.oids"), Hash::Sha1)
        .into_iter()
        .collect();
    assert_eq!(ids(&objects), expected);
    let tip = oid_text(fixture!("tip.oid"), Hash::Sha1);
    assert!(ids(&objects).contains(&tip));
}

#[test]
fn ref_delta_pack_reads_to_the_oids_git_listed() {
    let objects = pack::read(
        fixture!("ref.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let expected: BTreeSet<Oid> = oid_list(fixture!("delta.oids"), Hash::Sha1)
        .into_iter()
        .collect();
    assert_eq!(ids(&objects), expected);
}

#[test]
fn thin_pack_resolves_bases_from_the_store() {
    let store = load_base();
    let objects = pack::read(
        fixture!("thin.pack"),
        Hash::Sha1,
        &Limits::generous(),
        |id| store.get(id),
    )
    .unwrap();
    let expected: BTreeSet<Oid> = oid_list(fixture!("delta.oids"), Hash::Sha1)
        .into_iter()
        .collect();
    assert_eq!(ids(&objects), expected);
}

#[test]
fn thin_pack_without_bases_reports_the_missing_base() {
    let result = pack::read(
        fixture!("thin.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    );
    let Err(Error::MissingBase(missing)) = result else {
        panic!("expected MissingBase, got {result:?}");
    };
    let base = load_base();
    assert!(base.has(&missing).unwrap());
}

#[test]
fn delta_depth_is_capped() {
    let store = load_base();
    let limits = Limits {
        max_delta_depth: 1,
        ..Limits::generous()
    };
    let result = pack::read(fixture!("thin.pack"), Hash::Sha1, &limits, |id| {
        store.get(id)
    });
    assert_eq!(result.err(), Some(Error::DeltaTooDeep));
}

#[test]
fn object_count_and_size_are_capped() {
    let few = Limits {
        max_objects: 3,
        ..Limits::generous()
    };
    assert_eq!(
        pack::read(fixture!("base.pack"), Hash::Sha1, &few, no_base).err(),
        Some(Error::TooManyObjects)
    );
    let small = Limits {
        max_object_size: 100,
        ..Limits::generous()
    };
    assert_eq!(
        pack::read(fixture!("base.pack"), Hash::Sha1, &small, no_base).err(),
        Some(Error::ObjectTooLarge)
    );
}

#[test]
fn corrupted_checksum_is_rejected() {
    let mut bytes = fixture!("base.pack").to_vec();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    assert_eq!(
        pack::read(&bytes, Hash::Sha1, &Limits::generous(), no_base).err(),
        Some(Error::BadChecksum)
    );
    let mut wrong_version = fixture!("base.pack").to_vec();
    wrong_version[7] = 3;
    let result = pack::read(&wrong_version, Hash::Sha1, &Limits::generous(), no_base);
    assert_eq!(result.err(), Some(Error::BadChecksum));
}

#[test]
fn sha256_pack_reads_with_sha256_ids() {
    let objects = pack::read(
        fixture!("sha256.pack"),
        Hash::Sha256,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let expected: BTreeSet<Oid> = oid_list(fixture!("sha256.oids"), Hash::Sha256)
        .into_iter()
        .collect();
    assert_eq!(ids(&objects), expected);
    assert!(objects.iter().all(|(id, _)| id.hash() == Hash::Sha256));
    let tip = oid_text(fixture!("sha256.tip.oid"), Hash::Sha256);
    let commit = objects.iter().find(|(id, _)| *id == tip).unwrap();
    let parsed = Commit::parse(&commit.1.body, Hash::Sha256).unwrap();
    assert_eq!(parsed.tree.hash(), Hash::Sha256);
    assert_eq!(
        pack::read(
            fixture!("sha256.pack"),
            Hash::Sha1,
            &Limits::generous(),
            no_base
        )
        .err(),
        Some(Error::BadChecksum)
    );
}

#[test]
fn write_then_read_gives_identical_objects() {
    let original = pack::read(
        fixture!("ofs.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let objects: Vec<Object> = original.iter().map(|(_, o)| o.clone()).collect();
    let bytes = pack::write(objects, Hash::Sha1).unwrap();
    let again = pack::read(&bytes, Hash::Sha1, &Limits::generous(), no_base).unwrap();
    assert_eq!(again, original);
    let empty = pack::write(Vec::new(), Hash::Sha1).unwrap();
    assert_eq!(empty.len(), 12 + 20);
    assert!(
        pack::read(&empty, Hash::Sha1, &Limits::generous(), no_base)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn streaming_writer_matches_collected_writer() {
    let original = pack::read(
        fixture!("sha256.pack"),
        Hash::Sha256,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let objects: Vec<Object> = original.iter().map(|(_, o)| o.clone()).collect();
    let collected = pack::write(objects.clone(), Hash::Sha256).unwrap();
    let mut pieces: Vec<Vec<u8>> = Vec::new();
    let mut sink = |bytes: &[u8]| pieces.push(bytes.to_vec());
    let mut writer = PackWriter::new(&mut sink, Hash::Sha256, objects.len() as u32);
    for object in &objects {
        writer.add(object);
    }
    let checksum = writer.finish().unwrap();
    let streamed: Vec<u8> = pieces.concat();
    assert_eq!(streamed, collected);
    assert!(pieces.len() > objects.len());
    assert_eq!(&streamed[streamed.len() - 32..], checksum.as_bytes());
}

#[test]
fn git_accepts_a_pack_we_wrote() {
    let git = git_binary();
    let original = pack::read(
        fixture!("ofs.pack"),
        Hash::Sha1,
        &Limits::generous(),
        no_base,
    )
    .unwrap();
    let objects: Vec<Object> = original.iter().map(|(_, o)| o.clone()).collect();
    let bytes = pack::write(objects, Hash::Sha1).unwrap();
    let dir = temp_dir("forge-git-pack");
    let init = std::process::Command::new(&git)
        .args(["init", "-q"])
        .current_dir(&dir)
        .status()
        .unwrap();
    assert!(init.success());
    let mut child = std::process::Command::new(&git)
        .args(["index-pack", "--stdin"])
        .current_dir(&dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), &bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "index-pack failed: {output:?}");
    let tip = oid_text(fixture!("tip.oid"), Hash::Sha1);
    let cat = std::process::Command::new(&git)
        .args(["cat-file", "-t", &tip.to_hex()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&cat.stdout).trim(), "commit");
    let expected = original.iter().find(|(id, _)| *id == tip).unwrap();
    let body = std::process::Command::new(&git)
        .args(["cat-file", "commit", &tip.to_hex()])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(body.stdout, expected.1.body);
    std::fs::remove_dir_all(&dir).ok();
}
