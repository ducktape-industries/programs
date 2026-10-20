//! What chat tells attribution, with attribution seated beside it on a `MockChain`: each
//! message's mentions, a direct message's counterpart, and who took part in a thread before
//! a reply.
use ::program::{MockChain, Origin, code};
use abi::role::identity::{Kind, Profile};
use attribution::{
    Actor, Attribution, Cause, Change, Kind as Changed, Object, PageRequest, Reason, Relations,
    Source,
};

use crate::attributed::{DIRECT, LISTEN, MESSAGE, THREAD, WATCH};
use crate::{Followed, Notices};

const ADA: &[u8] = b"ada-key";
const BO: &[u8] = b"bo-key";
const CY: &[u8] = b"cy-key";

fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Attribution>(attribution::PROGRAM);
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.register(attribution::PROGRAM, 900);
    chain.register(crate::PROGRAM, 901);
    for (key, number, name) in [(ADA, 1, "Ada"), (BO, 2, "Bo"), (CY, 3, "Cy")] {
        chain.hold(key.to_vec(), number);
        chain.profile(Profile {
            number,
            name: name.into(),
            kind: Kind::User,
        });
    }
    chain
}

fn signed(key: &[u8]) -> Origin {
    Origin::Signed(key.to_vec())
}

fn chat_source(message: &str) -> Source {
    Source {
        program: crate::PROGRAM.into(),
        object: Object {
            kind: MESSAGE.into(),
            id: message.into(),
        },
    }
}

fn relations(chain: &MockChain, source: Source) -> Option<Relations> {
    let asked = attribution::Query::Relations { source };
    let attribution::Reply::Relations(relations) =
        chain.query(attribution::PROGRAM, &asked).unwrap()
    else {
        panic!()
    };
    relations
}

fn held(chain: &MockChain, source: Source) -> Vec<(u64, Reason)> {
    relations(chain, source)
        .map(|row| {
            row.relations
                .into_iter()
                .map(|relation| (relation.recipient, relation.reason))
                .collect()
        })
        .unwrap_or_default()
}

fn changes(chain: &MockChain, asked: attribution::Query) -> Vec<Change> {
    let attribution::Reply::Changes(page) = chain.query(attribution::PROGRAM, &asked).unwrap()
    else {
        panic!()
    };
    page.items
}

fn changes_to(chain: &MockChain, recipient: u64) -> Vec<(String, Reason, Changed)> {
    let asked = attribution::Query::ChangesTo {
        recipient,
        page: PageRequest::default(),
    };
    changes(chain, asked)
        .into_iter()
        .map(|change| (change.source.object.id, change.reason, change.kind))
        .collect()
}

fn post(chain: &MockChain, key: &[u8], channel: &str, id: &str, text: &str) {
    post_in(chain, key, channel, id, text, None);
}

fn post_in(
    chain: &MockChain,
    key: &[u8],
    channel: &str,
    id: &str,
    text: &str,
    thread: Option<u64>,
) {
    let op = crate::Op::PostMessage {
        channel_id: channel.into(),
        message_id: id.into(),
        blocks: crate::parse_message(text),
        thread,
    };
    chain.submit(signed(key), crate::PROGRAM, &op).unwrap();
}

fn direct() -> Reason {
    Reason::Defined(DIRECT.into())
}

fn in_thread() -> Reason {
    Reason::Defined(THREAD.into())
}

fn listening() -> Reason {
    Reason::Defined(LISTEN.into())
}

fn open_general(chain: &MockChain) {
    let create = crate::Op::CreateChannel {
        channel_id: "general".into(),
        name: "general".into(),
        post_policy: crate::PostPolicy::Open,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &create).unwrap();
}

#[test]
fn chat_reports_each_messages_mentions_and_a_direct_messages_counterpart() {
    let chain = chain();
    open_general(&chain);
    post(&chain, ADA, "general", "plain", "no one in particular");
    assert_eq!(relations(&chain, chat_source("plain")), None);

    post(&chain, ADA, "general", "m1", "hi <@2> and <@1>");
    assert_eq!(held(&chain, chat_source("m1")), [(2, Reason::Mention)]);
    let edit = crate::Op::EditMessage {
        channel_id: "general".into(),
        seq: 2,
        blocks: crate::parse_message("only you, <@3>"),
        base_rev: Some(0),
    };
    chain.submit(signed(ADA), crate::PROGRAM, &edit).unwrap();
    assert_eq!(held(&chain, chat_source("m1")), [(3, Reason::Mention)]);
    let delete = crate::Op::DeleteMessage {
        channel_id: "general".into(),
        seq: 2,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &delete).unwrap();
    let row = relations(&chain, chat_source("m1")).unwrap();
    assert_eq!((row.revision, row.relations), (3, Vec::new()));
    assert_eq!(
        changes_to(&chain, 2),
        [
            ("m1".into(), Reason::Mention, Changed::Added),
            ("m1".into(), Reason::Mention, Changed::Withdrawn)
        ]
    );
    assert_eq!(
        changes_to(&chain, 3),
        [
            ("m1".into(), Reason::Mention, Changed::Added),
            ("m1".into(), Reason::Mention, Changed::Withdrawn)
        ]
    );

    let dm = crate::Op::CreateDmChannel {
        counterpart: 2,
        name: "us".into(),
    };
    chain.submit(signed(ADA), crate::PROGRAM, &dm).unwrap();
    let room = crate::dm_channel_id(1, 2);
    post(&chain, ADA, &room, "d1", "a word");
    post(&chain, BO, &room, "d2", "back to you, <@1>");
    assert_eq!(held(&chain, chat_source("d1")), [(2, direct())]);
    assert_eq!(
        held(&chain, chat_source("d2")),
        [(1, Reason::Mention), (1, direct())]
    );
    let asked = attribution::Query::ChangesOf {
        source: chat_source("d2"),
        page: PageRequest::default(),
    };
    let Change { actor, cause, .. } = changes(&chain, asked).remove(0);
    assert_eq!(actor, Actor::Account(2));
    assert!(matches!(cause, Cause::Message(_)));
}

#[test]
fn a_typed_name_reaches_attribution_as_a_picked_mention_does() {
    let chain = chain();
    open_general(&chain);
    post(&chain, ADA, "general", "typed", "thanks @bo.");
    assert_eq!(held(&chain, chat_source("typed")), [(2, Reason::Mention)]);
    post(&chain, ADA, "general", "unknown", "thanks @nobody");
    assert_eq!(relations(&chain, chat_source("unknown")), None);
    let edit = crate::Op::EditMessage {
        channel_id: "general".into(),
        seq: 2,
        blocks: crate::parse_message("thanks @Ada and @bo"),
        base_rev: Some(0),
    };
    chain.submit(signed(ADA), crate::PROGRAM, &edit).unwrap();
    assert_eq!(held(&chain, chat_source("unknown")), [(2, Reason::Mention)]);
}

#[test]
fn a_thread_reply_relates_everyone_who_took_part_before_it() {
    let chain = chain();
    open_general(&chain);
    post(&chain, ADA, "general", "root", "a question");
    assert_eq!(relations(&chain, chat_source("root")), None);
    post_in(&chain, BO, "general", "b1", "an answer", Some(1));
    assert_eq!(held(&chain, chat_source("b1")), [(1, in_thread())]);
    post_in(&chain, CY, "general", "c1", "another", Some(1));
    assert_eq!(
        held(&chain, chat_source("c1")),
        [(1, in_thread()), (2, in_thread())]
    );
    post_in(&chain, BO, "general", "b2", "and <@1>", Some(1));
    assert_eq!(
        held(&chain, chat_source("b2")),
        [(1, Reason::Mention), (1, in_thread()), (3, in_thread())]
    );
    let edit = crate::Op::EditMessage {
        channel_id: "general".into(),
        seq: 3,
        blocks: crate::parse_message("another, edited"),
        base_rev: Some(0),
    };
    chain.submit(signed(CY), crate::PROGRAM, &edit).unwrap();
    assert_eq!(
        held(&chain, chat_source("c1")),
        [(1, in_thread()), (2, in_thread())]
    );
    post(&chain, CY, "general", "elsewhere", "a new topic");
    assert_eq!(relations(&chain, chat_source("elsewhere")), None);
}

fn listen(chain: &MockChain, key: &[u8], channel: &str, listening: bool) -> Result<(), String> {
    let op = crate::Op::Listen {
        channel_id: channel.into(),
        listening,
    };
    chain
        .submit(signed(key), crate::PROGRAM, &op)
        .map(drop)
        .map_err(|refusal| refusal.code)
}

fn listened(chain: &MockChain, account: u64) -> Vec<String> {
    let asked = crate::Query::Listening {
        account,
        page: PageRequest::default(),
    };
    let crate::Reply::Listening(page) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

#[test]
fn a_listener_is_related_to_each_new_message_in_its_channel_it_relates_to_in_no_other_way() {
    let chain = chain();
    open_general(&chain);
    post(&chain, ADA, "general", "before", "before anyone listens");
    chain.at(2, 0);
    listen(&chain, BO, "general", true).unwrap();
    listen(&chain, CY, "general", true).unwrap();
    listen(&chain, CY, "general", true).unwrap();
    assert_eq!(listened(&chain, 2), ["general"]);
    assert!(listened(&chain, 1).is_empty());

    post(&chain, ADA, "general", "m1", "a plain message");
    assert_eq!(
        held(&chain, chat_source("m1")),
        [(2, listening()), (3, listening())]
    );
    post(&chain, BO, "general", "m2", "hi <@3>");
    assert_eq!(
        held(&chain, chat_source("m2")),
        [(3, Reason::Mention)],
        "a mention wins, and the author hears nothing of its own"
    );
    post_in(&chain, CY, "general", "m3", "a reply", Some(1));
    assert_eq!(
        held(&chain, chat_source("m3")),
        [(1, in_thread()), (2, listening())]
    );
    let edit = crate::Op::EditMessage {
        channel_id: "general".into(),
        seq: 1,
        blocks: crate::parse_message("before anyone listens, edited"),
        base_rev: Some(0),
    };
    chain.submit(signed(ADA), crate::PROGRAM, &edit).unwrap();
    assert_eq!(relations(&chain, chat_source("before")), None);

    listen(&chain, BO, "general", false).unwrap();
    assert!(listened(&chain, 2).is_empty());
    post(&chain, ADA, "general", "m4", "after Bo stopped");
    assert_eq!(held(&chain, chat_source("m4")), [(3, listening())]);
    let random = crate::Op::CreateChannel {
        channel_id: "random".into(),
        name: "random".into(),
        post_policy: crate::PostPolicy::Open,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &random).unwrap();
    post(&chain, ADA, "random", "r1", "nobody listens here");
    assert_eq!(relations(&chain, chat_source("r1")), None);
    assert_eq!(
        listen(&chain, BO, "nowhere", true),
        Err(code::NOT_FOUND.to_owned())
    );
}

fn set_notices(chain: &MockChain, key: &[u8], channel: &str, notices: Option<Notices>) {
    let op = crate::Op::SetNotices {
        channel_id: channel.into(),
        notices,
    };
    chain.submit(signed(key), crate::PROGRAM, &op).unwrap();
}

fn notices_cut(chain: &MockChain, account: u64) -> Vec<(String, Notices)> {
    let asked = crate::Query::Notices {
        account,
        page: PageRequest::default(),
    };
    let crate::Reply::Notices(page) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

#[test]
fn a_rooms_notices_are_one_choice_listening_among_them() {
    let chain = chain();
    open_general(&chain);
    let general = |notices| vec![("general".to_owned(), notices)];
    listen(&chain, BO, "general", true).unwrap();
    set_notices(&chain, BO, "general", Some(Notices::Mentions));
    assert_eq!(notices_cut(&chain, 2), general(Notices::Mentions));
    assert!(listened(&chain, 2).is_empty(), "cut, Bo no longer listens");
    set_notices(&chain, BO, "general", Some(Notices::Off));
    assert_eq!(notices_cut(&chain, 2), general(Notices::Off));
    assert!(notices_cut(&chain, 3).is_empty());

    listen(&chain, BO, "general", true).unwrap();
    assert!(
        notices_cut(&chain, 2).is_empty(),
        "listening takes the cut off"
    );
    assert_eq!(listened(&chain, 2), ["general"]);
    set_notices(&chain, BO, "general", None);
    assert!(notices_cut(&chain, 2).is_empty());
    assert!(listened(&chain, 2).is_empty(), "the default is neither");
}

fn followed(chain: &MockChain, channel: &str, account: u64, roots: &[u64]) -> Followed {
    let asked = crate::Query::Followed {
        channel_id: channel.into(),
        account,
        roots: roots.to_vec(),
    };
    let crate::Reply::Followed(followed) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    followed
}

#[test]
fn where_an_account_is_comes_from_what_it_wrote_and_what_mentioned_it() {
    let chain = chain();
    open_general(&chain);
    post(&chain, BO, "general", "m1", "a topic");
    post_in(&chain, ADA, "general", "m2", "a reply", Some(1));
    post_in(
        &chain,
        ADA,
        "general",
        "m3",
        "what do you think, <@3>?",
        Some(1),
    );
    post(&chain, CY, "general", "m4", "hello");
    post(&chain, CY, "general", "m5", "again");
    post(&chain, ADA, "general", "m6", "<@2> have a look");
    post_in(&chain, CY, "general", "m7", "a reply", Some(6));
    let at = |room, threads| Followed { room, threads };
    let general = |account| followed(&chain, "general", account, &[1, 6]);
    assert_eq!(
        general(1),
        at(Some(0), vec![(1, 2), (6, 6)]),
        "Ada opened it"
    );
    assert_eq!(general(2), at(Some(1), vec![(1, 1), (6, 6)]));
    assert_eq!(general(3), at(Some(4), vec![(1, 3), (6, 7)]));

    let team = crate::Op::CreateChannel {
        channel_id: "team".into(),
        name: "team".into(),
        post_policy: crate::PostPolicy::MembersOnly,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &team).unwrap();
    let seat = crate::Op::SetMembership {
        channel_id: "team".into(),
        principal: crate::Principal::Account(2),
        member: true,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &seat).unwrap();
    assert_eq!(followed(&chain, "team", 2, &[]), at(Some(0), vec![]));
    assert_eq!(followed(&chain, "team", 3, &[]), at(None, vec![]));
}

#[test]
fn a_post_stands_when_no_attribution_is_seated() {
    let mut chain = MockChain::default();
    chain.seat::<crate::Chat>(crate::PROGRAM);
    chain.seat::<access::Access>("access");
    chain.hold(ADA.to_vec(), 1);
    let create = crate::Op::CreateChannel {
        channel_id: "general".into(),
        name: "general".into(),
        post_policy: crate::PostPolicy::Open,
    };
    chain.submit(signed(ADA), crate::PROGRAM, &create).unwrap();
    post(&chain, ADA, "general", "m1", "hi <@2>");
    let asked = crate::Query::MessageById {
        message_id: "m1".into(),
    };
    let crate::Reply::Message(Some(_)) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!()
    };
}

fn watch(chain: &MockChain, key: &[u8], channel: &str, watching: bool) -> Result<(), String> {
    let op = crate::Op::Watch {
        channel_id: channel.into(),
        watching,
    };
    chain
        .submit(signed(key), crate::PROGRAM, &op)
        .map(drop)
        .map_err(|refusal| refusal.code)
}

fn watched(chain: &MockChain, account: u64) -> Vec<String> {
    let asked = crate::Query::Watching {
        account,
        page: PageRequest::default(),
    };
    let crate::Reply::Watching(page) = chain.query(crate::PROGRAM, &asked).unwrap() else {
        panic!()
    };
    page.items
}

fn chosen(chain: &MockChain, account: u64, channel: &str) -> (bool, Option<Notices>) {
    let asked = crate::Query::Chosen {
        account,
        channel_id: channel.into(),
    };
    let crate::Reply::Chosen { listening, notices } = chain.query(crate::PROGRAM, &asked).unwrap()
    else {
        panic!()
    };
    (listening, notices)
}

#[test]
fn a_watcher_is_related_to_new_messages_and_its_choice_stays() {
    let chain = chain();
    open_general(&chain);
    chain.at(2, 0);
    set_notices(&chain, BO, "general", Some(Notices::Off));
    watch(&chain, BO, "general", true).unwrap();
    listen(&chain, CY, "general", true).unwrap();
    watch(&chain, CY, "general", true).unwrap();
    assert_eq!(watched(&chain, 2), ["general"]);
    assert_eq!(chosen(&chain, 2, "general"), (false, Some(Notices::Off)));
    assert_eq!(chosen(&chain, 3, "general"), (true, None));

    let watching = || Reason::Defined(WATCH.into());
    post(&chain, ADA, "general", "m1", "a plain message");
    assert_eq!(
        held(&chain, chat_source("m1")),
        [(2, watching()), (3, listening())]
    );
    post(&chain, ADA, "general", "m2", "hi <@2>");
    assert_eq!(
        held(&chain, chat_source("m2")),
        [(2, Reason::Mention), (3, listening())]
    );
    watch(&chain, BO, "general", false).unwrap();
    assert!(watched(&chain, 2).is_empty());
    assert_eq!(chosen(&chain, 2, "general"), (false, Some(Notices::Off)));
    post(&chain, ADA, "general", "m3", "after Bo stopped");
    assert_eq!(held(&chain, chat_source("m3")), [(3, listening())]);
    assert_eq!(
        watch(&chain, BO, "nowhere", true),
        Err(code::NOT_FOUND.to_owned())
    );
}

#[test]
fn an_account_or_its_manager_chooses_for_it() {
    let chain = chain();
    chain.profile(Profile {
        number: 4,
        name: "Scout".into(),
        kind: Kind::Program {
            script: None,
            manager: 1,
            category: abi::role::identity::Category::Agent,
            standing: abi::role::identity::Standing::Active,
        },
    });
    open_general(&chain);
    let as_key = |key: &[u8], op: crate::Op| {
        chain
            .submit(signed(key), crate::PROGRAM, &op)
            .map(drop)
            .map_err(|refusal| refusal.code)
    };
    let listen_for = |account, listening| crate::Op::ListenFor {
        account,
        channel_id: "general".into(),
        listening,
    };
    let cut_for = |account, notices| crate::Op::SetNoticesFor {
        account,
        channel_id: "general".into(),
        notices,
    };
    as_key(ADA, listen_for(4, true)).unwrap();
    assert_eq!(listened(&chain, 4), ["general"]);
    as_key(ADA, cut_for(4, Some(Notices::Off))).unwrap();
    assert_eq!(chosen(&chain, 4, "general"), (false, Some(Notices::Off)));
    assert!(listened(&chain, 4).is_empty(), "one choice");
    let refused = [
        (BO, listen_for(4, true)),
        (CY, cut_for(4, None)),
        (ADA, cut_for(2, Some(Notices::Off))),
    ];
    for (key, op) in refused {
        assert_eq!(as_key(key, op), Err(code::UNAUTHORIZED.to_owned()));
    }
    assert_eq!(chosen(&chain, 4, "general"), (false, Some(Notices::Off)));
    assert_eq!(chosen(&chain, 2, "general"), (false, None));
    as_key(BO, cut_for(2, Some(Notices::Mentions))).unwrap();
    assert_eq!(
        chosen(&chain, 2, "general"),
        (false, Some(Notices::Mentions))
    );
    as_key(ADA, cut_for(4, None)).unwrap();
    assert_eq!(chosen(&chain, 4, "general"), (false, None));
}
