//! The fixture story, natively: the same objects real git built through
//! the harness (Ada, one fixed second, the same files), pushed as one op,
//! then heights that move like the kernel's.

use super::*;
use forge::{ChangeFilter, Query, ReviewDraft, Revision, Verdict};
use gitcore::Tag;

pub const REPO: &str = "project";
pub const TESTER: &[u8] = b"tester";
/// The story's accounts: the tester is account 1 and the reviewer 2; a
/// talker in chat is 4. Every other key holds no account.
pub const HELD: [(&[u8], u64); 3] = [(TESTER, 1), (b"reviewer", 2), (b"talker", 4)];

/// forge over `MemorySandbox` with the kernel's height discipline: an
/// op lands in a new block, and what it emits to chat runs in its frame; a
/// chat write is a block of its own; a query moves nothing.
pub struct Rig {
    pub sandbox: MemorySandbox,
    pub height: u64,
    pub actor: Vec<u8>,
}

impl Rig {
    pub fn start(bounds: Bounds, hash: HashKind) -> Rig {
        let sandbox = MemorySandbox::default();
        for (key, account) in HELD {
            sandbox.hold(key, account);
        }
        Forge::init(&sandbox.exec(0), &abi::encode(&bounds)).unwrap();
        let mut rig = Rig {
            sandbox,
            height: 0,
            actor: TESTER.to_vec(),
        };
        rig.execute(&Op::Create {
            repo: REPO.into(),
            hash,
        })
        .unwrap();
        rig
    }

    /// Who `key` signs as; it holds an account.
    pub fn principal(&self, key: &[u8]) -> Principal {
        self.sandbox
            .principal(key)
            .expect("the key holds an account")
    }

    pub fn advance(&mut self) {
        self.height += 1;
    }

    /// The actor's op in a new block, and what it emitted run in its frame;
    /// a refusal left forge's store as it was.
    #[track_caller]
    pub fn execute(&mut self, op: &Op) -> Result<Vec<u8>, program::Error> {
        self.advance();
        let (actor, height) = (self.actor.clone(), self.height);
        self.sandbox
            .forge
            .attempt(|| signed_op(&self.sandbox, &actor, height, op))
    }

    /// The refusal of the actor's op, which left forge's store as it was.
    #[track_caller]
    pub fn refused(&mut self, op: &Op) -> program::Error {
        self.advance();
        let (actor, height) = (self.actor.clone(), self.height);
        self.sandbox
            .forge
            .refused(|| signed_op(&self.sandbox, &actor, height, op))
    }

    pub fn query(&self, query: &Query) -> Result<Vec<u8>, program::Error> {
        Forge::answer(&self.sandbox.reads(self.height), query.clone())
    }

    /// `principal`'s chat message in a new block, signed by the key that
    /// holds its account.
    pub fn chat_execute(&mut self, principal: Principal, msg: chat::Op) {
        self.advance();
        let origin = match principal {
            Principal::Account(number) => match sandbox::program_of(number) {
                Some(program) => Origin::Program(program.into()),
                None => {
                    let roster = self.sandbox.chain.roster();
                    let (key, _) = roster
                        .keys
                        .iter()
                        .find(|(_, held)| **held == number)
                        .expect("a key holds the account");
                    Origin::Signed(key.clone())
                }
            },
            Principal::Root => Origin::Root,
            Principal::Foreign(foreign) => {
                panic!("{foreign} signs nothing here: it arrives through crosspond")
            }
        };
        self.sandbox
            .chat_execute(origin, self.height, TIME, msg)
            .unwrap();
    }
}

pub struct Story {
    pub objects: MemoryObjects,
    pub root: String,
    pub feature: String,
    pub clean: String,
    pub conflict: String,
    pub unrelated: String,
}

fn entry(mode: Mode, name: &str, id: Oid) -> TreeEntry {
    TreeEntry {
        mode,
        name: name.as_bytes().to_vec(),
        id,
    }
}

fn put_tree(store: &mut MemoryObjects, entries: Vec<TreeEntry>) -> Oid {
    store
        .put(Kind::Tree, &Tree { entries }.serialize())
        .unwrap()
}

impl Story {
    /// Git's history, object for object: `Base` on main with a `v1` tag,
    /// `feature` editing every kind of file, `clean` and `conflict` off main,
    /// an orphan `unrelated` over `conflict`'s tree.
    pub fn pushed(rig: &mut Rig) -> Story {
        let mut store = MemoryObjects::new(Hash::Sha1);
        let s = &mut store;
        let readme = blob(s, b"# Project\n");
        let gone = blob(s, b"remove me\n");
        let image = blob(s, &[0, 1, 2, 3]);
        let large = blob(s, &[b'x'; 257]);
        let mode_sh = blob(s, b"echo ok\n");
        let base_lib = blob(s, b"one\nkeep\nold\nend\n");
        let base_src = put_tree(s, vec![entry(Mode::Regular, "lib.rs", base_lib)]);
        let base = [
            entry(Mode::Regular, "README.md", readme),
            entry(Mode::Regular, "gone.txt", gone),
            entry(Mode::Regular, "image.bin", image),
            entry(Mode::Regular, "large.txt", large),
            entry(Mode::Regular, "mode.sh", mode_sh),
        ];
        let mut base_entries = base.to_vec();
        base_entries.push(entry(Mode::Directory, "src", base_src));
        let base_tree = put_tree(s, base_entries);
        let root = commit(s, base_tree, &[], TIME as i64, "Base");
        let tag = Tag {
            object: root,
            kind: Kind::Commit,
            name: b"v1".to_vec(),
            tagger: Some(signature(TIME as i64)),
            message: b"Version one\n".to_vec(),
        };
        let v1 = s.put(Kind::Tag, &tag.serialize()).unwrap();
        let feature_lib = blob(s, b"one\nkeep\n++ x\nend\nadded");
        let feature_src = put_tree(s, vec![entry(Mode::Regular, "lib.rs", feature_lib)]);
        let empty = blob(s, b"");
        let image_edited = blob(s, &[0, 9, 2, 3]);
        let large_edited = blob(s, &[b'y'; 258]);
        let new = blob(s, b"new\n");
        let feature_tree = put_tree(
            s,
            vec![
                entry(Mode::Regular, "README.md", readme),
                entry(Mode::Regular, "empty.txt", empty),
                entry(Mode::Regular, "image.bin", image_edited),
                entry(Mode::Regular, "large.txt", large_edited),
                entry(Mode::Executable, "mode.sh", mode_sh),
                entry(Mode::Regular, "new.txt", new),
                entry(Mode::Directory, "src", feature_src),
                entry(Mode::Gitlink, "vendor", root),
            ],
        );
        let feature = commit(
            s,
            feature_tree,
            &[root],
            TIME as i64,
            "Feature\n\nReview these bytes.",
        );
        let ours = blob(s, b"ours\n");
        let mut clean_entries = base.to_vec();
        clean_entries.push(entry(Mode::Regular, "ours.txt", ours));
        clean_entries.push(entry(Mode::Directory, "src", base_src));
        let clean_tree = put_tree(s, clean_entries);
        let clean = commit(s, clean_tree, &[root], TIME as i64, "ours.txt");
        let conflict_lib = blob(s, b"one\nkeep\nconflict\nend\n");
        let conflict_src = put_tree(s, vec![entry(Mode::Regular, "lib.rs", conflict_lib)]);
        let mut conflict_entries = base.to_vec();
        conflict_entries.push(entry(Mode::Directory, "src", conflict_src));
        let conflict_tree = put_tree(s, conflict_entries);
        let conflict = commit(s, conflict_tree, &[root], TIME as i64, "src/lib.rs");
        let unrelated = commit(s, conflict_tree, &[], TIME as i64, "Unrelated root");
        let zero = Oid::Sha1([0; 20]);
        let commands = [
            (zero, root, "refs/heads/main"),
            (zero, feature, "refs/heads/feature"),
            (zero, clean, "refs/heads/clean"),
            (zero, conflict, "refs/heads/conflict"),
            (zero, unrelated, "refs/heads/unrelated"),
            (zero, v1, "refs/tags/v1"),
        ];
        let pack = held_blob(
            &rig.sandbox,
            forge::PACK_KIND,
            &pack_of(&store, &all_ids(&store)),
        );
        rig.execute(&Op::Push {
            repo: REPO.into(),
            request: push_request(&commands),
            pack: Some(pack),
        })
        .unwrap();
        forget(&rig.sandbox, pack);
        Story {
            objects: store,
            root: root.to_hex(),
            feature: feature.to_hex(),
            clean: clean.to_hex(),
            conflict: conflict.to_hex(),
            unrelated: unrelated.to_hex(),
        }
    }

    /// `git rev-parse feature:<path>` for a file at the feature tip.
    pub fn oid(&self, path: &str) -> String {
        let feature = Oid::from_hex(Hash::Sha1, self.feature.as_bytes()).unwrap();
        let commit = Commit::parse(
            &self.objects.get(&feature).unwrap().unwrap().body,
            Hash::Sha1,
        )
        .unwrap();
        let mut tree = commit.tree;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let parsed =
                Tree::parse(&self.objects.get(&tree).unwrap().unwrap().body, Hash::Sha1).unwrap();
            let found = parsed
                .entries
                .iter()
                .find(|e| e.name == part.as_bytes())
                .unwrap_or_else(|| panic!("{path}: no {part}"))
                .id;
            if parts.peek().is_none() {
                return found.to_hex();
            }
            tree = found;
        }
        unreachable!()
    }

    /// `commit_file` then `git push feature`: one more commit on the feature
    /// tip carrying `name`, pushed with only the new objects.
    pub fn push_follow_up(&mut self, rig: &mut Rig, name: &str, content: &[u8]) -> String {
        let old = Oid::from_hex(Hash::Sha1, self.feature.as_bytes()).unwrap();
        let commit_bytes = self.objects.get(&old).unwrap().unwrap().body;
        let parsed = Commit::parse(&commit_bytes, Hash::Sha1).unwrap();
        let mut tree = Tree::parse(
            &self.objects.get(&parsed.tree).unwrap().unwrap().body,
            Hash::Sha1,
        )
        .unwrap();
        let s = &mut self.objects;
        let file = blob(s, content);
        tree.entries.push(entry(Mode::Regular, name, file));
        let new_tree = put_tree(s, tree.entries);
        let tip = commit(s, new_tree, &[old], TIME as i64, name);
        let pack = held_blob(
            &rig.sandbox,
            forge::PACK_KIND,
            &pack_of(s, &[file, new_tree, tip]),
        );
        rig.execute(&Op::Push {
            repo: REPO.into(),
            request: push_request(&[(old, tip, "refs/heads/feature")]),
            pack: Some(pack),
        })
        .unwrap();
        forget(&rig.sandbox, pack);
        self.feature = tip.to_hex();
        tip.to_hex()
    }

    pub fn open(&self, title: &str) -> Op {
        Op::ChangeOpen {
            repo: REPO.into(),
            from: reference("feature"),
            into: b"refs/heads/main".to_vec(),
            title: title.into(),
            body: "The author's body.".into(),
            reviewers: vec![Principal::Account(2)],
        }
    }
}

pub fn reference(name: &str) -> Revision {
    Revision::Ref(format!("refs/heads/{name}").into_bytes())
}

pub fn change(n: u64) -> Query {
    Query::Change {
        repo: REPO.into(),
        n,
        page: PageRequest::first(128),
    }
}

pub fn changes() -> Query {
    Query::Changes {
        repo: REPO.into(),
        filter: ChangeFilter::default(),
        page: PageRequest::first(128),
    }
}

pub fn compare(from: &str, into: &str) -> Query {
    Query::Compare {
        repo: REPO.into(),
        from: reference(from),
        into: reference(into),
    }
}

pub fn review(feature: &str, root: &str, verdict: Verdict) -> Op {
    Op::ReviewSubmit {
        repo: REPO.into(),
        n: 1,
        review: ReviewDraft {
            commit_oid: feature.into(),
            base_oid: Some(root.into()),
            verdict,
            body: "Review body".into(),
            comments: vec![
                forge::LineComment {
                    path: b"src/lib.rs".to_vec(),
                    side: forge::Side::Old,
                    line: 3,
                    body: "Old side".into(),
                },
                forge::LineComment {
                    path: b"src/lib.rs".to_vec(),
                    side: forge::Side::New,
                    line: 2,
                    body: "Context is commentable".into(),
                },
            ],
        },
    }
}

/// The bounds the harness captured with: its defaults, blobs inline to 64 bytes.
pub fn fixture_bounds() -> Bounds {
    Bounds {
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
