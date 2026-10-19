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
fn every_frame_and_the_frames_of_a_key_are_refused_naming_where_the_node_lists_them() {
    let (host, _) = told();
    let every = Query::Frames {
        page: PageRequest::default(),
    };
    let by_key = Query::Signed {
        key: ADA.to_vec(),
        page: PageRequest::default(),
    };
    for query in [every, by_key] {
        let refused = Activity::query(&host.query(MockHost::env(PROGRAM)), query).unwrap_err();
        assert_eq!(refused.code, code::UNSUPPORTED);
        assert!(
            refused.message.contains("/v1/blocks") && refused.message.contains("/v1/activity"),
            "{}",
            refused.message
        );
    }
}
