mod common;
use abi::role::identity::Standing;
use common::*;
use forge::Content;

fn owner_of(sandbox: &MemorySandbox, repo: &str) -> Principal {
    let asked = Query::Repo { repo: repo.into() };
    let Reply::Repo { repo, .. } = abi::decode(&ask(sandbox, &asked).unwrap()).unwrap() else {
        panic!("a repo answers Repo");
    };
    repo.repo.owner
}

#[test]
fn founding_requires_bounds_and_an_op_acts_as_an_account() {
    let sandbox = MemorySandbox::default();
    let unfounded = Forge::init(&sandbox.exec(1), b"").unwrap_err();
    assert_eq!(unfounded.code, code::INVALID_INPUT);
    sandbox.chain.init("forge", &bounds()).unwrap();

    let create = Op::Create {
        repo: "r".into(),
        hash: HashKind::Sha1,
    };
    for origin in [Origin::Root, Origin::Signed(b"no-account".to_vec())] {
        let refusal = sandbox
            .forge
            .refused(|| sandbox.frame(origin, 1, TIME, &create));
        assert_eq!(refusal.code, code::UNAUTHORIZED);
    }
    sandbox
        .frame(Origin::Program("chat".into()), 1, TIME, &create)
        .unwrap();
    let chat = sandbox::PROGRAMS[1].1;
    assert_eq!(owner_of(&sandbox, "r"), Principal::Account(chat));
}

#[test]
fn an_agent_creates_and_pushes_through_the_program_that_runs_it() {
    const AGENT: u64 = 20;
    let sandbox = founded();
    sandbox.agent(AGENT, Standing::Active);
    let create = Op::Create {
        repo: "notes".into(),
        hash: HashKind::Sha1,
    };
    sandbox
        .forge
        .attempt(|| sandbox.run_for(AGENT, 1, create))
        .unwrap();
    assert_eq!(owner_of(&sandbox, "notes"), Principal::Account(AGENT));

    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("README.md", b"# Notes\n")]);
    let pack_bytes = pack_of(&source, &all_ids(&source));
    let pushed = with_pack(&sandbox, &pack_bytes, |pack| {
        let op = Op::Push {
            repo: "notes".into(),
            request: push_request(&[(Hash::Sha1.zero(), tip, "refs/heads/main")]),
            pack,
        };
        sandbox.forge.attempt(|| sandbox.run_for(AGENT, 2, op))
    });
    pushed.unwrap();
    assert_eq!(refs_of(&sandbox, "notes")["refs/heads/main"], tip.to_hex());
}

#[test]
fn a_git_wire_query_answers_raw_bytes_and_is_no_reply() {
    let sandbox = MemorySandbox::default();
    sandbox.chain.init("forge", &bounds()).unwrap();
    let advertise = Query::Advertise {
        repo: "r".into(),
        service: Service::UploadPack,
    };
    let refusal = Forge::query(&sandbox.reads(1), advertise.clone()).unwrap_err();
    assert_eq!(refusal.code, code::INVALID_INPUT);
    let answered = Forge::answer(&sandbox.reads(1), advertise).unwrap_err();
    assert_eq!(answered.code, code::NOT_FOUND);
    let repos = Query::Repos {
        page: PageRequest::first(2),
    };
    let bytes = Forge::answer(&sandbox.reads(1), repos.clone()).unwrap();
    assert_eq!(
        abi::decode::<Reply>(&bytes).unwrap(),
        Forge::query(&sandbox.reads(1), repos).unwrap()
    );
}

#[test]
fn create_names_an_owner_and_refuses_bad_or_taken_names() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let again = act(
        &sandbox,
        WRITER,
        &Op::Create {
            repo: "project".into(),
            hash: HashKind::Sha1,
        },
    );
    assert_eq!(again.unwrap_err().code, code::ALREADY_EXISTS);
    for bad in ["", "a/b", ".hidden", "x.git", "sp ace"] {
        let refused = act(
            &sandbox,
            OWNER,
            &Op::Create {
                repo: bad.into(),
                hash: HashKind::Sha1,
            },
        );
        assert_eq!(refused.unwrap_err().code, code::INVALID_INPUT, "{bad:?}");
    }
    let listed: Reply = abi::decode(
        &ask(
            &sandbox,
            &Query::Repos {
                page: PageRequest::first(128),
            },
        )
        .unwrap(),
    )
    .unwrap();
    let Reply::Repos { page, .. } = listed else {
        panic!();
    };
    let repos = page.items;
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].name, "project");
    assert_eq!(repos[0].repo.owner, person(OWNER));
    assert_eq!(repos[0].repo.settings, Settings::default());
}

#[test]
fn anyone_pushes_until_the_owner_lists_writers() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let zero = Hash::Sha1.zero();
    let pack_bytes = pack_of(&source, &all_ids(&source));
    let open = [(zero, tip, "refs/heads/open")];
    let commands = [(zero, tip, "refs/heads/main")];

    let unlisted = push(&sandbox, STRANGER, "project", &open, &pack_bytes).unwrap();
    assert_eq!(unlisted, ["unpack ok", "ok refs/heads/open"]);

    sandbox.list_writers(OWNER, "project", &[]).unwrap();
    let stranger = push(&sandbox, STRANGER, "project", &commands, &pack_bytes);
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);

    let listed_by_stranger = sandbox.list_writers(STRANGER, "project", &[person(WRITER)]);
    assert_eq!(listed_by_stranger.unwrap_err().code, code::UNAUTHORIZED);

    sandbox
        .list_writers(OWNER, "project", &[person(WRITER)])
        .unwrap();
    let writer = push(&sandbox, WRITER, "project", &commands, &pack_bytes).unwrap();
    assert_eq!(writer, ["unpack ok", "ok refs/heads/main"]);

    sandbox.list_writers(OWNER, "project", &[]).unwrap();
    let revoked = push(&sandbox, WRITER, "project", &commands, &pack_bytes);
    assert_eq!(revoked.unwrap_err().code, code::UNAUTHORIZED);
}

#[test]
fn a_non_fast_forward_is_reported_and_not_applied_unless_allowed() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let root = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let left = file_commit(&mut source, &[root], 2, &[("a", b"left")]);
    let right = file_commit(&mut source, &[root], 3, &[("a", b"right")]);
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));

    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, left, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let rewound = push(
        &sandbox,
        OWNER,
        "project",
        &[(left, right, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(
        rewound,
        ["unpack ok", "ng refs/heads/main non-fast-forward"]
    );
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        left.to_hex()
    );

    let deleted = push(
        &sandbox,
        OWNER,
        "project",
        &[(left, zero, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(
        deleted,
        ["unpack ok", "ng refs/heads/main deletion prohibited"]
    );

    act(
        &sandbox,
        OWNER,
        &Op::Configure {
            repo: "project".into(),
            settings: Settings {
                head: b"refs/heads/main".to_vec(),
                allow_force: true,
                allow_delete: true,
            },
        },
    )
    .unwrap();
    let forced = push(
        &sandbox,
        OWNER,
        "project",
        &[(left, right, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(forced, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        right.to_hex()
    );
    let deleted = push(
        &sandbox,
        OWNER,
        "project",
        &[(right, zero, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(deleted, ["unpack ok", "ok refs/heads/main"]);
    assert!(refs_of(&sandbox, "project").is_empty());
}

#[test]
fn an_import_arrives_as_fast_forward_steps_and_an_open_pack_is_refused_whole() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let first = file_commit(&mut source, &[], 1, &[("a", b"1"), ("b", b"1")]);
    let after_first: BTreeSet<Oid> = source.ids().copied().collect();
    let second = file_commit(&mut source, &[first], 2, &[("a", b"2"), ("b", b"1")]);
    let third = file_commit(&mut source, &[second], 3, &[("a", b"3"), ("b", b"1")]);
    let later: Vec<Oid> = source
        .ids()
        .copied()
        .filter(|id| !after_first.contains(id))
        .collect();
    let zero = Hash::Sha1.zero();

    let open = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, third, "refs/heads/main")],
        &pack_of(&source, &later),
    )
    .unwrap();
    let missing = open[0]
        .strip_prefix("unpack missing object ")
        .map(|hex| Oid::from_hex(Hash::Sha1, hex).unwrap())
        .unwrap();
    assert!(after_first.contains(&missing), "{}", open[0]);
    assert_eq!(open[1], "ng refs/heads/main n/a (unpacker error)");
    assert_eq!(sandbox.blob_count(), 0);
    assert!(refs_of(&sandbox, "project").is_empty());

    let step_one = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, first, "refs/heads/main")],
        &pack_of(&source, &after_first.iter().copied().collect::<Vec<_>>()),
    )
    .unwrap();
    assert_eq!(step_one, ["unpack ok", "ok refs/heads/main"]);
    let step_two = push(
        &sandbox,
        OWNER,
        "project",
        &[(first, third, "refs/heads/main")],
        &pack_of(&source, &later),
    )
    .unwrap();
    assert_eq!(step_two, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(sandbox.blob_count(), source.count());
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        third.to_hex()
    );
}

#[test]
fn a_walk_past_the_bound_asks_for_smaller_steps() {
    let sandbox = MemorySandbox::default();
    Forge::init(
        &sandbox.exec(1),
        &abi::encode(&Bounds {
            push_walk: 2,
            ..bounds()
        }),
    )
    .unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let mut tip = file_commit(&mut source, &[], 1, &[("a", b"0")]);
    let root = tip;
    for step in 1..5u8 {
        tip = file_commit(&mut source, &[tip], 1 + step as i64, &[("a", &[step])]);
    }
    let zero = Hash::Sha1.zero();
    let everything = pack_of(&source, &all_ids(&source));
    push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, root, "refs/heads/main")],
        &everything,
    )
    .unwrap();
    let too_far = push(
        &sandbox,
        OWNER,
        "project",
        &[(root, tip, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(
        too_far,
        [
            "unpack ok",
            "ng refs/heads/main history too long to verify; push in smaller steps"
        ]
    );
}

#[test]
fn a_cursor_outlives_empty_blocks_and_a_forge_write_restarts_it() {
    use common::story::*;
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let log = |after| Query::Log {
        repo: REPO.into(),
        from: forge::Revision::Oid(story.feature.clone()),
        exclude: None,
        page: PageRequest {
            after,
            limit: Some(1),
        },
    };
    let page = |rig: &Rig, after| match abi::decode(&rig.query(&log(after)).unwrap()).unwrap() {
        Reply::Log { page, .. } => page,
        _ => panic!("a log"),
    };
    let first = page(&rig, None);
    assert_eq!(first.items[0].oid, story.feature);
    rig.advance();
    rig.advance();
    let second = page(&rig, first.next.clone());
    assert_eq!(second.items[0].oid, story.root);
    rig.execute(&Op::Configure {
        repo: REPO.into(),
        settings: Settings::default(),
    })
    .unwrap();
    assert_eq!(rig.query(&log(first.next)).unwrap_err().code, code::STALE);
}

#[test]
fn a_log_leaves_out_what_its_exclude_reaches() {
    use common::story::*;
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let bytes = rig
        .query(&Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: Some(reference("main")),
            page: PageRequest::first(64),
        })
        .unwrap();
    let Reply::Log { page, .. } = abi::decode(&bytes).unwrap() else {
        panic!("a log");
    };
    let oids: Vec<_> = page.items.into_iter().map(|c| c.oid).collect();
    assert_eq!(oids, [story.feature]);
}

#[test]
fn a_forge_write_at_the_answering_height_restarts_the_walk() {
    use common::story::*;
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    let log = |after| Query::Log {
        repo: REPO.into(),
        from: reference("feature"),
        exclude: None,
        page: PageRequest {
            after,
            limit: Some(1),
        },
    };
    let Reply::Log { page, .. } = abi::decode(&rig.query(&log(None)).unwrap()).unwrap() else {
        panic!("a log");
    };
    assert_eq!(page.items[0].oid, story.feature);
    let (actor, height) = (rig.actor.clone(), rig.height);
    signed_op(
        &rig.sandbox,
        &actor,
        height,
        &Op::Configure {
            repo: REPO.into(),
            settings: Settings::default(),
        },
    )
    .unwrap();
    assert_eq!(rig.query(&log(page.next)).unwrap_err().code, code::STALE);
}

#[test]
fn a_log_exclude_spends_its_own_walk_budget() {
    use common::story::*;
    let mut rig = Rig::start(
        forge::Bounds {
            log_walk: 2,
            ..bounds()
        },
        HashKind::Sha1,
    );
    let story = Story::pushed(&mut rig);
    let bytes = rig
        .query(&Query::Log {
            repo: REPO.into(),
            from: reference("feature"),
            exclude: Some(reference("unrelated")),
            page: PageRequest::first(64),
        })
        .unwrap();
    let Reply::Log { page, .. } = abi::decode(&bytes).unwrap() else {
        panic!("a log");
    };
    let oids: Vec<_> = page.items.into_iter().map(|c| c.oid).collect();
    assert_eq!(oids, [story.feature, story.root]);
}

fn stage(sandbox: &MemorySandbox, actor: &[u8], pack: Vec<u8>) -> Result<Vec<u8>, program::Error> {
    with_pack(sandbox, &pack, |pack| {
        let op = Op::Stage {
            repo: "project".into(),
            pack: pack.expect("a stage carries objects"),
        };
        act(sandbox, actor, &op)
    })
}

fn objects_of(source: &MemoryObjects, commit: Oid) -> (Vec<Oid>, Oid) {
    let object = source.get(&commit).unwrap().unwrap();
    let parsed = Commit::parse(&object.body, Hash::Sha1).unwrap();
    let tree = source.get(&parsed.tree).unwrap().unwrap();
    let mut ids = vec![parsed.tree];
    ids.extend(
        Tree::parse(&tree.body, Hash::Sha1)
            .unwrap()
            .entries
            .iter()
            .map(|entry| entry.id),
    );
    (ids, commit)
}

#[test]
fn a_stage_stores_objects_and_moves_no_ref() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1"), ("b", b"2")]);
    let (below, _) = objects_of(&source, tip);

    let open = stage(&sandbox, OWNER, pack_of(&source, &[tip]));
    assert_eq!(open.unwrap_err().code, code::NOT_FOUND);
    assert_eq!(sandbox.blob_count(), 0);

    stage(&sandbox, OWNER, pack_of(&source, &below)).unwrap();
    assert_eq!(sandbox.blob_count(), below.len());
    stage(&sandbox, OWNER, pack_of(&source, &[tip])).unwrap();
    assert_eq!(sandbox.blob_count(), source.count());
    assert!(refs_of(&sandbox, "project").is_empty());

    let garbage = stage(&sandbox, OWNER, b"PACK not really".to_vec());
    assert_eq!(garbage.unwrap_err().code, code::INVALID_INPUT);
}

#[test]
fn a_push_and_a_stage_read_only_a_held_pack() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let pack = pack_of(&source, &all_ids(&source));
    let commands = [(Hash::Sha1.zero(), tip, "refs/heads/main")];
    let push_with = |request: Vec<u8>, pack: Option<BlobId>| Op::Push {
        repo: "project".into(),
        request,
        pack,
    };

    let unheld = program::blob_id(HashKind::Sha256, forge::PACK_KIND, &pack).unwrap();
    let missing = push_with(push_request(&commands), Some(unheld));
    assert_eq!(refused(&sandbox, OWNER, &missing).code, code::NOT_FOUND);
    let missing = Op::Stage {
        repo: "project".into(),
        pack: unheld,
    };
    assert_eq!(refused(&sandbox, OWNER, &missing).code, code::NOT_FOUND);

    let page = held_blob(&sandbox, "page", &pack);
    let not_a_pack = push_with(push_request(&commands), Some(page));
    assert_eq!(
        refused(&sandbox, OWNER, &not_a_pack).code,
        code::INVALID_INPUT
    );
    forget(&sandbox, page);

    let mut inline = push_request(&commands);
    inline.extend_from_slice(&pack);
    let inline = push_with(inline, None);
    assert_eq!(refused(&sandbox, OWNER, &inline).code, code::INVALID_INPUT);
    assert_eq!(sandbox.blob_count(), 0);
    assert!(refs_of(&sandbox, "project").is_empty());

    let moved = push(&sandbox, OWNER, "project", &commands, &pack).unwrap();
    assert_eq!(moved, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(sandbox.blob_count(), source.count());
}

#[test]
fn anyone_stages_until_the_owner_lists_writers() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let everything = pack_of(&source, &all_ids(&source));

    stage(&sandbox, STRANGER, everything.clone()).unwrap();
    assert_eq!(sandbox.blob_count(), source.count());

    sandbox.list_writers(OWNER, "project", &[]).unwrap();
    let stranger = stage(&sandbox, STRANGER, everything.clone());
    assert_eq!(stranger.unwrap_err().code, code::UNAUTHORIZED);

    sandbox
        .list_writers(OWNER, "project", &[person(WRITER)])
        .unwrap();
    stage(&sandbox, WRITER, everything).unwrap();
    assert_eq!(sandbox.blob_count(), source.count());
}

#[test]
fn a_push_moves_a_ref_onto_staged_objects() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let first = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let second = file_commit(&mut source, &[first], 2, &[("a", b"2")]);
    let zero = Hash::Sha1.zero();

    let (below_first, _) = objects_of(&source, first);
    stage(&sandbox, OWNER, pack_of(&source, &below_first)).unwrap();
    let thin = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, first, "refs/heads/main")],
        &pack_of(&source, &[first]),
    )
    .unwrap();
    assert_eq!(thin, ["unpack ok", "ok refs/heads/main"]);

    let (mut below_second, _) = objects_of(&source, second);
    below_second.retain(|id| !below_first.contains(id));
    below_second.push(second);
    stage(&sandbox, OWNER, pack_of(&source, &below_second)).unwrap();
    let empty = push(
        &sandbox,
        OWNER,
        "project",
        &[(first, second, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(empty, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(
        refs_of(&sandbox, "project")["refs/heads/main"],
        second.to_hex()
    );
    assert_eq!(sandbox.blob_count(), source.count());
}

#[test]
fn a_refused_ref_keeps_the_objects_its_pack_brought() {
    let sandbox = founded();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let tip = file_commit(&mut source, &[], 1, &[("a", b"1")]);
    let zero = Hash::Sha1.zero();
    let stale = push(
        &sandbox,
        OWNER,
        "project",
        &[(tip, tip, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    assert_eq!(stale, ["unpack ok", "ng refs/heads/main stale info"]);
    assert_eq!(sandbox.blob_count(), source.count());
    let again = push(
        &sandbox,
        OWNER,
        "project",
        &[(zero, tip, "refs/heads/main")],
        b"",
    )
    .unwrap();
    assert_eq!(again, ["unpack ok", "ok refs/heads/main"]);
}

fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

#[test]
fn a_commit_bigger_than_one_part_lands_in_parts() {
    const PART: usize = 16 << 20;
    let sandbox = MemorySandbox::default();
    Forge::init(
        &sandbox.exec(1),
        &abi::encode(&Bounds {
            max_object_size: 8 << 20,
            ..bounds()
        }),
    )
    .unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let fonts: Vec<(String, Vec<u8>)> = (0..3)
        .map(|n| (format!("font-{n}.ttf"), noise(6 << 20, n * 2 + 1)))
        .collect();
    let files: Vec<(&str, &[u8])> = fonts
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let tip = file_commit(&mut source, &[], 1, &files);
    let (below, _) = objects_of(&source, tip);
    let (tree, blobs) = below.split_first().unwrap();
    assert!(pack_of(&source, &all_ids(&source)).len() > PART);

    for blob in blobs {
        let part = pack_of(&source, &[*blob]);
        assert!(part.len() < PART);
        stage(&sandbox, OWNER, part).unwrap();
    }
    let last = pack_of(&source, &[*tree, tip]);
    assert!(last.len() < PART);
    let moved = push(
        &sandbox,
        OWNER,
        "project",
        &[(Hash::Sha1.zero(), tip, "refs/heads/main")],
        &last,
    )
    .unwrap();
    assert_eq!(moved, ["unpack ok", "ok refs/heads/main"]);
    assert_eq!(sandbox.blob_count(), source.count());
}

fn history(bounds: Bounds) -> (MemorySandbox, MemoryObjects, [Oid; 5]) {
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
    (sandbox, source, [c1, c2, side, c3, c4])
}

fn closure_page(sandbox: &MemorySandbox, walk: gitcore::walk::Walk) -> Reply {
    let query = Query::Closure {
        repo: "project".into(),
        walk: forge::Walk::of(walk),
    };
    abi::decode(&ask(sandbox, &query).unwrap()).unwrap()
}

fn listed_ids(sandbox: &MemorySandbox, wants: &[Oid], haves: &[Oid]) -> BTreeSet<Oid> {
    closure_of(sandbox, "project", HashKind::Sha1, wants, haves)
        .into_iter()
        .map(|item| item.id)
        .collect()
}

#[test]
fn a_closure_lists_every_object_of_a_history_and_never_its_bytes() {
    let (sandbox, source, [_, _, _, _, c4]) = history(bounds());
    let listed = closure_of(&sandbox, "project", HashKind::Sha1, &[c4], &[]);
    let ids: BTreeSet<Oid> = listed.iter().map(|item| item.id).collect();
    assert_eq!(ids, source.ids().copied().collect());
    assert_eq!(ids.len(), listed.len(), "each object once");
    for item in &listed {
        assert_eq!(source.get(&item.id).unwrap().unwrap().kind, item.kind);
    }
}

#[test]
fn a_closure_stops_at_the_commits_the_client_holds() {
    let (sandbox, source, [_, c2, side, c3, c4]) = history(bounds());
    let (below_c3, _) = objects_of(&source, c3);
    let (below_c4, _) = objects_of(&source, c4);
    let mut expected: BTreeSet<Oid> = [c3, c4].into_iter().collect();
    expected.extend(below_c3);
    expected.extend(below_c4);
    assert_eq!(listed_ids(&sandbox, &[c4], &[c2, side]), expected);
    assert!(listed_ids(&sandbox, &[c4], &[c4]).is_empty());
}

#[test]
fn a_closure_pages_under_the_read_budget_and_lists_the_same_objects() {
    let (sandbox, _, [.., c4]) = history(bounds());
    let whole = listed_ids(&sandbox, &[c4], &[]);
    let (sandbox, _, [.., c4]) = history(Bounds {
        tree_walk: 3,
        ..bounds()
    });
    let first = closure_page(&sandbox, gitcore::walk::Walk::of(&[c4], &[]));
    let Reply::Closure { listed, next, .. } = first else {
        panic!("a closure answers Closure");
    };
    assert!(!listed.is_empty());
    assert!(!next.parse(HashKind::Sha1).unwrap().is_done(), "{next:?}");
    assert_eq!(listed_ids(&sandbox, &[c4], &[]), whole);
}

#[test]
fn a_closure_names_each_object_by_the_blob_the_node_holds_it_as() {
    let (sandbox, _, [.., c4]) = history(bounds());
    for item in closure_of(&sandbox, "project", HashKind::Sha1, &[c4], &[]) {
        let id = forge::blob_id_of(&item.id);
        let held = sandbox.forge.borrow().blobs.get(&id).cloned();
        let blob = held.unwrap_or_else(|| panic!("{} is held as {id:?}", item.id));
        let framed = gitcore::oid_of(Hash::Sha1, item.kind, &blob.body).unwrap();
        assert_eq!(framed, item.id);
    }
}

fn history_of(
    sandbox: &MemorySandbox,
    source: &MemoryObjects,
    wants: &[Oid],
    haves: &[Oid],
) -> BTreeSet<Oid> {
    let mut listing = Listing::of(wants, haves);
    while let Some(walk) = listing.pending() {
        let query = Query::History {
            repo: "project".into(),
            walk: forge::Walk::of(walk),
        };
        let reply = abi::decode(&ask(sandbox, &query).unwrap()).unwrap();
        let Reply::Closure { listed, next, .. } = reply else {
            panic!("a history answers Closure");
        };
        listing.receive(Closed {
            listed: listed
                .iter()
                .map(|item| item.parse(HashKind::Sha1).unwrap())
                .collect(),
            next: next.parse(HashKind::Sha1).unwrap(),
        });
    }
    let listed = listing.into_listed();
    let body = |id: &Oid| source.get(id).unwrap().unwrap().body;
    let mut trees = gitcore::walk::Trees::default();
    for item in &listed {
        assert_eq!(
            item.kind,
            Kind::Commit,
            "a history of commits lists commits"
        );
        let commit = Commit::parse(&body(&item.id), Hash::Sha1).unwrap();
        let parents = commit
            .parents
            .iter()
            .map(|parent| Commit::parse(&body(parent), Hash::Sha1).unwrap().tree)
            .collect();
        trees.commit(commit.tree, parents);
    }
    while !trees.is_done() {
        trees.expand(source).unwrap();
    }
    let trees = trees.into_listed();
    listed.iter().chain(&trees).map(|item| item.id).collect()
}

#[test]
fn a_history_and_the_trees_its_client_expands_list_what_a_closure_lists() {
    let (sandbox, source, [_, c2, side, _, c4]) = history(bounds());
    for haves in [vec![], vec![c2, side], vec![c4]] {
        let closure = listed_ids(&sandbox, &[c4], &haves);
        assert_eq!(history_of(&sandbox, &source, &[c4], &haves), closure);
    }
    let (narrow, source, [.., c4]) = history(Bounds {
        tree_walk: 3,
        ..bounds()
    });
    let first = Query::History {
        repo: "project".into(),
        walk: forge::Walk::of(gitcore::walk::Walk::of(&[c4], &[])),
    };
    let Reply::Closure { next, .. } = abi::decode(&ask(&narrow, &first).unwrap()).unwrap() else {
        panic!("a history answers Closure");
    };
    assert!(!next.parse(HashKind::Sha1).unwrap().is_done(), "{next:?}");
    let closure = listed_ids(&narrow, &[c4], &[]);
    assert_eq!(history_of(&narrow, &source, &[c4], &[]), closure);
}

#[test]
fn a_closure_of_an_object_the_node_does_not_hold_is_refused() {
    let (sandbox, _, _) = history(bounds());
    let absent = Oid::from_hex(Hash::Sha1, "ab".repeat(20)).unwrap();
    let query = Query::Closure {
        repo: "project".into(),
        walk: forge::Walk::of(gitcore::walk::Walk::of(&[absent], &[])),
    };
    assert_eq!(ask(&sandbox, &query).unwrap_err().code, code::NOT_FOUND);
}

#[test]
fn a_long_one_sided_edit_shows_and_a_too_long_search_marks_the_file() {
    let sandbox = MemorySandbox::default();
    Forge::init(
        &sandbox.exec(1),
        &abi::encode(&Bounds {
            merge_cost: 1024,
            ..bounds()
        }),
    )
    .unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    let mut source = MemoryObjects::new(Hash::Sha1);
    let line = |n: usize| format!("line {n}\n");
    let text = |range: std::ops::Range<usize>| range.map(line).collect::<String>();
    let mut shuffled = String::new();
    for n in 0..1500 {
        shuffled += &line(n);
        shuffled += "x\n";
    }
    let before = text(0..1500);
    let c1 = file_commit(
        &mut source,
        &[],
        1,
        &[
            ("keep", b"k\n"),
            ("mid", before.as_bytes()),
            ("rewrite", before.as_bytes()),
        ],
    );
    let grown = text(0..700) + &text(5000..6500) + &text(700..1500);
    let c2 = file_commit(
        &mut source,
        &[c1],
        2,
        &[
            ("added", text(0..2000).as_bytes()),
            ("keep", b"k\n"),
            ("mid", grown.as_bytes()),
            ("rewrite", shuffled.as_bytes()),
        ],
    );
    push(
        &sandbox,
        OWNER,
        "project",
        &[(Hash::Sha1.zero(), c2, "refs/heads/main")],
        &pack_of(&source, &all_ids(&source)),
    )
    .unwrap();
    let query = Query::Diff {
        repo: "project".into(),
        base: Some(c1.to_hex()),
        head: c2.to_hex(),
        path: None,
        page: PageRequest::first(128),
    };
    let Reply::Diff { page, .. } = abi::decode(&ask(&sandbox, &query).unwrap()).unwrap() else {
        panic!("diff");
    };
    let file = |name: &str| {
        page.items
            .iter()
            .find(|f| f.new_path.as_deref() == Some(name.as_bytes()))
            .unwrap()
    };
    let (added, mid, rewrite) = (file("added"), file("mid"), file("rewrite"));
    assert_eq!(
        (added.content, added.additions, added.deletions),
        (Content::Text, 2000, 0)
    );
    assert_eq!(
        (mid.content, mid.additions, mid.deletions),
        (Content::Text, 1500, 0)
    );
    assert_eq!(mid.hunks.len(), 1);
    assert_eq!(
        (rewrite.content, rewrite.hunks.len()),
        (Content::Oversize, 0)
    );
    assert_eq!(page.items.len(), 3);
}

fn project(bounds: Bounds) -> MemorySandbox {
    let sandbox = MemorySandbox::default();
    Forge::init(&sandbox.exec(1), &abi::encode(&bounds)).unwrap();
    create(&sandbox, "project", HashKind::Sha1);
    sandbox
}

fn pushed_main(sandbox: &MemorySandbox, source: &MemoryObjects, tip: Oid) {
    let report = push(
        sandbox,
        OWNER,
        "project",
        &[(Hash::Sha1.zero(), tip, "refs/heads/main")],
        &pack_of(source, &all_ids(source)),
    )
    .unwrap();
    assert_eq!(report, ["unpack ok", "ok refs/heads/main"]);
}

#[test]
fn a_tree_wider_than_the_tree_walk_is_listed_in_one_page() {
    let sandbox = project(bounds());
    let mut source = MemoryObjects::new(Hash::Sha1);
    let width = bounds().tree_walk as usize + 10;
    let bodies: Vec<String> = (0..width).map(|n| format!("{n}\n")).collect();
    let names: Vec<String> = (0..width).map(|n| format!("f{n:06}")).collect();
    let files: Vec<(&str, &[u8])> = names
        .iter()
        .zip(&bodies)
        .map(|(name, body)| (name.as_str(), body.as_bytes()))
        .collect();
    let tip = file_commit(&mut source, &[], 1, &files);
    pushed_main(&sandbox, &source, tip);
    let first = Query::Closure {
        repo: "project".into(),
        walk: forge::Walk::of(gitcore::walk::Walk::of(&[tip], &[])),
    };
    let Reply::Closure { listed, next, .. } = abi::decode(&ask(&sandbox, &first).unwrap()).unwrap()
    else {
        panic!("a closure answers Closure");
    };
    let listed: BTreeSet<String> = listed.into_iter().map(|item| item.oid).collect();
    let all: BTreeSet<String> = all_ids(&source).iter().map(Oid::to_hex).collect();
    assert_eq!(listed, all);
    assert!(next.parse(HashKind::Sha1).unwrap().is_done());
}
