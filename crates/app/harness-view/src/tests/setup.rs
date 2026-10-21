use super::*;

const PINNED: &str = "+++\nmodel = \"claude-sonnet-5-5\"\nmax_tokens = 4096\nservice = \"ccs\"\nprovider = \"anthropic\"\n+++\n\nYou are Scout.\n\n  Kept as it is.\n";

fn pinned() -> Net {
    Net {
        files: vec![(path_of(EDDY, "Scout"), PINNED.into())],
        ..Net::default()
    }
}

fn commit_of(path: &str, text: &str) -> files::Op {
    files::Op::Commit {
        base: Some(SNAPSHOT),
        message: "Configure agent Scout (account 12)".into(),
        changes: vec![files::Change::Put {
            path: path.into(),
            exec: false,
            meta: Default::default(),
            content: files::Content::Chunks {
                size: text.len() as u64,
                chunks: vec![BlobId::Sha256(held_as(text.as_bytes()))],
            },
        }],
    }
}

fn open_setup(cx: &mut TestAppContext) {
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("setup-edit");
    cx.run_until_parked();
}

#[test]
fn a_save_rewrites_the_front_matter_alone_and_keeps_the_pins_and_the_body() {
    let (mut cx, _) = seated(pinned(), EDDY);
    files_take(&cx);
    open_setup(&mut cx);
    cx.simulate_click("setup-thinking-4");
    cx.simulate_click("setup-tool-web_search");
    cx.simulate_input("setup-rooms", "dep");
    cx.run_until_parked();
    cx.simulate_click("setup-room-deploys");
    cx.simulate_input("setup-settle", "10000");
    cx.simulate_input("setup-compact-at", "650000");
    cx.run_until_parked();
    assert!(cx.has_text("#deploys"), "{:?}", cx.texts());
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    let written = "+++\nmodel = \"claude-sonnet-5-5\"\nmax_tokens = 4096\nthinking = \"high\"\nserver_tools = [\"web_search\"]\nlisten = [\"deploys\"]\nsettle = \"10s\"\ncompact_at = 650000\nservice = \"ccs\"\nprovider = \"anthropic\"\n+++\n\nYou are Scout.\n\n  Kept as it is.\n";
    assert_eq!(commits(&cx), [commit_of(&path_of(EDDY, "Scout"), written)]);
    let reads: Vec<files::Query> = cx
        .host()
        .requests::<Query<files::Files>>()
        .into_iter()
        .filter(|query| matches!(query, files::Query::Read { .. }))
        .collect();
    let at_snapshot = |query: &files::Query| matches!(query, files::Query::Read { at: Some(at), .. } if *at == SNAPSHOT);
    assert!(reads.iter().all(at_snapshot), "{reads:?}");
    assert!(cx.find("setup-save").is_none(), "{:?}", cx.texts());
}

#[test]
fn a_refused_save_shows_the_sentence_and_keeps_the_editor() {
    let (mut cx, _) = seated(pinned(), EDDY);
    cx.host()
        .refuse::<Submit<files::Files>>("stale", "the file changed since the base snapshot");
    open_setup(&mut cx);
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    assert!(
        cx.has_text("That didn’t go through: the file changed since the base snapshot"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("setup-save").is_some(), "{:?}", cx.texts());
}

#[test]
fn a_number_that_is_none_says_so_and_sends_nothing() {
    let (mut cx, _) = seated(pinned(), EDDY);
    files_take(&cx);
    open_setup(&mut cx);
    cx.simulate_input("setup-max-tokens", "0");
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    assert!(
        cx.has_text("Max output tokens is a whole number, at least 1."),
        "{:?}",
        cx.texts()
    );
    assert!(commits(&cx).is_empty());
}

#[test]
fn the_setup_says_how_harness_read_the_file_and_why_not() {
    let (mut cx, _) = seated(Net::default(), EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    assert!(cx.has_text("Ready"), "{:?}", cx.texts());
    let mut net = Net::default();
    net.setups[0].configured = Configured::Malformed {
        snapshot: SNAPSHOT,
        malformed: harness::Malformed::NoClosingFence,
    };
    let (mut cx, _) = seated(net, EDDY);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    let malformed = "Malformed: its front matter has no closing +++ line";
    assert!(cx.has_text(malformed), "{:?}", cx.texts());
    assert!(cx.find("setup-edit").is_none());
}

#[test]
fn the_edit_form_writes_the_prompt_and_keeps_the_front_matter() {
    let (mut cx, _) = seated(pinned(), EDDY);
    identity_takes(&cx);
    files_take(&cx);
    cx.simulate_click("agent-12");
    cx.run_until_parked();
    cx.simulate_click("agent-edit");
    cx.run_until_parked();
    assert!(cx.find("create-max-tokens").is_none(), "the setup edits it");
    cx.simulate_input("create-system", "You are Scout, on call.");
    cx.simulate_click("create-next");
    cx.run_until_parked();
    let written = "+++\nmodel = \"claude-sonnet-5-5\"\nmax_tokens = 4096\nservice = \"ccs\"\nprovider = \"anthropic\"\n+++\nYou are Scout, on call.\n";
    assert_eq!(commits(&cx), [commit_of(&path_of(EDDY, "Scout"), written)]);
}

#[test]
fn on_a_phone_the_model_page_edits_the_front_matter() {
    let (mut cx, _) = seated(pinned(), EDDY);
    files_take(&cx);
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    for key in ["agent-12", "agent-part-model", "setup-edit"] {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
    cx.simulate_input("setup-wait", "600");
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    let written = "+++\nmodel = \"claude-sonnet-5-5\"\nmax_tokens = 4096\nwait = 600\nservice = \"ccs\"\nprovider = \"anthropic\"\n+++\n\nYou are Scout.\n\n  Kept as it is.\n";
    assert_eq!(commits(&cx), [commit_of(&path_of(EDDY, "Scout"), written)]);
    assert!(cx.find("part-model").is_some(), "{:?}", cx.texts());
}

const UNPROVIDED: &str = "+++\nmodel = \"claude-sonnet-5-5\"\nmax_tokens = 4096\nservice = \"ccs\"\n+++\n\nYou are Scout.\n";

#[test]
fn an_agent_whose_file_names_no_provider_opens_its_setup_to_pick_one() {
    let mut net = Net {
        files: vec![(path_of(EDDY, "Scout"), UNPROVIDED.into())],
        ..Net::default()
    };
    net.setups[0].configured = Configured::Unprovided {
        snapshot: SNAPSHOT,
        config: config_of("Scout"),
    };
    let (mut cx, _) = seated(net, EDDY);
    files_take(&cx);
    open_setup(&mut cx);
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    let pick = "Pick the provider whose route reaches it.";
    assert!(cx.has_text(pick), "{:?}", cx.texts());
    assert!(commits(&cx).is_empty());
    cx.simulate_click("setup-provider-anthropic");
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    let written = "+++\nmodel = \"claude-sonnet-5-5\"\nmax_tokens = 4096\nprovider = \"anthropic\"\n+++\n\nYou are Scout.\n";
    assert_eq!(commits(&cx), [commit_of(&path_of(EDDY, "Scout"), written)]);
}

#[test]
fn a_save_whose_snapshot_was_pruned_between_two_reads_reads_the_file_again_at_the_head() {
    const BEFORE: &str = "+++\nmodel = \"claude-older-model\"\n+++\n\nYou were Scout.\n";
    const FIRST: usize = 24;
    let (mut cx, _) = seated(pinned(), EDDY);
    files_take(&cx);
    let scout = path_of(EDDY, "Scout");
    cx.host()
        .handle::<Query<files::Files>>(move |query| match query {
            files::Query::Guarded { path } => {
                Ok(files::Reply::Guarded((path == scout).then_some(path)))
            }
            files::Query::Read { at, offset, .. } => {
                let (bytes, eof) = match (at, offset) {
                    (Some(_), 0) => (BEFORE.as_bytes()[..FIRST].to_vec(), false),
                    (Some(_), _) => {
                        return Err(view::host::Error::new(
                            files::code::NOT_FOUND,
                            "tree 00 is pruned on this node; an archival node keeps history",
                        ));
                    }
                    (None, offset) => (PINNED.as_bytes()[offset as usize..].to_vec(), true),
                };
                Ok(files::Reply::Contents(files::Contents {
                    snapshot: SNAPSHOT,
                    size: PINNED.len() as u64,
                    offset,
                    bytes,
                    eof,
                }))
            }
            other => panic!("unexpected query: {other:?}"),
        });
    open_setup(&mut cx);
    cx.simulate_click("setup-save");
    cx.run_until_parked();
    assert_eq!(commits(&cx).len(), 1, "{:?}", cx.texts());
    let reads: Vec<(Option<BlobId>, u64)> = cx
        .host()
        .requests::<Query<files::Files>>()
        .into_iter()
        .filter_map(|query| match query {
            files::Query::Read { at, offset, .. } => Some((at, offset)),
            _ => None,
        })
        .collect();
    let restarted_at_the_head_from_its_start = [
        (Some(SNAPSHOT), 0),
        (Some(SNAPSHOT), FIRST as u64),
        (None, 0),
    ];
    assert!(
        reads.ends_with(&restarted_at_the_head_from_its_start),
        "{reads:?}"
    );
    assert_eq!(
        commits(&cx),
        [commit_of(&path_of(EDDY, "Scout"), PINNED)],
        "the head's text whole, none of the pruned read's first chunk"
    );
}
