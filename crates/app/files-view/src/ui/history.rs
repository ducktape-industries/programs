//! The history: every snapshot newest first, each opening its page, then
//! the pins that name some of them; and one snapshot's page, who made it
//! and when, and what it changed against its parent. Split, the history
//! is listed beside the snapshot open, its rows marked. Reads the
//! history, the authors' names and the snapshot on screen; writes the
//! route.
use files::{BlobId, PinRow, SnapshotRow};
use view::design::{date, grouped};
use view::{Div, Stateful};

use super::*;
use crate::state::{History, PIN_GLYPH, SNAPSHOT_GLYPH, Snapshot};

pub(super) fn history(view: &Files, marked: Option<BlobId>, cx: Cx, k: &Kit) -> AnyElement {
    let history = match &view.history {
        Loadable::Ready(history) | Loadable::Reloading(history, _) => history,
        Loadable::Failed(refusal) => return refused("files-history", refusal, cx, k),
        Loadable::Idle | Loadable::Loading(_) => {
            return reading("files-history-reading", words::READING_HISTORY, k);
        }
    };
    div()
        .id("files-history")
        .flex()
        .flex_col()
        .child(snapshots(view, history, marked, cx, k))
        .child(heading(
            "files/pins-head",
            words::PINS,
            Some(grouped(history.pins.len() as u64)),
            k,
        ))
        .child(pins(view, history, marked, cx, k))
        .into_any_element()
}

fn snapshots(
    view: &Files,
    history: &History,
    marked: Option<BlobId>,
    cx: Cx,
    k: &Kit,
) -> AnyElement {
    if history.snapshots.is_empty() {
        return design::empty_state(
            "files-no-snapshots",
            "history",
            words::NO_SNAPSHOTS,
            words::NO_SNAPSHOTS_DETAIL,
            &k.ladder,
            &k.theme,
        )
        .into_any_element();
    }
    let older = history.older.then(|| {
        design::quiet(words::OLDER, &k.theme)
            .id("files-older")
            .px_5()
            .py_3()
    });
    div()
        .flex()
        .flex_col()
        .child(heading("files/snapshots-head", words::HISTORY, None, k))
        .children(history.snapshots.iter().map(|snapshot| {
            let row = snapshot_row(view, snapshot, cx, k);
            mark(row, marked == Some(snapshot.id), k)
        }))
        .children(older)
        .into_any_element()
}

pub(super) fn snapshot_row(view: &Files, snapshot: &SnapshotRow, cx: Cx, k: &Kit) -> Stateful<Div> {
    let id = snapshot.id;
    let open = cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
        view.open(Route::Snapshot(id), cx)
    });
    let author = words::author(&snapshot.author, view.names.ready());
    let aside = words::byline(author, snapshot.time);
    row(
        format!("files/snapshot/{}", snapshot.seq),
        SNAPSHOT_GLYPH,
        words::message_or_none(&snapshot.message).to_owned(),
        aside,
        k,
        open,
    )
}

fn pins(view: &Files, history: &History, marked: Option<BlobId>, cx: Cx, k: &Kit) -> AnyElement {
    if history.pins.is_empty() {
        return design::quiet(words::NO_PINS, &k.theme)
            .id("files-no-pins")
            .px_5()
            .py_3()
            .into_any_element();
    }
    let rows = history.pins.iter().map(|pin| {
        mark(
            pin_row(view, history, pin, cx, k),
            marked == Some(pin.snapshot),
            k,
        )
    });
    div().flex().flex_col().children(rows).into_any_element()
}

fn pin_row(view: &Files, history: &History, pin: &PinRow, cx: Cx, k: &Kit) -> Stateful<Div> {
    let seq = history.named(pin).map(|named| named.seq);
    let aside = words::pin_caption(seq, words::author(&pin.owner, view.names.ready()));
    let snapshot = pin.snapshot;
    let open = cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
        view.open(Route::Snapshot(snapshot), cx)
    });
    row(
        format!("files/pin/{}", pin.name),
        PIN_GLYPH,
        pin.name.clone(),
        aside,
        k,
        open,
    )
}

pub(super) fn snapshot(view: &Files, id: BlobId, cx: Cx, k: &Kit) -> AnyElement {
    let page = div().id("files-snapshot").flex().flex_col();
    let Some(snapshot) = view.snapshot_at(id) else {
        let waiting = match view.snapshot.failed() {
            Some(refusal) => refused("files-snapshot", refusal, cx, k),
            None => reading("files-snapshot-reading", words::READING_SNAPSHOT, k),
        };
        return page.child(waiting).into_any_element();
    };
    let Some(row) = &snapshot.row else {
        return page
            .child(
                design::quiet(words::NO_SUCH_SNAPSHOT, &k.theme)
                    .id("files-no-snapshot")
                    .px_5()
                    .py_4(),
            )
            .into_any_element();
    };
    page.child(title(
        "files-snapshot-title",
        words::message_or_none(&row.message),
        Some(words::snapshot(row.seq)),
        k,
    ))
    .child(line(
        words::AUTHOR,
        words::author(&row.author, view.names.ready()),
        k,
    ))
    .child(line(words::TIME, date(row.time), k))
    .child(line(words::BLOCK, data(grouped(row.height)), k))
    .child(line(words::SNAPSHOT, data(abi::hex(row.id.digest())), k))
    .child(heading("files/changes-head", words::CHANGES, None, k))
    .child(changes(row, snapshot, k))
    .into_any_element()
}

fn changes(row: &SnapshotRow, snapshot: &Snapshot, k: &Kit) -> AnyElement {
    if let Some(note) = words::changes_note(row, &snapshot.changes) {
        return design::quiet(note, &k.theme)
            .id("files-changes-note")
            .px_5()
            .py_3()
            .into_any_element();
    }
    let lines = snapshot.changes.iter().map(|entry| {
        line(words::diff_kind(entry.kind), data(entry.path.clone()), k).id(ElementId::Name(
            format!("files/change:{}", entry.path).into(),
        ))
    });
    div()
        .id("files-changes")
        .flex()
        .flex_col()
        .children(lines)
        .into_any_element()
}
