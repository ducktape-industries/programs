//! A room's tag filters over its timeline: "All" and a chip per tag its `filters` meta names, the
//! picked one filled, `#tag` under a pointer and the bare tag under a finger, where chips share
//! the row. Nothing is drawn for a room without the meta.
use view::prelude::*;

use crate::Chat;
use crate::words;
use view::design::Ladder;

pub(crate) fn chips(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let tags = chat.room_filters();
    if tags.is_empty() {
        return None;
    }
    let picked = chat
        .filtered()
        .and_then(|filter| tags.iter().position(|tag| *tag == filter.tag))
        .map_or(0, |at| at + 1);
    let all = (
        ElementId::from("chat-filter-all"),
        SharedString::from(words::ALL),
    );
    let each = tags.iter().map(|tag| {
        let id = ElementId::from(format!("chat-filter-{tag}"));
        let label = match ladder.touch {
            true => tag.clone(),
            false => format!("#{tag}"),
        };
        (id, SharedString::from(label))
    });
    let choices: Vec<(ElementId, SharedString)> = std::iter::once(all).chain(each).collect();
    let pick = cx.processor(move |chat, index: usize, _window, cx| {
        let tag = index.checked_sub(1).and_then(|at| tags.get(at)).cloned();
        chat.pick_filter(tag, cx);
    });
    let chips = design::segmented(
        "chat-room-filters",
        words::ROOM_FILTER,
        ladder,
        theme,
        Some(picked),
        choices,
        pick,
    );
    let row = div()
        .id("chat-room-filter-row")
        .flex()
        .flex_col()
        .when(!ladder.touch, |row| row.items_start())
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .child(chips);
    Some(row.into_any_element())
}
