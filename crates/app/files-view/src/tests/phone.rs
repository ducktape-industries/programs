//! The phone's screens over the seeded network: each screen's states, the
//! sheets and Back, a write committed from a sheet, and the sweep at the
//! three phone widths.
use files::DiffKind;
use view::testing::sweep;

use super::*;
use crate::state::Holds;
use crate::words;

fn tap(cx: &mut TestAppContext, key: &str) {
    cx.simulate_click(key);
    cx.run_until_parked();
}

#[test]
fn the_root_lists_the_places_and_the_newest_snapshot() {
    let net = seeded();
    let cx = phone(&net);
    assert!(cx.has_text(words::TITLE), "{:?}", cx.texts());
    assert!(cx.has_text("/home/dev"));
    assert!(cx.has_text(words::SHARED_CAPTION));
    assert!(
        cx.has_text("Say more"),
        "the history row names the newest snapshot"
    );
    assert!(
        cx.back_handler().is_none(),
        "at the root Back leaves the program"
    );
}

#[test]
fn a_folder_screen_lists_its_entries_and_back_returns_to_the_places() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    assert!(cx.has_text("notes.txt"), "{:?}", cx.texts());
    assert!(cx.has_text("/home/dev"), "the path is the screen's caption");
    assert!(cx.back_handler().is_some());
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.has_text(words::PLACES));
    assert!(!cx.has_text("notes.txt"));
}

#[test]
fn a_file_screen_shows_what_it_is_and_its_text() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    tap(&mut cx, "files/entry/notes.txt");
    assert!(cx.has_text(NOTES), "{:?}", cx.texts());
    assert!(cx.has_text(words::kind(&file(), Holds::Text)));
    assert!(cx.has_text(&words::size(NOTES.len() as u64)));
}

#[test]
fn an_image_screen_shows_its_picture() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/shared");
    tap(&mut cx, "files/entry/dot.png");
    assert!(
        cx.has_text(words::kind(&file(), Holds::Picture)),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("files-picture").is_some(), "{:?}", cx.texts());
}

#[test]
fn an_empty_folder_says_what_would_fill_it() {
    let net = seeded();
    // the newest snapshot's byline is drawn: its time pinned as the seed's
    net.at(T0 + 240_000);
    commit(
        &net,
        net.me(),
        "Make room",
        vec![Change::Mkdir {
            path: "/home/dev/empty".into(),
        }],
    );
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    tap(&mut cx, "files/entry/empty");
    assert!(cx.has_text(words::EMPTY_FOLDER), "{:?}", cx.texts());
    assert!(cx.has_text(words::EMPTY_WRITABLE));
}

#[test]
fn the_actions_sheet_offers_the_folders_writes_and_back_closes_it() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    tap(&mut cx, "files/actions");
    assert!(cx.has_text("New folder"), "{:?}", cx.texts());
    assert!(cx.has_text("New text file"));
    cx.simulate_back();
    cx.run_until_parked();
    assert!(cx.find("files/actions-sheet").is_none());
    assert!(
        cx.has_text("notes.txt"),
        "Back closed the sheet, not the folder"
    );
}

/// The host's capsule beside the title row draws `ellipsis`: the page's
/// own Actions draw `ellipsis-vertical`, so the two never read as one.
#[test]
fn the_actions_control_is_not_drawn_as_the_capsules_ellipsis() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    let row = format!("{:?}", cx.node("files-folder-title"));
    assert!(!row.contains("icons/ellipsis.svg"), "{row}");
    let actions = format!("{:?}", cx.node("files/actions"));
    assert!(actions.contains("icons/ellipsis-vertical.svg"), "{actions}");
    let name = &cx.interactivity("files/actions").aria.label;
    assert_eq!(name.as_deref(), Some(words::ACTIONS));
}

#[test]
fn a_folder_the_reader_cannot_write_has_no_actions() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/everything");
    assert!(cx.has_text("shared"), "{:?}", cx.texts());
    assert!(
        cx.find("files/actions").is_some(),
        "no list covers the root"
    );
    tap(&mut cx, "files/entry/home");
    assert!(cx.has_text("ada"), "{:?}", cx.texts());
    assert!(
        cx.find("files/actions").is_none(),
        "what is made in /home is a home, the network's"
    );
    tap(&mut cx, "files/entry/ada");
    assert!(cx.has_text("diary.txt"), "{:?}", cx.texts());
    assert!(cx.find("files/actions").is_none(), "ada listed her home");
}

#[test]
fn a_picked_file_is_uploaded_from_its_sheet() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/shared");
    net.device(&[("photo.png", &PNG)]);
    tap(&mut cx, "files/actions");
    tap(&mut cx, "files/begin/upload");
    assert!(cx.has_text("photo.png"), "{:?}", cx.texts());
    tap(&mut cx, "files/write/submit");
    assert!(
        cx.has_text("Committed: Upload photo.png"),
        "{:?}",
        cx.texts()
    );
    tap(&mut cx, "files/entry/photo.png");
    assert!(cx.find("files-picture").is_some(), "{:?}", cx.texts());
}

#[test]
fn a_new_text_file_is_committed_from_its_sheet() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/shared");
    tap(&mut cx, "files/actions");
    tap(&mut cx, "files/begin/new-file");
    cx.simulate_input("files/write/name", "hello.md");
    cx.simulate_input("files/write/text", "hi from a phone");
    tap(&mut cx, "files/write/submit");
    assert!(cx.has_text("hello.md"), "{:?}", cx.texts());
    assert!(cx.has_text("Committed: Write hello.md"));
    assert!(cx.find("files/write").is_none());
}

#[test]
fn a_new_text_file_from_its_sheet_carries_the_list_picked() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/shared");
    tap(&mut cx, "files/actions");
    tap(&mut cx, "files/begin/new-file");
    cx.simulate_input("files/write/name", "open.md");
    tap(&mut cx, "files/write/list/anyone");
    tap(&mut cx, "files/write/submit");
    assert!(cx.find("files/write").is_none());
    let (list, _) = list_at(&net, "/shared/open.md");
    assert_eq!(list, Some(files::Ownership::Anyone));
}

#[test]
fn a_file_removed_from_its_sheet_returns_to_its_folder() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    tap(&mut cx, "files/entry/notes.txt");
    tap(&mut cx, "files/actions");
    tap(&mut cx, "files/begin/remove");
    tap(&mut cx, "files/write/submit");
    assert!(cx.has_text("docs"), "{:?}", cx.texts());
    assert!(!cx.has_text("notes.txt"));
}

#[test]
fn an_edit_from_its_sheet_shows_the_new_text() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    tap(&mut cx, "files/entry/notes.txt");
    tap(&mut cx, "files/actions");
    tap(&mut cx, "files/begin/edit");
    cx.simulate_input("files/write/text", "written on a phone");
    tap(&mut cx, "files/write/submit");
    assert!(cx.has_text("written on a phone"), "{:?}", cx.texts());
    assert!(cx.find("files/write").is_none());
}

#[test]
fn cancel_closes_a_form_with_nothing_committed() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/home");
    tap(&mut cx, "files/actions");
    tap(&mut cx, "files/begin/new-folder");
    cx.simulate_input("files/write/name", "never");
    tap(&mut cx, "files/write/cancel");
    assert!(cx.find("files/write").is_none());
    assert!(!cx.has_text("never"), "{:?}", cx.texts());
}

#[test]
fn the_history_opens_a_snapshot_with_what_it_changed() {
    let net = seeded();
    let mut cx = phone(&net);
    tap(&mut cx, "files/place/history");
    assert!(cx.has_text("Share the readme"), "{:?}", cx.texts());
    assert!(cx.has_text("v1"));
    tap(&mut cx, "files/snapshot/3");
    assert!(cx.has_text("Say more"));
    assert!(cx.has_text("/home/dev/notes.txt"), "{:?}", cx.texts());
    assert!(cx.has_text(words::diff_kind(DiffKind::Modified)));
}

#[test]
fn a_reader_signed_out_has_no_home_to_open() {
    let net = seeded();
    let signed_out = Session {
        account: None,
        ..net.session()
    };
    let cx = opened_as(&net, signed_out, true);
    assert!(cx.has_text(words::SIGNED_OUT), "{:?}", cx.texts());
}

#[test]
fn every_screen_and_sheet_fits_a_phone() {
    let net = seeded();
    let mut cx = phone(&net);
    sweep(&mut cx, |cx| {
        tap(cx, "files/place/home");
        // the folder's writes, and a name its field refuses
        tap(cx, "files/actions");
        tap(cx, "files/begin/new-folder");
        cx.simulate_input("files/write/name", "a/b");
        tap(cx, "files/write/submit");
        cx.simulate_back();
        cx.run_until_parked();
        tap(cx, "files/entry/notes.txt");
        tap(cx, "files/actions");
        tap(cx, "files/begin/edit");
        cx.simulate_back();
        cx.run_until_parked();
        cx.simulate_back();
        cx.run_until_parked();
        cx.simulate_back();
        cx.run_until_parked();
        tap(cx, "files/place/shared");
        tap(cx, "files/entry/dot.png");
        cx.simulate_back();
        cx.run_until_parked();
        cx.simulate_back();
        cx.run_until_parked();
        tap(cx, "files/place/history");
        tap(cx, "files/snapshot/2");
        cx.simulate_back();
        cx.run_until_parked();
        cx.simulate_back();
        cx.run_until_parked();
    });
}
