//! A review forge asks of an agent reaches it through harness: once the agent answers forge, the
//! change's opened notice and each new head of it reach the agent; before that harness drops them.
use ::program::{HashKind, Ownership, Party};
use gitcore::wire::pktline;
use gitcore::{
    Commit, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tree, TreeEntry,
};

use super::forge_commit::forged;
use super::*;

const REPO: &str = "notes";

fn signature() -> Signature {
    Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time: 1_700_000_000,
        offset_minutes: 0,
    }
}

fn step(parent: Option<Oid>, text: &str) -> (Oid, Vec<u8>) {
    let mut store = MemoryObjects::new(Hash::Sha1);
    let file = store.put(Kind::Blob, text.as_bytes()).unwrap();
    let entry = TreeEntry {
        mode: Mode::Regular,
        name: b"notes.md".to_vec(),
        id: file,
    };
    let tree = Tree {
        entries: vec![entry],
    };
    let tree = store.put(Kind::Tree, &tree.serialize()).unwrap();
    let commit = Commit {
        tree,
        parents: parent.into_iter().collect(),
        author: signature(),
        committer: signature(),
        extra: Vec::new(),
        message: format!("{text}\n").into_bytes(),
    };
    let id = store.put(Kind::Commit, &commit.serialize()).unwrap();
    let objects: Vec<Object> = store
        .ids()
        .map(|held| store.get(held).unwrap().unwrap())
        .collect();
    (id, gitcore::pack::write(objects, Hash::Sha1).unwrap())
}

fn push(world: &World, pack: Vec<u8>, old: Oid, new: Oid, branch: &str) {
    let mut request = Vec::new();
    let line = format!("{old} {new} refs/heads/{branch}\0report-status side-band-64k");
    pktline::push(&mut request, line.as_bytes());
    request.extend_from_slice(pktline::flush());
    let pack = world.chain.put_blob(forge::PACK_KIND, pack).unwrap();
    let op = forge::Op::Push {
        repo: REPO.into(),
        request,
        pack: Some(pack),
    };
    world
        .chain
        .submit(signed(ADA), forge::PROGRAM, &op)
        .unwrap();
}

fn opened(world: &World, branch: &str) -> u64 {
    let open = forge::Op::ChangeOpen {
        repo: REPO.into(),
        from: forge::Revision::Ref(format!("refs/heads/{branch}").into_bytes()),
        into: b"refs/heads/main".to_vec(),
        title: format!("Teach {branch}"),
        body: "Why it changes.".into(),
        reviewers: vec![Principal::Account(world.agent)],
    };
    let made = world
        .chain
        .submit(signed(ADA), forge::PROGRAM, &open)
        .unwrap();
    let forge::OpReply::Change { n, .. } = abi::decode(&made).unwrap() else {
        panic!("an open answers Change")
    };
    n
}

fn bodies(world: &World) -> Vec<Body> {
    let chain = &world.chain;
    let threads = world.threads().into_iter();
    threads
        .flat_map(|row| frames(chain, row.id).into_iter().chain(held(chain, row.id)))
        .collect()
}

fn posts(world: &World) -> Vec<String> {
    let bodies = bodies(world).into_iter();
    bodies
        .filter_map(|body| match body {
            Body::Post { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn a_reviewer_hears_forges_request_and_each_new_head_once_it_answers_forge() {
    let world = forged(10_000);
    let register = identity::Op::RegisterProgram {
        program: forge::PROGRAM.into(),
    };
    world
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
    let create = forge::Op::Create {
        repo: REPO.into(),
        hash: HashKind::Sha1,
    };
    world
        .chain
        .submit(signed(ADA), forge::PROGRAM, &create)
        .unwrap();
    let zero = Hash::Sha1.zero();
    let (main, pack) = step(None, "first");
    push(&world, pack, zero, main, "main");
    let (feature, pack) = step(Some(main), "second");
    push(&world, pack, zero, feature, "feature");
    let (other, pack) = step(Some(main), "third");
    push(&world, pack, zero, other, "other");

    manager_only(&world);
    let unheard = opened(&world, "feature");
    let noticed: Vec<String> = bodies(&world)
        .into_iter()
        .filter_map(|body| match body {
            Body::Notice { text } => Some(text),
            _ => None,
        })
        .collect();
    let [dropped] = &noticed[..] else {
        panic!("{noticed:?}")
    };
    assert!(
        dropped.starts_with("dropped a notice from account"),
        "{dropped}"
    );
    assert!(
        posts(&world).is_empty(),
        "an agent that answers only its manager hears nothing of forge"
    );

    answering(
        &world,
        Ownership::Listed(vec![Party::Program(forge::PROGRAM.into())]),
    );
    let n = opened(&world, "other");
    let (tip, pack) = step(Some(feature), "fourth");
    push(&world, pack, feature, tip, "feature");
    let heard = posts(&world);
    let open = format!("{REPO} #{n}: Teach other running — opened");
    let head = format!("{REPO} #{unheard}: Teach feature running — new head {tip}");
    let told = |words: &str| {
        heard
            .iter()
            .filter(|text| text.contains("sent you a notice"))
            .filter(|text| text.contains(words))
            .count()
    };
    assert_eq!(
        (told(&open), told(&head), heard.len()),
        (1, 1, 2),
        "{heard:?}"
    );
}
