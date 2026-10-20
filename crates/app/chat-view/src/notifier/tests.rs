//! Chat's notifier against a fake host and a fake device store that outlives
//! its runs, as the app keeps it between launches.
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chat::{Block, ChannelRow, Mark, PageResponse, PostPolicy, Query, Reply, Span};
use view::methods::{
    self, Change, Changes, HostBadge, HostScreen, HostSession, NotifyPost, Query as Ask, Session,
    StoreChanges, StoreGet, StoreSet,
};
use view::testing::{FakeHost, TestNotifier};

fn page<T>(items: Vec<T>) -> PageResponse<T> {
    PageResponse {
        height: 1,
        items,
        next: None,
    }
}

fn room(id: &str, name: &str, head_seq: u64) -> ChannelInfo {
    ChannelInfo {
        channel: ChannelRow {
            id: id.into(),
            name: name.into(),
            created_at: 0,
            post_policy: PostPolicy::Open,
            owner: Principal::Account(9),
            archived: false,
        },
        head_seq,
        meta: Default::default(),
    }
}

fn row(channel: &str, seq: u64, author: u64, blocks: Vec<Block>) -> MsgRow {
    MsgRow {
        channel_id: channel.into(),
        seq,
        message_id: format!("m{seq}"),
        blocks,
        ..MsgRow::by(Principal::Account(author))
    }
}

fn reply(channel: &str, seq: u64, author: u64, root: u64, blocks: Vec<Block>) -> MsgRow {
    MsgRow {
        thread: Some(root),
        ..row(channel, seq, author, blocks)
    }
}

fn said(text: &str, marks: Vec<Mark>) -> Vec<Block> {
    vec![Block::Paragraph(vec![Span {
        text: text.into(),
        marks,
    }])]
}

fn mention(channel: &str, seq: u64) -> MsgRow {
    row(
        channel,
        seq,
        8,
        said("@eddy", vec![Mark::Mention(Principal::Account(7))]),
    )
}

fn session() -> Session {
    Session {
        signer: "0102".into(),
        account: Some(7),
        connected: true,
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    }
}

#[derive(Clone, Default)]
struct World {
    rooms: Rc<RefCell<Vec<ChannelInfo>>>,
    rows: Rc<RefCell<Vec<MsgRow>>>,
    followed: Rc<RefCell<BTreeMap<String, Followed>>>,
    asked_roots: Rc<RefCell<Vec<Vec<u64>>>>,
    refuse_followed: Rc<Cell<bool>>,
    refuse_accounts: Rc<Cell<bool>>,
    refuse_unread: Rc<Cell<bool>>,
    refuse_messages: Rc<Cell<bool>>,
    choices: Rc<RefCell<BTreeMap<String, Choice>>>,
    kept: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
}

impl World {
    fn notifier(&self) -> TestNotifier<ChatNotices> {
        let notifier = TestNotifier::new();
        let host = notifier.host();
        self.answer(&host);
        host.handle::<Ask<Identity>>(|query| panic!("unexpected identity query: {query:?}"));
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
        notifier
    }

    fn answer(&self, host: &FakeHost) {
        let world = self.clone();
        host.handle::<Ask<chat::Chat>>(move |query| {
            Ok(match query {
                Query::Accounts { .. } => {
                    if world.refuse_accounts.get() {
                        return Err(view::host::Error::new("refused", "the node is busy"));
                    }
                    let profile = |number, name: &str, kind| chat::Profile {
                        number,
                        name: name.into(),
                        kind,
                    };
                    let idle = chat::Kind::Program {
                        script: None,
                        manager: 7,
                        category: chat::Category::Agent,
                        standing: chat::Standing::Suspended,
                    };
                    Reply::Accounts(page(vec![
                        profile(7, "eddy", chat::Kind::User),
                        profile(8, "reviewer", chat::Kind::User),
                        profile(9, "idle", idle),
                    ]))
                }
                Query::Channels { .. } => Reply::Channels(page(world.rooms.borrow().clone())),
                Query::Channel { channel_id } => {
                    let rooms = world.rooms.borrow();
                    let room = rooms.iter().find(|room| room.channel.id == channel_id);
                    Reply::Channel(room.cloned())
                }
                Query::MessagesAround { .. } if world.refuse_messages.get() => {
                    return Err(view::host::Error::new("refused", "the node is busy"));
                }
                Query::MessagesAround {
                    channel_id,
                    seq,
                    page: asked,
                    ..
                } => {
                    let half = asked.limit.unwrap_or(0) / 2;
                    Reply::Messages(
                        world
                            .rows
                            .borrow()
                            .iter()
                            .filter(|row| row.channel_id == channel_id)
                            .filter(|row| row.seq + half >= seq && row.seq <= seq + half)
                            .cloned()
                            .collect(),
                    )
                }
                Query::Followed {
                    channel_id, roots, ..
                } => {
                    if world.refuse_followed.get() {
                        return Err(view::host::Error::new("refused", "the node is busy"));
                    }
                    world.asked_roots.borrow_mut().push(roots.clone());
                    let followed = world.followed.borrow();
                    let mut followed = followed.get(&channel_id).cloned().unwrap_or_default();
                    followed.threads.retain(|(root, _)| roots.contains(root));
                    Reply::Followed(followed)
                }
                Query::Listening { .. } => Reply::Listening(page(
                    (world.choices.borrow().iter())
                        .filter(|(_, choice)| **choice == Choice::Every)
                        .map(|(room, _)| room.clone())
                        .collect(),
                )),
                Query::Notices { .. } => Reply::Notices(page(
                    (world.choices.borrow().iter())
                        .filter_map(|(room, choice)| match choice {
                            Choice::Cut(notices) => Some((room.clone(), *notices)),
                            Choice::Every | Choice::Default => None,
                        })
                        .collect(),
                )),
                Query::Unread { .. } if world.refuse_unread.get() => {
                    return Err(view::host::Error::new("refused", "the node is busy"));
                }
                Query::Unread { account, rooms } => Reply::Unread(
                    rooms
                        .into_iter()
                        .map(|(channel_id, since)| {
                            let held = world.rooms.borrow();
                            let head = (held.iter())
                                .find(|info| info.channel.id == channel_id)
                                .map_or(0, |info| info.head_seq)
                                .max(since);
                            let followed = world.followed.borrow();
                            let followed = followed.get(&channel_id).cloned().unwrap_or_default();
                            let choices = world.choices.borrow();
                            let choice = choices.get(&channel_id).copied().unwrap_or_default();
                            let meant = (world.rows.borrow().iter())
                                .filter(|row| row.channel_id == channel_id)
                                .filter(|row| row.seq > since && row.seq <= head)
                                .filter(|row| {
                                    notification_reason(row, account, &followed, choice).is_some()
                                })
                                .count();
                            chat::UnreadCount {
                                channel_id,
                                to: head,
                                meant: meant as u64,
                            }
                        })
                        .collect(),
                ),
                query => panic!("unexpected chat query: {query:?}"),
            })
        });
    }

    fn start(&self, notifier: &mut TestNotifier<ChatNotices>) {
        notifier.start();
        let host = notifier.host();
        host.stream::<HostSession>().send(session());
        host.stream::<HostScreen>().send(Vec::new());
        notifier.run_until_parked();
    }

    fn keep(&self, key: &str, value: &BTreeMap<String, u64>) {
        self.kept
            .borrow_mut()
            .insert(key.into(), methods::encode(value));
    }

    fn post(&self, notifier: &mut TestNotifier<ChatNotices>, rows: Vec<MsgRow>) {
        for row in rows {
            for info in self.rooms.borrow_mut().iter_mut() {
                if info.channel.id == row.channel_id {
                    info.head_seq = info.head_seq.max(row.seq);
                }
            }
            let key = chat::tables::HEADS.key(&row.channel_id);
            self.rows.borrow_mut().push(row);
            notifier
                .host()
                .stream::<Changes<chat::Chat>>()
                .send(Some(Change {
                    height: 2,
                    keys: vec![key],
                }));
        }
        notifier.run_until_parked();
    }

    fn choose(&self, notifier: &mut TestNotifier<ChatNotices>, room: &str, choice: Choice) {
        self.choices.borrow_mut().insert(room.into(), choice);
        let key = chat::tables::NOTICES.key(&(7_u64, room.to_owned()));
        notifier
            .host()
            .stream::<Changes<chat::Chat>>()
            .send(Some(Change {
                height: 3,
                keys: vec![key],
            }));
        notifier.run_until_parked();
    }
}

fn posts(notifier: &TestNotifier<ChatNotices>) -> Vec<Notification> {
    notifier.host().requests::<NotifyPost>()
}

fn badge(notifier: &TestNotifier<ChatNotices>) -> Option<i64> {
    notifier.host().requests::<HostBadge>().last().copied()
}

#[test]
fn what_is_meant_for_me_and_why() {
    let names = Names::default();
    let chain = "testnet-0a1b2c3d";
    let me = Mark::Mention(Principal::Account(3));
    let nowhere = Followed::default();
    let why =
        |row: &MsgRow, followed: &Followed| notification_reason(row, 3, followed, Choice::Default);

    let mention = row("design", 7, 5, said("@me", vec![me.clone()]));
    assert_eq!(why(&mention, &nowhere), Some(Why::Mention));
    let post = notice(&mention, Why::Mention, 3, "design", chain, &names);
    assert_eq!(post.title, "account 5 mentioned you");
    assert_eq!(post.tag, "#design");
    assert_eq!(post.link, "duck://testnet-0a1b2c3d/chat/design/7");

    let other = Mark::Mention(Principal::Account(4));
    let plain = |seq| row("design", seq, 5, said("hi", vec![]));
    assert_eq!(
        why(&row("design", 7, 5, said("@x", vec![other])), &nowhere),
        None
    );
    assert_eq!(
        why(&row("design", 7, 3, said("@me", vec![me])), &nowhere),
        None
    );
    assert_eq!(why(&plain(9), &nowhere), None);

    let direct = row("dm-3-5", 7, 5, said("hi", vec![]));
    assert_eq!(why(&direct, &nowhere), Some(Why::Direct));
    let direct = notice(&direct, Why::Direct, 3, "", chain, &names);
    assert_eq!(
        (direct.title.as_str(), direct.body.as_str()),
        ("account 5", "hi")
    );
    assert_eq!(
        why(&row("dm-4-5", 7, 5, said("hi", vec![])), &nowhere),
        None
    );

    let there = Followed {
        room: Some(8),
        threads: vec![(10, 11)],
    };
    assert_eq!(why(&plain(9), &there), Some(Why::Room));
    assert_eq!(why(&plain(8), &there), None, "from before I came in");
    let post = notice(&plain(9), Why::Room, 3, "design", chain, &names);
    assert_eq!(post.title, "account 5 in #design");

    let answer = |seq, root| reply("design", seq, 5, root, said("done", vec![]));
    assert_eq!(why(&answer(12, 10), &there), Some(Why::Thread));
    assert_eq!(why(&answer(11, 10), &there), None);
    assert_eq!(why(&answer(12, 4), &there), None, "a thread I am not in");
    let post = notice(&answer(12, 10), Why::Thread, 3, "design", chain, &names);
    assert_eq!(post.title, "account 5 replied in a thread");
}

#[test]
fn a_choice_goes_over_where_i_am() {
    let me = Mark::Mention(Principal::Account(3));
    let there = Followed {
        room: Some(0),
        threads: vec![(10, 10)],
    };
    let nowhere = Followed::default();
    let plain = row("design", 12, 5, said("hi", vec![]));
    let stranger = reply("design", 12, 5, 4, said("done", vec![]));
    let mine = reply("design", 12, 5, 10, said("done", vec![]));
    let mention = row("design", 12, 5, said("@me", vec![me]));
    let direct = row("dm-3-5", 12, 5, said("hi", vec![]));
    let why =
        |row: &MsgRow, followed: &Followed, choice| notification_reason(row, 3, followed, choice);

    let every = Choice::Every;
    assert_eq!(why(&plain, &nowhere, every), Some(Why::Room));
    assert_eq!(why(&stranger, &nowhere, every), Some(Why::Room));
    assert_eq!(why(&mine, &there, every), Some(Why::Thread));

    let mentions = Choice::Cut(Notices::Mentions);
    assert_eq!(why(&plain, &there, mentions), None);
    assert_eq!(why(&mine, &there, mentions), None);
    assert_eq!(why(&mention, &there, mentions), Some(Why::Mention));
    assert_eq!(why(&direct, &there, mentions), Some(Why::Direct));

    let off = Choice::Cut(Notices::Off);
    for row in [&plain, &mine, &mention, &direct] {
        assert_eq!(why(row, &there, off), None, "{row:?}");
    }
}

#[test]
fn a_mention_with_no_pane_open_is_one_notice_and_the_badge_until_read() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    world.rows.borrow_mut().push(mention("general", 1));
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    assert!(posts(&notifier).is_empty(), "history is not news");
    assert_eq!(badge(&notifier), Some(0));

    world.post(&mut notifier, vec![mention("general", 2)]);
    let posted = posts(&notifier);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert_eq!(
        (posted[0].title.as_str(), posted[0].body.as_str()),
        ("reviewer mentioned you", "@eddy")
    );
    assert_eq!(posted[0].tag, "#General");
    assert_eq!(posted[0].link, "duck://testnet-0a1b2c3d/chat/general/2");
    assert_eq!(badge(&notifier), Some(1));

    world.keep("reads/0102", &BTreeMap::from([("general".into(), 2)]));
    notifier
        .host()
        .stream::<StoreChanges>()
        .send("reads/0102".into());
    notifier.run_until_parked();
    assert_eq!(badge(&notifier), Some(0));
    assert_eq!(posts(&notifier).len(), 1);
}

#[test]
fn what_came_while_closed_is_announced_once() {
    let world = World::default();
    world.keep("notified/0102", &BTreeMap::from([("general".into(), 1)]));
    world.keep(
        "reads/0102",
        &BTreeMap::from([("general".into(), 1), ("dm-7-8".into(), 0)]),
    );
    *world.rooms.borrow_mut() = vec![room("general", "General", 3), room("dm-7-8", "", 1)];
    world.rows.borrow_mut().extend([
        mention("general", 1),
        mention("general", 2),
        row("general", 3, 8, said("plain", vec![])),
        row("dm-7-8", 1, 8, said("hi", vec![])),
    ]);
    let mut notifier = world.notifier();
    notifier.start();
    notifier.host().stream::<HostSession>().send(Session {
        account: None,
        ..session()
    });
    notifier.host().stream::<HostScreen>().send(Vec::new());
    notifier.run_until_parked();
    assert!(posts(&notifier).is_empty(), "no account, nothing is meant");

    notifier.host().stream::<HostSession>().send(session());
    notifier.run_until_parked();
    let mut posted: Vec<String> = posts(&notifier).into_iter().map(|post| post.link).collect();
    posted.sort();
    assert_eq!(
        posted,
        [
            "duck://testnet-0a1b2c3d/chat/dm-7-8/1",
            "duck://testnet-0a1b2c3d/chat/general/2"
        ]
    );
    assert_eq!(badge(&notifier), Some(2));

    let mut relaunched = world.notifier();
    world.start(&mut relaunched);
    assert!(posts(&relaunched).is_empty(), "announced once");
    assert_eq!(badge(&relaunched), Some(2));
}

#[test]
fn a_room_on_screen_gets_no_notice() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1), room("random", "Random", 0)];
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    let screen = notifier.host().stream::<HostScreen>();
    screen.send(vec!["general".into()]);
    notifier.run_until_parked();

    world.post(&mut notifier, vec![mention("general", 2)]);
    assert!(posts(&notifier).is_empty(), "{:?}", posts(&notifier));
    assert_eq!(badge(&notifier), Some(0));

    screen.send(Vec::new());
    notifier.run_until_parked();
    assert!(posts(&notifier).is_empty());
    assert_eq!(badge(&notifier), Some(0));

    world.post(&mut notifier, vec![mention("random", 1)]);
    assert_eq!(posts(&notifier).len(), 1);
    assert_eq!(badge(&notifier), Some(1));
}

#[test]
fn a_catch_up_announces_every_room_once() {
    let world = World::default();
    let ids: Vec<String> = (0..20).map(|n| format!("c{n}")).collect();
    *world.rooms.borrow_mut() = ids.iter().map(|id| room(id, id, 1)).collect();
    world
        .rows
        .borrow_mut()
        .extend(ids.iter().map(|id| mention(id, 1)));
    world.keep(
        "notified/0102",
        &ids.iter().map(|id| (id.clone(), 0)).collect(),
    );
    let mut notifier = world.notifier();
    world.start(&mut notifier);

    let mut announced = 0;
    for (key, value) in notifier.host().requests::<StoreSet>() {
        if key != "notified/0102" {
            continue;
        }
        let kept: BTreeMap<String, u64> = methods::decode(&value.unwrap()).unwrap();
        announced = kept.values().filter(|seq| **seq == 1).count();
    }
    assert_eq!(announced, ids.len());
    assert_eq!(posts(&notifier).len(), ids.len());
    assert_eq!(badge(&notifier), Some(ids.len() as i64));
}

fn titles(notifier: &TestNotifier<ChatNotices>) -> Vec<String> {
    posts(notifier).into_iter().map(|post| post.title).collect()
}

#[test]
fn a_room_i_am_in_tells_me_of_its_timeline() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 2), room("random", "Random", 1)];
    world.rows.borrow_mut().extend([
        row("general", 1, 8, said("hi", vec![])),
        row("general", 2, 7, said("hello", vec![])),
        row("random", 1, 8, said("hi", vec![])),
    ]);
    world.followed.borrow_mut().insert(
        "general".into(),
        Followed {
            room: Some(2),
            threads: Vec::new(),
        },
    );
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(
        &mut notifier,
        vec![
            row("general", 3, 8, said("how are you", vec![])),
            reply("general", 4, 8, 1, said("an aside", vec![])),
            row("random", 2, 8, said("anyone?", vec![])),
        ],
    );
    assert_eq!(titles(&notifier), ["reviewer in #General"]);
    assert_eq!(posts(&notifier)[0].body, "how are you");
    assert_eq!(badge(&notifier), Some(1));
}

#[test]
fn a_thread_i_am_in_tells_me_of_every_reply() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 4)];
    world.rows.borrow_mut().extend([
        row("general", 1, 8, said("ship it?", vec![])),
        reply("general", 2, 7, 1, said("not yet", vec![])),
        mention("general", 3),
        row("general", 4, 8, said("lunch?", vec![])),
    ]);
    world.followed.borrow_mut().insert(
        "general".into(),
        Followed {
            room: None,
            threads: vec![(1, 2), (3, 3)],
        },
    );
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(
        &mut notifier,
        vec![
            reply("general", 5, 8, 1, said("why not?", vec![])),
            reply("general", 6, 8, 3, said("see above", vec![])),
            reply("general", 7, 8, 4, said("noodles", vec![])),
            row("general", 8, 8, said("anyone?", vec![])),
        ],
    );
    assert_eq!(
        titles(&notifier),
        [
            "reviewer replied in a thread",
            "reviewer replied in a thread"
        ]
    );
    let links: Vec<String> = posts(&notifier).into_iter().map(|post| post.link).collect();
    assert_eq!(
        links,
        [
            "duck://testnet-0a1b2c3d/chat/general/5",
            "duck://testnet-0a1b2c3d/chat/general/6"
        ]
    );
    assert_eq!(badge(&notifier), Some(2));
    let asked = world.asked_roots.borrow().clone();
    assert!(asked.iter().all(|roots| roots.len() <= 3), "{asked:?}");
    assert!(asked.contains(&vec![1, 3, 4]), "{asked:?}");
}

#[test]
fn a_choice_counts_its_room_again() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    world.rows.borrow_mut().push(row("general", 1, 8, say_hi()));
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(&mut notifier, vec![mention("general", 2)]);
    assert_eq!(badge(&notifier), Some(1));

    world.choose(&mut notifier, "general", Choice::Cut(Notices::Off));
    assert_eq!(badge(&notifier), Some(0));
    world.post(&mut notifier, vec![mention("general", 3)]);
    assert_eq!(posts(&notifier).len(), 1, "nothing from a room cut off");

    world.choose(&mut notifier, "general", Choice::Every);
    assert_eq!(badge(&notifier), Some(2));
    assert_eq!(posts(&notifier).len(), 1);
    world.post(&mut notifier, vec![row("general", 4, 8, say_hi())]);
    assert_eq!(titles(&notifier)[1..], ["reviewer in #General"]);
}

#[test]
fn a_mention_waits_for_where_i_am_and_is_told_once_the_read_is_whole() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 0)];
    world.refuse_followed.set(true);
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(&mut notifier, vec![mention("general", 1)]);
    assert!(posts(&notifier).is_empty(), "{:?}", titles(&notifier));

    world.refuse_followed.set(false);
    world.post(&mut notifier, vec![mention("general", 2)]);
    assert_eq!(
        titles(&notifier),
        ["reviewer mentioned you", "reviewer mentioned you"]
    );
}

#[test]
fn a_read_whose_where_i_am_fails_leaves_a_followed_reply_to_the_next_run() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    (world.rows.borrow_mut()).push(row("general", 1, 7, said("ship it?", vec![])));
    world.followed.borrow_mut().insert(
        "general".into(),
        Followed {
            room: None,
            threads: vec![(1, 1)],
        },
    );
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.refuse_followed.set(true);
    let answer = reply("general", 2, 8, 1, said("why not?", vec![]));
    world.post(&mut notifier, vec![answer]);
    assert!(posts(&notifier).is_empty(), "{:?}", titles(&notifier));

    world.refuse_followed.set(false);
    let mut relaunched = world.notifier();
    world.start(&mut relaunched);
    assert_eq!(titles(&relaunched), ["reviewer replied in a thread"]);
}

#[test]
fn a_notice_names_a_person_and_a_suspended_agent() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 0)];
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    let at_me = || said("@eddy", vec![Mark::Mention(Principal::Account(7))]);
    world.post(
        &mut notifier,
        vec![row("general", 1, 8, at_me()), row("general", 2, 9, at_me())],
    );
    assert_eq!(
        titles(&notifier),
        ["reviewer mentioned you", "idle (suspended) mentioned you"]
    );
}

#[test]
fn a_roster_that_would_not_read_is_read_again_before_any_notice() {
    let world = World::default();
    world.keep("notified/0102", &BTreeMap::from([("general".into(), 0)]));
    *world.rooms.borrow_mut() = vec![room("general", "General", 0)];
    world.refuse_accounts.set(true);
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(&mut notifier, vec![mention("general", 1)]);
    assert!(posts(&notifier).is_empty(), "{:?}", posts(&notifier));

    world.refuse_accounts.set(false);
    world.post(&mut notifier, vec![mention("general", 2)]);
    assert_eq!(
        titles(&notifier),
        ["reviewer mentioned you", "reviewer mentioned you"]
    );
}

fn say_hi() -> Vec<Block> {
    said("hi", vec![])
}

#[test]
fn a_message_reads_its_room_alone_and_not_the_channel_list() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("design", "Design", 0), room("general", "General", 1)];
    world.rows.borrow_mut().push(mention("general", 1));
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    let asked = |notifier: &TestNotifier<ChatNotices>| {
        let asked = notifier.host().requests::<Ask<chat::Chat>>();
        let lists = asked
            .iter()
            .filter(|query| matches!(query, Query::Channels { .. }));
        let rooms = asked.iter().filter_map(|query| match query {
            Query::Channel { channel_id } => Some(channel_id.clone()),
            _ => None,
        });
        (lists.count(), rooms.collect::<Vec<_>>())
    };
    assert_eq!(asked(&notifier), (1, Vec::new()));
    world.post(&mut notifier, vec![mention("general", 2)]);
    assert_eq!(asked(&notifier), (1, vec!["general".to_owned()]));
    assert_eq!(posts(&notifier).len(), 1, "the new mention is announced");
    notifier.host().stream::<Changes<chat::Chat>>().send(None);
    notifier.run_until_parked();
    assert_eq!(asked(&notifier).0, 2);
}

#[test]
fn a_relaunch_counts_every_room_in_one_read_and_reads_no_message() {
    let world = World::default();
    let ids: Vec<String> = (0..12).map(|n| format!("c{n}")).collect();
    *world.rooms.borrow_mut() = ids.iter().map(|id| room(id, id, 3)).collect();
    for id in &ids {
        let rows = [mention(id, 1), mention(id, 2), row(id, 3, 8, say_hi())];
        world.rows.borrow_mut().extend(rows);
    }
    world.keep(
        "notified/0102",
        &ids.iter().map(|id| (id.clone(), 3)).collect(),
    );
    world.keep(
        "reads/0102",
        &ids.iter().map(|id| (id.clone(), 0)).collect(),
    );
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    let asked = notifier.host().requests::<Ask<chat::Chat>>();
    let read = asked
        .iter()
        .filter(|query| matches!(query, Query::MessagesAround { .. }))
        .count();
    let counts: Vec<usize> = (asked.iter())
        .filter_map(|query| match query {
            Query::Unread { rooms, .. } => Some(rooms.len()),
            _ => None,
        })
        .collect();
    assert_eq!(read, 0, "messages read to count them");
    assert_eq!(counts, [ids.len()], "not one count for every room");
    assert!(posts(&notifier).is_empty(), "announced before");
    assert_eq!(badge(&notifier), Some(2 * ids.len() as i64));
}

#[test]
fn a_message_meant_for_no_one_here_reads_no_message() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    world.rows.borrow_mut().push(row("general", 1, 8, say_hi()));
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    let reads = |notifier: &TestNotifier<ChatNotices>| {
        let asked = notifier.host().requests::<Ask<chat::Chat>>();
        let read = asked
            .iter()
            .filter(|query| matches!(query, Query::MessagesAround { .. } | Query::Followed { .. }));
        let counted = asked
            .iter()
            .filter(|query| matches!(query, Query::Unread { .. }));
        (read.count(), counted.count())
    };
    let (read, counted) = reads(&notifier);
    world.post(&mut notifier, vec![row("general", 2, 8, say_hi())]);
    assert_eq!(
        reads(&notifier),
        (read, counted + 1),
        "counted, and nothing meant: no message read"
    );
    assert!(posts(&notifier).is_empty());
    world.post(&mut notifier, vec![mention("general", 3)]);
    assert_eq!(reads(&notifier).0, read + 2, "a mention is read and told");
    assert_eq!(posts(&notifier).len(), 1);
}

#[test]
fn a_read_that_fails_leaves_its_notice_to_the_next_head_and_the_next_run() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 0)];
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.refuse_messages.set(true);
    world.post(&mut notifier, vec![mention("general", 1)]);
    assert!(posts(&notifier).is_empty(), "{:?}", posts(&notifier));

    world.refuse_messages.set(false);
    let mut relaunched = world.notifier();
    world.start(&mut relaunched);
    assert_eq!(titles(&relaunched), ["reviewer mentioned you"]);

    world.refuse_messages.set(true);
    world.post(&mut relaunched, vec![mention("general", 2)]);
    world.refuse_messages.set(false);
    world.post(&mut relaunched, vec![mention("general", 3)]);
    let links: Vec<String> = posts(&relaunched)
        .into_iter()
        .map(|post| post.link)
        .collect();
    assert_eq!(
        links,
        [
            "duck://testnet-0a1b2c3d/chat/general/1",
            "duck://testnet-0a1b2c3d/chat/general/2",
            "duck://testnet-0a1b2c3d/chat/general/3"
        ]
    );
}

#[test]
fn a_run_that_ends_while_it_reads_leaves_the_read_to_the_next_run() {
    let world = World::default();
    world.keep("notified/0102", &BTreeMap::from([("general".into(), 0)]));
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    world.rows.borrow_mut().push(mention("general", 1));
    let mut notifier = world.notifier();
    let host = notifier.host();
    host.never::<Ask<chat::Chat>>();
    world.start(&mut notifier);
    let reading = |host: &FakeHost| {
        (host.requests::<Ask<chat::Chat>>().iter())
            .any(|query| matches!(query, Query::MessagesAround { .. }))
    };
    let mut asked = 0;
    while !reading(&host) {
        let now = host.requests::<Ask<chat::Chat>>().len();
        assert!(
            now > asked,
            "the notifier stopped before reading the mention"
        );
        asked = now;
        world.answer(&host);
        host.answer_held::<Ask<chat::Chat>>();
        host.never::<Ask<chat::Chat>>();
        notifier.run_until_parked();
    }
    assert!(posts(&notifier).is_empty(), "the read is still out");

    world.answer(&host);
    let feed = host.stream::<HostSession>();
    feed.send(Session {
        connected: false,
        ..session()
    });
    notifier.run_until_parked();
    feed.send(session());
    notifier.run_until_parked();
    assert_eq!(titles(&notifier), ["reviewer mentioned you"]);
}

#[test]
fn a_room_never_opened_here_keeps_its_badge_across_a_relaunch() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    world.rows.borrow_mut().push(row("general", 1, 8, say_hi()));
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(&mut notifier, vec![mention("general", 2)]);
    assert_eq!(posts(&notifier).len(), 1);
    assert_eq!(badge(&notifier), Some(1));

    let mut relaunched = world.notifier();
    world.start(&mut relaunched);
    assert!(posts(&relaunched).is_empty(), "announced once");
    assert_eq!(badge(&relaunched), Some(1));
}

#[test]
fn a_count_that_does_not_answer_still_reads_the_new_messages() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 0)];
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.refuse_unread.set(true);
    world.post(&mut notifier, vec![mention("general", 1)]);
    assert_eq!(posts(&notifier).len(), 1, "told without its count");
}
