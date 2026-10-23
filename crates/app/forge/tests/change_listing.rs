//! Change pages count matching records and resume in number order across forge writes.

mod common;
use common::story::*;
use common::*;
use forge::{ChangeFilter, ChangeState, ChangeSummary, OpReply};
use store::PageResponse;

fn story() -> (Rig, Story) {
    let mut rig = Rig::start(bounds(), HashKind::Sha1);
    let story = Story::pushed(&mut rig);
    (rig, story)
}

fn open(rig: &mut Rig, story: &Story, title: &str) -> u64 {
    let output = rig.execute(&story.open(title)).unwrap();
    let OpReply::Change { n, .. } = abi::decode(&output).unwrap() else {
        panic!("a change number")
    };
    n
}

fn close(rig: &mut Rig, n: u64) {
    rig.execute(&Op::ChangeClose {
        repo: REPO.into(),
        n,
    })
    .unwrap();
}

fn query(filter: ChangeFilter, page: PageRequest) -> Query {
    Query::Changes {
        repo: REPO.into(),
        filter,
        page,
    }
}

fn page(rig: &Rig, filter: ChangeFilter, request: PageRequest) -> PageResponse<ChangeSummary> {
    let Reply::Changes { height, page } =
        abi::decode(&rig.query(&query(filter, request)).unwrap()).unwrap()
    else {
        panic!("a change page")
    };
    assert_eq!(height, rig.height);
    assert_eq!(page.height, height);
    page
}

fn open_filter() -> ChangeFilter {
    ChangeFilter {
        state: Some(ChangeState::Open),
        ..Default::default()
    }
}

fn numbers(page: &PageResponse<ChangeSummary>) -> Vec<u64> {
    page.items.iter().map(|change| change.n).collect()
}

#[test]
fn a_filtered_page_fills_from_matches_and_looks_past_nonmatching_rows() {
    let (mut rig, story) = story();
    for title in ["Closed first", "Closed second", "Closed third"] {
        let n = open(&mut rig, &story, title);
        close(&mut rig, n);
    }
    let first = open(&mut rig, &story, "First open");
    let gap = open(&mut rig, &story, "Closed gap");
    close(&mut rig, gap);
    let second = open(&mut rig, &story, "Second open");
    for title in ["Closed before the lookahead", "Another closed gap"] {
        let n = open(&mut rig, &story, title);
        close(&mut rig, n);
    }
    let third = open(&mut rig, &story, "Third open");
    for title in ["Closed tail", "Last closed"] {
        let n = open(&mut rig, &story, title);
        close(&mut rig, n);
    }

    let first_page = page(&rig, open_filter(), PageRequest::first(2));
    assert_eq!(numbers(&first_page), [first, second]);
    assert!(first_page.next.is_some());
    let last_page = page(&rig, open_filter(), PageRequest::resume(first_page.next, 2));
    assert_eq!(numbers(&last_page), [third]);
    assert_eq!(last_page.next, None);

    let absent = page(
        &rig,
        ChangeFilter {
            state: Some(ChangeState::Merged),
            ..Default::default()
        },
        PageRequest::first(2),
    );
    assert!(absent.items.is_empty());
    assert_eq!(absent.next, None);
}

#[test]
fn author_and_involvement_filters_are_applied_before_the_page_limit() {
    let (mut rig, story) = story();
    let reviewer = rig.principal(b"reviewer");
    let author = rig.principal(TESTER);
    let not_requested = open(&mut rig, &story, "Reviewer removed");
    rig.execute(&Op::ChangeEdit {
        repo: REPO.into(),
        n: not_requested,
        title: None,
        body: None,
        reviewers: Some(vec![]),
    })
    .unwrap();
    rig.actor = b"reviewer".to_vec();
    let other_author = open(&mut rig, &story, "Another author");
    rig.actor = TESTER.to_vec();
    let closed = open(&mut rig, &story, "Closed match");
    close(&mut rig, closed);
    let wanted = open(&mut rig, &story, "Matching author and reviewer");

    let authored = page(
        &rig,
        ChangeFilter {
            author: Some(reviewer.clone()),
            ..Default::default()
        },
        PageRequest::first(1),
    );
    assert_eq!(numbers(&authored), [other_author]);
    assert_eq!(authored.next, None);
    let involved = page(
        &rig,
        ChangeFilter {
            involves: Some(reviewer.clone()),
            ..Default::default()
        },
        PageRequest::first(1),
    );
    assert_eq!(numbers(&involved), [other_author]);
    assert!(involved.next.is_some());
    let combined = page(
        &rig,
        ChangeFilter {
            author: Some(author),
            involves: Some(reviewer),
            state: Some(ChangeState::Open),
        },
        PageRequest::first(1),
    );
    assert_eq!(numbers(&combined), [wanted]);
    assert_eq!(combined.next, None);
}

#[test]
fn a_change_cursor_survives_churn_and_returns_current_records_after_its_number() {
    let (mut rig, mut story) = story();
    let first = open(&mut rig, &story, "First");
    let closed_next = open(&mut rig, &story, "Close before resuming");
    let edited_next = open(&mut rig, &story, "Edit before resuming");
    let first_page = page(&rig, open_filter(), PageRequest::first(1));
    assert_eq!(numbers(&first_page), [first]);
    assert!(first_page.next.is_some());

    rig.execute(&Op::Create {
        repo: "other".into(),
        hash: HashKind::Sha1,
    })
    .unwrap();
    close(&mut rig, closed_next);
    rig.execute(&Op::ChangeEdit {
        repo: REPO.into(),
        n: edited_next,
        title: Some("Edited title".into()),
        body: None,
        reviewers: None,
    })
    .unwrap();
    story.push_follow_up(&mut rig, "later.txt", b"later\n");
    let appended = open(&mut rig, &story, "Opened after the first page");

    let last_page = page(&rig, open_filter(), PageRequest::resume(first_page.next, 2));
    assert_eq!(numbers(&last_page), [edited_next, appended]);
    assert_eq!(last_page.items[0].title, "Edited title");
    assert!(last_page.height > first_page.height);
    assert_eq!(last_page.next, None);
}

#[test]
fn a_change_cursor_survives_a_write_in_its_answering_block() {
    let (mut rig, story) = story();
    let first = open(&mut rig, &story, "First");
    let second = open(&mut rig, &story, "Second");
    let first_page = page(&rig, open_filter(), PageRequest::first(1));
    assert_eq!(numbers(&first_page), [first]);
    assert!(first_page.next.is_some());
    rig.sandbox
        .forge
        .attempt(|| {
            signed_op(
                &rig.sandbox,
                TESTER,
                rig.height,
                &Op::Configure {
                    repo: REPO.into(),
                    settings: Settings::default(),
                },
            )
        })
        .unwrap();
    let last_page = page(&rig, open_filter(), PageRequest::resume(first_page.next, 1));
    assert_eq!(numbers(&last_page), [second]);
    assert_eq!(last_page.height, first_page.height);
    assert_eq!(last_page.next, None);
}

#[test]
fn a_change_cursor_still_belongs_to_its_repository_and_every_filter_argument() {
    let (mut rig, story) = story();
    open(&mut rig, &story, "First");
    open(&mut rig, &story, "Second");
    rig.execute(&Op::Create {
        repo: "other".into(),
        hash: HashKind::Sha1,
    })
    .unwrap();
    let first_page = page(&rig, open_filter(), PageRequest::first(1));
    assert!(first_page.next.is_some());
    let request = PageRequest::resume(first_page.next, 1);
    for filter in [
        ChangeFilter::default(),
        ChangeFilter {
            author: Some(rig.principal(TESTER)),
            ..open_filter()
        },
        ChangeFilter {
            involves: Some(rig.principal(b"reviewer")),
            ..open_filter()
        },
    ] {
        let wrong_filter = query(filter, request.clone());
        assert_eq!(rig.query(&wrong_filter).unwrap_err().code, code::STALE);
    }
    let wrong_repo = Query::Changes {
        repo: "other".into(),
        filter: open_filter(),
        page: request.clone(),
    };
    assert_eq!(rig.query(&wrong_repo).unwrap_err().code, code::STALE);
    let wrong_query = Query::Refs {
        repo: REPO.into(),
        page: request,
    };
    assert_eq!(rig.query(&wrong_query).unwrap_err().code, code::STALE);
    let malformed = query(open_filter(), PageRequest::resume(Some(vec![1]), 1));
    assert_eq!(rig.query(&malformed).unwrap_err().code, code::INVALID_INPUT);
}

#[test]
fn a_change_page_keeps_the_founded_bound_and_can_resume_with_a_different_limit() {
    let mut rig = Rig::start(
        Bounds {
            page_size: 2,
            ..bounds()
        },
        HashKind::Sha1,
    );
    let story = Story::pushed(&mut rig);
    let first = open(&mut rig, &story, "First");
    let second = open(&mut rig, &story, "Second");
    let third = open(&mut rig, &story, "Third");
    let first_page = page(&rig, ChangeFilter::default(), PageRequest::first(3));
    assert_eq!(numbers(&first_page), [first, second]);
    assert!(first_page.next.is_some());
    let last_page = page(
        &rig,
        ChangeFilter::default(),
        PageRequest::resume(first_page.next, 1),
    );
    assert_eq!(numbers(&last_page), [third]);
    assert_eq!(last_page.next, None);
}
