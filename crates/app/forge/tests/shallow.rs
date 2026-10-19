//! A shallow fetch over forge: `Query::Shallow` pages, the boundary they name, and a client
//! that deepens what it holds or fetches from its own boundary.

mod common;
use common::*;
use forge::DeepWalk;
use gitcore::walk::{Deep, Deepened, ShallowListing};

// c1 ← c2 ← c3 ← c4, and a side commit off c1 merged at c4, each with a file of its own.
fn project(bounds: Bounds) -> (MemorySandbox, [Oid; 5]) {
    let sandbox = MemorySandbox::default();
    Forge::init(&sandbox.exec(1), &abi::encode(&bounds)).unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let c1 = file_commit(&mut source, &[], 1, &[("one", b"1")]);
    let c2 = file_commit(&mut source, &[c1], 2, &[("two", b"2")]);
    let side = file_commit(&mut source, &[c1], 3, &[("side", b"s")]);
    let c3 = file_commit(&mut source, &[c2], 4, &[("three", b"3")]);
    let c4 = file_commit(&mut source, &[c3, side], 5, &[("four", b"4")]);
    let moved = push(
        &sandbox,
        OWNER,
        "project",
        &[(Hash::Sha1.zero(), c4, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    assert_eq!(moved, ["unpack ok", "ok refs/heads/main"]);
    (sandbox, [c1, c2, side, c3, c4])
}

struct Answer {
    commits: BTreeSet<Oid>,
    shallow: BTreeSet<Oid>,
    unshallow: BTreeSet<Oid>,
    pages: usize,
}

fn oids(hexes: &[String]) -> Vec<Oid> {
    hexes
        .iter()
        .map(|hex| Oid::from_hex(Hash::Sha1, hex).unwrap())
        .collect()
}

fn set(ids: &[Oid]) -> BTreeSet<Oid> {
    ids.iter().copied().collect()
}

// What a shallow client lists from `Shallow` pages, and the boundary the last page names.
fn deepened(
    sandbox: &MemorySandbox,
    wants: &[Oid],
    haves: &[Oid],
    deepen: Option<u32>,
    client: &[Oid],
) -> Answer {
    let mut listing = ShallowListing::of(wants, haves, deepen, client);
    let mut pages = 0;
    while let Some(deep) = listing.pending() {
        pages += 1;
        let query = Query::Shallow {
            repo: "project".into(),
            walk: DeepWalk::of(deep),
        };
        let reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
        let Reply::Shallow {
            listed,
            next,
            shallow,
            unshallow,
            ..
        } = reply
        else {
            panic!("a shallow walk answers Shallow");
        };
        listing.receive(Deepened {
            listed: listed
                .iter()
                .map(|item| item.parse(HashKind::Sha1).unwrap())
                .collect(),
            next: next.parse(HashKind::Sha1).unwrap(),
            shallow: oids(&shallow),
            unshallow: oids(&unshallow),
        });
    }
    let shallow = set(listing.shallow());
    let unshallow = set(listing.unshallow());
    let mut commits = BTreeSet::new();
    for item in listing.into_listed() {
        assert_eq!(item.kind, Kind::Commit, "a walk of commits lists commits");
        commits.insert(item.id);
    }
    Answer {
        commits,
        shallow,
        unshallow,
        pages,
    }
}

#[test]
fn a_shallow_walk_lists_the_commits_within_its_depth_and_names_the_boundary() {
    let (sandbox, [c1, c2, side, c3, c4]) = project(bounds());
    let one = deepened(&sandbox, &[c4], &[], Some(1), &[]);
    assert_eq!(one.commits, set(&[c4]));
    assert_eq!(one.shallow, set(&[c4]));
    let two = deepened(&sandbox, &[c4], &[], Some(2), &[]);
    assert_eq!(two.commits, set(&[c4, c3, side]));
    assert_eq!(two.shallow, set(&[c3, side]));
    let three = deepened(&sandbox, &[c4], &[], Some(3), &[]);
    assert_eq!(three.commits, set(&[c4, c3, side, c2, c1]));
    assert_eq!(three.shallow, set(&[c2, c1]));
    let whole = deepened(&sandbox, &[c4], &[], Some(4), &[]);
    assert_eq!(whole.commits, three.commits);
    assert!(whole.shallow.is_empty());
    for answer in [one, two, three, whole] {
        assert!(answer.unshallow.is_empty());
    }
}

#[test]
fn a_shallow_walk_pages_under_the_read_budget_and_lists_the_same() {
    let (wide, [.., c4]) = project(bounds());
    let (narrow, _) = project(Bounds {
        tree_walk: 3,
        ..bounds()
    });
    for depth in 1..=4 {
        let whole = deepened(&wide, &[c4], &[], Some(depth), &[]);
        let paged = deepened(&narrow, &[c4], &[], Some(depth), &[]);
        assert_eq!(paged.commits, whole.commits, "depth {depth}");
        assert_eq!(paged.shallow, whole.shallow, "depth {depth}");
    }
    assert!(deepened(&narrow, &[c4], &[], Some(4), &[]).pages > 1);
}

#[test]
fn a_client_deepening_its_boundary_hears_it_lifted_and_gets_what_lies_below() {
    let (sandbox, [c1, c2, side, c3, c4]) = project(bounds());
    let deeper = deepened(&sandbox, &[c4], &[c4], Some(3), &[c4]);
    assert_eq!(deeper.commits, set(&[c3, side, c2, c1]));
    assert_eq!(deeper.shallow, set(&[c2, c1]));
    assert_eq!(deeper.unshallow, set(&[c4]));
}

#[test]
fn a_shallow_clients_fetch_walks_from_its_boundary_and_never_past_it() {
    let (sandbox, [c1, _, side, c3, c4]) = project(bounds());
    let fetched = deepened(&sandbox, &[c4], &[c3], None, &[c3]);
    assert_eq!(fetched.commits, set(&[c4, side, c1]));
    assert!(fetched.shallow.is_empty() && fetched.unshallow.is_empty());
    let whole_client: BTreeSet<Oid> = closure_of(&sandbox, "project", HashKind::Sha1, &[c4], &[c3])
        .into_iter()
        .map(|item| item.id)
        .collect();
    assert!(
        !whole_client.contains(&c1),
        "a client that is not shallow holds c1"
    );
}

#[test]
fn a_shallow_walk_of_no_depth_or_of_an_id_not_of_the_repository_is_refused() {
    let (sandbox, [.., c4]) = project(bounds());
    let shallow = |walk: DeepWalk| Query::Shallow {
        repo: "project".into(),
        walk,
    };
    let no_depth = DeepWalk::of(Deep::of(&[c4], &[], Some(0), &[]));
    let refusal = ask(&sandbox, &shallow(no_depth)).unwrap_err();
    assert_eq!(refusal.code, code::INVALID_INPUT);
    let foreign = DeepWalk {
        frontier: vec![("zz".into(), 1)],
        ..DeepWalk::of(Deep::of(&[c4], &[], Some(1), &[]))
    };
    let refusal = ask(&sandbox, &shallow(foreign)).unwrap_err();
    assert_eq!(refusal.code, code::INVALID_INPUT);
}
