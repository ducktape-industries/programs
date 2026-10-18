//! The desktop's pages over the seeded network: what each page shows, a
//! refused read, and each write committed through the real program, as dev.
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use files::{BlobId, DiffKind, Node};
use view::methods::{
    self, BlobBody, ClipboardWrite, FilePick, HostRoute, Method, NewBlob, OpPut, PutBlobs,
    Query as Ask, Submit,
};
use view::prelude::{Error, MouseButton, MouseDownEvent, Role, px};
use view::{Point, wire};

use super::*;
use crate::menu::MENU;
use crate::state::{Holds, SHOW_MOST};
use crate::upload::UPLOAD_MOST;
use crate::words;

/// The place in the texts on screen of the first text containing `part`.
fn place_of(cx: &TestAppContext, part: &str) -> usize {
    cx.texts()
        .iter()
        .position(|text| text.contains(part))
        .unwrap_or_else(|| panic!("{part:?} is not on screen: {:?}", cx.texts()))
}

fn open_home(cx: &mut TestAppContext) {
    cx.simulate_click("files/place/home");
    cx.run_until_parked();
}

fn open_notes(cx: &mut TestAppContext) {
    open_home(cx);
    cx.simulate_click("files/entry/notes.txt");
    cx.run_until_parked();
}

/// Opens `write`'s form from the page on screen.
fn begin(cx: &mut TestAppContext, write: &str) {
    cx.simulate_click(&format!("files/begin/{write}"));
    cx.run_until_parked();
}

fn commit_form(cx: &mut TestAppContext) {
    cx.simulate_click("files/write/submit");
    cx.run_until_parked();
}

#[test]
fn the_places_name_the_readers_home_shared_and_the_newest_snapshots() {
    let net = seeded();
    let cx = desktop(&net);
    assert!(cx.has_text("/home/dev"), "{:?}", cx.texts());
    assert!(cx.has_text(words::SHARED));
    assert!(cx.has_text(words::EVERYTHING));
    assert!(place_of(&cx, "Say more") < place_of(&cx, "Start my notes"));
}

#[test]
fn a_home_lists_its_folders_first() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    assert!(place_of(&cx, "docs") < place_of(&cx, "notes.txt"));
    assert!(
        cx.has_text("1 item"),
        "a folder says how many entries it holds"
    );
}

#[test]
fn a_text_file_opens_with_its_text() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_notes(&mut cx);
    assert!(cx.has_text(NOTES), "{:?}", cx.texts());
    assert!(shows(&cx, "Text · 14 bytes"));
}

#[test]
fn a_file_that_is_not_text_says_how_big_it_is() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    cx.simulate_click("files/entry/blob.bin");
    cx.run_until_parked();
    assert!(cx.has_text(&words::not_text(4)), "{:?}", cx.texts());
}

#[test]
fn a_link_says_where_it_points() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    cx.simulate_click("files/entry/latest");
    cx.run_until_parked();
    assert!(cx.has_text(words::TARGET));
    assert!(cx.has_text("/shared/readme.txt"), "{:?}", cx.texts());
}

#[test]
fn a_crumb_opens_the_folder_it_names() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    cx.simulate_click("files/entry/docs");
    cx.run_until_parked();
    assert!(cx.has_text("plan.md"));
    cx.simulate_click("files/crumb:/home/dev");
    cx.run_until_parked();
    assert!(cx.has_text("notes.txt"), "{:?}", cx.texts());
}

#[test]
fn the_history_lists_snapshots_newest_first_their_authors_and_the_pins() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/tab/history");
    cx.run_until_parked();
    assert!(place_of(&cx, "Say more") < place_of(&cx, "Share the readme"));
    assert!(shows(&cx, "ada · "), "an author is named: {:?}", cx.texts());
    assert!(cx.has_text("v1"));
    assert!(
        shows(&cx, &words::snapshot(1)),
        "the pin names its snapshot"
    );
}

#[test]
fn a_snapshot_lists_what_it_changed() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/tab/history");
    cx.run_until_parked();
    cx.simulate_click("files/snapshot/2");
    cx.run_until_parked();
    assert!(cx.has_text("/shared/readme.txt"), "{:?}", cx.texts());
    assert!(cx.has_text(words::diff_kind(DiffKind::Added)));
    assert!(
        !cx.has_text("/home/dev/notes.txt"),
        "dev's notes did not change"
    );
}

#[test]
fn the_first_snapshot_says_everything_in_it_is_new() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/tab/history");
    cx.run_until_parked();
    cx.simulate_click("files/snapshot/1");
    cx.run_until_parked();
    assert!(cx.has_text(words::FIRST_SNAPSHOT), "{:?}", cx.texts());
}

#[test]
fn another_accounts_listed_home_offers_no_writes() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/place/everything");
    cx.run_until_parked();
    cx.simulate_click("files/entry/home");
    cx.run_until_parked();
    cx.simulate_click("files/entry/ada");
    cx.run_until_parked();
    assert!(cx.has_text("diary.txt"));
    assert!(cx.find("files/begin/new-folder").is_none());
}

#[test]
fn a_new_folder_is_committed_and_listed() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    begin(&mut cx, "new-folder");
    cx.simulate_input("files/write/name", "drafts");
    commit_form(&mut cx);
    assert!(cx.has_text("drafts"), "{:?}", cx.texts());
    assert!(cx.has_text("Committed: Make drafts"));
    assert!(cx.find("files/write").is_none(), "the form closes");
}

#[test]
fn a_new_text_file_picked_only_mine_is_committed_with_its_list() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    begin(&mut cx, "new-file");
    assert!(cx.has_text(words::AS_ITS_FOLDER_NOTE), "{:?}", cx.texts());
    cx.simulate_input("files/write/name", "mine.txt");
    cx.simulate_input("files/write/text", "keep out");
    cx.simulate_click("files/write/list/me");
    cx.run_until_parked();
    assert!(!cx.has_text(words::AS_ITS_FOLDER_NOTE));
    commit_form(&mut cx);
    assert!(cx.find("files/write").is_none(), "the form closes");
    let (list, owner) = list_at(&net, "/shared/mine.txt");
    assert_eq!(list, Some(files::Ownership::Owner));
    assert_eq!(owner, Some(files::Party::Account(net.me())));
}

#[test]
fn a_new_folder_listed_for_accounts_and_programs_is_made_with_its_list() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    begin(&mut cx, "new-folder");
    cx.simulate_input("files/write/name", "team");
    cx.simulate_click("files/write/list/listed");
    cx.run_until_parked();
    commit_form(&mut cx);
    assert!(cx.has_text(words::LIST_SOMEONE), "{:?}", cx.texts());
    let ada = net.person("ada");
    cx.simulate_input("files/write/listed", &format!("{ada}, files"));
    commit_form(&mut cx);
    assert!(cx.find("files/write").is_none(), "the form closes");
    let (list, _) = list_at(&net, "/home/dev/team");
    assert_eq!(
        list,
        Some(files::Ownership::Listed(vec![
            files::Party::Account(ada),
            files::Party::Program(files::PROGRAM.into()),
        ]))
    );
}

#[test]
fn a_new_text_file_is_committed_with_the_typed_message() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    begin(&mut cx, "new-file");
    cx.simulate_input("files/write/name", "todo.txt");
    cx.simulate_input("files/write/text", "buy milk");
    cx.simulate_input("files/write/message", "Add a todo");
    commit_form(&mut cx);
    assert!(cx.has_text("Committed: Add a todo"));
    cx.simulate_click("files/entry/todo.txt");
    cx.run_until_parked();
    assert!(cx.has_text("buy milk"), "{:?}", cx.texts());
}

#[test]
fn an_edit_writes_the_new_text_over_the_old() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_notes(&mut cx);
    begin(&mut cx, "edit");
    assert!(
        cx.has_text(NOTES),
        "the field starts from the text as it is"
    );
    cx.simulate_input("files/write/text", "rewritten");
    commit_form(&mut cx);
    assert!(cx.has_text("rewritten"), "{:?}", cx.texts());
    assert!(!cx.has_text(NOTES));
}

#[test]
fn a_rename_moves_the_page_to_the_new_path() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_notes(&mut cx);
    begin(&mut cx, "rename");
    cx.simulate_input("files/write/name", "/home/dev/docs/notes.txt");
    commit_form(&mut cx);
    assert!(cx.has_text(NOTES), "{:?}", cx.texts());
    assert!(cx.has_text("docs"), "the crumbs name its new folder");
    cx.simulate_click("files/crumb:/home/dev");
    cx.run_until_parked();
    assert!(!cx.has_text("notes.txt"), "{:?}", cx.texts());
}

#[test]
fn a_removed_file_leaves_its_folder_on_screen() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_notes(&mut cx);
    begin(&mut cx, "remove");
    assert!(cx.has_text(&words::removes("/home/dev/notes.txt", false)));
    commit_form(&mut cx);
    assert!(cx.has_text("docs"), "{:?}", cx.texts());
    assert!(!cx.has_text("notes.txt"));
    assert!(cx.has_text("Committed: Remove notes.txt"));
}

#[test]
fn a_name_files_would_refuse_is_said_on_its_field() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    begin(&mut cx, "new-folder");
    cx.simulate_input("files/write/name", "a/b");
    commit_form(&mut cx);
    assert!(cx.has_text(words::ONE_NAME), "{:?}", cx.texts());
    assert!(cx.find("files/write").is_some(), "the form stays open");
}

#[test]
fn a_commit_refused_as_stale_says_why_in_its_form() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    begin(&mut cx, "new-file");
    // dev writes the same path from another device meanwhile
    commit(
        &net,
        net.me(),
        "Elsewhere",
        vec![put(&net, "/home/dev/x.txt", b"first")],
    );
    cx.run_until_parked();
    cx.simulate_input("files/write/name", "x.txt");
    commit_form(&mut cx);
    assert!(
        shows(&cx, "changed since the base snapshot"),
        "{:?}",
        cx.texts()
    );
    assert!(
        cx.find("files/write").is_some(),
        "the form stays open with the program's words for why"
    );
}

#[test]
fn the_arrow_keys_on_the_tabs_open_the_history() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_focus("files-tabs");
    cx.simulate_key_down("files-tabs", "right");
    cx.run_until_parked();
    assert!(cx.has_text("v1"), "the pins show: {:?}", cx.texts());
    cx.simulate_key_down("files-tabs", "left");
    cx.run_until_parked();
    assert!(cx.has_text(words::EVERYTHING));
}

#[test]
fn a_pin_opens_the_snapshot_it_names() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/tab/history");
    cx.run_until_parked();
    cx.simulate_click("files/pin/v1");
    cx.run_until_parked();
    assert!(cx.has_text("Start my notes"), "{:?}", cx.texts());
    assert!(cx.has_text(words::FIRST_SNAPSHOT));
}

#[test]
fn every_page_says_why_files_refused_its_read() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.host()
        .refuse::<Ask<files::Files>>("unavailable", "the node is away");
    cx.simulate_click("files/tab/history");
    cx.run_until_parked();
    assert!(
        cx.find("files-history-refused").is_some(),
        "{:?}",
        cx.texts()
    );
    cx.simulate_click("files/tab/places");
    cx.run_until_parked();
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    assert!(
        cx.find("files-folder-refused").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text("the node is away"));
}

#[test]
fn a_refused_snapshot_is_asked_once_and_again_on_retry() {
    let net = seeded();
    let mut cx = desktop(&net);
    let asked = Rc::new(Cell::new(0));
    let counted = asked.clone();
    let chain = net.clone();
    cx.host()
        .handle::<Ask<files::Files>>(move |query| match query {
            files::Query::Snapshot { .. } => {
                counted.set(counted.get() + 1);
                Err(view::prelude::Error::new("unavailable", "the blob is gone"))
            }
            query => chain.query(files::PROGRAM, &query),
        });
    cx.simulate_click("files/tab/history");
    cx.run_until_parked();
    cx.simulate_click("files/snapshot/2");
    cx.run_until_parked();
    assert!(cx.has_text("the blob is gone"), "{:?}", cx.texts());
    assert_eq!(asked.get(), 1, "a refusal is not asked again by drawing it");
    cx.simulate_click("files-snapshot-retry");
    cx.run_until_parked();
    assert_eq!(asked.get(), 2);
}

#[test]
fn a_refused_commit_says_why_in_its_form() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.host()
        .refuse::<Submit<files::Files>>("refused", "the ledger is full");
    open_home(&mut cx);
    begin(&mut cx, "new-folder");
    cx.simulate_input("files/write/name", "drafts");
    commit_form(&mut cx);
    assert!(cx.find("files/write/refused").is_some(), "{:?}", cx.texts());
    assert!(cx.has_text("That didn’t go through: the ledger is full"));
}

#[test]
fn another_account_signing_in_closes_the_open_form() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    begin(&mut cx, "new-folder");
    let ada = net.person("ada");
    cx.host().stream::<HostSession>().send(Session {
        account: Some(ada),
        ..net.session()
    });
    cx.run_until_parked();
    assert!(cx.find("files/write").is_none(), "{:?}", cx.texts());
}

#[test]
fn a_renamed_folder_stays_on_screen_at_its_new_path() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    cx.simulate_click("files/entry/docs");
    cx.run_until_parked();
    begin(&mut cx, "rename");
    cx.simulate_input("files/write/name", "/home/dev/papers");
    commit_form(&mut cx);
    assert!(cx.find("files/crumb:/home/dev").is_some());
    assert!(cx.has_text("papers"), "{:?}", cx.texts());
    assert!(cx.has_text("plan.md"));
}

#[test]
fn a_removed_folder_returns_to_the_folder_it_was_in() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    cx.simulate_click("files/entry/docs");
    cx.run_until_parked();
    begin(&mut cx, "remove");
    assert!(cx.has_text(&words::removes("/home/dev/docs", true)));
    commit_form(&mut cx);
    assert!(cx.has_text("notes.txt"), "{:?}", cx.texts());
    assert!(cx.find("files/entry/docs").is_none());
}

#[test]
fn a_wide_pane_lists_the_places_down_the_side_in_place_of_the_tabs() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1000.);
    assert!(cx.find("files-side").is_some(), "{:?}", cx.texts());
    assert!(cx.find("files-tabs").is_none());
    assert!(
        cx.find("files/side/pin/v1").is_some(),
        "the pins are listed"
    );
    cx.simulate_click("files/side/home");
    cx.run_until_parked();
    assert!(cx.has_text(words::KIND), "a wide folder lists in columns");
    assert!(
        cx.has_text(words::kind(&file(), Holds::Text)),
        "{:?}",
        cx.texts()
    );
    cx.simulate_resize(700., 900.);
    cx.run_until_parked();
    assert!(cx.find("files-side").is_none());
    assert!(
        cx.find("files-tabs").is_some(),
        "narrowed, the tabs are back"
    );
}

#[test]
fn split_a_file_opens_beside_its_folder_and_a_folder_there_takes_its_place() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1440.);
    cx.simulate_click("files/side/home");
    cx.run_until_parked();
    assert!(cx.has_text(words::PICK_A_FILE));
    cx.simulate_click("files/entry/notes.txt");
    cx.run_until_parked();
    assert!(cx.find("files-list").is_some() && cx.find("files-detail").is_some());
    assert!(cx.has_text(NOTES), "the file shows: {:?}", cx.texts());
    assert!(cx.has_text("docs"), "its folder stays listed beside it");
    cx.simulate_click("files/entry/docs");
    cx.run_until_parked();
    assert!(cx.has_text("plan.md"), "{:?}", cx.texts());
    assert!(
        cx.has_text(words::PICK_A_FILE),
        "the folder took the file's place"
    );
}

#[test]
fn split_a_snapshot_opens_beside_the_history() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1440.);
    cx.simulate_click("files/side/pin/v1");
    cx.run_until_parked();
    assert!(cx.has_text(words::FIRST_SNAPSHOT), "{:?}", cx.texts());
    assert!(
        cx.has_text("Say more"),
        "the history stays listed beside it"
    );
}

#[test]
fn an_image_shows_as_its_picture() {
    let net = seeded();
    let mut cx = desktop(&net);
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    cx.simulate_click("files/entry/dot.png");
    cx.run_until_parked();
    let under = format!(
        "{} · {}",
        words::kind(&file(), Holds::Picture),
        words::size(PNG.len() as u64)
    );
    assert!(shows(&cx, &under), "{:?}", cx.texts());
    assert!(
        matches!(cx.find("files-picture"), Some(wire::Node::Image { .. })),
        "{:?}",
        cx.texts()
    );
}

/// Whether the side's row for `key` is the one picked.
fn picked(cx: &TestAppContext, key: &str) -> bool {
    cx.interactivity(&format!("files/side/{key}")).aria.selected == Some(true)
}

#[test]
fn a_pin_made_while_a_folder_is_open_joins_the_side() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1000.);
    cx.simulate_click("files/side/home");
    cx.run_until_parked();
    let snapshot = commit(
        &net,
        net.me(),
        "More notes",
        vec![put(&net, "/home/dev/more.txt", b"more")],
    );
    let pin = Op::Pin {
        name: "v2".into(),
        snapshot,
    };
    net.submit(net.me(), files::PROGRAM, &pin)
        .expect("a pin on a minted snapshot");
    cx.run_until_parked();
    assert!(cx.find("files/side/pin/v2").is_some(), "{:?}", cx.texts());
}

#[test]
fn a_restored_view_reads_what_landed_while_it_was_away() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1000.);
    cx.simulate_click("files/side/home");
    cx.run_until_parked();
    let bytes = cx.snapshot().unwrap();
    let snapshot = commit(
        &net,
        net.me(),
        "Share more",
        vec![put(&net, "/shared/more.txt", b"more")],
    );
    let pin = Op::Pin {
        name: "v2".into(),
        snapshot,
    };
    net.submit(net.me(), files::PROGRAM, &pin)
        .expect("a pin on a minted snapshot");
    cx.restore::<Files>(&bytes).unwrap();
    cx.run_until_parked();
    assert!(cx.find("files/side/pin/v2").is_some(), "{:?}", cx.texts());
}

#[test]
fn the_side_picks_the_place_or_the_pin_the_page_is_under() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1000.);
    cx.simulate_click("files/side/home");
    cx.run_until_parked();
    cx.simulate_click("files/entry/notes.txt");
    cx.run_until_parked();
    assert!(picked(&cx, "home") && !picked(&cx, "everything"));
    cx.simulate_click("files/side/shared");
    cx.run_until_parked();
    cx.simulate_click("files/entry/readme.txt");
    cx.run_until_parked();
    assert!(picked(&cx, "shared"));
    cx.simulate_click("files/side/pin/v1");
    cx.run_until_parked();
    assert!(picked(&cx, "pin/v1") && !picked(&cx, "history"));
    // one Tab stop: the arrows open the item over or under
    cx.simulate_focus("files-side-list");
    cx.simulate_key_down("files-side-list", "up");
    cx.run_until_parked();
    assert!(picked(&cx, "history"), "{:?}", cx.texts());
    assert!(cx.has_text("Say more"), "the history opened");
    cx.simulate_key_down("files-side-list", "down");
    cx.run_until_parked();
    assert!(picked(&cx, "pin/v1"));
}

/// The snapshot each `Read` a page asked was at.
type Reads = Rc<RefCell<Vec<Option<BlobId>>>>;

/// A file at `/shared/big.png` that begins as a PNG and is `size` bytes in
/// all, committed by dev; then its page, the `Read`s it asked kept.
fn picture_page(size: usize) -> (TestAppContext, Network, Reads) {
    let net = seeded();
    let mut bytes = PNG.to_vec();
    bytes.resize(size, 0);
    commit(
        &net,
        net.me(),
        "A big picture",
        vec![put(&net, "/shared/big.png", &bytes)],
    );
    let mut cx = desktop(&net);
    let reads = Reads::default();
    let kept = reads.clone();
    let chain = net.clone();
    cx.host().handle::<Ask<files::Files>>(move |query| {
        if let files::Query::Read { at, .. } = &query {
            kept.borrow_mut().push(*at);
        }
        chain.query(files::PROGRAM, &query)
    });
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    cx.simulate_click("files/entry/big.png");
    cx.run_until_parked();
    (cx, net, reads)
}

#[test]
fn a_picture_larger_than_one_read_is_read_whole_at_one_snapshot() {
    let size = 2 * files::CHUNK_SIZE as usize + 8;
    let (cx, _, reads) = picture_page(size);
    // the first 64 KiB, then a chunk at a time
    let reads = reads.borrow();
    assert_eq!(reads.len(), 3);
    assert!(reads[0].is_some() && reads.iter().all(|at| *at == reads[0]));
    let Some(wire::Node::Image {
        data: Some(wire::ImageData::Encoded(bytes)),
        ..
    }) = cx.find("files-picture")
    else {
        panic!("no picture: {:?}", cx.texts());
    };
    assert_eq!(bytes.len(), size);
}

#[test]
fn a_picture_on_screen_is_not_read_again_by_a_commit_that_leaves_it() {
    let (mut cx, net, reads) = picture_page(2 * files::CHUNK_SIZE as usize + 8);
    assert_eq!(reads.borrow().len(), 3);
    commit(
        &net,
        net.me(),
        "Another file",
        vec![put(&net, "/shared/other.txt", b"other")],
    );
    cx.run_until_parked();
    assert_eq!(reads.borrow().len(), 4, "its first bytes again, no more");
    assert!(
        matches!(cx.find("files-picture"), Some(wire::Node::Image { .. })),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn a_picture_read_short_of_its_size_says_so() {
    let net = seeded();
    let mut bytes = PNG.to_vec();
    bytes.resize(2 * files::CHUNK_SIZE as usize, 0);
    commit(
        &net,
        net.me(),
        "A big picture",
        vec![put(&net, "/shared/big.png", &bytes)],
    );
    let mut cx = desktop(&net);
    let chain = net.clone();
    // every read past the first brings nothing
    cx.host().handle::<Ask<files::Files>>(move |query| {
        let past = matches!(query, files::Query::Read { offset, .. } if offset > 0);
        match chain.query(files::PROGRAM, &query) {
            Ok(files::Reply::Contents(contents)) if past => {
                Ok(files::Reply::Contents(files::Contents {
                    bytes: Vec::new(),
                    eof: true,
                    ..contents
                }))
            }
            reply => reply,
        }
    });
    cx.simulate_click("files/place/shared");
    cx.run_until_parked();
    cx.simulate_click("files/entry/big.png");
    cx.run_until_parked();
    assert!(cx.find("files-picture").is_none());
    assert!(cx.find("files-file-refused").is_some(), "{:?}", cx.texts());
}

#[test]
fn a_picture_too_large_to_draw_says_its_size_and_is_not_read_on() {
    let (cx, _, reads) = picture_page(SHOW_MOST as usize + 1);
    assert_eq!(
        reads.borrow().len(),
        1,
        "only the first bytes, which say it is a picture"
    );
    assert!(cx.find("files-picture").is_none());
    assert!(
        shows(&cx, &words::too_big_to_show(SHOW_MOST + 1)),
        "{:?}",
        cx.texts()
    );
}

/// What files holds at `path` now, read whole.
fn held(net: &Network, path: &str) -> Vec<u8> {
    let read = files::Query::Read {
        path: path.into(),
        at: None,
        offset: 0,
        len: u64::MAX,
    };
    match net.query(files::PROGRAM, &read) {
        Ok(files::Reply::Contents(contents)) => contents.bytes,
        other => panic!("{path}: {other:?}"),
    }
}

/// `size` bytes no two chunks of which are alike.
fn film(size: usize) -> Vec<u8> {
    (0..size).map(|at| (at % 251) as u8).collect()
}

/// Every `op.put` the view asks, by its blobs, answered by `answer` (by
/// the ask's count from 1, then the blobs); and every op it submits to
/// `files`, run on `net`.
struct Sends {
    puts: Rc<RefCell<Vec<Vec<NewBlob>>>>,
    ops: Rc<RefCell<Vec<files::Op>>>,
}

fn sends(
    cx: &TestAppContext,
    net: &Network,
    answer: impl Fn(usize, &PutBlobs) -> Option<Error> + 'static,
) -> Sends {
    let puts = Rc::new(RefCell::new(Vec::new()));
    let asked = puts.clone();
    let chain = net.clone();
    cx.host().handle::<OpPut>(move |ask| {
        asked.borrow_mut().push(ask.blobs.clone());
        if let Some(refusal) = answer(asked.borrow().len(), &ask) {
            return Err(refusal);
        }
        let reply = chain.answer("op.put", None, &methods::encode(&ask))?;
        Ok(methods::decode(&reply).expect("op.put answers its ids"))
    });
    let ops = Rc::new(RefCell::new(Vec::new()));
    let sent = ops.clone();
    let chain = net.clone();
    cx.host().handle::<Submit<files::Files>>(move |op| {
        sent.borrow_mut().push(op.clone());
        let reply = chain.answer(
            "op.submit",
            Some(files::PROGRAM),
            &Submit::<files::Files>::encode_request(&op),
        )?;
        Ok(methods::decode(&reply).expect("op.submit answers its outcome"))
    });
    Sends { puts, ops }
}

/// Whether any op the view sent carries a file's bytes: none may.
fn carries_bytes(op: &files::Op) -> bool {
    match op {
        files::Op::Stage { .. } => true,
        files::Op::Commit { changes, .. } => changes.iter().any(|change| {
            matches!(
                change,
                Change::Put {
                    content: Content::Inline(_),
                    ..
                }
            )
        }),
        files::Op::Pin { .. } | files::Op::Unpin { .. } => false,
    }
}

#[test]
fn an_uploaded_picture_is_committed_and_opens_as_itself() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    net.device(&[("dot.png", &PNG)]);
    begin(&mut cx, "upload");
    assert!(cx.has_text("dot.png"), "{:?}", cx.texts());
    commit_form(&mut cx);
    assert!(
        shows(&cx, &words::uploads(&["dot.png"])),
        "{:?}",
        cx.texts()
    );
    assert_eq!(held(&net, "/home/dev/dot.png"), PNG);
    cx.simulate_click("files/entry/dot.png");
    cx.run_until_parked();
    assert!(cx.find("files-picture").is_some(), "{:?}", cx.texts());
}

#[test]
fn an_upload_puts_each_file_from_the_device_then_commits_only_their_ids() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    let big = film(2 * files::CHUNK_SIZE as usize + 3);
    net.device(&[("film.mov", &big), ("note.txt", b"hi")]);
    let sends = sends(&cx, &net, |_, _| None);
    begin(&mut cx, "upload");
    commit_form(&mut cx);
    let picked = |file, offset, len| NewBlob {
        kind: files::CHUNK.into(),
        body: BlobBody::Picked { file, offset, len },
    };
    let chunk = files::CHUNK_SIZE;
    assert_eq!(
        *sends.puts.borrow(),
        [
            vec![
                picked(1, 0, chunk),
                picked(1, chunk, chunk),
                picked(1, 2 * chunk, 3)
            ],
            vec![picked(2, 0, 2)],
        ],
        "a put per file, its chunks read by the host, never by the view"
    );
    let ops = sends.ops.borrow();
    assert_eq!(ops.len(), 1, "one commit");
    assert!(!ops.iter().any(carries_bytes), "{ops:?}");
    assert!(
        shows(&cx, &words::uploads(&["film.mov", "note.txt"])),
        "{:?}",
        cx.texts()
    );
    assert_eq!(held(&net, "/home/dev/film.mov"), big);
    assert_eq!(held(&net, "/home/dev/note.txt"), b"hi");
}

#[test]
fn a_refused_put_stops_the_upload_and_commit_again_goes_on_from_it() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    let big = film(files::CHUNK_SIZE as usize + 3);
    net.device(&[("film.mov", &big), ("note.txt", b"hi")]);
    let sends = sends(&cx, &net, |count, _| {
        (count == 2).then(|| Error::new("unavailable", "the node is away"))
    });
    begin(&mut cx, "upload");
    commit_form(&mut cx);
    assert!(shows(&cx, "the node is away"), "{:?}", cx.texts());
    assert!(sends.ops.borrow().is_empty(), "no commit before every put");
    commit_form(&mut cx);
    let puts = sends.puts.borrow();
    assert_eq!(puts.len(), 3, "the film once, the note twice");
    assert_eq!(puts[1], puts[2], "the note again, not the film");
    assert_eq!(held(&net, "/home/dev/film.mov"), big);
    assert_eq!(held(&net, "/home/dev/note.txt"), b"hi");
}

#[test]
fn a_text_file_goes_up_as_a_chunk_and_its_commit_names_it() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    let sends = sends(&cx, &net, |_, _| None);
    begin(&mut cx, "new-file");
    cx.simulate_input("files/write/name", "todo.txt");
    cx.simulate_input("files/write/text", "buy milk");
    commit_form(&mut cx);
    assert_eq!(
        *sends.puts.borrow(),
        [vec![NewBlob {
            kind: files::CHUNK.into(),
            body: BlobBody::Bytes(b"buy milk".to_vec()),
        }]]
    );
    // the id the network holds those bytes by
    let chunk = net.put_blob(files::CHUNK, b"buy milk".to_vec()).unwrap();
    let ops = sends.ops.borrow();
    let [files::Op::Commit { changes, .. }] = ops.as_slice() else {
        panic!("one commit: {ops:?}");
    };
    assert!(
        matches!(&changes[..], [Change::Put { content: Content::Chunks { size: 8, chunks }, .. }] if *chunks == [chunk]),
        "{changes:?}"
    );
    assert_eq!(held(&net, "/home/dev/todo.txt"), b"buy milk");
}

#[test]
fn an_empty_text_file_puts_nothing_and_commits_no_chunks() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    let sends = sends(&cx, &net, |_, _| None);
    begin(&mut cx, "new-file");
    cx.simulate_input("files/write/name", "empty.txt");
    commit_form(&mut cx);
    assert!(sends.puts.borrow().is_empty(), "no blob to put");
    assert_eq!(held(&net, "/home/dev/empty.txt"), b"");
}

#[test]
fn a_file_too_big_is_left_out_and_said_why() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    let huge = vec![0; UPLOAD_MOST as usize + 1];
    net.device(&[("huge.iso", &huge), ("notes.txt", b"new")]);
    begin(&mut cx, "upload");
    assert!(
        shows(&cx, &words::too_big_to_upload(UPLOAD_MOST + 1)),
        "{:?}",
        cx.texts()
    );
    assert!(shows(&cx, words::REPLACES), "notes.txt is there already");
    commit_form(&mut cx);
    assert_eq!(held(&net, "/home/dev/notes.txt"), b"new");
}

#[test]
fn a_pick_with_nothing_to_send_offers_no_commit() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    net.device(&[("docs", b"not a folder")]);
    begin(&mut cx, "upload");
    assert!(
        shows(
            &cx,
            &words::held_by(&Node::Dir {
                tree: BlobId::Sha256([0; 32]),
                entries: 1,
            })
        ),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text(words::NOTHING_TO_UPLOAD));
    assert_eq!(
        cx.interactivity("files/write/submit").aria.disabled,
        Some(true)
    );
}

#[test]
fn a_cancel_opens_nothing_and_a_refused_pick_says_why() {
    let net = seeded();
    let mut cx = desktop(&net);
    open_home(&mut cx);
    net.device(&[]);
    begin(&mut cx, "upload");
    assert!(cx.find("files/write").is_none(), "cancelled: no form");
    cx.host()
        .refuse::<FilePick>("host_fault", "File picker not yet implemented for iOS");
    begin(&mut cx, "upload");
    assert!(
        shows(&cx, "File picker not yet implemented for iOS"),
        "{:?}",
        cx.texts()
    );
}

// The menu a right press opens.

/// The session on a chain links are minted in: the rig's own, `live`, has
/// no salt, so it names none.
fn on_a_chain(net: &Network) -> Session {
    Session {
        chain_id: "live#0a1b2c3d".into(),
        ..net.session()
    }
}

/// The view over `net`, read by dev on a chain links are minted in.
fn linked(net: &Network) -> TestAppContext {
    opened_as(net, on_a_chain(net), false)
}

/// Where the pointer is pressed in the window.
const PRESS: (f32, f32) = (240., 180.);

/// A right press on `key`, at [`PRESS`].
fn right_press(cx: &mut TestAppContext, key: &str) {
    let event = MouseDownEvent {
        button: MouseButton::Right,
        position: Point {
            x: px(PRESS.0),
            y: px(PRESS.1),
        },
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    };
    cx.simulate_mouse_down(key, event);
    cx.run_until_parked();
}

/// The open menu: where it is, and its items by the slug of their keys
/// (`files/menu/<slug>`), in order; `None` while none shows.
fn menu(cx: &TestAppContext) -> Option<((f32, f32), Vec<String>)> {
    fn find(node: &wire::Node) -> Option<((f32, f32), Vec<String>)> {
        if let wire::Node::Anchored {
            position: Some([x, y]),
            children,
            ..
        } = node
            && let Some(menu) = children.iter().find(|child| child.key() == Some(MENU))
        {
            let items = menu.children().iter().filter_map(|item| {
                let slug = item.key()?.strip_prefix("files/menu/")?;
                Some(slug.to_owned())
            });
            return Some(((*x, *y), items.collect()));
        }
        node.children().iter().find_map(find)
    }
    find(cx.root())
}

/// The open menu's items.
fn items(cx: &TestAppContext) -> Vec<String> {
    menu(cx).map(|(_, items)| items).unwrap_or_default()
}

fn focused(cx: &TestAppContext) -> Option<&str> {
    cx.focused().and_then(wire::Node::key)
}

#[test]
fn a_right_press_on_a_file_opens_its_menu_at_the_pointer() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    assert_eq!(menu(&cx), None);
    right_press(&mut cx, "files/entry/notes.txt");
    assert_eq!(
        menu(&cx),
        Some((
            PRESS,
            vec![
                "open".into(),
                "copy-link".into(),
                "rename".into(),
                "remove".into()
            ]
        )),
        "{:?}",
        cx.texts()
    );
    assert!(cx.has_text(words::OPEN) && cx.has_text(words::COPY_LINK));
    assert_eq!(cx.interactivity(MENU).role, Some(Role::Menu));
    assert_eq!(focused(&cx), Some(MENU), "the menu takes the keys");
}

#[test]
fn a_folders_row_offers_its_page_its_link_and_its_own_writes() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/docs");
    assert_eq!(items(&cx), ["open", "copy-link", "rename", "remove"]);
    cx.simulate_click("files/menu/remove");
    cx.run_until_parked();
    assert!(
        cx.has_text(&words::removes("/home/dev/docs", true)),
        "a folder's removal takes what it holds: {:?}",
        cx.texts()
    );
    assert_eq!(menu(&cx), None, "the form opens in the menu's place");
}

#[test]
fn a_row_another_account_writes_offers_its_page_and_its_link_only() {
    let net = seeded();
    let mut cx = linked(&net);
    cx.simulate_click("files/place/everything");
    cx.run_until_parked();
    cx.simulate_click("files/entry/home");
    cx.run_until_parked();
    cx.simulate_click("files/entry/ada");
    cx.run_until_parked();
    right_press(&mut cx, "files/entry/diary.txt");
    assert_eq!(items(&cx), ["open", "copy-link"]);
}

#[test]
fn copy_link_puts_the_rows_link_on_the_clipboard_and_says_so() {
    let net = seeded();
    let mut cx = linked(&net);
    let copied = Rc::new(RefCell::new(Vec::new()));
    let held = copied.clone();
    cx.host().handle::<ClipboardWrite>(move |text| {
        held.borrow_mut().push(text);
        Ok(())
    });
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/notes.txt");
    cx.simulate_click("files/menu/copy-link");
    cx.run_until_parked();
    assert_eq!(
        *copied.borrow(),
        ["duck://live-0a1b2c3d/files/home/dev/notes.txt"]
    );
    assert!(cx.has_text(words::LINK_COPIED), "{:?}", cx.texts());
    assert_eq!(menu(&cx), None);
    assert_eq!(focused(&cx), Some("files/entry/notes.txt"));
}

#[test]
fn a_refused_copy_says_why() {
    let net = seeded();
    let mut cx = linked(&net);
    cx.host()
        .refuse::<ClipboardWrite>("host_fault", "the clipboard is locked");
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/notes.txt");
    cx.simulate_click("files/menu/copy-link");
    cx.run_until_parked();
    assert!(shows(&cx, "the clipboard is locked"), "{:?}", cx.texts());
    assert!(!cx.has_text(words::LINK_COPIED));
}

#[test]
fn before_the_session_names_a_chain_no_link_is_offered() {
    let net = seeded();
    // the rig's chain id, `live`, has no salt: no link names a place on it
    let mut cx = desktop(&net);
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/notes.txt");
    assert_eq!(items(&cx), ["open", "rename", "remove"]);
}

#[test]
fn open_from_the_menu_opens_the_rows_page() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/notes.txt");
    cx.simulate_click("files/menu/open");
    cx.run_until_parked();
    assert!(cx.has_text(NOTES), "{:?}", cx.texts());
    assert_eq!(menu(&cx), None);
}

#[test]
fn a_rename_from_a_rows_menu_moves_that_row() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/notes.txt");
    cx.simulate_click("files/menu/rename");
    cx.run_until_parked();
    assert!(
        cx.has_text(&words::form_title(&crate::write::Write::Rename {
            path: "/home/dev/notes.txt".into()
        })),
        "the row's form, not the folder's: {:?}",
        cx.texts()
    );
    cx.simulate_input("files/write/name", "/home/dev/todo.txt");
    commit_form(&mut cx);
    assert!(
        cx.find("files/entry/todo.txt").is_some(),
        "{:?}",
        cx.texts()
    );
    assert!(cx.find("files/entry/notes.txt").is_none());
    assert!(
        cx.find("files/entry/docs").is_some(),
        "the folder stays on screen"
    );
}

#[test]
fn split_the_row_of_the_file_open_beside_offers_its_edit() {
    let net = seeded();
    let mut cx = desktop_at(&net, 1440.);
    cx.simulate_click("files/side/home");
    cx.run_until_parked();
    right_press(&mut cx, "files/entry/notes.txt");
    assert!(
        !items(&cx).contains(&"edit".to_owned()),
        "a row's text is not read: no edit"
    );
    cx.simulate_dismiss("files-menu-overlay");
    cx.simulate_click("files/entry/notes.txt");
    cx.run_until_parked();
    right_press(&mut cx, "files/entry/notes.txt");
    assert_eq!(items(&cx), ["open", "edit", "rename", "remove"]);
    cx.simulate_click("files/menu/edit");
    cx.run_until_parked();
    assert!(
        cx.has_text(NOTES),
        "the edit starts from the text: {:?}",
        cx.texts()
    );
}

#[test]
fn a_right_press_on_the_room_under_the_rows_offers_what_is_made_in_the_folder() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    right_press(&mut cx, "files-entries");
    assert_eq!(items(&cx), ["new-folder", "new-file", "upload"]);
    net.device(&[("todo.txt", b"buy milk")]);
    cx.simulate_click("files/menu/upload");
    cx.run_until_parked();
    assert!(
        cx.has_text("todo.txt"),
        "the upload's form: {:?}",
        cx.texts()
    );
    assert_eq!(menu(&cx), None);
}

#[test]
fn a_folder_the_reader_cannot_write_opens_no_menu_on_its_room() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    cx.simulate_click("home");
    cx.run_until_parked();
    cx.simulate_click("files/entry/ada");
    cx.run_until_parked();
    right_press(&mut cx, "files-entries");
    assert_eq!(menu(&cx), None, "{:?}", cx.texts());
}

#[test]
fn a_home_in_the_folder_of_homes_opens_no_writes_and_its_room_no_menu() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    cx.simulate_click("home");
    cx.run_until_parked();
    right_press(&mut cx, "files/entry/dev");
    assert_eq!(items(&cx), ["open", "copy-link"], "{:?}", cx.texts());
    cx.simulate_key_down(MENU, "escape");
    right_press(&mut cx, "files-entries");
    assert_eq!(menu(&cx), None, "{:?}", cx.texts());
}

#[test]
fn escape_or_a_press_outside_closes_the_menu_and_the_keys_go_back_to_its_row() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    right_press(&mut cx, "files/entry/notes.txt");
    cx.simulate_key_down(MENU, "escape");
    assert_eq!(menu(&cx), None);
    assert_eq!(focused(&cx), Some("files/entry/notes.txt"));
    right_press(&mut cx, "files/entry/docs");
    cx.simulate_dismiss("files-menu-overlay");
    assert_eq!(menu(&cx), None);
    assert_eq!(focused(&cx), Some("files/entry/docs"));
}

/// The menu the keys open waits for its row to say where it is, then
/// opens under it.
fn opened_under_the_row(cx: &mut TestAppContext) {
    assert_eq!(menu(cx), None, "not before the row says where it is");
    cx.simulate_bounds("files-menu-anchor", (20., 300.), (600., 40.));
    assert_eq!(menu(cx).map(|(at, _)| at), Some((20., 340.)));
    assert_eq!(focused(cx), Some(MENU));
}

#[test]
fn shift_f10_or_the_menu_key_opens_the_focused_rows_menu_under_it() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    cx.simulate_focus("files/entry/notes.txt");
    cx.simulate_key_down("files/entry/notes.txt", "shift-f10");
    opened_under_the_row(&mut cx);
    cx.simulate_key_down(MENU, "escape");
    cx.simulate_key_down("files/entry/notes.txt", "menu");
    opened_under_the_row(&mut cx);
    // ↓ ↓ walks to Rename, Enter runs it
    cx.simulate_key_down(MENU, "down");
    cx.simulate_key_down(MENU, "down");
    cx.simulate_key_down(MENU, "enter");
    cx.run_until_parked();
    assert!(
        cx.has_text(&words::form_title(&crate::write::Write::Rename {
            path: "/home/dev/notes.txt".into()
        })),
        "{:?}",
        cx.texts()
    );
}

#[test]
fn assistive_tech_opens_a_rows_menu_under_it() {
    let net = seeded();
    let mut cx = linked(&net);
    open_home(&mut cx);
    cx.simulate_a11y_action(
        "files/entry/notes.txt",
        view::accesskit::Action::ShowContextMenu,
        None,
    );
    opened_under_the_row(&mut cx);
    assert_eq!(items(&cx), ["open", "copy-link", "rename", "remove"]);
}

#[test]
fn a_link_opened_into_files_opens_what_is_at_its_path() {
    let net = seeded();
    let mut cx = linked(&net);
    let routes = cx.host().stream::<HostRoute>();
    // the tail of the link a row's menu copies
    let link = crate::actions::link(&on_a_chain(&net).chain_id, "/home/dev/notes.txt")
        .expect("a link on a chain");
    let tail = ducklink::Link::parse(&link)
        .expect("a minted link parses")
        .tail;
    routes.send(tail.join("/"));
    cx.run_until_parked();
    assert!(cx.has_text(NOTES), "the file's page: {:?}", cx.texts());
    routes.send("home/dev/docs".into());
    cx.run_until_parked();
    assert!(
        cx.has_text("plan.md"),
        "the folder's page: {:?}",
        cx.texts()
    );
    routes.send("home/dev/gone.txt".into());
    cx.run_until_parked();
    assert!(cx.has_text(words::GONE), "{:?}", cx.texts());
}
