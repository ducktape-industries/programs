//! Chat's notifier against a fake host and a fake device store that outlives
//! its runs, as the app keeps it between launches.
use super::*;
use std::cell::RefCell;
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
    /// Each room's member seats.
    members: Rc<RefCell<Vec<(String, u64)>>>,
    kept: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
}

impl World {
    fn notifier(&self) -> TestNotifier<ChatNotices> {
        let notifier = TestNotifier::new();
        let host = notifier.host();
        let (rooms, rows, members) = (self.rooms.clone(), self.rows.clone(), self.members.clone());
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
                Query::Channels { .. } => Reply::Channels(page(rooms.borrow().clone())),
                // as the program pages it: half before `seq`, half from it
                Query::MessagesAround {
                    channel_id,
                    seq,
                    page: asked,
                    ..
                } => {
                    let half = asked.limit.unwrap_or(0) / 2;
                    Reply::Messages(
                        rows.borrow()
                            .iter()
                            .filter(|row| row.channel_id == channel_id)
                            .filter(|row| row.seq + half >= seq && row.seq <= seq + half)
                            .cloned()
                            .collect(),
                    )
                }
                Query::Thread {
                    channel_id,
                    root_seq,
                    ..
                } => {
                    let rows = rows.borrow();
                    let at = |row: &&MsgRow| row.channel_id == channel_id;
                    Reply::Thread {
                        root: rows
                            .iter()
                            .filter(at)
                            .find(|row| row.seq == root_seq)
                            .cloned(),
                        replies: page(
                            rows.iter()
                                .filter(at)
                                .filter(|row| row.thread == Some(root_seq))
                                .cloned()
                                .collect(),
                        ),
                    }
                }
                Query::Members { channel_id, .. } => Reply::Members(page(
                    members
                        .borrow()
                        .iter()
                        .filter(|(room, _)| *room == channel_id)
                        .map(|(_, account)| chat::MemberRow {
                            principal: Principal::Account(*account),
                            height: 1,
                            time: 0,
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
            let mut rows = self.rows.borrow_mut();
            if let Some(root) = row.thread {
                let root = rows
                    .iter_mut()
                    .find(|kept| kept.channel_id == row.channel_id && kept.seq == root);
                root.expect("a reply's root is posted first").reply_count += 1;
            }
            rows.push(row);
            drop(rows);
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
}

fn posts(notifier: &TestNotifier<ChatNotices>) -> Vec<Notification> {
    notifier.host().requests::<NotifyPost>()
}

fn badge(notifier: &TestNotifier<ChatNotices>) -> Option<i64> {
    notifier.host().requests::<HostBadge>().last().copied()
}

/// Why a message from someone else is meant for me: my direct room, a
/// mention of me, a thread I am in, a room I am in; the first that holds
/// names the notice. My own words, a mention of another, a plain message
/// in a room I am not in, or one from before I came in: none.
#[test]
fn what_is_meant_for_me_and_why() {
    let names = Names::default();
    let chain = "testnet-0a1b2c3d";
    let me = Mark::Mention(Principal::Account(3));
    let mut following = Following::default();
    let why = |row: &MsgRow, following: &Following| meant_for(row, 3, false, following);

    let mention = row("design", 7, 5, said("@me", vec![me.clone()]));
    assert_eq!(why(&mention, &following), Some(Why::Mention));
    let post = notice(&mention, Why::Mention, 3, "design", chain, &names);
    assert_eq!(post.title, "account 5 mentioned you");
    assert_eq!(post.tag, "#design");
    assert_eq!(post.link, "duck://testnet-0a1b2c3d/chat/design/7");

    let other = Mark::Mention(Principal::Account(4));
    assert_eq!(
        why(&row("design", 7, 5, said("@x", vec![other])), &following),
        None
    );
    assert_eq!(
        why(&row("design", 7, 3, said("@me", vec![me])), &following),
        None
    );

    let direct = row("dm-3-5", 7, 5, said("hi", vec![]));
    assert_eq!(why(&direct, &following), Some(Why::Direct));
    let direct = notice(&direct, Why::Direct, 3, "", chain, &names);
    assert_eq!(
        (direct.title.as_str(), direct.body.as_str()),
        ("account 5", "hi")
    );
    assert_eq!(
        why(&row("dm-4-5", 7, 5, said("hi", vec![])), &following),
        None
    );

    // a plain message: meant for me once I wrote in its room, or opened it
    let plain = row("design", 9, 5, said("hi", vec![]));
    assert_eq!(why(&plain, &following), None);
    assert_eq!(meant_for(&plain, 3, true, &following), Some(Why::Room));
    assert!(following.learn(&row("design", 8, 3, said("me too", vec![])), 3));
    assert_eq!(why(&plain, &following), Some(Why::Room));
    assert_eq!(
        why(&row("design", 6, 5, said("before", vec![])), &following),
        None
    );
    let post = notice(&plain, Why::Room, 3, "design", chain, &names);
    assert_eq!(post.title, "account 5 in #design");

    // a reply: meant for me once I wrote in its thread, which puts me in
    // that thread alone
    let answer = reply("review", 12, 5, 10, said("done", vec![]));
    assert_eq!(why(&answer, &following), None);
    assert!(following.learn(&reply("review", 11, 3, 10, said("ok?", vec![])), 3));
    assert_eq!(why(&answer, &following), Some(Why::Thread));
    assert_eq!(
        why(&row("review", 13, 5, said("hi", vec![])), &following),
        None
    );
    assert_eq!(
        why(
            &reply("review", 12, 5, 4, said("other", vec![])),
            &following
        ),
        None
    );
    let post = notice(&answer, Why::Thread, 3, "review", chain, &names);
    assert_eq!(post.title, "account 5 replied in a thread");
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

/// A catch-up of more rooms than are read at once: no turn announces more
/// rooms than it reads, so what lands in a turn stays small however many
/// rooms the reader has, and every room is still announced, once.
#[test]
fn a_catch_up_reads_a_few_rooms_at_a_time() {
    let world = World::default();
    let ids: Vec<String> = (0..5 * READS).map(|n| format!("c{n}")).collect();
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
        let now = kept.values().filter(|seq| **seq == 1).count();
        assert!(
            now - announced <= READS,
            "{now} announced after {announced}"
        );
        announced = now;
    }
    assert_eq!(announced, ids.len());
    assert_eq!(posts(&notifier).len(), ids.len());
    assert_eq!(badge(&notifier), Some(ids.len() as i64));
}

fn titles(notifier: &TestNotifier<ChatNotices>) -> Vec<String> {
    posts(notifier).into_iter().map(|post| post.title).collect()
}

/// A room I wrote in tells me of every message after, and keeps telling at
/// the next run; a room I never wrote in tells me of mentions only.
#[test]
fn a_room_i_wrote_in_tells_me_of_every_message() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 2), room("random", "Random", 1)];
    world.rows.borrow_mut().extend([
        row("general", 1, 8, said("hi", vec![])),
        row("general", 2, 7, said("hello", vec![])),
        row("random", 1, 8, said("hi", vec![])),
    ]);
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(
        &mut notifier,
        vec![
            row("general", 3, 8, said("how are you", vec![])),
            row("random", 2, 8, said("anyone?", vec![])),
        ],
    );
    assert_eq!(titles(&notifier), ["reviewer in #General"]);
    assert_eq!(posts(&notifier)[0].body, "how are you");
    assert_eq!(badge(&notifier), Some(1));

    let kept = world.kept.borrow()["following/0102"].clone();
    let kept: Following = methods::decode(&kept).unwrap();
    assert_eq!(kept.rooms, BTreeMap::from([("general".into(), 2)]));
}

/// A thread I replied in, or was mentioned in, tells me of every reply
/// after, though I never wrote in its room's timeline; a stranger's thread
/// and the room's timeline do not.
#[test]
fn a_thread_i_am_in_tells_me_of_every_reply() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 4)];
    world.rows.borrow_mut().extend([
        MsgRow {
            reply_count: 1,
            ..row("general", 1, 8, said("ship it?", vec![]))
        },
        reply("general", 2, 7, 1, said("not yet", vec![])),
        mention("general", 3),
        row("general", 4, 8, said("lunch?", vec![])),
    ]);
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
}

/// What I write in the room on screen is never read by the notifier: once
/// the room leaves the screen it is looked through again, and what comes
/// after my words tells me.
#[test]
fn what_i_wrote_on_screen_puts_me_in_the_room() {
    let world = World::default();
    *world.rooms.borrow_mut() = vec![room("general", "General", 1)];
    world.rows.borrow_mut().push(row("general", 1, 8, say_hi()));
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    // looked through once already, and I am not in it
    world.post(&mut notifier, vec![row("general", 2, 8, say_hi())]);
    assert!(posts(&notifier).is_empty());

    let screen = notifier.host().stream::<HostScreen>();
    screen.send(vec!["general".into()]);
    notifier.run_until_parked();
    world.post(&mut notifier, vec![row("general", 3, 7, say_hi())]);
    screen.send(Vec::new());
    notifier.run_until_parked();
    world.post(&mut notifier, vec![row("general", 4, 8, say_hi())]);
    assert_eq!(titles(&notifier), ["reviewer in #General"]);
}

/// A members-only room I sit in tells me of every message, though I never
/// wrote there.
#[test]
fn a_seat_in_a_members_only_room_tells_me_of_every_message() {
    let world = World::default();
    let mut team = room("team", "Team", 1);
    team.channel.post_policy = PostPolicy::MembersOnly;
    *world.rooms.borrow_mut() = vec![team, room("open", "Open", 1)];
    world
        .rows
        .borrow_mut()
        .extend([row("team", 1, 8, say_hi()), row("open", 1, 8, say_hi())]);
    world
        .members
        .borrow_mut()
        .extend([("team".into(), 7), ("open".into(), 7)]);
    let mut notifier = world.notifier();
    world.start(&mut notifier);
    world.post(
        &mut notifier,
        vec![row("team", 2, 8, say_hi()), row("open", 2, 8, say_hi())],
    );
    assert_eq!(titles(&notifier), ["reviewer in #Team"]);
}

fn say_hi() -> Vec<Block> {
    said("hi", vec![])
}
