//! Forge's notifier against a fake host and a fake device store that
//! outlives its runs.
use super::*;
use std::cell::RefCell;
use std::rc::Rc;

use forge::{ChangeState, ChangeSummary, PageResponse, ReplyAttention, ReviewCounts, Revision};
use view::methods::{Change, Changes, HostScreen, HostSession, StoreGet, StoreSet};

const FORGE: u64 = 9;
use view::testing::TestNotifier;

fn owed(n: u64, requested: bool, reply: u64) -> Judgment {
    Judgment {
        change: ChangeSummary {
            repo: "web".into(),
            n,
            from: Revision::Ref(b"refs/heads/feature".to_vec()),
            into: b"refs/heads/main".to_vec(),
            title: "Faster builds".into(),
            author: Principal::Account(3),
            state: ChangeState::Open,
            updated_height: 1,
            review_count: 0,
            comment_count: 0,
            verdicts: ReviewCounts::default(),
        },
        requested,
        replies: (reply > 0).then_some(ReplyAttention {
            review: None,
            root_seq: 1,
            last_reply_seq: reply,
        }),
    }
}

fn session() -> Session {
    Session {
        signer: "0102".into(),
        account: Some(2),
        connected: true,
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    }
}

#[derive(Clone, Default)]
struct World {
    owed: Rc<RefCell<Vec<Judgment>>>,
    kept: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
    asked: Rc<RefCell<usize>>,
}

impl World {
    fn start(&self) -> TestNotifier<ForgeNotices> {
        let mut notifier = TestNotifier::new();
        let host = notifier.host();
        let (owed, asked) = (self.owed.clone(), self.asked.clone());
        host.handle::<Ask>(move |query| match query {
            Query::Judgment { principal, .. } => {
                assert_eq!(principal, Principal::Account(2));
                *asked.borrow_mut() += 1;
                Ok(Reply::Judgment {
                    height: 1,
                    page: PageResponse {
                        height: 1,
                        items: owed.borrow().clone(),
                        next: None,
                    },
                })
            }
            query => panic!("unexpected forge query: {query:?}"),
        });
        host.handle::<Who>(|query| match query {
            identity::Query::OfProgram { program } => {
                assert_eq!(program, forge::PROGRAM);
                Ok(identity::Reply::Number(Some(FORGE)))
            }
            query => panic!("unexpected identity query: {query:?}"),
        });
        let kept = self.kept.clone();
        host.handle::<StoreGet>(move |key| Ok(kept.borrow().get(&key).cloned()));
        let kept = self.kept.clone();
        host.handle::<StoreSet>(move |(key, value)| {
            match value {
                Some(value) => kept.borrow_mut().insert(key, value),
                None => kept.borrow_mut().remove(&key),
            };
            Ok(())
        });
        notifier.start();
        host.stream::<HostSession>().send(session());
        host.stream::<HostScreen>().send(Vec::new());
        notifier.run_until_parked();
        notifier
    }

    fn block(&self, notifier: &mut TestNotifier<ForgeNotices>, owed: Vec<Judgment>) {
        *self.owed.borrow_mut() = owed;
        notifier.host().stream::<Changes<forge::Forge>>().send(None);
        notifier.run_until_parked();
    }
}

fn posts(notifier: &TestNotifier<ForgeNotices>) -> Vec<(String, String)> {
    notifier
        .host()
        .requests::<NotifyPost>()
        .into_iter()
        .map(|post| (post.title, post.link))
        .collect()
}

#[test]
fn a_review_request_and_a_reply_are_one_notice_each() {
    let world = World::default();
    *world.owed.borrow_mut() = vec![owed(1, true, 0)];
    let mut notifier = world.start();
    assert!(posts(&notifier).is_empty(), "history is not news");

    world.block(&mut notifier, vec![owed(1, true, 0), owed(3, true, 0)]);
    let link = "duck://testnet-0a1b2c3d/forge/web/3";
    assert_eq!(
        posts(&notifier),
        [("Your review is requested".into(), link.into())]
    );
    let post = &notifier.host().requests::<NotifyPost>()[0];
    assert_eq!(
        (post.body.as_str(), post.tag.as_str()),
        ("web #3: Faster builds", "web#3")
    );

    world.block(&mut notifier, vec![owed(1, true, 0), owed(3, true, 5)]);
    assert_eq!(posts(&notifier).len(), 2);
    assert_eq!(
        posts(&notifier)[1],
        ("New reply to you".into(), link.into())
    );

    world.block(&mut notifier, vec![owed(1, true, 0), owed(3, true, 5)]);
    let relaunched = world.start();
    assert_eq!(posts(&notifier).len(), 2, "once");
    assert!(posts(&relaunched).is_empty(), "announced once");
}

#[test]
fn a_change_on_screen_gets_no_notice() {
    let world = World::default();
    let mut notifier = world.start();
    let screen = notifier.host().stream::<HostScreen>();
    screen.send(vec!["web/3".into()]);
    notifier.run_until_parked();
    world.block(&mut notifier, vec![owed(3, true, 0)]);
    screen.send(Vec::new());
    notifier.run_until_parked();
    assert!(posts(&notifier).is_empty());

    world.block(&mut notifier, vec![owed(3, true, 0), owed(4, true, 0)]);
    assert_eq!(posts(&notifier).len(), 1);
}

fn answered(author: u64) -> Vec<u8> {
    chat::tables::ANSWERED.key(&("forge:web:3".to_owned(), Principal::Account(author), 7u64))
}

#[test]
fn a_chat_block_asks_only_when_it_answered_the_readers_thread_or_a_review() {
    let world = World::default();
    let mut notifier = world.start();
    let chat = notifier.host().stream::<Changes<chat::Chat>>();
    let asked = |keys: Vec<Vec<u8>>, notifier: &mut TestNotifier<ForgeNotices>| {
        let before = *world.asked.borrow();
        chat.send(Some(Change { height: 5, keys }));
        notifier.run_until_parked();
        *world.asked.borrow() - before
    };
    let posted = vec![
        chat::tables::MESSAGES.key(&("general".to_owned(), 7u64)),
        chat::tables::MESSAGE_IDS.key(&"m7".to_owned()),
    ];
    assert_eq!(asked(posted, &mut notifier), 0, "a post in a room");
    assert_eq!(
        asked(vec![answered(5)], &mut notifier),
        0,
        "a reply to someone else"
    );
    assert_eq!(
        asked(vec![answered(2)], &mut notifier),
        1,
        "a reply to the reader"
    );
    assert_eq!(
        asked(vec![answered(FORGE)], &mut notifier),
        1,
        "a reply to a review"
    );
    chat.send(None);
    notifier.run_until_parked();
    assert_eq!(*world.asked.borrow(), 4, "a reopened link reads everything");
}
