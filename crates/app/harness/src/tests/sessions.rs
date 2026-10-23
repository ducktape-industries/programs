use programs::Programs;

use super::kept::{codes, done};
use super::network::served;
use super::*;
use crate::schema::tests::listed;
use crate::state::{OPEN_BY_AGENT, newest_first};

fn bind(world: &World, channel: &str) -> u64 {
    let room = chat::Op::CreateChannel {
        channel_id: channel.into(),
        name: channel.into(),
        post_policy: chat::PostPolicy::Open,
    };
    world
        .chain
        .submit(signed(ADA), chat::PROGRAM, &room)
        .unwrap();
    let open = Op::OpenThread {
        agent: world.agent,
        place: Place::Channel {
            channel: channel.into(),
        },
    };
    abi::decode(&world.chain.submit(signed(ADA), PROGRAM, &open).unwrap()).unwrap()
}

fn close(world: &World, thread: u64) {
    let close = Op::Close { thread };
    world.chain.submit(signed(ADA), PROGRAM, &close).unwrap();
}

fn asked(world: &World, status: Option<SessionStatus>, request: PageRequest) -> Query {
    Query::Sessions {
        agent: world.agent,
        status,
        page: request,
    }
}

fn page(
    world: &World,
    status: Option<SessionStatus>,
    request: PageRequest,
) -> PageResponse<Session> {
    let asked = asked(world, status, request);
    let Reply::Sessions(page) = world.chain.query(PROGRAM, &asked).unwrap() else {
        panic!("sessions answer as a page")
    };
    page
}

fn pages(world: &World, status: Option<SessionStatus>, limit: Option<u64>) -> Vec<Vec<u64>> {
    let mut pages = Vec::new();
    let mut after = None;
    loop {
        let read = page(world, status, PageRequest { after, limit });
        pages.push(read.items.iter().map(|session| session.row.id).collect());
        match read.next {
            Some(next) => after = Some(next),
            None => return pages,
        }
    }
}

fn row(world: &World, thread: u64) -> ThreadRow {
    let Reply::Thread(row) = world
        .chain
        .query(PROGRAM, &Query::Thread { thread })
        .unwrap()
    else {
        panic!("a thread answers as one")
    };
    row.expect("the thread is there")
}

fn from_before_the_index(chain: &MockChain, agent: AccountNumber, threads: &[u64]) {
    let host = chain.host(PROGRAM);
    let mut host = host.borrow_mut();
    for thread in threads {
        host.state
            .remove(&OPEN_BY_AGENT.key(&(agent, newest_first(*thread))));
    }
}

#[test]
fn a_page_holds_only_conversations_of_its_status_however_many_newer_ones_it_passes() {
    let world = world();
    let running = world.opened("m1", "hello", Some(WINDOW));
    new_thread_retiring_the_current_one(&world);
    let canonical = world.threads()[0].id;
    assert_ne!(
        canonical, running,
        "a new conversation took the canonical place"
    );
    assert_ne!(
        row(&world, running).asking,
        Asking::Idle,
        "the one it superseded still asks"
    );
    assert!(!row(&world, running).closed);
    let open = bind(&world, "a");
    let closed: Vec<u64> = ["b", "c", "d"]
        .into_iter()
        .map(|room| bind(&world, room))
        .collect();
    for thread in &closed {
        close(&world, *thread);
    }
    let newest = bind(&world, "e");
    let [b, c, d] = closed[..] else {
        panic!("three closed conversations")
    };
    let live = Some(SessionStatus::Live);
    assert_eq!(
        pages(&world, live, Some(2)),
        [vec![newest, open], vec![canonical, running]]
    );
    assert_eq!(
        pages(&world, None, None),
        [vec![newest, open, canonical, running]],
        "live unless asked otherwise, in one page of at most twenty"
    );
    let closed = Some(SessionStatus::Closed);
    assert_eq!(pages(&world, closed, Some(2)), [vec![d, c], vec![b]]);
    assert_eq!(
        pages(&world, Some(SessionStatus::All), Some(3)),
        [vec![newest, d, c], vec![b, open, canonical], vec![running]]
    );
    assert_eq!(
        pages(&world, Some(SessionStatus::Running), Some(1)),
        [vec![running]]
    );
    assert_eq!(
        pages(&world, Some(SessionStatus::Idle), Some(2)),
        [vec![newest, open], vec![canonical]]
    );
    assert_eq!(
        pages(&world, Some(SessionStatus::Stuck), Some(1)),
        [Vec::<u64>::new()]
    );
    let first = page(&world, live, PageRequest::first(1));
    let crossed = asked(&world, closed, PageRequest::resume(first.next, 1));
    let refused: Result<Reply, _> = world.chain.query(PROGRAM, &crossed);
    assert!(
        refused.is_err(),
        "a live page's cursor reads no closed list"
    );
    let running_page = page(&world, Some(SessionStatus::Running), PageRequest::first(1));
    let [session] = &running_page.items[..] else {
        panic!("{running_page:?}")
    };
    let model = session.kept.as_ref().map(|kept| kept.model.as_str());
    assert_eq!(
        model,
        Some(MODEL),
        "each session carries the model it keeps"
    );
}

#[test]
fn conversations_open_before_the_index_are_listed_once_new_harness_code_first_runs() {
    let mut world = world();
    world.chain.seat::<Programs>(programs::PROGRAM);
    listed(&world.chain, &codes(b"1"));
    served(&world.chain);
    let world = world;
    let chain = &world.chain;
    let canonical = world.opened("m1", "hello", Some(WINDOW));
    chain.at(3, 0);
    done(chain);
    let side = bind(&world, "side");
    from_before_the_index(chain, world.agent, &[canonical, side]);
    assert_eq!(pages(&world, None, None), [Vec::<u64>::new()]);
    chain.at(4, 0);
    listed(chain, &codes(b"2"));
    world.mention("m2", "still there?");
    assert_eq!(pages(&world, None, None), [vec![side, canonical]]);
}
