//! The history screen: every snapshot, newest first, each opening its
//! screen, then the pins that name some of them. New design in the kit's
//! look; nearest Expo screen
//! `mobile/app/(tabs)/forge/[repo]/code/[...path].tsx` (its rows). Reads
//! the history and the authors' names; writes the route.
use files::{PinRow, SnapshotRow};
use view::design::grouped;
use view::{Div, Stateful};

use super::*;
use crate::state::{History, PIN_GLYPH, SNAPSHOT_GLYPH};

pub(super) fn history(view: &Files, cx: Cx, k: &Kit) -> AnyElement {
    let body = match &*view.history {
        Loadable::Ready(history) | Loadable::Reloading(history, _) => listed(view, history, cx, k),
        Loadable::Failed(refusal) => refused("files-history", refusal, cx, k),
        Loadable::Idle | Loadable::Loading(_) => {
            quiet("files-history-reading", words::READING_HISTORY, k)
        }
    };
    page("files-history", words::HISTORY, view, cx, k)
        .child(scroller("files-page").child(body))
        .into_any_element()
}

fn listed(view: &Files, history: &History, cx: Cx, k: &Kit) -> AnyElement {
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
    let older = history.older.then(|| quiet("files-older", words::OLDER, k));
    let pins = match history.pins.is_empty() {
        true => quiet("files-no-pins", words::NO_PINS, k),
        false => div()
            .flex()
            .flex_col()
            .children(
                history
                    .pins
                    .iter()
                    .map(|pin| pin_row(view, history, pin, cx, k)),
            )
            .into_any_element(),
    };
    div()
        .flex()
        .flex_col()
        .children(
            history
                .snapshots
                .iter()
                .map(|snapshot| snapshot_row(view, snapshot, cx, k)),
        )
        .children(older)
        .child(
            design::section_head("files/pins-head", words::PINS, &k.ladder, &k.theme)
                .child(grouped(history.pins.len() as u64)),
        )
        .child(pins)
        .into_any_element()
}

fn snapshot_row(view: &Files, snapshot: &SnapshotRow, cx: Cx, k: &Kit) -> Stateful<Div> {
    let id = snapshot.id;
    let open = cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
        view.open(Route::Snapshot(id), cx)
    });
    let author = words::author(&snapshot.author, view.names.ready());
    let caption = words::byline(author, snapshot.time);
    opening_row(
        ElementId::Name(format!("files/snapshot/{}", snapshot.seq).into()),
        SNAPSHOT_GLYPH,
        words::message_or_none(&snapshot.message).to_owned(),
        caption,
        k,
        open,
    )
}

fn pin_row(view: &Files, history: &History, pin: &PinRow, cx: Cx, k: &Kit) -> Stateful<Div> {
    let seq = history.named(pin).map(|named| named.seq);
    let caption = words::pin_caption(seq, words::author(&pin.owner, view.names.ready()));
    let snapshot = pin.snapshot;
    let open = cx.listener(move |view: &mut Files, _: &ClickEvent, _, cx| {
        view.open(Route::Snapshot(snapshot), cx)
    });
    opening_row(
        ElementId::Name(format!("files/pin/{}", pin.name).into()),
        PIN_GLYPH,
        pin.name.clone(),
        caption,
        k,
        open,
    )
}
