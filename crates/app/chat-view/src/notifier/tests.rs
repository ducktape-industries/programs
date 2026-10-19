//! Chat's notifier against a fake host and a fake device store that outlives
//! its runs, as the app keeps it between launches.
use super::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use chat::{ChannelRow, PageResponse, PostPolicy, Query, Reply, Span};
use view::methods::{
    self, Change, Changes, HostBadge, HostScreen, HostSession, NotifyPost, Query as Ask, Session,
    StoreChanges, StoreGet, StoreSet,
};
use view::testing::TestNotifier;

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

/// A reply under `root`.
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

/// A message from account 8 that mentions account 7, the reader.
fn mention(channel: &str, seq: u64) -> MsgRow {
    row(
        channel,
        seq,
        8,
        said("@eddy", vec![Mark::Mention(Principal::Account(7))]),
    )
}

/// The reader the host seats: key `0102`, account 7, on testnet.
fn session() -> Session {
    Session {
        signer: "0102".into(),
        account: Some(7),
        connected: true,
        chain_id: "testnet#0a1b2c3d".into(),
        ..Session::default()
    }
}

/// The node as the notifier reads it, and the device's store, all of which
/// outlive the notifier's runs.
#[derive(Clone, Default)]
struct World {
    rooms: Rc<RefCell<Vec<ChannelInfo>>>,
    rows: Rc<RefCell<Vec<MsgRow>>>,
    /// Where the reader is in each room, as chat keeps it on chain.
    followed: Rc<RefCell<BTreeMap<String, Followed>>>,
    /// The roots each `Followed` read asked about, in order.
    asked_roots: Rc<RefCell<Vec<Vec<u64>>>>,
    /// Whether a `Followed` read is refused.
    refuse_followed: Rc<Cell<bool>>,
    /// What the reader chose, room by room, as chat keeps it on chain.
    choices: Rc<RefCell<BTreeMap<String, Choice>>>,
    kept: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
}

impl World {
    fn notifier(&self) -> TestNotifier<ChatNotices> {
        let notifier = TestNotifier::new();
        let host = notifier.host();
        let world = self.clone();
        host.handle::<Ask<chat::Chat>>(move |query| {
            Ok(match query {
                Query::Accounts { .. } => Reply::Accounts(page(
                    [(7, "eddy"), (8, "reviewer")]
                        .map(|(number, name)| chat::Profile {
                            number,
                            name: name.into(),
                            kind: chat::Kind::User,
                        })
                        .into(),
                )),
                Query::Channels { .. } => Reply::Channels(page(world.rooms.borrow().clone())),
                // as the program pages it: half before `seq`, half from it
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
                query => panic!("unexpected chat query: {query:?}"),
            })
        });
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

    /// A run started as a connect starts it: the reader seated, nothing on
    /// screen.
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

    /// `rows` posted: each room's head moves to its newest, and a block
    /// that wrote the heads reaches the notifier.
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

    /// The reader chose `choice` for `room`: a block that wrote what chat
    /// keeps of it reaches the notifier.
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

/// Why a message from someone else is meant for me: my direct room, a
/// mention of me, a thread I am in, a room's timeline I am in; the first
/// that holds names the notice. My own words, a mention of another, a
/// plain message in a room I am not in, one from before I came in, or a
/// reply in a thread I am not in: none.
#[test]
fn what_is_meant_for_me_and_why() {
    let names = Names::default();
    let chain = "testnet-0a1b2c3d";
    let me = Mark::Mention(Principal::Account(3));
    let nowhere = Followed::default();
    let why = |row: &MsgRow, followed: &Followed| meant_for(row, 3, followed, Choice::Default);

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

    // in the room since 8, and in thread 10 since 11
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

/// What I chose for a room goes over where I am: every message, a reply in
/// a stranger's thread too; mentions and my direct room only; or nothing.
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
    let why = |row: &MsgRow, followed: &Followed, choice| meant_for(row, 3, followed, choice);

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

/// No pane is open: what was there at the first run is history, a mention
/// that lands after is one notice and counts on the badge, and the reader
/// reading the room in a pane (its cursor moved in the store) takes the
/// badge down.
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

    // a pane read the room: another instance moved the cursor
    world.keep("reads/0102", &BTreeMap::from([("general".into(), 2)]));
    notifier
        .host()
        .stream::<StoreChanges>()
        .send("reads/0102".into());
    notifier.run_until_parked();
    assert_eq!(badge(&notifier), Some(0));
    assert_eq!(posts(&notifier).len(), 1);
}

/// What came for the reader while nothing ran (the app closed) is
/// announced once at the next run: past both what was announced and the
/// reader's cursor, a room first seen read from its start, and not again
/// at the run after. The run waits for the reader's account.
#[test]
fn what_came_while_closed_is_announced_once() {
    let world = World::default();
    world.keep("notified/0102", &BTreeMap::from([("general".into(), 1)]));
    // the reader has the direct room listed, unread, but it was never
    // announced: new since the last run
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

    // the next launch: nothing new, nothing posted, the badge counted again
    let mut relaunched = world.notifier();
    world.start(&mut relaunched);
    assert!(posts(&relaunched).is_empty(), "announced once");
    assert_eq!(badge(&relaunched), Some(2));
}

/// The room a pane shows on screen is in front of the reader: a mention in
/// it is no notice and no badge, and stays none once the pane leaves it.
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

    // a room off screen still is news
    world.post(&mut notifier, vec![mention("random", 1)]);
    assert_eq!(posts(&notifier).len(), 1);
    assert_eq!(badge(&notifier), Some(1));
}

// A catch-up of many rooms reads every room that moved, all at once, and
// announces each, once.
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

/// A room I am in tells me of every message in its timeline after I came
/// in, not of a reply in a thread I am not in; a room I am not in, of
/// mentions only.
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

/// A thread I am in tells me of every reply after I came in, though I am
/// not in its room; a stranger's thread does not. Each read asks chat once
/// where I am, about the threads its new messages reply in.
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

/// A choice made while the notifier runs counts the room again from the
/// reader's cursor: a room cut to nothing leaves the badge, one opened to
/// every message counts what came and tells of the next.
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

    // counted again from the cursor, with no cursor kept (as on a server):
    // both mentions count, and neither is posted again
    world.choose(&mut notifier, "general", Choice::Every);
    assert_eq!(badge(&notifier), Some(2));
    assert_eq!(posts(&notifier).len(), 1);
    world.post(&mut notifier, vec![row("general", 4, 8, say_hi())]);
    assert_eq!(titles(&notifier)[1..], ["reviewer in #General"]);
}

/// Where I am will not read: what is meant for me whatever room it is in,
/// a mention, is still told.
#[test]
fn a_mention_is_told_though_where_i_am_will_not_read() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 0)];
    world.refuse_followed.set(true);
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(&mut notifier, vec![mention("general", 1)]);
    assert_eq!(titles(&notifier), ["reviewer mentioned you"]);
}

fn say_hi() -> Vec<Block> {
    said("hi", vec![])
}
