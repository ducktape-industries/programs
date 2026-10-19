// The program natively over `program::MockHost`, told of frames as the kernel tells a hook.

use ::program::{Env, MockHost, Program, code, hook};
use store::PageRequest;

use crate::{Activity, Frame, PROGRAM, Query, Reply, Used};

mod frames;
mod view;

const ADA: &[u8] = &[1; 32];
const ADA_PHONE: &[u8] = &[2; 32];
const STRANGER: &[u8] = &[3; 32];

fn kernel(height: u64) -> Env {
    Env {
        height,
        time: height * 1000,
        ..MockHost::env(PROGRAM).root()
    }
}

fn record(
    height: u64,
    position: u32,
    signer: &[u8],
    account: Option<u64>,
    target: &str,
) -> hook::Record {
    hook::Record {
        height,
        position,
        signer: signer.to_vec(),
        account,
        target: target.into(),
        applied: true,
    }
}

fn frame(record: &hook::Record) -> Frame {
    Frame {
        height: record.height,
        position: record.position,
        time: record.height * 1000,
        signer: record.signer.clone(),
        target: record.target.clone(),
        applied: record.applied,
    }
}

fn tell(host: &MockHost, record: &hook::Record) {
    ::program::hook::<Activity>(&host.exec(kernel(record.height)), record.clone()).unwrap();
}

fn ask(host: &MockHost, query: Query) -> Reply {
    Activity::query(&host.query(MockHost::env(PROGRAM)), query).unwrap()
}

fn frames(reply: Reply) -> (Vec<Frame>, Option<Vec<u8>>) {
    match reply {
        Reply::Frames(page) => (page.items, page.next),
        other => panic!("{other:?}"),
    }
}

fn told() -> (MockHost, Vec<hook::Record>) {
    let host = MockHost::default();
    let records = vec![
        record(3, 0, ADA, Some(5), "chat"),
        hook::Record {
            applied: false,
            ..record(3, 1, ADA_PHONE, Some(5), "forge")
        },
        record(3, 2, STRANGER, None, "identity"),
        record(4, 0, ADA, Some(5), "chat"),
        record(9, 3, ADA_PHONE, Some(5), "chat"),
    ];
    for record in &records {
        tell(&host, record);
    }
    (host, records)
}

#[test]
fn an_account_reads_what_it_did_newest_first() {
    let (host, records) = told();
    let (acted, next) = frames(ask(
        &host,
        Query::Acted {
            account: 5,
            page: PageRequest::default(),
        },
    ));
    let newest_first: Vec<Frame> = [4, 3, 1, 0].iter().map(|at| frame(&records[*at])).collect();
    assert_eq!(acted, newest_first);
    assert_eq!(next, None);
}

#[test]
fn a_page_resumes_where_the_last_one_stopped() {
    let (host, records) = told();
    let acted = |page| frames(ask(&host, Query::Acted { account: 5, page }));
    let (first, next) = acted(PageRequest::first(3));
    assert_eq!(first.len(), 3);
    let (rest, last) = acted(PageRequest::resume(next, 3));
    assert_eq!(rest, [frame(&records[0])]);
    assert_eq!(last, None);
}

#[test]
fn each_account_counts_its_frames_by_program_and_the_count_pages() {
    let (host, _) = told();
    tell(&host, &record(10, 0, b"\x04", Some(8), "chat"));
    let used = |page| match ask(&host, Query::Used { page }) {
        Reply::Used(page) => (page.items, page.next),
        other => panic!("{other:?}"),
    };
    let count = |account, program: &str, frames| Used {
        account,
        program: program.into(),
        frames,
    };
    let (all, next) = used(PageRequest::default());
    assert_eq!(
        all,
        [
            count(5, "chat", 3),
            count(5, "forge", 1),
            count(8, "chat", 1)
        ]
    );
    assert_eq!(next, None);
    let (first, next) = used(PageRequest::first(2));
    assert_eq!(first, all[..2]);
    assert_eq!(used(PageRequest::resume(next, 2)).0, all[2..]);
}

#[test]
fn only_the_kernel_tells_it_of_a_frame_and_no_one_writes_to_it() {
    let host = MockHost::default();
    let forged = record(3, 0, ADA, Some(5), "chat");
    for env in [
        Env {
            height: 3,
            ..MockHost::env(PROGRAM)
        },
        Env {
            height: 3,
            ..MockHost::env(PROGRAM).from_program("chat", Some(8))
        },
    ] {
        let refused = host.refused(|| ::program::hook::<Activity>(&host.exec(env), forged.clone()));
        assert_eq!(refused.code, code::UNAUTHORIZED);
    }
    let op =
        host.refused(|| ::program::execute::<Activity>(&host.exec(MockHost::env(PROGRAM)), &[]));
    assert_eq!(op.code, code::UNSUPPORTED);
}
