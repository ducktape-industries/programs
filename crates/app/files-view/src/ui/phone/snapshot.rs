//! A snapshot's screen: who made it, when, at which block, its id, then
//! what it changed against its parent, path by path. New design in the
//! kit's look; nearest Expo screen
//! `mobile/app/(tabs)/forge/[repo]/code/[...path].tsx`. Reads the snapshot
//! on screen; writes Back.
use files::{BlobId, DiffEntry, SnapshotRow};
use view::FontWeight;
use view::design::{date, grouped, space};

use super::*;

pub(super) fn snapshot(view: &Files, id: BlobId, cx: Cx, k: &Kit) -> AnyElement {
    let shown = view.snapshot_at(id);
    let title = match shown.and_then(|snapshot| snapshot.row.as_ref()) {
        Some(row) => words::snapshot(row.seq),
        None => words::SNAPSHOT.to_owned(),
    };
    let body = match shown {
        Some(snapshot) => match &snapshot.row {
            Some(row) => found(view, row, &snapshot.changes, k),
            None => quiet("files-no-snapshot", words::NO_SUCH_SNAPSHOT, k),
        },
        None => match view.snapshot.failed() {
            Some(refusal) => refused("files-snapshot", refusal, cx, k),
            None => quiet("files-snapshot-reading", words::READING_SNAPSHOT, k),
        },
    };
    page("files-snapshot", title, view, cx, k)
        .child(scroller("files-page").child(body))
        .into_any_element()
}

fn found(view: &Files, row: &SnapshotRow, changes: &[DiffEntry], k: &Kit) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(
            div()
                .id("files-snapshot-message")
                .p(space::BLOCK)
                .text_type(k.ladder.heading)
                .font_weight(FontWeight::MEDIUM)
                .child(words::message_or_none(&row.message).to_owned()),
        )
        .child(key_value(
            words::AUTHOR,
            words::author(&row.author, view.names.ready()),
            k,
        ))
        .child(key_value(words::TIME, date(row.time), k))
        .child(key_value(words::BLOCK, data(grouped(row.height), k), k))
        .child(key_value(
            words::SNAPSHOT,
            data(abi::hex(row.id.digest()), k),
            k,
        ))
        .child(design::section_head(
            "files/changes-head",
            words::CHANGES,
            &k.ladder,
            &k.theme,
        ))
        .child(changed(row, changes, k))
        .into_any_element()
}

fn changed(row: &SnapshotRow, changes: &[DiffEntry], k: &Kit) -> AnyElement {
    if let Some(note) = words::changes_note(row, changes) {
        return quiet("files-changes-note", note, k);
    }
    div()
        .id("files-changes")
        .flex()
        .flex_col()
        .children(changes.iter().map(|entry| {
            design::row(
                ElementId::Name(format!("files/change:{}", entry.path).into()),
                &k.ladder,
                &k.theme,
            )
            .min_h(k.ladder.row_tall)
            .child(
                design::row_words(data(entry.path.clone(), k).truncate()).child(
                    design::row_caption(words::diff_kind(entry.kind), &k.ladder, &k.theme),
                ),
            )
        }))
        .into_any_element()
}
