//! Forge and chat over the host: what forge emits to chat runs in forge's
//! frame, so a change and its discussion channel land in one block.

use super::*;
use abi::HashKind;
use gitcore::wire::pktline;
use gitcore::{
    Commit, Hash, Kind, MemoryObjects, Mode, Objects as _, Oid, Signature, Tree, TreeEntry, pack,
};

const REPO: &str = "project";
const AUTHOR: u64 = 1;

pub(super) fn bounds() -> forge::Bounds {
    forge::Bounds {
        max_objects: 1_000_000,
        max_delta_depth: 64,
        max_object_size: 256 << 20,
        push_walk: 10_000,
        fetch_walk: 1_000_000,
        merge_cost: 1024,
        page_size: 128,
        log_walk: 10_000,
        tree_walk: 1024,
        diff_bytes: 32 << 20,
        blob_bytes: 64,
        record_bytes: 64 << 10,
    }
}

fn apps() -> Vec<Founding> {
    vec![
        founding(chat::PROGRAM, &program("chat")),
        Founding {
            program: forge::PROGRAM.to_owned(),
            code: program("forge"),
            params: abi::encode(&bounds()),
            roles: Vec::new(),
        },
    ]
}

fn commit(store: &mut MemoryObjects, parents: &[Oid], files: &[(&str, &[u8])]) -> Oid {
    let entries = files
        .iter()
        .map(|(name, content)| TreeEntry {
            mode: Mode::Regular,
            name: name.as_bytes().to_vec(),
            id: store.put(Kind::Blob, content).unwrap(),
        })
        .collect();
    let tree = store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap();
    let who = Signature {
        name: b"Ada".to_vec(),
        email: b"ada@example.com".to_vec(),
        time: TIME as i64 / 1000,
        offset_minutes: 0,
    };
    let commit = Commit {
        tree,
        parents: parents.to_vec(),
        author: who.clone(),
        committer: who,
        extra: Vec::new(),
        message: b"step\n".to_vec(),
    };
    store.put(Kind::Commit, &commit.serialize()).unwrap()
}

fn push_request(store: &MemoryObjects, refs: &[(Oid, &str)]) -> (Vec<u8>, Vec<u8>) {
    let zero = Oid::Sha1([0; 20]);
    let mut request = Vec::new();
    for (index, (new, name)) in refs.iter().enumerate() {
        let mut line = format!("{zero} {new} {name}").into_bytes();
        if index == 0 {
            line.extend_from_slice(b"\0report-status side-band-64k");
        }
        pktline::push(&mut request, &line);
    }
    request.extend_from_slice(pktline::flush());
    let objects = store.ids().map(|id| store.get(id).unwrap().unwrap());
    let pack = pack::write(objects.collect::<Vec<_>>(), store.hash()).unwrap();
    (request, pack)
}

#[test]
fn a_push_and_a_stage_read_their_pack_only_once_a_quorum_holds_it() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let author = public(AUTHOR);
        let create = identity::Op::Create {
            name: "Ada".into(),
            scheme: Scheme::Ed25519,
        };
        net.apply(&author, identity::PROGRAM, &create).await;
        let repo = forge::Op::Create {
            repo: REPO.into(),
            hash: HashKind::Sha1,
        };
        net.apply(&author, forge::PROGRAM, &repo).await;
        let mut store = MemoryObjects::new(Hash::Sha1);
        let main = commit(&mut store, &[], &[("README.md", b"# Project\n")]);
        let (request, pack) = push_request(&store, &[(main, "refs/heads/main")]);
        let stored = net.stored(forge::PACK_KIND, &pack);
        let pack = stored.id;
        let bundle = net.put(vec![stored]).await;
        let push = forge::Op::Push {
            repo: REPO.into(),
            request,
            pack: Some(pack),
        };
        let stage = forge::Op::Stage {
            repo: REPO.into(),
            pack,
        };
        for unheld in [&push, &stage] {
            let refused = net.refuse(&author, forge::PROGRAM, unheld).await;
            assert_eq!(refused, reason::NOT_FOUND, "{unheld:?}");
        }
        let object = forge::blob_id_of(&main);
        assert_eq!(net.blob(&object).await, None);

        net.receipted(bundle, &FOUNDERS).await;
        net.apply(&author, forge::PROGRAM, &stage).await;
        assert!(
            net.blob(&object).await.is_some(),
            "a stage stores the objects"
        );
        let report = net.apply(&author, forge::PROGRAM, &push).await;
        assert!(
            report
                .windows(b"ok refs/heads/main".len())
                .any(|line| line == b"ok refs/heads/main"),
            "{}",
            String::from_utf8_lossy(&report)
        );
        let refs = forge::Query::Refs {
            repo: REPO.into(),
            page: PageRequest::default(),
        };
        let forge::Reply::Refs { page, .. } = net.ask(forge::PROGRAM, &refs).await else {
            panic!("Refs answers Refs");
        };
        assert_eq!(page.items.len(), 1, "{page:?}");
    });
}

#[test]
fn a_change_and_its_channel_land_in_one_block() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let author = public(AUTHOR);
        let create = identity::Op::Create {
            name: "Ada".into(),
            scheme: Scheme::Ed25519,
        };
        net.apply(&author, identity::PROGRAM, &create).await;
        net.apply(
            &author,
            forge::PROGRAM,
            &forge::Op::Create {
                repo: REPO.into(),
                hash: HashKind::Sha1,
            },
        )
        .await;
        let mut store = MemoryObjects::new(Hash::Sha1);
        let main = commit(&mut store, &[], &[("README.md", b"# Project\n")]);
        let feature = commit(
            &mut store,
            &[main],
            &[("README.md", b"# Project\n\nMore.\n")],
        );
        let (request, pack) = push_request(
            &store,
            &[(main, "refs/heads/main"), (feature, "refs/heads/feature")],
        );
        let pack = net.hold(forge::PACK_KIND, &pack).await;
        let push = forge::Op::Push {
            repo: REPO.into(),
            request,
            pack: Some(pack),
        };
        net.apply(&author, forge::PROGRAM, &push).await;
        let object = forge::blob_id_of(&feature);
        assert!(net.blob(&object).await.is_some(), "its objects are stored");

        let open = forge::Op::ChangeOpen {
            repo: REPO.into(),
            from: forge::Revision::Ref(b"refs/heads/feature".to_vec()),
            into: b"refs/heads/main".to_vec(),
            title: "Feature".into(),
            body: String::new(),
            reviewers: Vec::new(),
        };
        let receipt = net.submit(&author, forge::PROGRAM, &open).await;
        assert!(
            matches!(receipt.outcome, Outcome::Applied { .. }),
            "{receipt:?}"
        );
        let in_frame = |nested: &Receipt| {
            nested.program == chat::PROGRAM && matches!(nested.outcome, Outcome::Applied { .. })
        };
        assert!(
            !receipt.nested.is_empty() && receipt.nested.iter().all(in_frame),
            "{receipt:?}"
        );
        let channel = format!("forge:{REPO}:1");
        let chat::Reply::Channel(Some(info)) = net
            .ask(
                chat::PROGRAM,
                &chat::Query::Channel {
                    channel_id: channel.clone(),
                },
            )
            .await
        else {
            panic!("no channel {channel} at height {}", net.height);
        };
        assert_eq!(info.channel.id, channel);
    });
}
