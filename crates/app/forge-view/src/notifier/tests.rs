//! Forge's notifier against a fake host and a fake device store that
//! outlives its runs.
use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use forge::{ChangeState, ChangeSummary, PageResponse, ReviewCounts, Revision};
use view::methods::{Changes, HostScreen, HostSession, StoreGet, StoreSet};

use view::testing::TestNotifier;

type Owed = (u64, bool, u64);

fn summary(n: u64) -> ChangeSummary {
    ChangeSummary {
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
    owed: Rc<RefCell<Vec<Owed>>>,
    kept: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
    asked: Rc<RefCell<usize>>,
}

impl World {
    fn start(&self) -> TestNotifier<ForgeNotices> {
        let mut notifier = TestNotifier::new();
        let host = notifier.host();
        let (owed, asked) = (self.owed.clone(), self.asked.clone());
        host.handle::<Ask>(move |query| {
            *asked.borrow_mut() += 1;
            let owed = owed.borrow().clone();
            match query {
                Query::ReviewRequests { principal, .. } => {
                    assert_eq!(principal, Principal::Account(2));
                    Ok(Reply::ReviewRequests {
                        height: 1,
                        page: PageResponse {
                            height: 1,
                            items: owed
                                .into_iter()
                                .filter(|(_, requested, _)| *requested)
                                .map(|(n, _, _)| summary(n))
                                .collect(),
                            next: None,
                        },
                    })
                }
                query => panic!("unexpected forge query: {query:?}"),
            }
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

    fn block(&self, notifier: &mut TestNotifier<ForgeNotices>, owed: Vec<Owed>) {
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

const REQUESTED: &str = "Your review is requested";

#[test]
fn a_review_request_is_one_notice_announced_once() {
    let world = World::default();
    *world.owed.borrow_mut() = vec![(1, true, 0)];
    let mut notifier = world.start();
    assert!(posts(&notifier).is_empty(), "history is not news");

    world.block(&mut notifier, vec![(1, true, 0), (3, true, 0)]);
    let link = "duck://testnet-0a1b2c3d/forge/web/3";
    assert_eq!(posts(&notifier), [(REQUESTED.into(), link.into())]);
    let post = &notifier.host().requests::<NotifyPost>()[0];
    assert_eq!(
        (post.body.as_str(), post.tag.as_str()),
        ("web #3: Faster builds", "web#3")
    );

    world.block(&mut notifier, vec![(1, true, 0), (3, true, 0)]);
    let relaunched = world.start();
    assert_eq!(posts(&notifier).len(), 1, "once");
    assert!(posts(&relaunched).is_empty(), "announced once");

    world.block(&mut notifier, vec![(1, true, 0)]);
    world.block(&mut notifier, vec![(1, true, 0), (3, true, 0)]);
    assert_eq!(
        posts(&notifier).len(),
        2,
        "asked again after it stopped asking"
    );
}

#[test]
fn a_reply_to_the_reader_is_chats_to_tell_never_a_forge_notice() {
    let world = World::default();
    let mut notifier = world.start();
    world.block(&mut notifier, vec![(3, true, 0)]);
    world.block(&mut notifier, vec![(3, true, 5)]);
    world.block(&mut notifier, vec![(3, false, 6)]);
    let link = "duck://testnet-0a1b2c3d/forge/web/3";
    assert_eq!(posts(&notifier), [(REQUESTED.into(), link.into())]);
}

#[test]
fn a_change_on_screen_gets_no_notice() {
    let world = World::default();
    let mut notifier = world.start();
    let screen = notifier.host().stream::<HostScreen>();
    screen.send(vec!["web/3".into()]);
    notifier.run_until_parked();
    world.block(&mut notifier, vec![(3, true, 0)]);
    screen.send(Vec::new());
    notifier.run_until_parked();
    assert!(posts(&notifier).is_empty());

    world.block(&mut notifier, vec![(3, true, 0), (4, true, 0)]);
    assert_eq!(posts(&notifier).len(), 1);
}

#[test]
fn the_notifier_follows_forge_and_never_chat() {
    let world = World::default();
    let notifier = world.start();
    let host = notifier.host();
    assert!(host.stream::<Changes<forge::Forge>>().subscribed());
    let chat = host.stream::<Changes<chat::Chat>>().subscribed();
    assert!(!chat, "replies are chat's to tell");
}
