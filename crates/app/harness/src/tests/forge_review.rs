//! A review forge asks of an agent reaches it through harness once the agent answers forge. A
//! change's notices reach the conversation that opened it or that one's heir, else the agent's
//! conversation bound to the room the change is mirrored into, else its current one.
use ::program::{HashKind, Ownership, Party};
use gitcore::wire::pktline;
use gitcore::{
    Commit, Hash, Kind, MemoryObjects, Mode, Object, Objects, Oid, Signature, Tree, TreeEntry,
};

use super::forge_commit::forged;
use super::forge_tools::{answered, seeded, turn, world as tooled};
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

fn registered(world: &World, program: &str) {
    let register = identity::Op::RegisterProgram {
        program: program.into(),
    };
    world
        .chain
        .submit(Origin::Root, identity::PROGRAM, &register)
        .unwrap();
}

fn founded(world: &World) -> Oid {
    registered(world, forge::PROGRAM);
    let create = forge::Op::Create {
        repo: REPO.into(),
        hash: HashKind::Sha1,
    };
    world
        .chain
        .submit(signed(ADA), forge::PROGRAM, &create)
        .unwrap();
    let (main, pack) = step(None, "first");
    push(world, pack, Hash::Sha1.zero(), main, "main");
    main
}

fn ruled(world: &mut World, branch: &str, room: &str) {
    world.chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    world
        .chain
        .seat::<forge_review_chat::ForgeReviewChat>(forge_review_chat::PROGRAM);
    registered(world, forge_review_chat::PROGRAM);
    world.chain.init(forge_review_chat::PROGRAM, &()).unwrap();
    let rule = forge_review_chat::Rule {
        branch: branch.into(),
        room: room.into(),
    };
    let rules = forge_review_chat::Op::SetRules { rules: vec![rule] };
    world
        .chain
        .submit(signed(ADA), forge_review_chat::PROGRAM, &rules)
        .unwrap();
}

fn mirroring(world: &mut World, rooms: &[&str]) {
    ruled(world, "j-{}/", "j-{}");
    for room in rooms {
        let create = chat::Op::CreateChannel {
            channel_id: (*room).into(),
            name: (*room).into(),
            post_policy: chat::PostPolicy::Open,
        };
        world
            .chain
            .submit(signed(ADA), chat::PROGRAM, &create)
            .unwrap();
    }
}

fn notices_in(world: &World, thread: u64) -> Vec<String> {
    let chain = &world.chain;
    let bodies = frames(chain, thread).into_iter().chain(held(chain, thread));
    bodies
        .filter_map(|body| match body {
            Body::Post { text, .. } if text.contains("sent you a notice") => Some(text),
            _ => None,
        })
        .collect()
}

fn told(texts: &[String], n: u64, words: &str) -> usize {
    let about = format!("{REPO} #{n}: ");
    texts
        .iter()
        .filter(|text| text.contains(&about) && text.contains(words))
        .count()
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

fn job_room(world: &World, channel: &str) -> u64 {
    let create = chat::Op::CreateChannel {
        channel_id: channel.into(),
        name: channel.into(),
        post_policy: chat::PostPolicy::Open,
    };
    world
        .chain
        .submit(signed(ADA), chat::PROGRAM, &create)
        .unwrap();
    bound_to(world, channel.into())
}

fn bound_to(world: &World, channel: String) -> u64 {
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel { channel },
    };
    let made = world.chain.submit(signed(ADA), PROGRAM, &open).unwrap();
    abi::decode(&made).unwrap()
}

fn opened_by_tool(world: &World, thread: u64) -> (u64, String) {
    let chain = &world.chain;
    world.post(
        ADA,
        "j-9",
        "m1",
        &format!("<@{}> open a change", world.agent),
    );
    chain.at(2, 0);
    learn(chain, Some(WINDOW));
    let dev = seeded(world, thread);
    let open = json!({ "repo": REPO, "from": "dev", "into": "main", "title": "dev's own" });
    turn(chain, 7, &[("o1", "forge_change_open", open)]);
    let n = answered(chain, thread, "o1")["number"].as_u64().unwrap();
    (n, dev)
}

fn commented(world: &World, n: u64, head: &str, body: &str) {
    let review = forge::Op::ReviewSubmit {
        repo: REPO.into(),
        n,
        review: forge::ReviewDraft {
            commit_oid: head.into(),
            base_oid: None,
            verdict: forge::Verdict::Comment,
            body: body.into(),
            comments: Vec::new(),
        },
    };
    world
        .chain
        .submit(signed(ADA), forge::PROGRAM, &review)
        .unwrap();
}

fn notices_outside(world: &World, threads: &[u64]) -> Vec<String> {
    let others = world.threads().into_iter();
    others
        .filter(|row| !threads.contains(&row.id))
        .flat_map(|row| notices_in(world, row.id))
        .collect()
}

#[test]
fn a_reviewer_hears_forges_request_and_each_new_head_once_it_answers_forge() {
    let world = forged(10_000);
    let main = founded(&world);
    let zero = Hash::Sha1.zero();
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
    let open = format!("{REPO} #{n}: Teach other running — opened, review requested");
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

#[test]
fn a_change_mirrored_into_a_room_is_heard_in_the_agents_conversation_bound_there() {
    let mut world = forged(10_000);
    let main = founded(&world);
    mirroring(&mut world, &["j-375", "j-376"]);
    answering(
        &world,
        Ownership::Listed(vec![Party::Program(forge::PROGRAM.into())]),
    );
    let bind = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: "j-375".into(),
        },
    };
    let made = world.chain.submit(signed(ADA), PROGRAM, &bind).unwrap();
    let bound: u64 = abi::decode(&made).unwrap();
    let zero = Hash::Sha1.zero();
    let branches = ["j-375/fix", "j-375/drop", "j-376/fix", "feature"];
    let heads = branches.map(|branch| {
        let (head, pack) = step(Some(main), branch);
        push(&world, pack, zero, head, branch);
        head
    });
    let [fixed, dropped, unbound, unruled] = branches.map(|branch| opened(&world, branch));
    let (tip, pack) = step(Some(heads[0]), "a second look");
    push(&world, pack, heads[0], tip, branches[0]);
    let review = forge::Op::ReviewSubmit {
        repo: REPO.into(),
        n: fixed,
        review: forge::ReviewDraft {
            commit_oid: tip.to_string(),
            base_oid: Some(main.to_string()),
            verdict: forge::Verdict::Comment,
            body: "Reads right.".into(),
            comments: Vec::new(),
        },
    };
    let merge = forge::Op::SquashMerge {
        repo: REPO.into(),
        change: Some(fixed),
        from: forge::Revision::Ref(format!("refs/heads/{}", branches[0]).into_bytes()),
        into: b"refs/heads/main".to_vec(),
        expected_from: tip.to_string(),
        expected_into: main.to_string(),
        message: None,
    };
    let close = forge::Op::ChangeClose {
        repo: REPO.into(),
        n: dropped,
    };
    for op in [review, merge, close] {
        world
            .chain
            .submit(signed(ADA), forge::PROGRAM, &op)
            .unwrap();
    }
    let here = notices_in(&world, bound);
    let opening = "opened, review requested";
    let head = format!("new head {tip}");
    assert_eq!(
        [
            told(&here, fixed, opening),
            told(&here, fixed, &head),
            told(&here, fixed, "commented"),
            told(&here, fixed, "merged into main"),
            told(&here, dropped, opening),
            told(&here, dropped, "closed"),
            here.len(),
        ],
        [1, 1, 1, 1, 1, 1, 6],
        "{here:#?}"
    );
    let elsewhere: Vec<String> = world
        .threads()
        .into_iter()
        .filter(|row| row.id != bound)
        .flat_map(|row| notices_in(&world, row.id))
        .collect();
    assert_eq!(
        [
            told(&elsewhere, unbound, opening),
            told(&elsewhere, unruled, opening),
            elsewhere.len(),
        ],
        [1, 1, 2],
        "{elsewhere:#?}"
    );
}

#[test]
fn a_conversation_bound_to_the_changes_own_room_hears_it_before_the_room_it_is_mirrored_into() {
    let mut world = forged(10_000);
    let main = founded(&world);
    mirroring(&mut world, &["j-375"]);
    answering(
        &world,
        Ownership::Listed(vec![Party::Program(forge::PROGRAM.into())]),
    );
    let bind = |channel: String| -> u64 {
        let open = Op::OpenThread {
            agent: world.agent,
            place: Place::Channel { channel },
        };
        let made = world.chain.submit(signed(ADA), PROGRAM, &open).unwrap();
        abi::decode(&made).unwrap()
    };
    let job = bind("j-375".into());
    let branch = "j-375/fix";
    let (head, pack) = step(Some(main), branch);
    push(&world, pack, Hash::Sha1.zero(), head, branch);
    let n = opened(&world, branch);
    let own = bind(chat::namespace::id(forge::PROGRAM, &format!("{REPO}:{n}")));
    let (tip, pack) = step(Some(head), "a second look");
    push(&world, pack, head, tip, branch);
    let new_head = format!("new head {tip}");
    let (in_own, in_job) = (notices_in(&world, own), notices_in(&world, job));
    assert_eq!(
        [
            told(&in_own, n, &new_head),
            told(&in_job, n, &new_head),
            told(&in_job, n, "opened, review requested"),
        ],
        [1, 0, 1],
        "{in_own:#?}\n{in_job:#?}"
    );
}

#[test]
fn a_change_is_heard_in_the_conversation_that_opened_it_unless_its_own_room_is_bound() {
    let mut world = tooled();
    ruled(&mut world, "dev", "j-mirror");
    answering(
        &world,
        Ownership::Listed(vec![Party::Program(forge::PROGRAM.into())]),
    );
    let mirror = job_room(&world, "j-mirror");
    let job = job_room(&world, "j-9");
    let (n, dev) = opened_by_tool(&world, job);
    commented(&world, n, &dev, "Reads right.");
    let own = bound_to(
        &world,
        chat::namespace::id(forge::PROGRAM, &format!("{REPO}:{n}")),
    );
    commented(&world, n, &dev, "One more look.");
    let (in_job, in_own) = (notices_in(&world, job), notices_in(&world, own));
    let elsewhere = notices_outside(&world, &[job, own]);
    assert_eq!(
        [
            told(&in_job, n, "commented"),
            in_job.len(),
            told(&in_own, n, "commented"),
            in_own.len(),
            notices_in(&world, mirror).len(),
            elsewhere.len(),
        ],
        [1, 1, 1, 1, 0, 0],
        "{in_job:#?}\n{in_own:#?}\n{elsewhere:#?}"
    );
}

#[test]
fn a_change_whose_opener_ended_is_heard_by_its_heir_then_where_it_was_before() {
    let mut world = tooled();
    ruled(&mut world, "dev", "j-mirror");
    answering(
        &world,
        Ownership::Listed(vec![Party::Program(forge::PROGRAM.into())]),
    );
    let mirror = job_room(&world, "j-mirror");
    let job = job_room(&world, "j-9");
    let (n, dev) = opened_by_tool(&world, job);
    let close = |thread: u64| {
        world
            .chain
            .submit(signed(ADA), PROGRAM, &Op::Close { thread })
            .unwrap();
    };
    close(job);
    let heir = bound_to(&world, "j-9".into());
    commented(&world, n, &dev, "For the heir.");
    close(heir);
    commented(&world, n, &dev, "For the mirror.");
    close(mirror);
    commented(&world, n, &dev, "For the current one.");
    let (in_job, in_heir) = (notices_in(&world, job), notices_in(&world, heir));
    let in_mirror = notices_in(&world, mirror);
    let elsewhere = notices_outside(&world, &[job, heir, mirror]);
    assert_eq!(
        [
            in_job.len(),
            told(&in_heir, n, "commented"),
            in_heir.len(),
            told(&in_mirror, n, "commented"),
            in_mirror.len(),
            told(&elsewhere, n, "commented"),
            elsewhere.len(),
        ],
        [0, 1, 1, 1, 1, 1, 1],
        "{in_job:#?}\n{in_heir:#?}\n{in_mirror:#?}\n{elsewhere:#?}"
    );
}
