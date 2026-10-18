//! Forge under a finger (`ui::phone`): each phone screen in the states the
//! Expo app draws it (full, empty, being read, refused) and each of its
//! actions; the width sweep at 360, 390 and 430 (SPEC §8.1d-1); and the
//! desktop kept under a pointer at every width.
use super::*;
use view::testing;

/// Forge on a 390 wide phone, seated as Ada (account 1: `project`'s owner
/// and change 1's author), once `mode`'s answers are set.
fn phone(mode: &'static str) -> TestAppContext {
    let mut cx = TestAppContext::new();
    configure(&mut cx, mode);
    seat_phone(cx)
}

/// Forge on a phone whose reads `answer` answers.
fn phone_answered(
    answer: impl Fn(&Query) -> Result<Reply, view::host::Error> + 'static,
) -> TestAppContext {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    cx.host().handle::<Ask>(move |query| answer(&query));
    seat_phone(cx)
}

fn seat_phone(cx: TestAppContext) -> TestAppContext {
    seat_view(cx, Some(1)).0
}

/// Forge on a phone and its view, seated as Ada, once `mode`'s answers
/// are set.
fn phone_view(mode: &'static str) -> (TestAppContext, Entity<Forge>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx, mode);
    seat_view(cx, Some(1))
}

/// Forge on a 390 wide phone, seated holding `account`.
fn seat_view(mut cx: TestAppContext, account: Option<u64>) -> (TestAppContext, Entity<Forge>) {
    let props = cx.host().stream::<HostSession>();
    let view = cx.open::<Forge>();
    cx.simulate_touch(true);
    cx.simulate_resize(390., 844.);
    cx.run_until_parked();
    props.send(seat(b"owner", account));
    cx.run_until_parked();
    (cx, view)
}

/// Every forge write refused, as the program's prediction says it.
fn refuse_writes(cx: &mut TestAppContext) {
    cx.host().handle::<SubmitForge>(|_| {
        Ok(view::methods::Submitted::Refused {
            frame: [0; 32],
            until: 0,
            refusal: refusal("refused-stale"),
        })
    });
}

/// The settings of `project` as the phone shows them.
fn shown_settings(view: &Entity<Forge>) -> forge::Settings {
    view.read(|forge| forge.repo().map(|(info, _)| info.repo.settings.clone()))
        .expect("the repository is read")
}

fn configured(cx: &TestAppContext) -> Vec<forge::Settings> {
    submitted(cx)
        .into_iter()
        .filter_map(|op| match op {
            forge::Op::Configure { settings, .. } => Some(settings),
            _ => None,
        })
        .collect()
}

fn press(cx: &mut TestAppContext, id: &str) {
    cx.simulate_click(id);
    cx.run_until_parked();
}

/// `project` open on the phone.
fn repository(mode: &'static str) -> TestAppContext {
    let mut cx = phone(mode);
    press(&mut cx, "forge-repo-project");
    cx
}

/// Change 1 of `project` open on the phone.
fn change(mode: &'static str) -> TestAppContext {
    let mut cx = repository(mode);
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-change-project-1");
    cx
}

/// `src/lib.rs` of change 1 open on its own page.
fn file(mode: &'static str) -> TestAppContext {
    let mut cx = change(mode);
    press(&mut cx, "forge-change-tab-files");
    press(&mut cx, "forge-file-src/lib.rs");
    cx
}

fn submitted(cx: &TestAppContext) -> Vec<forge::Op> {
    cx.host().requests::<SubmitForge>()
}

// ------------------------------------------------------------------ Forge

#[test]
fn the_root_lists_what_needs_my_judgment_then_every_repository() {
    let cx = phone("default");
    assert!(cx.find("forge-page-repos").is_some());
    assert_eq!(
        cx.back_handler(),
        None,
        "the root has nothing to go back to"
    );
    assert!(cx.has_text("Forge"));
    assert!(cx.has_text("testnet · Ada"), "{:?}", cx.texts());
    assert!(cx.has_text("Needs my judgment"));
    assert!(cx.find("forge-judgment-list").is_some());
    assert!(cx.has_text("Repositories"));
    assert!(cx.find("forge-repo-project").is_some());
}

#[test]
fn a_network_without_repositories_says_how_one_begins() {
    let cx = phone("empty");
    assert!(cx.has_text("No repositories yet"));
    assert!(cx.has_text("Create one with New repository, then push to it."));
}

#[test]
fn a_filter_that_keeps_no_repository_says_nothing_matches() {
    let mut cx = phone("default");
    cx.simulate_input("forge-repos-search", "nothing-by-this-name");
    cx.run_until_parked();
    assert!(cx.has_text("Nothing matches"));
    assert!(cx.find("forge-repo-project").is_none());
}

#[test]
fn the_repositories_say_so_while_they_are_read() {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    cx.host().never::<Ask>();
    let cx = seat_phone(cx);
    assert!(cx.has_text("Reading the repositories…"));
}

#[test]
fn a_refused_repository_list_says_why_and_retries() {
    let mut cx = phone_answered(|_| Err(refusal("refused-object-not-held")));
    let sentence = "object ffffffffffffffffffffffffffffffffffffffff is not held by this node";
    assert!(cx.has_text(sentence), "{:?}", cx.texts());
    let asked = cx.host().requests::<Ask>().len();
    press(&mut cx, "forge-repos-list-retry");
    assert!(
        cx.host().requests::<Ask>().len() > asked,
        "Retry asks again"
    );
}

#[test]
fn new_repository_opens_a_sheet_that_refuses_a_bad_name_then_creates() {
    let mut cx = phone("default");
    press(&mut cx, "forge-new-repo");
    assert!(cx.find("forge-new-repo-sheet").is_some());
    assert!(
        cx.find("forge-new-repo-head").is_some(),
        "the sheet's head is drawn over its form"
    );
    assert!(cx.back_handler().is_some(), "Back closes the sheet");
    cx.simulate_input("forge-new-repo-name", "bad name");
    press(&mut cx, "forge-new-repo-create");
    let refusal = "A repository name is letters, digits, dot, dash or underscore";
    assert!(cx.has_text(refusal));
    let view::wire::Node::Field { options, .. } = cx.node("forge-new-repo-name") else {
        panic!("the name is a field");
    };
    assert_eq!(options.invalid, Some(view::accesskit::Invalid::True));
    assert_eq!(
        options.description.as_deref(),
        Some(refusal),
        "a screen reader hears why"
    );
    cx.simulate_input("forge-new-repo-name", "garden");
    press(&mut cx, "forge-new-repo-create");
    assert!(cx.find("forge-new-repo-sheet").is_none());
    assert!(
        submitted(&cx)
            .iter()
            .any(|op| matches!(op, forge::Op::Create { repo, .. } if repo == "garden"))
    );
}

// ------------------------------------------------------------------ a repository

#[test]
fn a_repository_names_its_owner_its_ref_and_six_tabs() {
    let cx = repository("default");
    assert!(cx.find("forge-page-repo").is_some());
    assert!(cx.find("forge-page-repo-back").is_some());
    assert!(cx.back_handler().is_some());
    assert!(cx.find("forge-ref-picker").is_some(), "the ref button");
    for tab in ["readme", "code", "commits", "changes", "refs", "settings"] {
        assert!(cx.find(&format!("forge-tab-{tab}")).is_some(), "{tab}");
    }
    assert!(
        cx.find("forge-readme-body").is_some(),
        "it opens on its README"
    );
}

#[test]
fn about_opens_as_a_sheet_and_back_closes_it_first() {
    let mut cx = repository("default");
    press(&mut cx, "forge-dock-about");
    assert!(cx.find("forge-about-sheet").is_some());
    assert!(cx.has_text("Default head"));
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-about-sheet").is_none());
    assert!(cx.find("forge-page-repo").is_some(), "the page stays");
}

#[test]
fn back_closes_what_is_drawn_not_an_about_left_open_under_a_change() {
    let mut cx = repository("default");
    press(&mut cx, "forge-dock-about");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-change-project-1");
    assert!(
        cx.find("forge-about-sheet").is_none(),
        "a change has no About"
    );
    cx.simulate_back();
    cx.run_until_parked();
    assert!(
        cx.find("forge-page-change").is_none(),
        "Back pops the change"
    );
    assert!(cx.find("forge-page-repo").is_some());
}

#[test]
fn copy_puts_the_address_on_the_clipboard_and_its_glyph_says_so() {
    let mut cx = repository("default");
    press(&mut cx, "forge-repo-copy");
    assert_eq!(
        cx.interactivity("forge-repo-copy").aria.label.as_deref(),
        Some("Address copied")
    );
}

#[test]
fn the_ref_button_opens_the_ref_sheet_and_a_pick_closes_it() {
    let mut cx = repository("default");
    press(&mut cx, "forge-ref-picker");
    assert!(cx.find("forge-ref-picker-sheet").is_some());
    assert!(cx.has_text("Pick a ref"));
    press(&mut cx, "forge-ref-refs/heads/clean");
    assert!(cx.find("forge-ref-picker-sheet").is_none());
    assert!(cx.has_text("clean"));
}

#[test]
fn every_tab_draws_its_body() {
    let mut cx = repository("default");
    for (tab, body) in [
        ("code", "forge-tree-list"),
        ("commits", "forge-log-list"),
        ("changes", "forge-change-list"),
        ("refs", "forge-refs-list"),
        ("settings", "forge-settings"),
        ("readme", "forge-readme-body"),
    ] {
        press(&mut cx, &format!("forge-tab-{tab}"));
        assert!(cx.find(body).is_some(), "{tab} draws {body}");
    }
}

// ------------------------------------------------------------------ a change

#[test]
fn a_change_shows_its_state_who_wants_what_and_four_tabs() {
    let cx = change("default");
    assert!(cx.find("forge-page-change").is_some());
    assert!(cx.find("forge-change-state").is_some());
    assert!(
        cx.texts().iter().any(|text| text.contains(" wants ")),
        "{:?}",
        cx.texts()
    );
    for tab in ["conversation", "commits", "files", "details"] {
        assert!(
            cx.find(&format!("forge-change-tab-{tab}")).is_some(),
            "{tab}"
        );
    }
    assert!(cx.find("forge-merge").is_some());
    assert!(cx.find("forge-start-review").is_some());
    assert!(cx.find("forge-reply").is_some());
}

/// The strip says each tab's count whichever tab is open, as Expo's
/// does: the change's conversation and its diff are read with it.
#[test]
fn the_strip_counts_every_tab_whichever_is_open() {
    let mut cx = change("reviewed");
    let said = |cx: &TestAppContext, tab: &str| -> Vec<String> {
        let tab = cx.node(&format!("forge-change-tab-{tab}"));
        marked(tab).into_iter().map(|(text, _)| text).collect()
    };
    assert_eq!(said(&cx, "files").len(), 2, "{:?}", said(&cx, "files"));
    press(&mut cx, "forge-change-tab-files");
    let conversation = said(&cx, "conversation");
    assert_eq!(conversation.len(), 2, "{conversation:?}");
}

#[test]
fn the_conversation_lists_the_reviews_as_advisory_and_the_replies() {
    let cx = change("reviewed");
    assert!(cx.has_text("Reviews"));
    assert!(cx.has_text("advisory"));
    assert!(cx.has_text("Replies"));
    assert!(!cx.has_text("Nobody has reviewed this change yet."));
}

#[test]
fn a_change_nobody_reviewed_says_so() {
    let cx = change("default");
    assert!(
        cx.has_text("Nobody has reviewed this change yet."),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn merge_says_why_it_is_off_over_the_bar() {
    let cx = change("diverged");
    assert!(cx.has_text(
        "The endpoints diverged. Merge with git and push the result, then Merge turns on."
    ));
    assert!(disabled(&cx, "forge-merge"));
}

#[test]
fn a_refused_change_says_why_in_its_place() {
    let cx = {
        let mut cx = phone_answered(|query| match query {
            Query::Change { .. } => Err(refusal("refused-object-not-held")),
            other => Ok(answer(other, "default")),
        });
        press(&mut cx, "forge-repo-project");
        press(&mut cx, "forge-tab-changes");
        press(&mut cx, "forge-change-project-1");
        cx
    };
    assert!(
        cx.find("forge-change-refused").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("forge-change-retry").is_some());
}

#[test]
fn start_review_pins_a_review_and_opens_the_files() {
    let mut cx = change("default");
    press(&mut cx, "forge-start-review");
    assert!(cx.find("forge-files").is_some(), "the Files tab");
    assert!(cx.has_text("Tick a file once you have read it."));
}

/// The host's capsule beside the title row draws `ellipsis` and is named
/// More: a change's own Actions draw `ellipsis-vertical` and say Actions.
#[test]
fn a_changes_actions_are_not_drawn_as_the_capsules_more() {
    let cx = change("default");
    let row = format!("{:?}", cx.node("forge-page-change-title"));
    assert!(!row.contains("icons/ellipsis.svg"), "{row}");
    let actions = format!("{:?}", cx.node("forge-change-actions"));
    assert!(actions.contains("icons/ellipsis-vertical.svg"), "{actions}");
    let name = &cx.interactivity("forge-change-actions").aria.label;
    assert_eq!(name.as_deref(), Some("Actions"));
}

#[test]
fn actions_offer_edit_close_and_copy_link_and_close_asks_again() {
    let mut cx = change("default");
    press(&mut cx, "forge-change-actions");
    assert!(
        matches!(cx.node("forge-change-actions-sheet"),
            view::wire::Node::Sheet { label: Some(label), .. } if label == "Actions"),
        "the sheet is named as its control"
    );
    assert!(cx.find("forge-change-edit").is_some());
    assert!(cx.find("forge-change-copy-link").is_some());
    press(&mut cx, "forge-change-close");
    assert!(cx.has_text("Closing is final: a closed change cannot be reopened."));
    assert!(submitted(&cx).is_empty(), "the first press only asks");
    press(&mut cx, "forge-close-confirm");
    assert!(
        submitted(&cx)
            .iter()
            .any(|op| matches!(op, forge::Op::ChangeClose { n: 1, .. }))
    );
    assert!(cx.find("forge-change-actions-sheet").is_none());
}

#[test]
fn the_files_tab_lists_each_file_and_a_tick_marks_it_read() {
    let mut cx = change("default");
    press(&mut cx, "forge-change-tab-files");
    assert!(cx.find("forge-file-src/lib.rs").is_some());
    assert!(cx.find("forge-all-files").is_some());
    press(&mut cx, "forge-viewed-src/lib.rs");
    assert!(
        cx.find("forge-page-change").is_some(),
        "the tick does not open the file"
    );
    assert_eq!(
        cx.interactivity("forge-viewed-src/lib.rs").aria.toggled,
        Some(view::Toggled::True)
    );
}

#[test]
fn details_name_the_reviews_the_merge_status_the_line_comments_and_the_channel() {
    let mut cx = change("reviewed");
    press(&mut cx, "forge-change-tab-details");
    assert!(cx.find("forge-details").is_some());
    for said in [
        "Merge status",
        "Mergeability",
        "Source head",
        "Line comments",
        "Channel",
    ] {
        assert!(cx.has_text(said), "{said}");
    }
}

// ------------------------------------------------------------------ one file of a change

#[test]
fn a_file_page_walks_its_files_and_says_how_to_comment() {
    let cx = file("default");
    assert!(cx.find("forge-page-file").is_some());
    assert!(cx.has_text("lib.rs"));
    assert!(cx.find("forge-file-previous").is_some());
    assert!(cx.find("forge-file-next").is_some());
    assert!(cx.has_text("Comment on any line by pressing its number."));
}

#[test]
fn pressing_a_line_number_opens_the_composer_and_stage_keeps_the_comment() {
    let mut cx = file("default");
    let first = (0..64)
        .map(|index| format!("forge-diff-gutter-{index}"))
        .find(|id| cx.find(id).is_some())
        .unwrap_or_else(|| panic!("no line number to press"));
    press(&mut cx, &first);
    assert!(cx.find("forge-comment-body").is_some(), "the composer");
    cx.simulate_input("forge-comment-body", "trim first");
    press(&mut cx, "forge-comment-stage");
    assert!(cx.has_text("Reviewing · 1 staged"));
    assert!(cx.has_text("Your pending comment"));
}

#[test]
fn a_published_comment_sits_under_its_line() {
    let cx = file("reviewed");
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("line ") && text.contains(" · block ")),
        "{:?}",
        cx.texts()
    );
}

// ------------------------------------------------------------------ Finish your review

#[test]
fn finish_review_opens_the_sheet_and_submit_sends_one_review() {
    let mut cx = change("default");
    press(&mut cx, "forge-start-review");
    press(&mut cx, "forge-change-tab-conversation");
    press(&mut cx, "forge-finish-review");
    assert!(cx.find("forge-finish-sheet").is_some());
    assert!(cx.has_text("Finish your review"));
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("0 line comments staged against "))
    );
    press(&mut cx, "forge-verdict-approve");
    press(&mut cx, "forge-submit-review");
    let sent = submitted(&cx);
    assert!(sent.iter().any(|op| matches!(op, forge::Op::ReviewSubmit { review, .. } if review.verdict == forge::Verdict::Approve)));
}

/// An empty comment review is refused before it is sent, in Expo's
/// words (`review.tsx:46`).
#[test]
fn an_empty_comment_review_says_say_something_or_discard() {
    let mut cx = change("default");
    press(&mut cx, "forge-start-review");
    press(&mut cx, "forge-change-tab-conversation");
    press(&mut cx, "forge-finish-review");
    let before = submitted(&cx).len();
    press(&mut cx, "forge-verdict-comment");
    press(&mut cx, "forge-submit-review");
    assert_eq!(submitted(&cx).len(), before);
    assert!(
        cx.has_text("Say something, or discard."),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn discard_drops_the_review() {
    let mut cx = change("default");
    press(&mut cx, "forge-start-review");
    press(&mut cx, "forge-change-tab-conversation");
    press(&mut cx, "forge-finish-review");
    press(&mut cx, "forge-discard-review-sheet");
    assert!(cx.find("forge-finish-sheet").is_none());
    assert!(
        cx.find("forge-start-review").is_some(),
        "no review under way"
    );
}

// ------------------------------------------------------------------ a commit

#[test]
fn a_commit_shows_its_facts_its_files_and_their_diff() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-commits");
    press(
        &mut cx,
        "forge-commit-26607f522099476177a45a8058a93108fba5a84d",
    );
    assert!(cx.find("forge-page-commit").is_some());
    for said in ["Commit", "Tree", "Parents", "Author", "Files"] {
        assert!(cx.has_text(said), "{said}: {:?}", cx.texts());
    }
    assert!(cx.find("forge-commit-diff").is_some());
}

// ------------------------------------------------------------------ a file of the Code tab

/// A folder of the Code tab opens on its own page (Expo 33): `src/` over
/// the repository and the ref, the crumbs over its rows; the crumb of the
/// repository, and Back, go back to the tab.
#[test]
fn a_folder_opens_on_its_own_page_and_its_crumb_goes_back() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-code");
    press(&mut cx, "forge-tree-src");
    assert!(cx.find("forge-page-folder").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("src/"));
    assert!(cx.find("forge-folder-bar").is_some());
    press(&mut cx, "forge-crumb-0");
    assert!(cx.find("forge-page-folder").is_none());
    assert!(cx.find("forge-tree-src").is_some(), "the tab's root again");
    press(&mut cx, "forge-tree-src");
    cx.simulate_back();
    cx.run_until_parked();
    assert!(
        cx.find("forge-page-folder").is_none(),
        "Back pops the folder"
    );
    assert!(cx.find("forge-page-repo").is_some());
}

/// `project` with folders `a/b/c` (no fixture nests a folder in a
/// folder): the root's first page holds `a`, `a` holds `b`, `a/b` holds
/// `c`, and each holds `src`'s files as well. Seated on the list.
fn nested() -> (TestAppContext, Entity<Forge>) {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    cx.host().handle::<Ask>(|query| {
        let mut reply = answer(&query, "default");
        let inner: &[u8] = match &query {
            Query::Tree {
                path,
                page: PageRequest { after: None, .. },
                ..
            } => match path.as_slice() {
                b"" => b"a",
                b"a" => b"b",
                b"a/b" => b"c",
                _ => return Ok(reply),
            },
            _ => return Ok(reply),
        };
        if let Reply::Tree { page, .. } = &mut reply {
            page.items.push(forge::TreeInfo {
                name: inner.to_vec(),
                oid: format!("{:040x}", inner[0]),
                kind: forge::EntryKind::Directory,
            });
        }
        Ok(reply)
    });
    seat_view(cx, Some(1))
}

/// [`nested`] on `project`'s Code tab.
fn nested_view() -> (TestAppContext, Entity<Forge>) {
    let (mut cx, view) = nested();
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-code");
    (cx, view)
}

/// `path`s, each pressed in turn: `a`, `a/b`, ... down the tree.
fn walk(cx: &mut TestAppContext, paths: &[&str]) {
    for path in paths {
        press(cx, &format!("forge-tree-{path}"));
    }
}

/// The depth the folder page on top crosses with, and the folder it is.
fn folder_page(cx: &TestAppContext, view: &Entity<Forge>) -> (Option<u32>, Vec<u8>) {
    let depth = cx.interactivity("forge-page-folder").page;
    (
        depth,
        view.read(|forge| forge.nav().folder.clone()).unwrap(),
    )
}

/// Each folder is a page of its own over its parent's, as Expo pushes a
/// screen per folder (`code/[...path].tsx`): `a`, `a/b` and `a/b/c` sit
/// one, two and three over the Code tab, so each opens with a push, and a
/// file of `a/b/c` one over that; Back walks up one page at a time, each
/// a pop, to the tab.
#[test]
fn each_folder_is_a_page_over_its_parent_and_back_walks_up_one() {
    let (mut cx, view) = nested_view();
    assert_eq!(
        cx.interactivity("forge-page-repo").page,
        Some(1),
        "the Code tab, over the repositories"
    );
    for (up, path) in [(1, "a"), (2, "a/b"), (3, "a/b/c")] {
        walk(&mut cx, &[path]);
        assert_eq!(folder_page(&cx, &view), (Some(1 + up), path.into()));
    }
    walk(&mut cx, &["a/b/c/lib.rs"]);
    assert_eq!(cx.interactivity("forge-page-blob").page, Some(5));
    for (up, path) in [(3, "a/b/c"), (2, "a/b"), (1, "a")] {
        cx.simulate_back();
        cx.run_until_parked();
        assert_eq!(folder_page(&cx, &view), (Some(1 + up), path.into()));
    }
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-page-folder").is_none());
    assert_eq!(cx.interactivity("forge-page-repo").page, Some(1));
}

/// A crumb goes up every folder it skips at once, as Expo's
/// `router.dismiss(depth - i)`: `a` from `a/b/c` is a pop of two.
#[test]
fn a_crumb_pops_every_folder_under_it() {
    let (mut cx, view) = nested_view();
    walk(&mut cx, &["a", "a/b", "a/b/c"]);
    press(&mut cx, "forge-crumb-1");
    assert_eq!(folder_page(&cx, &view), (Some(2), b"a".to_vec()));
}

/// A link into `a/b` opens it at its own depth with `a`'s page under it:
/// Back goes to `a`, then to the tab, as a tap down to `a/b` would.
#[test]
fn a_link_into_a_folder_lands_at_its_depth_over_its_parents() {
    let (mut cx, view) = nested_view();
    cx.update(&view, |forge, _, cx| forge.follow_link(b"", "a/b", cx));
    cx.run_until_parked();
    assert_eq!(folder_page(&cx, &view), (Some(3), b"a/b".to_vec()));
    cx.simulate_back();
    cx.run_until_parked();
    assert_eq!(folder_page(&cx, &view), (Some(2), b"a".to_vec()));
    cx.simulate_back();
    cx.run_until_parked();
    assert_eq!(cx.interactivity("forge-page-repo").page, Some(1));
}

/// While a link's folder is still being read its page already stands
/// where the path lands, so the link slides once (Expo's
/// `code/[...path].tsx` before it reads its entry). Back from it goes to
/// its folder and forgets the link, so the read landing later moves
/// nothing.
#[test]
fn a_link_stands_at_its_depth_while_its_folder_is_read() {
    let (mut cx, view) = nested_view();
    cx.host().never::<Ask>();
    cx.update(&view, |forge, _, cx| forge.follow_link(b"", "a/b", cx));
    cx.run_until_parked();
    assert_eq!(cx.interactivity("forge-page-landing").page, Some(3));
    assert!(cx.has_text(crate::words::READING_TREE), "{:?}", cx.texts());
    cx.simulate_back();
    cx.run_until_parked();
    assert_eq!(folder_page(&cx, &view), (Some(2), b"a".to_vec()));
    view.read(|forge| assert_eq!(forge.nav().goto, None));
}

/// A route into a path of a repository (a file page's Copy link) opens
/// it at its depth from anywhere: a folder over its parents, a file over
/// its folder; a number after the name is still the change.
#[test]
fn a_route_into_a_path_opens_it_at_its_depth() {
    let (mut cx, view) = nested();
    let route = |cx: &mut TestAppContext, to: &str| {
        cx.update(&view, |forge, _, cx| forge.open_route(to, cx));
        cx.run_until_parked();
    };
    route(&mut cx, "project/a/b");
    assert_eq!(folder_page(&cx, &view), (Some(3), b"a/b".to_vec()));
    route(&mut cx, "project/a/b/lib.rs");
    assert_eq!(cx.interactivity("forge-page-blob").page, Some(4));
    cx.simulate_back();
    cx.run_until_parked();
    assert_eq!(folder_page(&cx, &view), (Some(3), b"a/b".to_vec()));
    route(&mut cx, "project/1");
    assert!(cx.find("forge-page-change").is_some(), "{:?}", cx.texts());
}

/// A link to the README from a file deep in the Code tab opens the
/// README tab and leaves the Code tab's pages: its next press opens on
/// its root, not on the file the link was pressed in.
#[test]
fn a_readme_link_leaves_the_code_tabs_pages() {
    let (mut cx, view) = nested_view();
    walk(&mut cx, &["a", "a/b", "a/b/lib.rs"]);
    cx.update(&view, |forge, _, cx| {
        forge.follow_link(b"a/b", "../../README.md", cx)
    });
    cx.run_until_parked();
    assert_eq!(cx.interactivity("forge-page-repo").page, Some(1));
    view.read(|forge| assert_eq!(forge.nav().tab, RepoTab::Readme));
    press(&mut cx, "forge-tab-code");
    assert_eq!(cx.interactivity("forge-page-repo").page, Some(1));
    assert!(cx.find("forge-tree-a").is_some(), "the Code tab's root");
}

#[test]
fn a_file_shows_its_lines_and_wrap_flips() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-code");
    press(&mut cx, "forge-tree-empty.txt");
    assert!(cx.find("forge-page-blob").is_some());
    assert!(cx.find("forge-blob-lines").is_some(), "{:?}", cx.texts());
    press(&mut cx, "forge-wrap");
    assert!(cx.find("forge-blob-lines").is_some());
}

#[test]
fn a_markdown_file_reads_as_its_document_until_source() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-code");
    press(&mut cx, "forge-tree-README.md");
    assert!(cx.find("forge-blob-document").is_some());
    press(&mut cx, "forge-source");
    assert!(cx.find("forge-blob-lines").is_some());
    assert!(
        cx.find("forge-wrap").is_some(),
        "Wrap once the source shows"
    );
}

// ------------------------------------------------------------------ New change

#[test]
fn edit_opens_the_form_filled_and_save_sends_the_edit() {
    let mut cx = change("default");
    press(&mut cx, "forge-change-actions");
    press(&mut cx, "forge-change-edit");
    assert!(cx.find("forge-page-change-form").is_some());
    assert!(cx.has_text("Edit change #1"));
    press(&mut cx, "forge-change-send");
    assert!(
        submitted(&cx)
            .iter()
            .any(|op| matches!(op, forge::Op::ChangeEdit { n: 1, .. }))
    );
}

#[test]
fn compare_opens_a_new_change_that_refuses_a_blank_title() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-refs");
    press(&mut cx, "forge-compare-clean");
    assert!(cx.has_text("New change"));
    press(&mut cx, "forge-change-send");
    assert!(cx.has_text("A change needs a title"));
    cx.simulate_input("forge-change-title", "Clean up");
    assert!(
        !cx.has_text("A change needs a title"),
        "typing takes the refusal away"
    );
}

/// New change from the Changes tab (Expo 38): no source yet; Source opens
/// the ref sheet titled "Source", a pick fills it and the endpoints
/// compare; Open sends the change from it into the default head.
#[test]
fn new_change_from_the_changes_tab_picks_its_source_on_the_ref_sheet() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-new-change");
    assert!(cx.has_text("Pick a source"));
    assert!(!cx.texts().iter().any(|text| text.contains(" ahead · ")));
    press(&mut cx, "forge-change-from");
    assert!(cx.find("forge-change-from-sheet").is_some());
    assert!(cx.has_text("Source"));
    press(&mut cx, "forge-ref-refs/heads/clean");
    assert!(
        cx.find("forge-change-from-sheet").is_none(),
        "a pick closes it"
    );
    assert!(cx.has_text("clean"));
    assert!(
        cx.texts().iter().any(|text| text.contains(" ahead · ")),
        "{:?}",
        cx.texts()
    );
    cx.simulate_input("forge-change-title", "Clean up");
    press(&mut cx, "forge-change-send");
    let opened = submitted(&cx).into_iter().find_map(|op| match op {
        forge::Op::ChangeOpen { from, into, .. } => Some((from, into)),
        _ => None,
    });
    assert_eq!(
        opened,
        Some((
            forge::Revision::Ref(b"refs/heads/clean".to_vec()),
            b"refs/heads/main".to_vec()
        ))
    );
}

// ------------------------------------------------------------------ Settings

/// A refused write keeps nothing off: its row says why in every view's
/// words and Dismiss takes it away, the switches stay on, and the next
/// flip sends the settings on screen with that one flipped.
#[test]
fn a_refused_setting_leaves_the_switches_on_and_the_next_flip_sends_what_shows() {
    let (mut cx, view) = phone_view("default");
    refuse_writes(&mut cx);
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-settings");
    let shown = shown_settings(&view);
    press(&mut cx, "forge-settings-force");
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("Saving these settings · That didn’t go through: ")),
        "{:?}",
        cx.texts()
    );
    for control in ["forge-settings-force", "forge-settings-delete"] {
        assert!(!disabled(&cx, control), "{control} stays on");
    }
    press(&mut cx, "forge-settings-force");
    let flipped = forge::Settings {
        allow_force: !shown.allow_force,
        ..shown.clone()
    };
    assert_eq!(configured(&cx), vec![flipped.clone(), flipped]);
    let rows = |cx: &TestAppContext| {
        (1..=2)
            .filter(|id| cx.find(&format!("forge-pending-{id}")).is_some())
            .count()
    };
    assert_eq!(rows(&cx), 2);
    press(&mut cx, "forge-pending-1-dismiss");
    assert_eq!(rows(&cx), 1, "Dismiss takes its row away");
}

#[test]
fn the_delete_switch_saves_the_settings_on_screen_with_it_flipped() {
    let (mut cx, view) = phone_view("default");
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-settings");
    let shown = shown_settings(&view);
    press(&mut cx, "forge-settings-delete");
    let flipped = forge::Settings {
        allow_delete: !shown.allow_delete,
        ..shown
    };
    assert_eq!(configured(&cx), vec![flipped]);
    assert!(
        cx.has_text("Saving these settings · Waiting for the next block…"),
        "{:?}",
        cx.texts()
    );
    assert!(
        disabled(&cx, "forge-settings-force"),
        "nothing else is sent while it is on its way"
    );
}

/// Default head opens the ref sheet with Branches | Tags, as every ref
/// sheet does; a pick saves it.
#[test]
fn default_head_opens_branches_and_tags_and_a_pick_saves_it() {
    let (mut cx, view) = phone_view("default");
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-settings");
    let shown = shown_settings(&view);
    press(&mut cx, "forge-settings-head");
    assert!(cx.find("forge-settings-head-sheet").is_some());
    assert!(
        cx.find("forge-settings-head-kind").is_some(),
        "Branches | Tags"
    );
    press(&mut cx, "forge-ref-refs/heads/clean");
    assert!(cx.find("forge-settings-head-sheet").is_none());
    let head = forge::Settings {
        head: b"refs/heads/clean".to_vec(),
        ..shown
    };
    assert_eq!(configured(&cx), vec![head]);
}

#[test]
fn revoke_takes_a_writer_away() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-settings");
    press(&mut cx, "forge-revoke-Wren");
    assert_eq!(set_lists(&cx), [None]);
}

/// Grant opens the people sheet without the owner and those who write;
/// a pick grants and closes it.
#[test]
fn grant_picks_a_member_on_the_people_sheet() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-settings");
    press(&mut cx, "forge-grant");
    assert!(cx.find("forge-grant-sheet").is_some());
    assert!(cx.find("forge-grant-sheet-1").is_none(), "not the owner");
    assert!(cx.find("forge-grant-sheet-9").is_none(), "not a writer");
    press(&mut cx, "forge-grant-sheet-2");
    assert!(cx.find("forge-grant-sheet").is_none());
    assert_eq!(set_lists(&cx), [listed(&[9, 2])]);
}

// ------------------------------------------------------------------ Changes and Refs

#[test]
fn a_filter_chip_reads_its_changes() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-filter-closed");
    assert!(cx.host().requests::<Ask>().iter().any(|query| matches!(
        query,
        Query::Changes {
            filter: ChangeFilter {
                state: Some(ChangeState::Closed),
                ..
            },
            ..
        }
    )));
    assert!(cx.find("forge-change-list").is_some(), "{:?}", cx.texts());
}

/// "Needs my judgment" in a repository lists that repository's changes
/// alone.
#[test]
fn the_judgment_chip_keeps_this_repositorys_changes() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-filter-judgment");
    let Reply::Judgment { page, .. } = reply("judgment") else {
        panic!("judgment is a judgment");
    };
    for item in &page.items {
        let row = format!("forge-change-{}-{}", item.change.repo, item.change.n);
        assert_eq!(
            cx.find(&row).is_some(),
            item.change.repo == "project",
            "{row}"
        );
    }
}

/// A filter about me asks nothing while the reader holds no account,
/// and says so.
#[test]
fn a_reader_without_an_account_is_told_the_judgment_needs_one() {
    let mut cx = TestAppContext::new();
    configure(&mut cx, "default");
    let (mut cx, _) = seat_view(cx, None);
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-filter-judgment");
    assert!(cx.find("forge-changes-no-account").is_some());
    assert!(cx.has_text("Create an account to see the changes that involve you."));
}

#[test]
fn the_refs_chips_switch_kinds_and_a_row_browses_its_ref() {
    let (mut cx, view) = phone_view("default");
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-refs");
    assert!(cx.find("forge-ref-row-main").is_some());
    press(&mut cx, "forge-refs-tags");
    assert!(cx.find("forge-ref-row-main").is_none(), "tags alone");
    press(&mut cx, "forge-refs-branches");
    press(&mut cx, "forge-ref-row-clean");
    view.read(|forge| assert_eq!(forge.nav().rev.as_deref(), Some(&b"refs/heads/clean"[..])));
    assert!(
        cx.texts().iter().any(|text| text.ends_with(" · browsing")),
        "{:?}",
        cx.texts()
    );
}

// ------------------------------------------------------------------ a change's actions

/// A reply keeps its text until it lands, and Send waits while it is
/// out; once it lands the box empties.
#[test]
fn a_reply_keeps_its_text_until_it_lands() {
    let (mut cx, view) = phone_view("default");
    cx.host().never::<Submit<::chat::Chat>>();
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-change-project-1");
    cx.simulate_input("forge-reply", "Looks good");
    press(&mut cx, "forge-reply-send");
    assert!(cx.has_text("Replying… · Submitting…"), "{:?}", cx.texts());
    view.read(|forge| assert_eq!(forge.reply.text(), "Looks good"));
    assert!(disabled(&cx, "forge-reply-send"), "Send waits");
    let mut cx = change("default");
    cx.simulate_input("forge-reply", "Looks good");
    press(&mut cx, "forge-reply-send");
    assert_eq!(cx.host().requests::<Submit<::chat::Chat>>().len(), 1);
    assert!(!cx.has_text("Replying… · Submitting…"));
    assert!(
        disabled(&cx, "forge-reply-send"),
        "an empty box sends nothing"
    );
}

#[test]
fn a_refused_reply_says_why_and_keeps_its_text() {
    let (mut cx, view) = phone_view("default");
    cx.host().handle::<Submit<::chat::Chat>>(|_| {
        Ok(view::methods::Submitted::Refused {
            frame: [0; 32],
            until: 0,
            refusal: refusal("refused-stale"),
        })
    });
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-change-project-1");
    cx.simulate_input("forge-reply", "Looks good");
    press(&mut cx, "forge-reply-send");
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("Replying… · That didn’t go through: ")),
        "{:?}",
        cx.texts()
    );
    view.read(|forge| assert_eq!(forge.reply.text(), "Looks good"));
    assert!(!disabled(&cx, "forge-reply-send"), "it may be sent again");
}

/// Back on the close confirmation closes the sheet, and Actions opens on
/// its list again.
#[test]
fn back_from_close_for_good_leaves_actions_on_its_list() {
    let mut cx = change("default");
    press(&mut cx, "forge-change-actions");
    press(&mut cx, "forge-change-close");
    assert!(cx.find("forge-close-confirm").is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-change-actions-sheet").is_none());
    press(&mut cx, "forge-change-actions");
    for row in [
        "forge-change-edit",
        "forge-change-close",
        "forge-change-copy-link",
    ] {
        assert!(cx.find(row).is_some(), "{row}");
    }
    assert!(cx.find("forge-close-confirm").is_none());
}

#[test]
fn reviewers_are_added_from_the_people_sheet_and_removed_by_their_chip() {
    let mut cx = repository("default");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-new-change");
    press(&mut cx, "forge-change-add-reviewer");
    assert!(cx.find("forge-reviewers-sheet").is_some());
    press(&mut cx, "forge-reviewers-sheet-4");
    assert!(cx.find("forge-reviewers-sheet").is_none());
    assert!(cx.find("forge-reviewer-Tal").is_some());
    press(&mut cx, "forge-change-add-reviewer");
    assert!(
        cx.find("forge-reviewers-sheet-4").is_none(),
        "one asked is not offered again"
    );
    cx.simulate_dismiss("forge-reviewers-sheet");
    cx.run_until_parked();
    press(&mut cx, "forge-reviewer-Tal");
    assert!(cx.find("forge-reviewer-Tal").is_none());
}

/// All files: every file of the diff on one page, `N files`, nothing to
/// walk.
#[test]
fn all_files_shows_every_file_on_one_page() {
    let mut cx = change("default");
    press(&mut cx, "forge-change-tab-files");
    press(&mut cx, "forge-all-files");
    assert!(cx.find("forge-page-file").is_some());
    assert!(cx.has_text("All files"));
    assert!(
        cx.texts().iter().any(|text| text.ends_with(" files")),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("forge-file-previous").is_none());
    assert!(cx.find("forge-diff").is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-page-change").is_some());
}

// ------------------------------------------------------------------ links

/// A link to a folder opens its page; to a file, its page over its
/// folder's; to a path the ref lacks, a page that says so. None unfolds
/// the desktop's tree.
#[test]
fn a_link_opens_its_folder_or_file_page_or_says_the_ref_lacks_it() {
    let (mut cx, view) = phone_view("default");
    press(&mut cx, "forge-repo-project");
    let follow = |cx: &mut TestAppContext, dest: &str| {
        cx.update(&view, |forge, _, cx| forge.follow_link(b"", dest, cx));
        cx.run_until_parked();
    };
    follow(&mut cx, "src");
    assert!(cx.find("forge-page-folder").is_some(), "{:?}", cx.texts());
    let inside = view.read(|forge| {
        let query = forge.tree_query(b"src".to_vec())?;
        match forge.ready(&query)? {
            Reply::Tree { page, .. } => page
                .items
                .iter()
                .find(|entry| entry.kind != forge::EntryKind::Directory)
                .map(|entry| entry.name.clone()),
            _ => None,
        }
    });
    let inside = String::from_utf8(inside.expect("a file inside src")).unwrap();
    follow(&mut cx, &format!("src/{inside}"));
    assert!(cx.find("forge-page-blob").is_some());
    view.read(|forge| {
        assert_eq!(forge.nav().folder.as_deref(), Some(&b"src"[..]));
        assert!(forge.nav().expanded.is_empty(), "no tree unfolds");
    });
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-page-folder").is_some(), "Back to its folder");
    follow(&mut cx, "missing.md");
    assert!(cx.find("forge-page-missing").is_some());
    assert!(cx.has_text("No missing.md on this ref."));
    assert!(cx.has_text("The Code tab has this ref's files."));
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("forge-page-missing").is_none());
}

// ------------------------------------------------------------------ states

/// An unborn repository: its README tab says how to push to it, its
/// Refs tab that it holds none.
#[test]
fn an_unborn_repository_says_how_to_push_and_holds_no_refs() {
    let mut cx = repository("unborn");
    assert!(
        cx.find("forge-tree-no-commits").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("No commits yet"));
    press(&mut cx, "forge-tab-refs");
    assert!(cx.has_text("No refs"));
    assert!(cx.has_text("This repository is unborn: nothing has been pushed to it yet."));
}

#[test]
fn a_root_without_a_readme_says_where_one_goes() {
    let mut cx = phone_answered(|query| match query {
        Query::Tree { path, .. } if path.is_empty() => Ok(reply("tree-directory")),
        other => Ok(answer(other, "default")),
    });
    press(&mut cx, "forge-repo-project");
    assert!(cx.find("forge-readme-none").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("No README"));
}

#[test]
fn a_readme_that_is_not_text_says_so() {
    let mut cx = phone_answered(|query| match query {
        Query::Blob { .. } => Ok(reply("blob-binary")),
        other => Ok(answer(other, "default")),
    });
    press(&mut cx, "forge-repo-project");
    assert!(cx.has_text("This README is not text."), "{:?}", cx.texts());
}

/// A file that is not text shows why: binary, too large, a submodule.
#[test]
fn a_file_that_is_not_text_says_why() {
    // the blob fixture as a submodule's entry reads (no fixture holds one)
    const GITLINK: &str = "6666666666666666666666666666666666666666";
    let (mut cx, view) = {
        let mut cx = TestAppContext::new();
        configure(&mut cx, "default");
        cx.host().handle::<Ask>(|query| match &query {
            Query::Blob { oid, .. } if oid == GITLINK => {
                let mut gitlink = reply("blob");
                if let Reply::Blob { blob, .. } = &mut gitlink {
                    blob.content = forge::Content::Gitlink;
                }
                Ok(gitlink)
            }
            other => Ok(answer(other, "default")),
        });
        seat_view(cx, Some(1))
    };
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-code");
    for (path, oid, said) in [
        (
            "image.bin",
            "95d586e774a04676a07a142f0e2f97a4f32562cb",
            "Binary file",
        ),
        (
            "large.txt",
            "b90a09e55e43808905fe881245853c1b35b3fb82",
            "Too large to show",
        ),
        (
            "vendor",
            GITLINK,
            "This entry points at another repository.",
        ),
    ] {
        cx.update(&view, |forge, _, cx| {
            forge.open_file(path.as_bytes().to_vec(), oid.into(), cx)
        });
        cx.run_until_parked();
        assert!(cx.find("forge-blob-unreadable").is_some(), "{path}");
        assert!(cx.has_text(said), "{path}: {:?}", cx.texts());
        cx.simulate_back();
        cx.run_until_parked();
    }
}

/// A refused read says why in its place, with Retry: the tree, the
/// history, the refs, a change's diff.
#[test]
fn a_refused_tree_history_refs_or_diff_says_why_and_retries() {
    let refused = |what: fn(&Query) -> bool| {
        phone_answered(move |query| match what(query) {
            true => Err(refusal("refused-object-not-held")),
            false => Ok(answer(query, "default")),
        })
    };
    let mut cx = refused(|query| matches!(query, Query::Tree { .. }));
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-code");
    assert!(cx.find("forge-tree-refused").is_some(), "{:?}", cx.texts());
    assert!(cx.find("forge-tree-retry").is_some());
    let mut cx = refused(|query| matches!(query, Query::Log { .. }));
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-commits");
    assert!(cx.find("forge-log-refused").is_some(), "{:?}", cx.texts());
    let mut cx = refused(|query| matches!(query, Query::Refs { .. }));
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-refs");
    assert!(
        cx.find("forge-refs-list-refused").is_some(),
        "{:?}",
        cx.texts()
    );
    let mut cx = refused(|query| matches!(query, Query::Diff { .. }));
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-changes");
    press(&mut cx, "forge-change-project-1");
    press(&mut cx, "forge-change-tab-files");
    assert!(cx.find("forge-diff-refused").is_some(), "{:?}", cx.texts());
}

/// A commit whose own read is refused says why, with Retry, not that it
/// is still being read.
#[test]
fn a_refused_commit_says_why_and_retries() {
    let (mut cx, view) = {
        let mut cx = TestAppContext::new();
        configure(&mut cx, "default");
        cx.host().handle::<Ask>(|query| match &query {
            Query::Log {
                from: forge::Revision::Oid(_),
                ..
            } => Err(refusal("refused-object-not-held")),
            other => Ok(answer(other, "default")),
        });
        seat_view(cx, Some(1))
    };
    press(&mut cx, "forge-repo-project");
    press(&mut cx, "forge-tab-commits");
    cx.update(&view, |forge, _, cx| {
        forge.open_commit(Some("f".repeat(40)), cx)
    });
    cx.run_until_parked();
    assert!(
        cx.find("forge-commit-refused").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(!cx.has_text("Reading the commit…"));
    let asked = cx.host().requests::<Ask>().len();
    press(&mut cx, "forge-commit-retry");
    assert!(
        cx.host().requests::<Ask>().len() > asked,
        "Retry asks again"
    );
}

// ------------------------------------------------------------------ the sweep

/// Every phone screen fits each phone width under the touch ladder, and
/// Back is set exactly while there is somewhere to go back to (rule 13).
#[test]
fn forge_fits_every_phone() {
    let mut cx = phone("reviewed");
    testing::sweep(&mut cx, |cx| {
        assert_eq!(cx.back_handler(), None, "nothing to go back to");
        press(cx, "forge-new-repo");
        cx.simulate_back();
        press(cx, "forge-repo-project");
        press(cx, "forge-dock-about");
        cx.simulate_back();
        press(cx, "forge-ref-picker");
        cx.simulate_dismiss("forge-ref-picker-sheet");
        for tab in ["code", "commits", "changes", "refs", "settings"] {
            press(cx, &format!("forge-tab-{tab}"));
        }
        press(cx, "forge-settings-head");
        cx.simulate_dismiss("forge-settings-head-sheet");
        press(cx, "forge-grant");
        cx.simulate_dismiss("forge-grant-sheet");
        press(cx, "forge-tab-commits");
        press(cx, "forge-commit-26607f522099476177a45a8058a93108fba5a84d");
        cx.simulate_back();
        cx.run_until_parked();
        press(cx, "forge-tab-code");
        press(cx, "forge-tree-README.md");
        cx.simulate_back();
        cx.run_until_parked();
        press(cx, "forge-tree-src");
        cx.simulate_back();
        cx.run_until_parked();
        press(cx, "forge-tab-changes");
        press(cx, "forge-new-change");
        press(cx, "forge-change-from");
        cx.simulate_dismiss("forge-change-from-sheet");
        press(cx, "forge-change-add-reviewer");
        cx.simulate_dismiss("forge-reviewers-sheet");
        cx.simulate_back();
        cx.run_until_parked();
        press(cx, "forge-change-project-1");
        press(cx, "forge-change-actions");
        press(cx, "forge-change-close");
        cx.simulate_dismiss("forge-change-actions-sheet");
        for tab in ["commits", "details", "files"] {
            press(cx, &format!("forge-change-tab-{tab}"));
        }
        press(cx, "forge-all-files");
        cx.simulate_back();
        cx.run_until_parked();
        press(cx, "forge-file-src/lib.rs");
        press(cx, "forge-start-review");
        press(cx, "forge-finish-review");
        cx.simulate_dismiss("forge-finish-sheet");
        press(cx, "forge-discard-review");
        let mut popped = 0;
        while cx.back_handler().is_some() {
            cx.simulate_back();
            cx.run_until_parked();
            popped += 1;
            assert!(popped < 8, "Back never reaches the root");
        }
        assert!(cx.find("forge-page-repos").is_some());
    });
}

/// Under a pointer the desktop draws at every width, never a phone page.
#[test]
fn a_pointer_keeps_the_desktop_at_every_width() {
    let (mut cx, _) = opened("default");
    for width in [360., 640., 1180.] {
        cx.simulate_resize(width, 760.);
        cx.run_until_parked();
        assert!(cx.find("forge-columns").is_some(), "the desktop at {width}");
        assert!(
            cx.find("forge-page-repo").is_none(),
            "no phone page at {width}"
        );
    }
}
