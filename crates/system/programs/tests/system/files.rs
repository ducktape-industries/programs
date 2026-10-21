//! files founded on ducktape's host: chunks held through the blobs role (stored, put,
//! receipted), named by a commit and read back; file bytes in a transaction refused;
//! an account writes anywhere no write list covers, another's home too, but never a namespace
//! root itself; a write list set on a folder decides who writes under it.
use super::*;

use files::{BlobId, CHUNK_SIZE, Change, Content, Op, Ownership, Party, Query, Reply};

const ADA_KEY: u64 = 11;
const BO_KEY: u64 = 12;
const CY_KEY: u64 = 13;

fn apps() -> Vec<Founding> {
    vec![founding(files::PROGRAM, &program("files"))]
}

fn put(path: &str, content: Content) -> Change {
    Change::Put {
        path: path.into(),
        exec: false,
        meta: Default::default(),
        content,
    }
}

fn commit(base: Option<BlobId>, changes: Vec<Change>) -> Op {
    Op::Commit {
        base,
        message: "work".into(),
        changes,
    }
}

impl Net {
    async fn read(&self, path: &str, offset: u64, len: u64) -> files::Contents {
        let asked = Query::Read {
            path: path.into(),
            at: None,
            offset,
            len,
        };
        let Reply::Contents(contents) = self.ask(files::PROGRAM, &asked).await else {
            panic!()
        };
        contents
    }
}

#[test]
fn files_names_held_chunks_and_never_takes_bytes_in_a_transaction() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let ada = public(ADA_KEY);
        let create = identity::Op::Create {
            name: "Ada".into(),
            scheme: Scheme::Ed25519,
        };
        net.apply(&ada, identity::PROGRAM, &create).await;

        let mut big = vec![b'a'; CHUNK_SIZE as usize];
        big.extend_from_slice(b"tail");
        let big = net.chunks(&big).await;
        let hello = net.chunks(b"hello").await;
        let snapshot: BlobId = abi::decode(
            &net.apply(
                &ada,
                files::PROGRAM,
                &commit(None, vec![put("/shared/big", big), put("/shared/a", hello)]),
            )
            .await,
        )
        .unwrap();
        let across = net.read("/shared/big", CHUNK_SIZE - 2, 10).await;
        assert_eq!(
            (across.bytes.as_slice(), across.eof),
            (&b"aatail"[..], true)
        );
        assert_eq!(across.snapshot, snapshot);
        assert_eq!(net.read("/shared/a", 0, 100).await.bytes, b"hello");

        let unheld = Content::Chunks {
            size: 5,
            chunks: vec![program::blob_id(abi::HashKind::Sha256, files::CHUNK, b"never").unwrap()],
        };
        let refused = commit(Some(snapshot), vec![put("/shared/new", unheld)]);
        assert_eq!(
            net.refuse(&ada, files::PROGRAM, &refused).await,
            reason::NOT_FOUND
        );
        let x = net.chunks(b"x").await;
        let under_a_file = commit(Some(snapshot), vec![put("/shared/a/under-a-file", x)]);
        assert_eq!(
            net.refuse(&ada, files::PROGRAM, &under_a_file).await,
            reason::WRONG_STATE
        );
        let Reply::Head(Some(head)) = net.ask(files::PROGRAM, &Query::Head).await else {
            panic!()
        };
        assert_eq!((head.id, head.seq), (snapshot, 1));

        let stranger = public(99);
        let s = net.chunks(b"s").await;
        assert_eq!(
            net.refuse(
                &stranger,
                files::PROGRAM,
                &commit(None, vec![put("/shared/s", s)])
            )
            .await,
            reason::UNAUTHORIZED
        );

        let mine = net.chunks(b"mine").await;
        net.apply(
            &ada,
            files::PROGRAM,
            &commit(Some(snapshot), vec![put("/home/Ada/notes", mine)]),
        )
        .await;
        assert_eq!(net.read("/home/Ada/notes", 0, 100).await.bytes, b"mine");
        let theirs = net.chunks(b"theirs").await;
        net.apply(
            &ada,
            files::PROGRAM,
            &commit(None, vec![put("/home/Bo/notes", theirs)]),
        )
        .await;
        assert_eq!(net.read("/home/Bo/notes", 0, 100).await.bytes, b"theirs");
        let root = net.chunks(b"root").await;
        assert_eq!(
            net.refuse(
                &ada,
                files::PROGRAM,
                &commit(None, vec![put("/home/Bo", root)])
            )
            .await,
            reason::UNAUTHORIZED
        );
    });
}

#[test]
fn a_write_list_on_a_shared_folder_lets_its_creator_and_the_listed_accounts_write_under_it() {
    deterministic::Runner::default().start(|context| async move {
        let dir = tempfile::tempdir().unwrap();
        let mut net = Net::found_with(context, dir.path(), apps(), Vec::new(), MEMBER_CAP).await;
        let mut numbers = Vec::new();
        for (key, name) in [(ADA_KEY, "Ada"), (BO_KEY, "Bo"), (CY_KEY, "Cy")] {
            let create = identity::Op::Create {
                name: name.into(),
                scheme: Scheme::Ed25519,
            };
            let number: u64 =
                abi::decode(&net.apply(&public(key), identity::PROGRAM, &create).await).unwrap();
            numbers.push(number);
        }
        let [ada, bo, _] = numbers[..] else { panic!() };
        let plan = net.chunks(b"ada").await;
        let first: BlobId = abi::decode(
            &net.apply(
                &public(ADA_KEY),
                files::PROGRAM,
                &commit(None, vec![put("/shared/team/plan", plan)]),
            )
            .await,
        )
        .unwrap();
        let team = access::Op::Set {
            program: files::PROGRAM.into(),
            key: "/shared/team".into(),
            list: Some(Ownership::Listed(vec![Party::Account(bo)])),
        };
        assert_eq!(
            net.refuse(&public(BO_KEY), access::PROGRAM, &team).await,
            reason::UNAUTHORIZED,
            "only the folder's creator lists its writers"
        );
        net.apply(&public(ADA_KEY), access::PROGRAM, &team).await;
        let asked = access::Query::Writers {
            program: files::PROGRAM.into(),
            key: "/shared/team/plan".into(),
        };
        let access::Reply::Writers(writers) = net.ask(access::PROGRAM, &asked).await else {
            panic!()
        };
        assert_eq!(
            writers,
            ::program::Writers::Listed {
                at: "/shared/team".into(),
                owner: Some(Party::Account(ada)),
                list: Ownership::Listed(vec![Party::Account(bo)]),
            }
        );
        let bo_plan = net.chunks(b"bo").await;
        net.apply(
            &public(BO_KEY),
            files::PROGRAM,
            &commit(Some(first), vec![put("/shared/team/plan", bo_plan)]),
        )
        .await;
        assert_eq!(net.read("/shared/team/plan", 0, 100).await.bytes, b"bo");
        let cy = net.chunks(b"cy").await;
        assert_eq!(
            net.refuse(
                &public(CY_KEY),
                files::PROGRAM,
                &commit(None, vec![put("/shared/team/cy", cy)])
            )
            .await,
            reason::UNAUTHORIZED
        );
    });
}
