use super::*;

#[test]
fn a_frame_is_kept_under_the_account_it_acted_as_and_nowhere_else() {
    let (host, _) = told();
    let kept: Vec<Vec<u8>> = host.borrow().state.keys().cloned().collect();
    assert!(!kept.is_empty());
    assert!(
        kept.iter()
            .all(|key| key.starts_with(b"a/") || key.starts_with(b"u/")),
        "nothing network-wide or by key: {kept:?}"
    );
}

#[test]
fn a_frame_no_account_signed_is_kept_nowhere() {
    let host = MockHost::default();
    tell(&host, &record(3, 2, STRANGER, None, "identity"));
    assert!(host.borrow().state.is_empty());
}

#[test]
fn the_retired_signed_tag_is_no_query_and_every_frame_stays_refused_at_its_tag() {
    let (host, _) = told();
    let key = borsh::to_vec(&ADA.to_vec()).unwrap();
    let page = borsh::to_vec(&PageRequest::default()).unwrap();
    let signed = [&[1u8][..], &key, &page].concat();
    let refused =
        ::program::query::<Activity>(&host.query(MockHost::env(PROGRAM)), &signed).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused:?}");
    let undecoded = refused
        .message
        .starts_with("activity: Query did not decode: ");
    assert!(undecoded, "{refused:?}");
    let every = Query::Frames {
        page: PageRequest::default(),
    };
    let refused = Activity::query(&host.query(MockHost::env(PROGRAM)), every).unwrap_err();
    assert_eq!(refused.code, code::UNSUPPORTED);
    assert!(
        refused.message.contains("/v1/blocks") && refused.message.contains("/v1/activity"),
        "{}",
        refused.message
    );
    let tag = |query: &Query| borsh::to_vec(query).unwrap()[0];
    let acted = Query::Acted {
        account: 5,
        page: PageRequest::default(),
    };
    let used = Query::Used {
        page: PageRequest::default(),
    };
    let frames = Query::Frames {
        page: PageRequest::default(),
    };
    assert_eq!([tag(&acted), tag(&used), tag(&frames)], [0, 2, 3]);
}
