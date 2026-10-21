#![allow(dead_code, unused_imports)]
//! The forge program end to end over MemorySandbox: founding, access,
//! pushes in steps, advertisement, fetch, merge, sha256.

pub use std::collections::{BTreeMap, BTreeSet};

pub use forge::{Bounds, Forge, Op, PageRequest, Principal, Query, Reply, Service, Settings};
pub use gitcore::walk::{Closed, Listing};
pub use gitcore::wire::pktline::{self, Pkt, Reader};
pub use gitcore::wire::upload::{Command, parse_command, respond};
pub use gitcore::{
    Commit, Hash, Kind, Limits, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tree,
    TreeEntry, pack,
};
pub use program::{Blob, BlobId, Env, HashKind, Origin, code};
pub use program::{MockHost, Program};

pub mod sandbox;
pub mod story;
pub use sandbox::MemorySandbox;

pub const OWNER: &[u8] = b"owner-key";
pub const WRITER: &[u8] = b"writer-key";
pub const STRANGER: &[u8] = b"stranger-key";
pub const TIME: u64 = 1_700_000_000;

pub fn bounds() -> Bounds {
    Bounds {
        max_objects: 10_000,
        max_delta_depth: 64,
        max_object_size: 1 << 20,
        push_walk: 1000,
        fetch_walk: 1000,
        merge_cost: 100_000,
        page_size: 128,
        log_walk: 1000,
        tree_walk: 256,
        diff_bytes: 4 << 20,
        blob_bytes: 1 << 20,
        record_bytes: 64 << 10,
    }
}

pub fn founded() -> MemorySandbox {
    let sandbox = MemorySandbox::default();
    sandbox.chain.init("forge", &bounds()).unwrap();
    sandbox
}

/// The accounts the harness keys hold, as identity would seat them: every
/// writer is a person, and a key that holds none writes nothing.
pub const HOLDERS: [(&[u8], u64); 3] = [(OWNER, 11), (WRITER, 12), (STRANGER, 13)];

/// The person a harness key signs as: the account it holds.
pub fn person(key: &[u8]) -> Principal {
    let (_, account) = HOLDERS
        .iter()
        .find(|(held, _)| *held == key)
        .expect("a harness key");
    Principal::Account(*account)
}

/// `actor`'s op at `height`, run as the wasm module runs it: the signer
/// resolved to its account, then the typed execute and what it emitted, in
/// one frame. forge's output.
pub fn signed_op(
    sandbox: &MemorySandbox,
    actor: &[u8],
    height: u64,
    op: &Op,
) -> Result<Vec<u8>, program::Error> {
    sandbox.frame(Origin::Signed(actor.to_vec()), height, TIME, op)
}

/// `actor`'s op; a refusal left forge's store as it was.
#[track_caller]
pub fn act(sandbox: &MemorySandbox, actor: &[u8], op: &Op) -> Result<Vec<u8>, program::Error> {
    sandbox.forge.attempt(|| signed_op(sandbox, actor, 1, op))
}

/// The refusal of `actor`'s op, which left forge's store as it was.
#[track_caller]
pub fn refused(sandbox: &MemorySandbox, actor: &[u8], op: &Op) -> program::Error {
    sandbox.forge.refused(|| signed_op(sandbox, actor, 1, op))
}

pub fn ask(sandbox: &MemorySandbox, query: &Query) -> Result<Vec<u8>, program::Error> {
    Forge::answer(&sandbox.reads(1), query.clone())
}

pub fn create(sandbox: &MemorySandbox, name: &str, hash: HashKind) {
    act(
        sandbox,
        OWNER,
        &Op::Create {
            repo: name.into(),
            hash,
        },
    )
    .unwrap();
}

pub fn predates_owner_default(sandbox: &MemorySandbox, repo: &str) {
    let key = forge::tables::REPOS_DEFAULTING_TO_OWNER.key(&repo.to_owned());
    sandbox.forge.borrow_mut().state.remove(&key);
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
    store.put(Kind::Blob, content).unwrap()
}

pub fn tree(store: &mut MemoryObjects, entries: &[(&str, Oid)]) -> Oid {
    let tree = Tree {
        entries: entries
            .iter()
            .map(|(name, id)| TreeEntry {
                mode: Mode::Regular,
                name: name.as_bytes().to_vec(),
                id: *id,
            })
            .collect(),
    };
    store.put(Kind::Tree, &tree.serialize()).unwrap()
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
        message: format!("{message}\n").into_bytes(),
    };
    store.put(Kind::Commit, &commit.serialize()).unwrap()
}

pub fn file_commit(
    store: &mut MemoryObjects,
    parents: &[Oid],
    time: i64,
    files: &[(&str, &[u8])],
) -> Oid {
    let mut entries = Vec::new();
    for (name, content) in files {
        let id = blob(store, content);
        entries.push((*name, id));
    }
    let tree = tree(store, &entries);
    commit(store, tree, parents, time, "step")
}

pub fn pack_of(store: &MemoryObjects, ids: &[Oid]) -> Vec<u8> {
    let objects: Vec<Object> = ids
        .iter()
        .map(|id| store.get(id).unwrap().unwrap())
        .collect();
    pack::write(objects, store.hash()).unwrap()
}

pub fn all_ids(store: &MemoryObjects) -> Vec<Oid> {
    store.ids().copied().collect()
}

pub fn push_request(commands: &[(Oid, Oid, &str)]) -> Vec<u8> {
    let mut request = Vec::new();
    for (index, (old, new, name)) in commands.iter().enumerate() {
        let mut line = format!("{old} {new} {name}").into_bytes();
        let first = index == 0;
        if first {
            line.extend_from_slice(b"\0report-status side-band-64k");
        }
        pktline::push(&mut request, &line);
    }
    request.extend_from_slice(pktline::flush());
    request
}

pub fn held_blob(sandbox: &MemorySandbox, kind: &str, body: &[u8]) -> BlobId {
    let id = program::blob_id(HashKind::Sha256, kind, body).unwrap();
    let blob = Blob {
        kind: kind.into(),
        body: body.to_vec(),
    };
    sandbox.forge.borrow_mut().blobs.insert(id, blob);
    id
}

pub fn forget(sandbox: &MemorySandbox, id: BlobId) {
    for host in [
        &sandbox.forge,
        &sandbox.chat,
        &sandbox.access,
        &sandbox.runner,
    ] {
        host.borrow_mut().blobs.remove(&id);
    }
}

pub fn with_pack<T>(
    sandbox: &MemorySandbox,
    pack: &[u8],
    run: impl FnOnce(Option<BlobId>) -> T,
) -> T {
    let carries_objects = !pack.is_empty();
    let id = carries_objects.then(|| held_blob(sandbox, forge::PACK_KIND, pack));
    let ran = run(id);
    if let Some(id) = id {
        forget(sandbox, id);
    }
    ran
}

pub fn push(
    sandbox: &MemorySandbox,
    actor: &[u8],
    repo: &str,
    commands: &[(Oid, Oid, &str)],
    pack_bytes: &[u8],
) -> Result<Vec<String>, program::Error> {
    let report = with_pack(sandbox, pack_bytes, |pack| {
        let op = Op::Push {
            repo: repo.into(),
            request: push_request(commands),
            pack,
        };
        act(sandbox, actor, &op)
    })?;
    Ok(report_lines(&report))
}

pub fn report_lines(report: &[u8]) -> Vec<String> {
    let mut reader = Reader::new(report);
    let Some(Ok(Pkt::Data(band))) = reader.next() else {
        panic!("a sideband report starts with a data packet");
    };
    assert_eq!(band[0], 1);
    Reader::new(&band[1..])
        .filter_map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => {
                Some(String::from_utf8_lossy(pktline::strip_newline(data)).into_owned())
            }
            _ => None,
        })
        .collect()
}

pub fn refs_of(sandbox: &MemorySandbox, repo: &str) -> BTreeMap<String, String> {
    let reply: Reply = abi::decode(
        &ask(
            sandbox,
            &Query::Refs {
                repo: repo.into(),
                page: PageRequest::first(128),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let Reply::Refs { page, .. } = reply else {
        panic!("refs reply");
    };
    page.items
        .into_iter()
        .map(|r| (String::from_utf8(r.name).unwrap(), r.target))
        .collect()
}

pub fn v2_request(command: &str, hash: Hash, args: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    pktline::push_line(&mut out, format!("command={command}").as_bytes());
    pktline::push_line(&mut out, b"agent=git/2.55.0");
    pktline::push_line(
        &mut out,
        format!("object-format={}", hash.name()).as_bytes(),
    );
    out.extend_from_slice(pktline::delim());
    for arg in args {
        pktline::push_line(&mut out, arg.as_bytes());
    }
    out.extend_from_slice(pktline::flush());
    out
}

pub fn pkt_lines(bytes: &[u8]) -> Vec<String> {
    Reader::new(bytes)
        .map(|pkt| match pkt.unwrap() {
            Pkt::Data(data) => String::from_utf8_lossy(pktline::strip_newline(data)).into_owned(),
            Pkt::Flush => "<flush>".into(),
            Pkt::Delim => "<delim>".into(),
            Pkt::ResponseEnd => "<end>".into(),
        })
        .collect()
}

pub fn fetched_pack(response: &[u8]) -> (Vec<String>, Vec<u8>) {
    let mut sections = Vec::new();
    let mut pack_bytes = Vec::new();
    for pkt in Reader::new(response) {
        match pkt.unwrap() {
            Pkt::Data(data) if sections.last().is_some_and(|s| s == "packfile") => {
                assert_eq!(data[0], 1);
                pack_bytes.extend_from_slice(&data[1..]);
            }
            Pkt::Data(data) => {
                sections.push(String::from_utf8_lossy(pktline::strip_newline(data)).into_owned())
            }
            Pkt::Flush => sections.push("<flush>".into()),
            Pkt::Delim => sections.push("<delim>".into()),
            Pkt::ResponseEnd => sections.push("<end>".into()),
        }
    }
    (sections, pack_bytes)
}

pub fn ids_in_pack(bytes: &[u8], hash: Hash) -> BTreeSet<Oid> {
    pack::read(bytes, hash, &Limits::generous(), |_| Ok(None))
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

pub fn closure_of(
    sandbox: &MemorySandbox,
    repo: &str,
    hash: HashKind,
    wants: &[Oid],
    haves: &[Oid],
) -> Vec<gitcore::walk::Listed> {
    let mut listing = Listing::of(wants, haves);
    while let Some(walk) = listing.pending() {
        let query = Query::Closure {
            repo: repo.into(),
            walk: forge::Walk::of(walk),
        };
        let Reply::Closure { listed, next, .. } =
            abi::decode(&ask(sandbox, &query).unwrap()).unwrap()
        else {
            panic!("a closure answers Closure");
        };
        listing.receive(Closed {
            listed: listed
                .iter()
                .map(|item| item.parse(hash).unwrap())
                .collect(),
            next: next.parse(hash).unwrap(),
        });
    }
    listing.into_listed()
}

pub fn fetch(sandbox: &MemorySandbox, repo: &str, hash: HashKind, request: &[u8]) -> Vec<u8> {
    let Ok(Some(Command::Fetch(command))) = parse_command(request, forge_hash(hash)) else {
        panic!("a fetch request");
    };
    let listed = closure_of(sandbox, repo, hash, &command.wants, &command.haves);
    let objects = listed.iter().map(|item| {
        let id = forge::blob_id_of(&item.id);
        let blob = sandbox.forge.borrow().blobs.get(&id).cloned().unwrap();
        assert_eq!(Kind::parse(blob.kind.as_bytes()).unwrap(), item.kind);
        Ok(Object::new(item.kind, blob.body))
    });
    let mut response = Vec::new();
    respond(&command, objects, &mut |chunk| {
        response.extend_from_slice(chunk)
    })
    .unwrap();
    response
}

pub fn forge_hash(hash: HashKind) -> Hash {
    match hash {
        HashKind::Sha1 => Hash::Sha1,
        HashKind::Sha256 => Hash::Sha256,
    }
}
