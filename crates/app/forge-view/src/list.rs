//! A list as one Tab stop, for either design: the arrows walk its rows
//! (the view's `list_cursor`), Enter presses the active one, and a long
//! list is virtual, painted a window at a time.
use std::ops::Range;

use view::prelude::*;
use view::{Div, Stateful};

use crate::{Forge, id};

pub(crate) fn cursor(forge: &Forge, id: &'static str) -> (usize, usize) {
    match forge.list_cursor {
        Some((list, row, cell)) if list == id => (row, cell),
        _ => (0, 0),
    }
}

pub(crate) fn walked(forge: &Forge, id: &'static str) -> bool {
    matches!(forge.list_cursor, Some((list, ..)) if list == id)
}

pub(crate) fn list(
    id_: &'static str,
    label: &str,
    count: usize,
    scroll: Option<&UniformListScrollHandle>,
    forge: &Forge,
    cx: &mut Context<Forge>,
    on_press: impl Fn(&mut Forge, usize, &mut Window, &mut Context<Forge>) + 'static,
) -> Stateful<Div> {
    let (at, _) = cursor(forge, id_);
    let mut list = design::composite(id(id_), Role::ListBox, label.to_owned())
        .active(at.min(count.saturating_sub(1)), count);
    if let Some(scroll) = scroll {
        list = list.track_scroll(scroll);
    }
    let list = list
        .on_move(cx.processor(move |forge, index: usize, _, cx| {
            forge.list_cursor = Some((id_, index, 0));
            cx.notify();
        }))
        .on_press(
            cx.processor(move |forge, index: usize, window, cx| on_press(forge, index, window, cx)),
        )
        .build()
        .flex_1()
        .min_h(px(0.))
        .flex()
        .flex_col();
    match scroll {
        Some(_) => list,
        None => list.overflow_y_scroll(),
    }
}

pub(crate) fn window(
    element_id: &str,
    count: usize,
    widest: Option<usize>,
    scroll: Option<&UniformListScrollHandle>,
    paint: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
) -> AnyElement {
    let mut list =
        uniform_list(id(element_id.to_owned()), count, paint).with_width_from_item(widest);
    if let Some(scroll) = scroll {
        list = list.track_scroll(scroll);
    }
    list.flex_1().min_h(px(0.)).into_any_element()
}
