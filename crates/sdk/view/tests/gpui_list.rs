use std::ops::Range;
use view::{
    AnyElement, App, FollowMode, IntoElement, List, ListAlignment, ListOffset, ListScrollEvent,
    ListSizingBehavior, ListState, Pixels, Styled, Window, div, list, px,
};

fn renderer(_: usize, _: &mut Window, _: &mut App) -> AnyElement {
    div().into_any_element()
}

#[test]
fn variable_list_api_keeps_pinned_gpui_names_and_signatures() {
    let state = ListState::new(8, ListAlignment::Bottom, px(160.));
    let _: List = list("rows", state.clone(), renderer)
        .with_sizing_behavior(ListSizingBehavior::Auto)
        .w_full();
    state.splice(0..0, 2);
    state.reset(10);
    state.remeasure();
    state.remeasure_items(Range { start: 2, end: 4 });
    assert_eq!(state.item_count(), 10);
    let offset: ListOffset = state.logical_scroll_top();
    state.scroll_to(offset);
    state.scroll_to_end();
    state.scroll_to_reveal_item(7);
    state.set_follow_mode(FollowMode::Tail);
    state.pause_following_tail();
    let _: bool = state.is_following_tail();
    state.set_scroll_handler(|_event: &ListScrollEvent, _window: &mut Window, _app: &mut App| {});
    let _: Pixels = px(1.);
}
