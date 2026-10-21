//! The Change screens: the list, the detail header, the conversation, the
//! reviewer's Files tab, and the one operation a review becomes.
use super::{answer, booted, change_screen, change_screen_as, focused, opened, opened_as, refusal};
use crate::api::{Ask, SubmitForge};
use crate::state::ChangeTab;
use forge::{LineComment, Op, PageRequest, Query, Side, Verdict};
use std::cell::Cell;
use std::rc::Rc;
use view::methods::Submit;
use view::wire;
use view::wire::Node;

#[test]
fn the_change_tabs_move_on_an_arrow_and_open_on_enter() {
    let (mut cx, view) = change_screen("default", ChangeTab::Conversation);
    assert!(cx.interactivity("forge-change-tabs").focusable);
    cx.simulate_key_down("forge-change-tabs", "right");
    view.read(|forge| assert_eq!(forge.nav().change_tab, ChangeTab::Conversation));
    assert!(
        cx.interactivity("forge-change-tab-commits")
            .aria
            .active_descendant
    );
    cx.simulate_key_down("forge-change-tabs", "enter");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.nav().change_tab, ChangeTab::Commits));
}

#[test]
fn the_change_state_filter_checks_on_an_arrow() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    use crate::state::Filter;
    view.read(|forge| assert_eq!(forge.filter, Filter::Open));
    assert!(!cx.interactivity("forge-filter-merged").focusable);
    cx.simulate_focus("forge-filter-states");
    cx.simulate_key_down("forge-filter-states", "right");
    cx.run_until_parked();
    view.read(|forge| assert_eq!(forge.filter, Filter::Merged));
    let merged = cx.interactivity("forge-filter-merged");
    assert_eq!(merged.aria.toggled, Some(view::Toggled::True));
    assert!(merged.aria.active_descendant);
}

#[test]
fn a_filter_about_me_checks_no_state() {
    let (mut cx, view) = opened("judgment");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-filter-judgment");
    cx.run_until_parked();
    use crate::state::Filter;
    view.read(|forge| assert_eq!(forge.filter, Filter::Judgment));
    for state in ["open", "merged", "closed"] {
        assert_eq!(
            cx.interactivity(&format!("forge-filter-{state}"))
                .aria
                .toggled,
            Some(view::Toggled::False),
            "{state} is not checked"
        );
    }
    assert!(cx.interactivity("forge-filter-open").aria.active_descendant);
}

#[test]
fn the_change_list_shows_the_plans_row_and_its_filters() {
    let (mut cx, view) = opened("default");
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    use crate::state::Filter;
    for filter in [
        Filter::Open,
        Filter::Merged,
        Filter::Closed,
        Filter::Judgment,
        Filter::Authored,
        Filter::Involves,
    ] {
        assert!(
            cx.find(&format!("forge-filter-{}", filter.slug()))
                .is_some(),
            "{} filter",
            filter.label()
        );
    }
    assert!(cx.has_text("Review this change"), "{:?}", cx.texts());
    assert!(cx.find("forge-change-state-1").is_some(), "the state dot");
    assert!(
        cx.has_text("feature → main · Ada"),
        "the author key resolves: {:?}",
        cx.texts()
    );
    cx.simulate_click("forge-filter-closed");
    cx.run_until_parked();
    assert!(cx.has_text("Close this change"), "{:?}", cx.texts());
    cx.simulate_input("forge-changes-search", "nothing like this");
    cx.run_until_parked();
    assert!(cx.has_text("No changes here"));
    view.read(|forge| assert_eq!(forge.filter, crate::state::Filter::Closed));
}

#[test]
fn the_change_header_carries_its_endpoints_and_a_merge_the_program_allows() {
    let (cx, view) = change_screen_as("default", ChangeTab::Conversation, 9);
    view.read(|forge| assert_eq!(forge.nav().change, Some(1)));
    assert!(cx.has_text("Review this change"), "{:?}", cx.texts());
    assert!(cx.has_text("#1") && cx.has_text("open"));
    assert!(cx.has_text("Ada wants feature → main · head 26607f52…a84d"));
    let Some(view::wire::Node::Container(node)) = cx.find("forge-merge") else {
        panic!("the merge button is a native container");
    };
    assert!(node.interactivity.as_ref().unwrap().on_click.is_some());
    assert_ne!(
        node.interactivity.as_ref().unwrap().aria.disabled,
        Some(true)
    );
    let Some(view::wire::Node::Container(edit)) = cx.find("forge-edit-change") else {
        panic!("the edit button stays visible")
    };
    assert_eq!(
        edit.interactivity.as_ref().unwrap().aria.disabled,
        Some(true)
    );
}

#[test]
fn a_diverged_comparison_says_why_it_cannot_merge() {
    let (cx, _view) = change_screen("diverged", ChangeTab::Conversation);
    assert!(
        cx.has_text(
            "The endpoints diverged. Merge with git and push the result, then Merge turns on."
        ),
        "{:?}",
        cx.texts()
    );
    let Some(view::wire::Node::Container(node)) = cx.find("forge-merge") else {
        panic!("the merge button stays visible");
    };
    assert_eq!(
        node.interactivity.as_ref().unwrap().aria.disabled,
        Some(true)
    );
    assert!(node.interactivity.as_ref().unwrap().on_click.is_none());
}

#[test]
fn merging_submits_the_client_computed_fast_forward() {
    let (mut cx, _) = change_screen_as("default", ChangeTab::Conversation, 9);
    cx.simulate_click("forge-merge");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<SubmitForge>()
            .iter()
            .any(|op| matches!(
                op,
                Op::Merge {
                    repo,
                    expected_into,
                    expected_from,
                    result,
                    change: Some(1),
                    ..
                } if repo == "project"
                    && expected_into == "ebfb8b62a50d6e5f7d10062af7cc5d15fd224e16"
                    && expected_from == "26607f522099476177a45a8058a93108fba5a84d"
                    && result == expected_from
            ))
    );
    assert!(cx.has_text("Merging this change"));
}

#[test]
fn an_operation_shows_its_submission_then_a_refusal_reverts_it_with_the_reason() {
    let (mut cx, _) = change_screen_as("default", ChangeTab::Conversation, 9);
    cx.host()
        .refuse::<SubmitForge>("unauthorized", "this key may not close that change");
    cx.simulate_click("forge-close-change");
    cx.run_until_parked();
    assert!(
        cx.has_text("Close for good") && !cx.has_text("Closing this change"),
        "the first press only asks"
    );
    cx.simulate_click("forge-close-change");
    cx.run_until_parked();
    assert!(
        cx.has_text("That didn’t go through: this key may not close that change"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("Closing this change"), "the row stays in place");
}

#[test]
fn an_op_whose_block_lands_before_its_receipt_leaves_the_screen() {
    use super::{Changes, block};
    let (mut cx, view) = change_screen_as("default", ChangeTab::Conversation, 9);
    let heads = cx.host().stream::<Changes<forge::Forge>>();
    cx.update(&view, |forge, _, cx| forge.merge(cx));
    heads.send(block(200, Vec::new()));
    cx.run_until_parked();
    assert!(
        !cx.has_text("Merging this change"),
        "the block retired the row: {:?}",
        cx.texts()
    );
}

#[test]
fn a_repository_filter_does_not_carry_into_its_change_search() {
    let (mut cx, _view) = booted("default");
    cx.simulate_input("forge-repos-search", "proj");
    cx.run_until_parked();
    cx.simulate_click("forge-repo-project-open");
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    assert!(cx.has_text("Review this change"), "{:?}", cx.texts());
}

#[test]
fn a_merged_change_wears_its_state_and_offers_nothing_more() {
    let (cx, _view) = change_screen("merged", ChangeTab::Conversation);
    assert!(cx.has_text("merged"), "{:?}", cx.texts());
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("merged into main as ")),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.texts()
            .windows(2)
            .any(|w| w[0] == "Ada" && w[1].starts_with("merged into main as ")),
        "the merger is named from the record: {:?}",
        cx.texts()
    );
    assert!(
        cx.has_text("This change is no longer open"),
        "{:?}",
        cx.texts()
    );
    for button in ["forge-edit-change", "forge-close-change", "forge-merge"] {
        assert!(cx.find(button).is_none(), "{button} on an ended change");
    }
}

#[test]
fn a_closed_change_names_who_closed_it_and_offers_no_edit_or_close() {
    let (cx, _view) = change_screen("closed", ChangeTab::Conversation);
    assert!(
        cx.texts()
            .windows(2)
            .any(|w| w[0] == "Ada" && w[1] == "closed this change"),
        "the closer is named from the record: {:?}",
        cx.texts()
    );
    for button in ["forge-edit-change", "forge-close-change", "forge-merge"] {
        assert!(cx.find(button).is_none(), "{button} on an ended change");
    }
}

#[test]
fn the_conversation_is_the_hidden_chat_channel_and_the_forge_body() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Conversation);
    assert!(
        cx.find("forge-change-body-text").is_some(),
        "the body is markdown"
    );
    assert!(cx.has_text("Ada") && cx.has_text("opened this change"));
    assert!(!cx.has_text("raw forge text"), "{:?}", cx.texts());
    assert!(!cx.texts().iter().any(|text| text.contains("program:")));
    assert!(cx.has_text("Reading it now"));
    assert!(cx.has_text("Rae"), "a chat handle resolves to a name");
    view.read(|forge| {
        let (_, _, _, reviews) = forge.change().expect("the change landed");
        assert_eq!(reviews.items.len(), 3, "the review cursor was followed");
    });
    assert!(cx.has_text("approved") && cx.has_text("requested changes"));
    assert!(
        cx.has_text("src/lib.rs:2 — Context is commentable"),
        "a line comment is quoted under its review: {:?}",
        cx.texts()
    );
    assert!(
        !cx.texts()
            .iter()
            .any(|text| text.starts_with("merged into"))
    );
    assert!(
        matches!(
            cx.find("forge-reply"),
            Some(view::wire::Node::Field {
                multiline: true,
                ..
            })
        ),
        "the reply is the host's multi-line editor"
    );
    cx.update(&view, |forge, _, cx| {
        forge.reply = view::TextField::new("looks right to me");
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("forge-reply-send");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(
                op,
                chat::Op::PostMessage { channel_id, .. } if channel_id == "forge:project:1"
            ))
    );
    view.read(|forge| assert!(forge.reply.text().is_empty()));
}

#[test]
fn a_write_between_two_pages_of_a_change_starts_the_read_over() {
    let (mut cx, view) = opened("reviewed");
    let crossed = Rc::new(Cell::new(false));
    cx.host().handle::<Ask>(move |query| match query {
        Query::Change {
            page: PageRequest { after: Some(_), .. },
            ..
        } if !crossed.replace(true) => Err(refusal("refused-stale")),
        query => Ok(answer(&query, "reviewed")),
    });
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    let sentence = refusal("refused-stale").message;
    assert!(!cx.has_text(&sentence), "{:?}", cx.texts());
    view.read(|forge| {
        let (_, _, _, reviews) = forge.change().expect("the change landed");
        assert_eq!(reviews.items.len(), 3, "both pages, read again");
    });
    let cursored: Vec<bool> = cx
        .host()
        .requests::<Ask>()
        .iter()
        .filter_map(|query| match query {
            Query::Change { page, .. } => Some(page.after.is_some()),
            _ => None,
        })
        .collect();
    assert_eq!(cursored, [false, true, false, true], "started over once");
}

#[test]
fn a_review_pinned_before_a_push_reads_as_outdated() {
    let (cx, _view) = change_screen("outdated", ChangeTab::Conversation);
    assert!(cx.has_text("outdated"), "{:?}", cx.texts());
}

#[test]
fn the_files_tab_marks_comments_and_viewed_files_and_can_show_one() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    assert!(cx.has_text("src/lib.rs"), "{:?}", cx.texts());
    assert!(cx.has_text("+2") && cx.has_text("−1"));
    assert!(
        cx.find("forge-file-comments-src/lib.rs").is_some(),
        "published line comments mark the file"
    );
    cx.simulate_click("forge-viewed-src/lib.rs");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.viewed.contains("project#1:src/lib.rs")));
    assert!(cx.has_text("✓"));
    let Some(wire::Node::Container(tick)) = cx.find("forge-viewed-src/lib.rs") else {
        panic!("the viewed tick");
    };
    assert_eq!(
        tick.interactivity.as_ref().unwrap().aria.toggled,
        Some(true.into()),
        "checked"
    );
    cx.simulate_click("forge-file-src/lib.rs-open");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/lib.rs".as_slice())
        )
    });
    let row = super::control(&cx, "forge-file-src/lib.rs");
    assert_eq!(
        row.interactivity.as_ref().unwrap().role,
        Some(view::Role::Row)
    );
    assert!(
        !row.interactivity.as_ref().unwrap().focusable
            && row.interactivity.as_ref().unwrap().on_click.is_none()
    );
    let open = super::control(&cx, "forge-file-src/lib.rs-open");
    assert_eq!(
        open.interactivity.as_ref().unwrap().role,
        Some(view::Role::Button)
    );
    assert_eq!(
        open.interactivity.as_ref().unwrap().aria.current,
        Some(view::accesskit::AriaCurrent::True)
    );
    assert!(!super::holds(
        &wire::Node::Container(open),
        "forge-viewed-src/lib.rs"
    ));
    cx.simulate_click("forge-files-all");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.nav().diff_path.is_none()));
    assert_eq!(
        super::lit_rows(&cx, "forge-file-tree-list", view::Role::Row),
        Vec::<String>::new()
    );
}

#[test]
fn a_release_sized_diff_opens_with_its_files_and_first_lines() {
    let (mut cx, _view) = change_screen("release", ChangeTab::Files);
    assert!(cx.has_text("100 files · +100000 −0"), "{:?}", cx.texts());
    assert!(cx.has_text("src/gen/file_000.rs · added · +1000 −0"));
    assert!(cx.has_text("pub fn f0_0(x: u64) -> u64 { x ^ 0 }"));
    let Some(wire::Node::UniformList { count, .. }) = cx.find("forge-diff") else {
        panic!("the diff list");
    };
    let count = *count;
    assert_eq!(count, 100 * 1002);
    cx.simulate_range("forge-diff", count - 20..count);
    cx.run_until_parked();
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("pub fn f99_")),
        "{:?}",
        cx.texts()
    );
    cx.simulate_range("forge-file-tree-rows", 90..100);
    cx.run_until_parked();
    cx.simulate_click("forge-file-src/gen/file_099.rs-open");
    cx.run_until_parked();
    assert_eq!(cx.style("forge-files-all-box").flex_shrink, Some(0.));
    assert_eq!(
        super::pixels(cx.style("forge-files-title").min_size.width),
        Some(0.)
    );
    assert!(
        cx.texts()
            .iter()
            .any(|text| text.starts_with("pub fn f99_")),
        "{:?}",
        cx.texts()
    );
}

fn file_rows_drawn(cx: &view::testing::TestAppContext) -> usize {
    fn count(node: &Node) -> usize {
        let own = node
            .key()
            .is_some_and(|key| key.starts_with("forge-file-") && key.ends_with("-open"));
        usize::from(own) + node.children().iter().map(count).sum::<usize>()
    }
    count(cx.root())
}

#[test]
fn a_264_file_change_lists_the_files_on_screen_and_opens_one_by_keyboard() {
    let (mut cx, view) = change_screen("wide", ChangeTab::Files);
    assert!(cx.has_text("264 files · +792 −0"), "{:?}", cx.texts());
    let drawn = file_rows_drawn(&cx);
    assert!(drawn > 0 && drawn < 64, "{drawn} of 264 files drawn");
    assert!(
        cx.style("forge-file-src/gen/file_000.rs")
            .size
            .width
            .is_some(),
        "a row is as wide as the list: a long path is cut, its counts stay in view"
    );
    let list = cx.interactivity("forge-file-tree-list");
    assert_eq!(list.role, Some(view::Role::Grid));
    assert!(
        list.focusable && list.tab_stop == Some(true),
        "one Tab stop"
    );
    cx.simulate_focus("forge-file-tree-list");
    for _ in 0..200 {
        cx.simulate_key_down("forge-file-tree-list", "down");
    }
    cx.run_until_parked();
    cx.simulate_viewport(20);
    let row = "forge-file-src/gen/file_200.rs-open";
    assert!(
        cx.interactivity(row).aria.active_descendant,
        "the row walked to is drawn and claimed"
    );
    cx.simulate_key_down("forge-file-tree-list", "enter");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/gen/file_200.rs".as_slice())
        )
    });
    assert!(cx.has_text("src/gen/file_200.rs · added · +3 −0"));
    cx.simulate_key_down("forge-file-tree-list", "right");
    assert!(
        cx.interactivity("forge-viewed-src/gen/file_200.rs")
            .aria
            .active_descendant,
        "→ reaches the tick"
    );
    let ring = &cx.style("forge-viewed-src/gen/file_200.rs").border_widths;
    assert_eq!(
        super::pixels(ring.top),
        Some(2.),
        "drawn where the arrows are"
    );
    cx.simulate_key_down("forge-file-tree-list", "space");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.viewed.contains("project#1:src/gen/file_200.rs")));
    let drawn = file_rows_drawn(&cx);
    assert!(drawn < 64, "{drawn} of 264 files drawn");
}

#[test]
fn a_file_picked_elsewhere_is_scrolled_into_the_list() {
    let (mut cx, view) = change_screen("wide", ChangeTab::Conversation);
    cx.update(&view, |forge, _, cx| {
        forge.open_file_comment(b"src/gen/file_230.rs".to_vec(), cx)
    });
    cx.run_until_parked();
    cx.simulate_viewport(20);
    let row = "forge-file-src/gen/file_230.rs-open";
    let press = cx.interactivity(row);
    assert_eq!(press.aria.current, Some(view::accesskit::AriaCurrent::True));
    assert!(press.aria.active_descendant, "the keys go on from it");
    cx.simulate_focus("forge-file-tree-list");
    cx.simulate_key_down("forge-file-tree-list", "down");
    cx.simulate_key_down("forge-file-tree-list", "enter");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/gen/file_231.rs".as_slice())
        )
    });
}

#[test]
fn every_comment_on_a_release_sized_diff_is_laid_out() {
    let (mut cx, view) = change_screen("release", ChangeTab::Files);
    let path = b"src/gen/file_000.rs".to_vec();
    let body = "word ".repeat(400);
    cx.update(&view, |forge, _, cx| {
        forge.start_review(cx);
        let review = forge.review_mut().expect("a review");
        for line in 1..=64 {
            review.stage(crate::state::PendingComment {
                path: path.clone(),
                new_side: true,
                line,
                body: view::TextField::new(body.clone()),
            });
        }
    });
    cx.update(&view, |forge, window, cx| {
        forge.open_comment(path.clone(), true, 64, window, cx)
    });
    cx.run_until_parked();
    assert!(
        cx.find("forge-comment-body").is_some(),
        "the composer opens"
    );
    let (count, _) = diff_rows(&cx);
    assert!(count > 100 * 1002 + 64, "{count}");
}

#[test]
fn the_diff_draws_typed_lines_and_believes_the_program_about_a_literal_plus_plus() {
    let (cx, _view) = change_screen("reviewed", ChangeTab::Files);
    assert!(cx.find("forge-diff").is_some());
    assert!(
        cx.find("forge-diff-hunk-1").is_some(),
        "the hunk header is a row: {:?}",
        cx.texts()
    );
    for line in ["one", "keep", "old", "++ x", "end", "added"] {
        assert!(cx.has_text(line), "{line} is drawn: {:?}", cx.texts());
    }
    for gutter in [
        "forge-gutter-src/lib.rs-old-1",
        "forge-gutter-src/lib.rs-new-1",
        "forge-gutter-src/lib.rs-new-5",
    ] {
        assert!(cx.find(gutter).is_some(), "{gutter} carries its number");
    }
    assert!(
        cx.has_text("+") && cx.has_text("\u{2212}"),
        "an added and a removed marker"
    );
    let Some(view::wire::Node::Container(gutter)) = cx.find("forge-gutter-src/lib.rs-new-5") else {
        panic!("the gutter number is the comment button");
    };
    assert_eq!(
        gutter.interactivity.as_ref().unwrap().aria.label.as_deref(),
        Some("Comment on this line")
    );
    assert!(gutter.interactivity.as_ref().unwrap().on_click.is_some());
}

#[test]
fn the_diff_gutters_are_a_grid_the_arrows_walk() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    let grid = cx.interactivity("forge-diff-lines");
    assert_eq!(grid.role, Some(view::Role::Grid));
    assert!(grid.focusable && grid.tab_stop == Some(true));
    let active_gutter = |cx: &view::testing::TestAppContext| -> String {
        fn find(node: &view::wire::Node) -> Option<String> {
            if node
                .interactivity()
                .is_some_and(|i| i.aria.active_descendant)
            {
                return node.key().map(str::to_owned);
            }
            node.children().iter().find_map(find)
        }
        find(cx.find("forge-diff-lines").expect("the grid")).expect("a gutter claims")
    };
    let first = active_gutter(&cx);
    assert!(first.starts_with("forge-gutter-src/lib.rs-old-"), "{first}");
    assert!(!cx.interactivity(&first).focusable);
    assert_eq!(
        cx.interactivity(&format!("{first}-cell")).role,
        Some(view::Role::GridCell)
    );
    cx.simulate_focus("forge-diff-lines");
    cx.simulate_key_down("forge-diff-lines", "right");
    let second = active_gutter(&cx);
    assert!(
        second.starts_with("forge-gutter-src/lib.rs-new-"),
        "{second}"
    );
    cx.simulate_key_down("forge-diff-lines", "ctrl-end");
    cx.simulate_key_down("forge-diff-lines", "end");
    let last = active_gutter(&cx);
    assert_ne!(last, second);
    let (side, number) = last
        .trim_start_matches("forge-gutter-src/lib.rs-")
        .split_once('-')
        .expect("side-number");
    cx.simulate_key_down("forge-diff-lines", "enter");
    cx.run_until_parked();
    view.read(|forge| {
        let open = forge
            .review()
            .unwrap()
            .open
            .as_ref()
            .expect("an open anchor");
        assert_eq!(open.line, number.parse::<u64>().unwrap());
        assert_eq!(open.new_side, side == "new");
    });
}

fn diff_rows(cx: &view::testing::TestAppContext) -> (usize, Vec<(usize, &Node)>) {
    let Some(Node::UniformList {
        count,
        indices,
        children,
        ..
    }) = cx.find("forge-diff")
    else {
        panic!("the diff list");
    };
    let mut rows: Vec<(usize, &Node)> = indices
        .iter()
        .map(|index| *index as usize)
        .zip(children)
        .collect();
    rows.sort_by_key(|(index, _)| *index);
    (*count, rows)
}

fn keys_from(cx: &view::testing::TestAppContext, index: usize, n: usize) -> Vec<String> {
    let (_, rows) = diff_rows(cx);
    (index..index + n)
        .map(|at| {
            let row = rows.iter().find(|(held, _)| *held == at);
            row.and_then(|(_, row)| row.key())
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

fn row_holding(cx: &view::testing::TestAppContext, key: &str) -> usize {
    fn holds(node: &Node, key: &str) -> bool {
        node.key() == Some(key) || node.children().iter().any(|child| holds(child, key))
    }
    let (_, rows) = diff_rows(cx);
    rows.iter()
        .find(|(_, row)| holds(row, key))
        .map(|(index, _)| *index)
        .unwrap_or_else(|| panic!("no row holds {key}"))
}

#[test]
fn a_comment_takes_rows_of_its_own_under_its_line() {
    let (mut cx, _) = change_screen("reviewed", ChangeTab::Files);
    let count = diff_rows(&cx).0;
    let line = row_holding(&cx, "forge-gutter-src/lib.rs-old-1");
    let next = keys_from(&cx, line + 1, 1).remove(0);
    cx.simulate_click("forge-gutter-src/lib.rs-old-1");
    cx.run_until_parked();
    assert_eq!(diff_rows(&cx).0, count + 5);
    assert_eq!(
        keys_from(&cx, line + 1, 6),
        [
            "forge-comment-0",
            "forge-comment-1",
            "forge-comment-2",
            "forge-comment-3",
            "forge-comment-4",
            &next
        ],
        "the composer, then the next line"
    );
    assert_eq!(row_holding(&cx, "forge-comment-body"), line + 2);
    assert_eq!(row_holding(&cx, "forge-comment-error"), line + 3);
    assert_eq!(row_holding(&cx, "forge-comment-save"), line + 4);
    cx.simulate_input("forge-comment-body", "rename this");
    cx.run_until_parked();
    assert_eq!(diff_rows(&cx).0, count + 5, "typing moves no row");
    cx.simulate_click("forge-comment-save");
    cx.run_until_parked();
    assert_eq!(diff_rows(&cx).0, count + 2);
    assert_eq!(
        keys_from(&cx, line + 1, 3),
        ["forge-diff-draft-0-0", "forge-diff-draft-0-1", &next],
        "the staged comment, then the next line"
    );
    assert!(cx.has_text("Your pending comment") && cx.has_text("rename this"));
}

#[test]
fn the_grid_keeps_still_while_its_composer_is_open() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-gutter-src/lib.rs-new-5");
    cx.run_until_parked();
    cx.simulate_input("forge-comment-body", "keep this");
    for key in ["space", "enter", "down"] {
        cx.simulate_key_down("forge-diff-lines", key);
        cx.run_until_parked();
    }
    view.read(|forge| {
        let open = forge.review().and_then(|review| review.open.as_ref());
        let open = open.expect("the composer stays open");
        assert_eq!((open.line, open.new_side), (5, true));
        assert_eq!(open.body.text(), "keep this");
        assert_eq!(forge.diff_cursor, None, "no walk either");
    });
    cx.simulate_click("forge-comment-cancel");
    cx.run_until_parked();
    cx.simulate_focus("forge-diff-lines");
    cx.simulate_key_down("forge-diff-lines", "down");
    view.read(|forge| assert!(forge.diff_cursor.is_some(), "walks again"));
}

#[test]
fn no_gutter_is_lit_until_the_arrows_walk_it() {
    let (mut cx, _) = change_screen("reviewed", ChangeTab::Files);
    let lit = |cx: &view::testing::TestAppContext| {
        let mut lit = super::lit_rows(cx, "forge-diff-lines", view::Role::Button);
        lit.retain(|key| key.starts_with("forge-gutter-"));
        lit
    };
    assert_eq!(lit(&cx), Vec::<String>::new(), "the diff at rest");
    cx.simulate_click("forge-gutter-src/lib.rs-new-5");
    cx.run_until_parked();
    assert_eq!(lit(&cx), Vec::<String>::new(), "a press lights nothing");
    cx.simulate_click("forge-comment-cancel");
    cx.run_until_parked();
    cx.simulate_focus("forge-diff-lines");
    cx.simulate_key_down("forge-diff-lines", "down");
    assert_eq!(lit(&cx).len(), 1, "{:?}", lit(&cx));
    cx.simulate_click("forge-change-tab-conversation");
    cx.run_until_parked();
    cx.simulate_click("forge-change-tab-files");
    cx.run_until_parked();
    assert_eq!(lit(&cx), Vec::<String>::new(), "a walk ends with a move");
}

#[test]
fn the_gutter_of_a_drawn_line_is_the_comment_button() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    assert!(
        cx.has_text("pinned at 26607f52…a84d · 0 line comments pending"),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("forge-gutter-src/lib.rs-new-5");
    cx.run_until_parked();
    view.read(|forge| {
        let review = forge.review().expect("a review session");
        let open = review.open.as_ref().expect("an open anchor");
        assert_eq!(open.line, 5);
        assert!(open.new_side);
    });
    assert!(cx.has_text("src/lib.rs:5 (new)"));
}

#[test]
fn the_verdicts_check_on_an_arrow() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    let group = cx.interactivity("forge-verdicts");
    assert_eq!(group.role, Some(view::Role::RadioGroup));
    assert!(group.focusable && group.tab_stop == Some(true));
    assert!(!cx.interactivity("forge-verdict-approve").focusable);
    cx.simulate_focus("forge-verdicts");
    cx.simulate_key_down("forge-verdicts", "down");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.review().unwrap().verdict, Some(Verdict::Approve));
    });
    let approve = cx.interactivity("forge-verdict-approve");
    assert_eq!(approve.aria.toggled, Some(view::Toggled::True));
    assert!(approve.aria.active_descendant);
    assert!(
        !cx.interactivity("forge-verdict-comment")
            .aria
            .active_descendant
    );
}

#[test]
fn a_review_batches_every_anchor_into_exactly_one_operation() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();

    for (path, new_side, line, body) in [
        (b"src/lib.rs".as_slice(), true, 3u64, "first thought"),
        (b"src/lib.rs".as_slice(), true, 3, "second thought"),
        (b"src/lib.rs".as_slice(), false, 1, "the old side too"),
    ] {
        cx.update(&view, |forge, window, cx| {
            forge.open_comment(path.to_vec(), new_side, line, window, cx)
        });
        cx.run_until_parked();
        cx.simulate_input("forge-comment-body", body);
        cx.simulate_click("forge-comment-save");
        cx.run_until_parked();
    }
    view.read(|forge| {
        let review = forge.review().expect("a review session");
        assert_eq!(review.comments.len(), 2, "re-staging replaces its anchor");
    });

    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    assert!(
        matches!(
            cx.find("forge-review-body"),
            Some(view::wire::Node::Field {
                multiline: true,
                ..
            })
        ),
        "the review body is the host's multi-line editor"
    );
    cx.update(&view, |forge, _, cx| {
        forge.review_mut().unwrap().body = view::TextField::new("one batch, one op");
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("forge-verdict-request-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-submit-review");
    cx.run_until_parked();

    let submitted = cx.host().requests::<SubmitForge>();
    let reviews: Vec<&Op> = submitted
        .iter()
        .filter(|op| matches!(op, Op::ReviewSubmit { .. }))
        .collect();
    assert_eq!(reviews.len(), 1, "one review is one operation");
    let Op::ReviewSubmit { repo, n, review } = reviews[0] else {
        unreachable!()
    };
    assert_eq!((repo.as_str(), *n), ("project", 1));
    assert_eq!(review.verdict, Verdict::RequestChanges);
    assert_eq!(review.body, "one batch, one op");
    assert_eq!(
        review.commit_oid, "26607f522099476177a45a8058a93108fba5a84d",
        "the pin is the head the reader read"
    );
    assert_eq!(
        review.base_oid.as_deref(),
        Some("ebfb8b62a50d6e5f7d10062af7cc5d15fd224e16"),
        "the old side addresses the merge base"
    );
    let mut anchors = review.comments.clone();
    anchors.sort_by_key(|comment| (comment.side, comment.line));
    assert_eq!(
        anchors,
        vec![
            LineComment {
                path: b"src/lib.rs".to_vec(),
                side: Side::Old,
                line: 1,
                body: "the old side too".into(),
            },
            LineComment {
                path: b"src/lib.rs".to_vec(),
                side: Side::New,
                line: 3,
                body: "second thought".into(),
            },
        ]
    );
    assert!(cx.has_text("Submitting this review"), "{:?}", cx.texts());
    view.read(|forge| {
        assert!(
            forge.review().is_none(),
            "a landed review ends its session: no second Finish at the old pin"
        )
    });
}

#[test]
fn a_refused_review_keeps_every_draft() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    cx.update(&view, |forge, window, cx| {
        forge.open_comment(b"src/lib.rs".to_vec(), true, 3, window, cx)
    });
    cx.run_until_parked();
    cx.simulate_input("forge-comment-body", "keep me");
    cx.simulate_click("forge-comment-save");
    cx.run_until_parked();
    cx.host()
        .refuse::<SubmitForge>("capacity", "this operation exceeds its bound");
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    cx.simulate_click("forge-verdict-approve");
    cx.run_until_parked();
    cx.simulate_click("forge-submit-review");
    cx.run_until_parked();
    assert!(
        cx.has_text("That didn’t go through: this operation exceeds its bound"),
        "{:?}",
        cx.texts()
    );
    view.read(|forge| {
        let review = forge.review().expect("the session survives a refusal");
        assert_eq!(review.comments.len(), 1);
        assert_eq!(review.comments[0].body.text(), "keep me");
    });
}

#[test]
fn a_line_comment_composer_opens_focused_and_enter_stages() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    cx.update(&view, |forge, window, cx| {
        forge.open_comment(b"src/lib.rs".to_vec(), true, 3, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(focused(&cx).as_deref(), Some("forge-comment-body"));
    cx.simulate_input("forge-comment-body", "rename this");
    cx.simulate_submit("forge-comment-body");
    cx.run_until_parked();
    view.read(|forge| {
        let review = forge.review().expect("a review session");
        assert!(review.open.is_none(), "the composer closed");
        assert_eq!(review.comments.len(), 1);
    });
}

#[test]
fn an_empty_comment_verdict_is_refused_before_it_reaches_the_program() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    let before = cx.host().requests::<SubmitForge>().len();
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    cx.simulate_click("forge-verdict-comment");
    cx.run_until_parked();
    cx.simulate_click("forge-submit-review");
    cx.run_until_parked();
    assert_eq!(cx.host().requests::<SubmitForge>().len(), before);
    assert!(
        cx.has_text("A comment review needs a body or a line comment"),
        "{:?}",
        cx.texts()
    );
    view.read(|forge| assert!(forge.review().is_some()));
}

#[test]
fn the_details_sidebar_holds_reviews_merge_status_and_line_comments() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Conversation);
    for section in [
        "forge-reviews",
        "forge-merge-status",
        "forge-comments",
        "forge-channel",
    ] {
        assert!(cx.find(section).is_some(), "{section}: {:?}", cx.texts());
    }
    assert!(cx.has_text("fast-forward"), "{:?}", cx.texts());
    assert!(cx.has_text("advisory"));
    assert!(cx.has_text("src/lib.rs:2 · Rae"), "{:?}", cx.texts());
    cx.simulate_click("forge-comment-1-src/lib.rs-2");
    cx.run_until_parked();
    view.read(|forge| {
        assert_eq!(forge.nav().change_tab, ChangeTab::Files);
        assert_eq!(
            forge.nav().diff_path.as_deref(),
            Some(b"src/lib.rs".as_slice())
        )
    });
    assert!(cx.find("forge-dock").is_none());
    cx.update(&view, |forge, _, cx| {
        forge.open_change_tab(ChangeTab::Conversation, cx);
    });
    cx.simulate_resize(720., 760.);
    assert!(cx.find("forge-dock").is_none(), "narrow folds the details");
    cx.simulate_click("forge-toggle-dock");
    cx.run_until_parked();
    assert!(cx.find("forge-details-over").is_some());
    assert!(cx.find("forge-merge-status").is_some());
}

#[test]
fn editing_a_change_keeps_the_paragraphs_of_its_body() {
    let (mut cx, view) = change_screen("default", ChangeTab::Conversation);
    let body = "What: a body.\n\nWhy: it reads.\n\nTest: this one.";
    cx.update(&view, |forge, window, cx| {
        forge.start_edit(window, cx);
        forge.form.as_mut().unwrap().body = view::TextField::new(body);
    });
    cx.run_until_parked();
    assert!(
        matches!(
            cx.find("forge-change-body"),
            Some(wire::Node::Field {
                multiline: true,
                ..
            })
        ),
        "the body field is the host's multi-line editor"
    );
    cx.simulate_click("forge-change-submit");
    cx.run_until_parked();
    assert!(
        cx.host().requests::<SubmitForge>().iter().any(
            |op| matches!(op, Op::ChangeEdit { n: 1, body: Some(saved), .. } if saved == body)
        ),
    );
}

#[test]
fn a_new_change_draft_does_not_show_inside_another_change() {
    let (mut cx, _) = opened_as("default", 9);
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    cx.simulate_click("forge-compare-clean");
    cx.run_until_parked();
    assert!(cx.find("forge-change-form").is_some(), "{:?}", cx.texts());
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_none(),
        "the draft is not this change's: {:?}",
        cx.texts()
    );
}

fn count_keys(node: &Node, key: &str) -> usize {
    usize::from(node.key() == Some(key))
        + node
            .children()
            .iter()
            .map(|c| count_keys(c, key))
            .sum::<usize>()
}

#[test]
fn a_change_form_opens_focused_on_its_title_and_enter_submits() {
    let (mut cx, view) = opened_as("default", 1);
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    cx.simulate_click("forge-compare-clean");
    cx.run_until_parked();
    assert_eq!(
        focused(&cx).as_deref(),
        Some("forge-change-title"),
        "a draft"
    );
    cx.simulate_input("forge-change-title", "Clean up");
    cx.simulate_submit("forge-change-title");
    cx.run_until_parked();
    view.read(|forge| assert!(forge.form.is_none(), "the draft left on Enter"));
    assert!(
        cx.host()
            .requests::<SubmitForge>()
            .iter()
            .any(|op| matches!(op, forge::Op::ChangeOpen { title, .. } if title == "Clean up")),
        "Enter opened it"
    );

    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    cx.simulate_click("forge-edit-change");
    cx.run_until_parked();
    assert_eq!(
        focused(&cx).as_deref(),
        Some("forge-change-title"),
        "an edit"
    );
    assert_eq!(
        count_keys(cx.root(), "forge-change-title"),
        1,
        "the heading does not share the field's id while editing"
    );
}

#[test]
fn an_edit_left_open_does_not_show_on_the_list_tabs() {
    let (mut cx, _) = change_screen_as("default", ChangeTab::Conversation, 1);
    cx.simulate_click("forge-edit-change");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_some(),
        "the edit opened on its page"
    );
    cx.simulate_click("forge-change-back");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_none(),
        "the Changes list shows no edit form: {:?}",
        cx.texts()
    );
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_none(),
        "nor does the Refs tab"
    );
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    cx.simulate_click("forge-change-1");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_some(),
        "back on its page the edit is still there"
    );
}

#[test]
fn a_change_form_stays_in_the_repository_it_was_opened_in() {
    let (mut cx, view) = change_screen_as("default", ChangeTab::Conversation, 1);
    cx.simulate_click("forge-edit-change");
    cx.run_until_parked();
    assert!(cx.find("forge-change-form").is_some());
    cx.update(&view, |forge, _, cx| forge.open_route("other/1", cx));
    cx.run_until_parked();
    assert!(cx.find("forge-change-header").is_some(), "other's #1");
    assert!(
        cx.find("forge-change-form").is_none(),
        "project's edit on other's change: {:?}",
        cx.texts()
    );
    cx.update(&view, |forge, _, cx| forge.open_route("project/1", cx));
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_some(),
        "the edit waits on its own page"
    );

    let (mut cx, view) = opened_as("default", 1);
    cx.simulate_click("forge-tab-refs");
    cx.run_until_parked();
    cx.simulate_click("forge-compare-clean");
    cx.run_until_parked();
    assert!(cx.find("forge-change-form").is_some());
    cx.update(&view, |forge, _, cx| forge.open_repo("other".into(), cx));
    cx.run_until_parked();
    cx.simulate_click("forge-tab-changes");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-form").is_none(),
        "project's draft on other's Changes: {:?}",
        cx.texts()
    );
}

#[test]
fn a_change_lists_its_own_commits() {
    let (cx, view) = change_screen("default", ChangeTab::Commits);
    let into = view.read(|forge| forge.change().map(|(change, ..)| change.into.clone()));
    let into = into.expect("the change landed");
    let asked = cx.host().requests::<crate::api::Ask>();
    assert!(
        asked.iter().any(|query| matches!(
            query,
            forge::Query::Log { exclude: Some(forge::Revision::Ref(target)), .. } if *target == into
        )),
        "{asked:?}"
    );
}

#[test]
fn a_merged_change_lists_the_commits_it_merged() {
    let (cx, view) = change_screen("merged", ChangeTab::Commits);
    let heads = view.read(|forge| {
        forge
            .change()
            .and_then(|(change, ..)| change.merged_heads.clone())
    });
    let heads = heads.expect("the merge recorded its heads");
    let asked = cx.host().requests::<crate::api::Ask>();
    assert!(
        asked.iter().any(|query| matches!(
            query,
            forge::Query::Log {
                from: forge::Revision::Oid(source),
                exclude: Some(forge::Revision::Oid(target)),
                ..
            } if *source == heads.source && *target == heads.target
        )),
        "{asked:?}"
    );
}

#[test]
fn a_change_opens_one_of_its_commits_and_comes_back() {
    let (mut cx, view) = change_screen("default", ChangeTab::Commits);
    let oid = "26607f522099476177a45a8058a93108fba5a84d";
    cx.simulate_click(&format!("forge-commit-{oid}"));
    cx.run_until_parked();
    assert!(
        cx.has_text("Feature\n\nReview these bytes.\n"),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("ebfb8b62…4e16"), "the parent is named");
    assert!(
        cx.find("forge-commit-diff-file-src/lib.rs").is_some(),
        "its diff: {:?}",
        cx.texts()
    );
    assert!(
        cx.find("forge-change-log").is_none(),
        "the page replaces the list"
    );
    view.read(|forge| assert_eq!(forge.nav().change, Some(1), "still in the change"));
    cx.simulate_click("forge-commit-close");
    cx.run_until_parked();
    assert!(cx.find("forge-change-log").is_some(), "{:?}", cx.texts());
    cx.simulate_click(&format!("forge-commit-{oid}"));
    cx.run_until_parked();
    cx.simulate_click("forge-change-tab-files");
    cx.run_until_parked();
    cx.simulate_click("forge-change-tab-commits");
    cx.run_until_parked();
    assert!(
        cx.find("forge-change-log").is_some(),
        "a tab left drops the commit: {:?}",
        cx.texts()
    );
}

#[test]
fn tab_stays_inside_the_finish_form() {
    let (mut cx, _) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    cx.simulate_focus("forge-submit-review");
    cx.simulate_tab(true);
    assert_eq!(focused(&cx).as_deref(), Some("forge-review-body"));
    cx.simulate_tab(false);
    assert_eq!(focused(&cx).as_deref(), Some("forge-submit-review"));
}

#[test]
fn the_finish_form_takes_the_keys_and_escape_folds_it() {
    let (mut cx, view) = change_screen("reviewed", ChangeTab::Files);
    cx.simulate_click("forge-start-review");
    cx.run_until_parked();
    cx.simulate_click("forge-finish-review");
    cx.run_until_parked();
    assert_eq!(focused(&cx).as_deref(), Some("forge-review-body"));
    cx.simulate_dismiss("forge-finish");
    cx.run_until_parked();
    view.read(|forge| assert!(!forge.review().unwrap().finishing));
}

#[test]
fn a_reader_without_write_picks_no_verdict() {
    let (mut cx, view) = change_screen("default", ChangeTab::Files);
    cx.update(&view, |forge, _, cx| {
        forge.start_review(cx);
        forge.finishing(true, cx);
        forge.session.connected = false;
    });
    cx.run_until_parked();
    assert!(super::disabled(&cx, "forge-verdict-approve"));
}

#[test]
fn a_reader_without_write_gets_no_close_or_merge() {
    let (cx, _) = change_screen("default", ChangeTab::Conversation);
    for id in ["forge-close-change", "forge-merge"] {
        let Some(view::wire::Node::Container(node)) = cx.find(id) else {
            panic!("{id} is a native container");
        };
        assert_eq!(
            node.interactivity.as_ref().unwrap().aria.disabled,
            Some(true),
            "{id}"
        );
    }
}

#[test]
fn a_gutter_comment_button_is_at_least_24_px_each_way() {
    use super::pixels;
    let (cx, _view) = change_screen("reviewed", ChangeTab::Files);
    for key in [
        "forge-gutter-src/lib.rs-old-1",
        "forge-gutter-src/lib.rs-new-1",
        "forge-gutter-src/lib.rs-new-5",
    ] {
        let height = pixels(cx.style(key).min_size.height);
        assert!(height >= Some(24.), "{key}: {height:?}");
        let column = pixels(cx.style(&format!("{key}-cell")).size.width);
        assert!(column > Some(24.), "{key}: a column of {column:?}");
    }
}

// A change from a job's branch talks in the job's room: its tab holds forge's lines about it and
// the replies under its opening line, never the room's other talk or another change's lines,
// and a reply goes under its opening line.
#[test]
fn a_change_in_a_job_room_shows_its_own_lines_and_their_thread() {
    let (mut cx, view) = change_screen("job", ChangeTab::Conversation);
    assert!(cx.has_text("opened this change"), "{:?}", cx.texts());
    assert!(cx.has_text("Reading it now"), "{:?}", cx.texts());
    assert!(!cx.has_text("Morning, all"), "{:?}", cx.texts());
    // the opening, three reviews and the one reply
    view.read(|forge| {
        assert_eq!(forge.change_tab_count(ChangeTab::Conversation), Some(5));
    });
    cx.update(&view, |forge, _, cx| {
        forge.reply = view::TextField::new("on it");
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_click("forge-reply-send");
    cx.run_until_parked();
    assert!(
        cx.host()
            .requests::<Submit<::chat::Chat>>()
            .iter()
            .any(|op| matches!(
                op,
                chat::Op::PostMessage { channel_id, thread: Some(2), .. } if channel_id == "j-7"
            )),
        "the reply goes under the opening line"
    );
}

#[test]
fn a_change_in_a_job_room_reads_the_same_from_notices_as_from_forge_code_lines() {
    let read = |mode| {
        let (cx, view) = change_screen(mode, ChangeTab::Conversation);
        let count = view.read(|forge| forge.change_tab_count(ChangeTab::Conversation));
        (cx.texts(), count)
    };
    let (texts, count) = read("job-notices");
    assert_eq!(count, Some(5), "{texts:?}");
    let in_j7: Vec<String> = texts
        .iter()
        .map(|text| text.replace("# j-8", "# j-7"))
        .collect();
    assert_eq!((in_j7, count), read("job"));
}

fn shows(cx: &view::testing::TestAppContext, said: &str) -> bool {
    cx.texts().iter().any(|text| text.contains(said))
}

#[test]
fn a_change_shows_the_checks_on_its_head_and_on_its_merge_preview() {
    let (cx, _view) = change_screen("default", ChangeTab::Conversation);
    assert!(cx.find("forge-checks-head").is_some(), "{:?}", cx.texts());
    assert!(
        cx.find("forge-checks-preview").is_some(),
        "{:?}",
        cx.texts()
    );
    for said in [
        "Checks",
        "Ada / build.sh#lane",
        "Rae / check.toml#check",
        "Rae / ci.toml#ci",
        "desktop:20261021-081734-1",
        "passed",
        "pending",
        "failed",
        "errored",
    ] {
        assert!(shows(&cx, said), "{said}: {:?}", cx.texts());
    }
    assert!(cx.find("forge-checks-preview-stale").is_none());
}

#[test]
fn a_preview_made_before_the_head_moved_is_marked_stale() {
    let (cx, _view) = change_screen("outdated", ChangeTab::Conversation);
    assert!(
        cx.find("forge-checks-preview-stale").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("forge-checks-preview-stale-why").is_some());
}

#[test]
fn a_preview_made_before_the_target_moved_is_marked_stale() {
    let (cx, _view) = change_screen("target-moved", ChangeTab::Conversation);
    assert!(
        cx.find("forge-checks-preview-stale").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("forge-checks-preview-stale-why").is_some());
}

#[test]
fn a_change_nobody_previewed_shows_only_its_head_checks() {
    let (cx, _view) = change_screen("unpreviewed", ChangeTab::Conversation);
    assert!(cx.find("forge-checks-head").is_some(), "{:?}", cx.texts());
    assert!(cx.find("forge-checks-preview").is_none());
}

#[test]
fn a_merged_change_shows_its_head_checks_and_asks_no_preview() {
    let (cx, _view) = change_screen("merged", ChangeTab::Conversation);
    assert!(cx.find("forge-checks-head").is_some(), "{:?}", cx.texts());
    let previews = cx
        .host()
        .requests::<Ask>()
        .into_iter()
        .filter(|query| matches!(query, Query::Preview { .. }))
        .count();
    assert_eq!(previews, 0);
}
