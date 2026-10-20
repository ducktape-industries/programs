//! forge-review-chat on a MockChain beside chat, ownership and a forge that answers `Change`: a
//! change from a branch a rule matches tells each event in its room and nothing else does, only
//! forge tells it events, and the rules are their first writer's.
use ::program::{Error, ExecCtx, MockChain, MockHost, Origin, Principal, Program, QueryCtx, code};
use chat::message::Status;
use forge::{Change, ChangeState, ReviewCounts, Revision, Verdict, Watched};
use store::{Item, PageRequest, PageResponse};

use crate::*;

const ADA: &[u8] = b"ada-key";
const ADA_ACCOUNT: u64 = 11;
const BO: &[u8] = b"bo-key";
const BO_ACCOUNT: u64 = 13;
const MIRROR_ACCOUNT: u64 = 905;
const DELIVER: Item<Vec<u8>> = Item::new("deliver");

struct Forge;

impl Program for Forge {
    const NAME: &'static str = forge::PROGRAM;
    type Op = forge::Op;
    type Query = forge::Query;
    type Reply = forge::Reply;

    fn execute(ctx: &ExecCtx, op: forge::Op) -> Result<(), Error> {
        let forge::Op::Watch { deliver } = op else {
            return Err(Error::new(code::UNSUPPORTED, "this forge takes Watch only"));
        };
        ctx.env().sent_by(PROGRAM)?;
        DELIVER.put(ctx, &deliver);
        Ok(())
    }

    fn query(_: &QueryCtx, query: forge::Query) -> Result<forge::Reply, Error> {
        let forge::Query::Change { repo, n, .. } = query else {
            return Err(Error::new(
                code::UNSUPPORTED,
                "this forge answers Change only",
            ));
        };
        Ok(forge::Reply::Change {
            height: 1,
            change: change(&repo, n),
            source_head: None,
            target_head: None,
            reviews: PageResponse {
                height: 1,
                items: Vec::new(),
                next: None,
            },
        })
    }
}

fn change(repo: &str, n: u64) -> Change {
    Change {
        n,
        from: branch("j-7/teach"),
        into: b"refs/heads/dev".to_vec(),
        title: format!("Teach {n}"),
        body: String::new(),
        author: Principal::Account(ADA_ACCOUNT),
        state: ChangeState::Open,
        reviewers: Vec::new(),
        created_height: 1,
        updated_height: 1,
        created_time: 0,
        updated_time: 0,
        review_count: 0,
        comment_count: 0,
        verdicts: ReviewCounts::default(),
        merge_oid: None,
        closed_by: None,
        merged_by: None,
        merged_heads: None,
        channel: chat::namespace::id(forge::PROGRAM, &format!("{repo}:{n}")),
        system_seq: 1,
    }
}

fn branch(name: &str) -> Revision {
    Revision::Ref(format!("refs/heads/{name}").into_bytes())
}

fn rule(branch: &str, room: &str) -> Rule {
    Rule {
        branch: branch.into(),
        room: room.into(),
    }
}

fn world() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<ForgeReviewChat>(PROGRAM);
    chain.seat::<Forge>(forge::PROGRAM);
    chain.seat::<chat::Chat>(chat::PROGRAM);
    chain.seat::<ownership::Ownership>(ownership::PROGRAM);
    chain.seat::<access::Access>(access::PROGRAM);
    chain.hold(ADA, ADA_ACCOUNT);
    chain.hold(BO, BO_ACCOUNT);
    chain.register(PROGRAM, MIRROR_ACCOUNT);
    chain.register(forge::PROGRAM, 900);
    chain.init(PROGRAM, &()).unwrap();
    let rooms = [
        ("j-7", chat::PostPolicy::Open),
        ("j-8", chat::PostPolicy::MembersOnly),
        ("j-9", chat::PostPolicy::Open),
    ];
    for (room, post_policy) in rooms {
        let create = chat::Op::CreateChannel {
            channel_id: room.into(),
            name: room.into(),
            post_policy,
        };
        chain
            .submit(Origin::Signed(ADA.to_vec()), chat::PROGRAM, &create)
            .unwrap();
    }
    archive(&chain, "j-9");
    keep_to_herself(&chain);
    set(&chain, ADA, vec![rule("j-{}/", "j-{}")]).unwrap();
    chain
}

fn keep_to_herself(chain: &MockChain) {
    let policy = ownership::Op::Create {
        name: "mine".into(),
        write: ownership::Writers::Owner,
    };
    let made = chain
        .submit(Origin::Signed(ADA.to_vec()), ownership::PROGRAM, &policy)
        .unwrap();
    let policy: ownership::PolicyId = abi::decode(&made).unwrap();
    let default = ownership::Op::SetDefault {
        scope: ownership::Scope::Kind("forge-review-chat.rules".into()),
        policy: Some(policy),
    };
    chain
        .submit(Origin::Signed(ADA.to_vec()), ownership::PROGRAM, &default)
        .unwrap();
}

fn set(chain: &MockChain, key: &[u8], rules: Vec<Rule>) -> Result<Vec<u8>, Error> {
    chain.submit(
        Origin::Signed(key.to_vec()),
        PROGRAM,
        &Op::SetRules { rules },
    )
}

fn archive(chain: &MockChain, room: &str) {
    let archive = chat::Op::SetChannelArchived {
        channel_id: room.into(),
        archived: true,
    };
    chain
        .submit(Origin::Signed(ADA.to_vec()), chat::PROGRAM, &archive)
        .unwrap();
}

fn tell(chain: &MockChain, event: Watched) -> Result<Vec<u8>, Error> {
    chain.submit(
        Origin::Program(forge::PROGRAM.into()),
        PROGRAM,
        &Op::Watched(event),
    )
}

fn opened(n: u64, from: Revision) -> Watched {
    Watched::Opened {
        repo: "project".into(),
        n,
        from,
        into: b"refs/heads/dev".to_vec(),
        head: "1".repeat(40),
        by: Principal::Account(ADA_ACCOUNT),
    }
}

fn reviewed(n: u64) -> Watched {
    Watched::Reviewed {
        repo: "project".into(),
        n,
        id: 1,
        verdict: Verdict::RequestChanges,
        by: Principal::Account(BO_ACCOUNT),
    }
}

fn notices(chain: &MockChain, room: &str) -> Vec<chat::Notice> {
    let query = chat::Query::Roots {
        channel_id: room.into(),
        page: PageRequest::default(),
        viewer: Vec::new(),
    };
    let chat::Reply::Roots(page) = chain.query(chat::PROGRAM, &query).unwrap() else {
        panic!("Roots answers rows");
    };
    page.items
        .iter()
        .rev()
        .filter(|row| row.author == Principal::Account(MIRROR_ACCOUNT))
        .filter_map(|row| row.notice().cloned())
        .collect()
}

fn room(chain: &MockChain, n: u64) -> Option<String> {
    let asked = Query::Room {
        repo: "project".into(),
        n,
    };
    let Reply::Room(room) = chain.query(PROGRAM, &asked).unwrap() else {
        panic!("Room answers a room");
    };
    room
}

#[test]
fn a_change_from_a_branch_a_rule_matches_tells_each_event_in_its_room() {
    let chain = world();
    tell(&chain, opened(1, branch("j-7/teach"))).unwrap();
    tell(&chain, reviewed(1)).unwrap();
    let left = Watched::LeftQueue {
        repo: "project".into(),
        n: 1,
        into: b"refs/heads/dev".to_vec(),
        why: "gates failed".into(),
        queued_by: Principal::Account(ADA_ACCOUNT),
    };
    tell(&chain, left).unwrap();
    assert_eq!(room(&chain, 1).as_deref(), Some("j-7"));
    let merged = Watched::Merged {
        repo: "project".into(),
        n: 1,
        oid: "2".repeat(40),
        by: Principal::Account(ADA_ACCOUNT),
    };
    tell(&chain, merged).unwrap();
    let told = notices(&chain, "j-7");
    let lines: Vec<(&str, &str)> = told
        .iter()
        .map(|notice| (notice.group.as_str(), notice.detail.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (
                "opened project 1",
                "opened by account 11 from j-7/teach; its conversation is in forge:project:1"
            ),
            (
                "review 1 project 1",
                "review 1: changes requested by account 13"
            ),
            (
                "left queue project 1",
                "left the merge queue of dev: gates failed"
            ),
            ("merged project 1", "merged into dev by account 11"),
        ]
    );
    let statuses: Vec<Status> = told.iter().map(|notice| notice.status).collect();
    assert_eq!(
        statuses,
        [
            Status::Running,
            Status::Failed,
            Status::Failed,
            Status::Passed
        ]
    );
    assert!(
        told.iter()
            .all(|notice| notice.subject == "project #1: Teach 1"
                && notice.kind == forge::NOTICE
                && notice.link.is_none()
                && notice.concerns.is_empty()),
        "{told:#?}"
    );
    assert_eq!(room(&chain, 1), None, "a merged change is mirrored no more");
}

#[test]
fn a_change_no_rule_mirrors_or_whose_room_is_closed_to_it_tells_nothing() {
    let chain = world();
    let sources = [
        branch("feature/teach"),
        branch("j-8/teach"),
        branch("j-9/teach"),
        branch("j-10/teach"),
        branch("jx-7/teach"),
        branch("j-7"),
        Revision::Oid("3".repeat(40)),
    ];
    for (n, from) in (1..).zip(sources) {
        tell(&chain, opened(n, from)).unwrap();
        tell(&chain, reviewed(n)).unwrap();
        assert_eq!(room(&chain, n), None, "change {n}");
    }
    for room in ["j-7", "j-8", "j-9"] {
        assert!(notices(&chain, room).is_empty(), "{room}");
    }
}

#[test]
fn a_room_archived_after_the_change_opened_refuses_its_notice_and_the_event_still_applies() {
    let chain = world();
    tell(&chain, opened(1, branch("j-7/teach"))).unwrap();
    archive(&chain, "j-7");
    let closed = Watched::Closed {
        repo: "project".into(),
        n: 1,
        by: Principal::Account(ADA_ACCOUNT),
    };
    tell(&chain, closed).unwrap();
    assert_eq!(notices(&chain, "j-7").len(), 1, "only the opening landed");
    assert_eq!(room(&chain, 1), None);
}

#[test]
fn only_forge_tells_it_an_event() {
    let chain = world();
    let event = Op::Watched(opened(1, branch("j-7/teach")));
    let forged = chain
        .submit(Origin::Signed(ADA.to_vec()), PROGRAM, &event)
        .unwrap_err();
    assert_eq!(forged.code, code::UNAUTHORIZED);
    assert!(notices(&chain, "j-7").is_empty());
}

#[test]
fn it_watches_forge_from_init_under_the_tag_forge_delivers_with() {
    let chain = world();
    let forge = chain.host(forge::PROGRAM);
    let deliver = DELIVER
        .get(&forge.query(MockHost::env(forge::PROGRAM)))
        .unwrap();
    assert_eq!(deliver, Some(watching()));
    let event = Op::Watched(opened(1, branch("j-7/teach")));
    assert_eq!(abi::encode(&event)[..1], watching());
}

#[test]
fn the_rules_are_their_first_writers_and_refuse_a_room_they_cannot_fill() {
    let chain = world();
    let taken = set(&chain, BO, vec![rule("{}/", "{}")]).unwrap_err();
    assert_eq!(taken.code, code::UNAUTHORIZED);
    let unfillable = [
        rule("", "j-{}"),
        rule("j-{}/", ""),
        rule("{}-{}/", "j-{}"),
        rule("release/", "release-{}"),
    ];
    for bad in unfillable {
        let refused = set(&chain, ADA, vec![bad.clone()]).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{bad:?}");
    }
    let rules = vec![rule("release/", "releases"), rule("j-{}/", "j-{}")];
    set(&chain, ADA, rules.clone()).unwrap();
    let held: Reply = chain.query(PROGRAM, &Query::Rules).unwrap();
    assert_eq!(held, Reply::Rules(rules));
}

fn create(chain: &MockChain, room: &str) {
    let create = chat::Op::CreateChannel {
        channel_id: room.into(),
        name: room.into(),
        post_policy: chat::PostPolicy::Open,
    };
    chain
        .submit(Origin::Signed(ADA.to_vec()), chat::PROGRAM, &create)
        .unwrap();
}

#[test]
fn a_rule_fills_its_room_from_one_branch_segment_and_the_first_match_wins() {
    let chain = world();
    for room in ["releases", "team-j-", "team-j-7", "team-feature", "j-171"] {
        create(&chain, room);
    }
    let rules = vec![
        rule("release/", "releases"),
        rule("j-{}/", "j-{}"),
        rule("{}", "team-{}"),
    ];
    set(&chain, ADA, rules).unwrap();
    let cases = [
        ("j-171/forge-review-chat", "j-171"),
        ("release/1.2", "releases"),
        ("release/j-7/a", "releases"),
        ("j-/x", "team-j-"),
        ("j-7", "team-j-7"),
        ("feature/x", "team-feature"),
    ];
    for (n, (name, expected)) in (1..).zip(cases) {
        tell(&chain, opened(n, branch(name))).unwrap();
        assert_eq!(room(&chain, n).as_deref(), Some(expected), "{name}");
    }
}
