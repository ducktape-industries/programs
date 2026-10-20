use crate::{AnyElement, App, Element, IntoElement, Lowering, Window, wire};
use gpui::{ElementId, Pixels, StyleRefinement, Styled, px};
use std::{cell::RefCell, ops::Range, rc::Rc};

use gpui::{FollowMode, ListAlignment, ListOffset, ListScrollEvent, ListSizingBehavior};
type ScrollHandler = dyn FnMut(&ListScrollEvent, &mut Window, &mut App) + 'static;
type ItemRenderer = Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement>;

#[derive(Clone)]
pub struct ListState(Rc<State>);
struct State {
    inner: RefCell<Inner>,
    scroll_handler: RefCell<Option<Box<ScrollHandler>>>,
}
struct Inner {
    item_count: usize,
    alignment: ListAlignment,
    overdraw: Pixels,
    requested: Range<usize>,
    logical_scroll_top: ListOffset,
    tail_mode: bool,
    following_tail: bool,
    revision: u64,
    commands: Vec<wire::ListCommand>,
}

impl std::fmt::Debug for ListState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ListState")
    }
}
impl Default for ListState {
    fn default() -> Self {
        Self::new(0, ListAlignment::Top, px(0.))
    }
}

impl ListState {
    pub fn new(item_count: usize, alignment: ListAlignment, overdraw: Pixels) -> Self {
        Self(Rc::new(State {
            inner: RefCell::new(Inner {
                item_count,
                alignment,
                overdraw,
                requested: initial_range(item_count, alignment),
                logical_scroll_top: ListOffset {
                    item_ix: if alignment == ListAlignment::Bottom {
                        item_count
                    } else {
                        0
                    },
                    offset_in_item: px(0.),
                },
                tail_mode: false,
                following_tail: false,
                revision: 0,
                commands: Vec::new(),
            }),
            scroll_handler: RefCell::new(None),
        }))
    }
    pub fn reset(&self, count: usize) {
        let mut inner = self.0.inner.borrow_mut();
        inner.item_count = count;
        inner.requested = initial_range(count, inner.alignment);
        inner.logical_scroll_top = ListOffset {
            item_ix: if inner.alignment == ListAlignment::Bottom {
                count
            } else {
                0
            },
            offset_in_item: px(0.),
        };
        inner.push(wire::ListCommand::Reset { count });
    }
    pub fn remeasure(&self) {
        self.remeasure_items(0..self.item_count());
    }
    pub fn remeasure_items(&self, range: Range<usize>) {
        let mut inner = self.0.inner.borrow_mut();
        let range = bounded_range(range, inner.item_count);
        if !range.is_empty() {
            inner.push(wire::ListCommand::Remeasure {
                start: range.start,
                end: range.end,
            });
        }
    }
    pub fn item_count(&self) -> usize {
        self.0.inner.borrow().item_count
    }
    /// Rows `old_range` become `count` rows. The window the host shows
    /// follows the edit: one that ends at or before its first row shifts
    /// the window by the rows added or removed, one inside it or at its
    /// end grows the window over the new rows, and one after it leaves the
    /// window alone. So the rows on screen stay on screen through an
    /// arrival at the tail or a page of history above, a reader at the
    /// first loaded row included. An edit that takes every row the window
    /// held, or finds it empty, leaves nothing to follow: the window is
    /// the new list's first screenful, as [`reset`](Self::reset) makes it.
    pub fn splice(&self, old_range: Range<usize>, count: usize) {
        let mut inner = self.0.inner.borrow_mut();
        let old_range = bounded_range(old_range, inner.item_count);
        inner.item_count = inner
            .item_count
            .saturating_sub(old_range.len())
            .saturating_add(count);
        let held = inner.requested.clone();
        let delta = count as isize - old_range.len() as isize;
        let shifted = |at: usize| at.saturating_add_signed(delta);
        let edited = if !held.is_empty() && old_range.end <= held.start {
            shifted(held.start)..shifted(held.end)
        } else if old_range.start > held.end {
            held
        } else if old_range.start <= held.start && held.end <= old_range.end {
            initial_range(inner.item_count, inner.alignment)
        } else {
            held.start.min(old_range.start)..shifted(held.end.max(old_range.end))
        };
        inner.requested = bounded_range(edited, inner.item_count);
        inner.push(wire::ListCommand::Splice {
            start: old_range.start,
            end: old_range.end,
            count,
        });
    }
    pub fn set_scroll_handler(
        &self,
        handler: impl FnMut(&ListScrollEvent, &mut Window, &mut App) + 'static,
    ) {
        *self.0.scroll_handler.borrow_mut() = Some(Box::new(handler));
    }
    pub fn logical_scroll_top(&self) -> ListOffset {
        self.0.inner.borrow().logical_scroll_top
    }
    pub fn scroll_to(&self, scroll_top: ListOffset) {
        let mut inner = self.0.inner.borrow_mut();
        let offset = ListOffset {
            item_ix: scroll_top.item_ix.min(inner.item_count),
            offset_in_item: px(f32::from(scroll_top.offset_in_item).max(0.)),
        };
        let start = offset
            .item_ix
            .min(inner.item_count.saturating_sub(INITIAL_ROWS));
        inner.hold(start..offset.item_ix.saturating_add(INITIAL_ROWS));
        inner.logical_scroll_top = offset;
        inner.push(wire::ListCommand::ScrollTo(to_wire_offset(offset)));
    }
    pub fn scroll_to_end(&self) {
        let mut inner = self.0.inner.borrow_mut();
        inner.logical_scroll_top = ListOffset {
            item_ix: inner.item_count,
            offset_in_item: px(0.),
        };
        inner.push(wire::ListCommand::ScrollToEnd);
    }
    pub fn scroll_to_reveal_item(&self, ix: usize) {
        let mut inner = self.0.inner.borrow_mut();
        let ix = ix.min(inner.item_count.saturating_sub(1));
        let held = inner.requested.clone();
        let end = held.end.max(ix.saturating_add(1)).min(inner.item_count);
        let start = held.start.min(ix);
        inner.requested = start..end;
        inner.push(wire::ListCommand::ScrollToRevealItem(ix));
    }
    pub fn set_follow_mode(&self, mode: FollowMode) {
        let tail = mode == FollowMode::Tail;
        let mut inner = self.0.inner.borrow_mut();
        if inner.tail_mode != tail {
            inner.tail_mode = tail;
            inner.following_tail = tail;
            inner.push(wire::ListCommand::SetFollowMode { tail });
        }
    }
    pub fn pause_following_tail(&self) {
        let mut inner = self.0.inner.borrow_mut();
        if inner.following_tail {
            inner.following_tail = false;
            inner.push(wire::ListCommand::PauseFollowingTail);
        }
    }
    pub fn is_following_tail(&self) -> bool {
        self.0.inner.borrow().following_tail
    }
    pub(crate) fn is(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    fn request(&self, request: &wire::ListRequest) -> bool {
        let mut inner = self.0.inner.borrow_mut();
        let held = inner.requested.clone();
        inner.hold(request.start..request.end);
        inner.requested != held
    }
    fn observe(&self, event: &wire::ListScroll, window: &mut Window, app: &mut App) {
        {
            let mut inner = self.0.inner.borrow_mut();
            inner.logical_scroll_top = from_wire_offset(event.offset);
            inner.following_tail = event.is_following_tail;
            let margin = margin_rows(inner.overdraw);
            let keep = event.visible_start.saturating_sub(margin)
                ..event.visible_end.saturating_add(margin);
            let held = inner.requested.clone();
            let trimmed = held.start.max(keep.start)..held.end.min(keep.end);
            if !trimmed.is_empty() && event.visible_start < event.visible_end {
                inner.requested = trimmed;
            }
        }
        let mut handler = self.0.scroll_handler.borrow_mut().take();
        if let Some(callback) = handler.as_mut() {
            callback(
                &ListScrollEvent {
                    visible_range: event.visible_start..event.visible_end,
                    count: event.count,
                    is_scrolled: event.is_scrolled,
                    is_following_tail: event.is_following_tail,
                },
                window,
                app,
            );
        }
        *self.0.scroll_handler.borrow_mut() = handler;
    }
}
impl Inner {
    fn hold(&mut self, asked: Range<usize>) {
        let count = self.item_count;
        let asked = bounded_range(asked, count);
        let held = self.requested.clone();
        let touches = asked.start <= held.end && held.start <= asked.end;
        let (start, end) = (held.start.min(asked.start), held.end.max(asked.end));
        self.requested = match touches {
            true => start..end,
            false => asked,
        };
    }
    fn push(&mut self, command: wire::ListCommand) {
        self.revision = self.revision.wrapping_add(1);
        self.commands.push(command);
    }
}

/// gpui's list, but for the id it is born with ([`list`]): an element with
/// a style and nothing to press or focus, so `.id()`, `.hover()` and
/// `.on_click()` are no methods of it. What a list's rows share goes on a
/// box around it.
///
/// ```
/// # use view::prelude::*;
/// let rows = list("rows", ListState::default(), |_, _, _| div().into_any_element()).flex_1();
/// ```
///
/// ```compile_fail,E0599
/// # use view::prelude::*;
/// let rows = list("rows", ListState::default(), |_, _, _| div().into_any_element()).id("other");
/// ```
///
/// ```compile_fail,E0599
/// # use view::prelude::*;
/// let rows = list("rows", ListState::default(), |_, _, _| div().into_any_element()).hover(|s| s);
/// ```
pub struct List {
    id: ElementId,
    state: ListState,
    render_item: ItemRenderer,
    style: Box<StyleRefinement>,
    sizing_behavior: ListSizingBehavior,
}
/// A variable-height list of `state`'s items, each row built by
/// `render_item`.
///
/// It takes an `id` where gpui's `list(state, ..)` takes none, as
/// [`uniform_list`](crate::uniform_list) does: the host files a list's
/// state (its scroll, its measured rows) and everything in its rows (a
/// field's text and selection, focus) by path, so a list is one segment of
/// that path, and its author names it. A new instance of the view then
/// files the same paths, and `Window::focus_path` names a row's field
/// through its list (`["thread", "rows", key, "edit"]`). Two lists under
/// one parent are told apart by their ids; one id on two of them is
/// refused, as any id written twice in a scope is.
///
/// The list and each of its rows are scopes of their own: a row is filed
/// under the id of its root element, else under its index, and the ids
/// inside a row never meet another row's or another list's. Give a row
/// that can move (history landing above it) its item's key as its id, and
/// its listeners stay its own wherever it goes. Two rows named by one key
/// are refused, naming the id and its scope.
///
/// A `state` is one list's: it holds that list's window of rows and the
/// commands waiting for it, so two lists drawn with one state in a frame
/// are refused, naming both. Two lists of the same items take two states.
pub fn list(
    id: impl Into<ElementId>,
    state: ListState,
    render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> List {
    List {
        id: id.into(),
        state,
        render_item: Box::new(render_item),
        style: Box::default(),
        sizing_behavior: ListSizingBehavior::default(),
    }
}
impl List {
    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing_behavior = behavior;
        self
    }
}
impl Styled for List {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
impl Element for List {
    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let Self {
            id: _,
            state,
            mut render_item,
            style,
            sizing_behavior,
        } = *self;
        lowering.draws_list(&state);
        let request_state = state.clone();
        let request_handler = lowering.route(
            crate::slots::Kind::ListRequest,
            move |request: &wire::ListRequest, _, app| {
                if request_state.request(request) {
                    app.notify();
                }
            },
        );
        let scroll_handler = state.0.scroll_handler.borrow().is_some().then(|| {
            let scroll_state = state.clone();
            lowering.route(
                crate::slots::Kind::ListScroll,
                move |event: &wire::ListScroll, window, app| {
                    scroll_state.observe(event, window, app)
                },
            )
        });
        let (item_count, alignment, overdraw, following_tail, revision, commands, range) = {
            let mut inner = state.0.inner.borrow_mut();
            (
                inner.item_count,
                inner.alignment,
                inner.overdraw,
                inner.tail_mode,
                inner.revision,
                std::mem::take(&mut inner.commands),
                inner.requested.clone(),
            )
        };
        let mut children = Vec::with_capacity(range.len());
        for index in range.clone() {
            let (window, app) = lowering.parts();
            let row = render_item(index, window, app);
            children.push(lowering.lower_row(index, row));
        }
        let path = lowering.current_path().to_vec();
        let id = path
            .last()
            .cloned()
            .expect("a list lowers inside its authored scope");
        wire::Node::List {
            id,
            path,
            item_count,
            alignment: wire_alignment(alignment),
            overdraw: f32::from(overdraw),
            sizing: wire_sizing(sizing_behavior),
            following_tail,
            revision,
            commands,
            request_handler,
            scroll_handler,
            range_start: range.start,
            style: lowering.style(&style),
            interactivity: Default::default(),
            children,
        }
    }
}
impl IntoElement for List {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl gpui::prelude::FluentBuilder for List {}

const INITIAL_ROWS: usize = 24;
pub(crate) const MARGIN_ROWS: usize = 12;
const SHORTEST_ROW: f32 = 24.;

fn margin_rows(overdraw: Pixels) -> usize {
    let covered = (f32::from(overdraw) / SHORTEST_ROW).ceil() as usize;
    MARGIN_ROWS.max(covered)
}

fn initial_range(count: usize, alignment: ListAlignment) -> Range<usize> {
    match alignment {
        ListAlignment::Top => 0..count.min(INITIAL_ROWS),
        ListAlignment::Bottom => count.saturating_sub(INITIAL_ROWS)..count,
    }
}
fn bounded_range(range: Range<usize>, count: usize) -> Range<usize> {
    let start = range.start.min(count);
    start..range.end.clamp(start, count)
}
fn wire_alignment(v: ListAlignment) -> wire::ListAlignment {
    match v {
        ListAlignment::Top => wire::ListAlignment::Top,
        ListAlignment::Bottom => wire::ListAlignment::Bottom,
    }
}
fn wire_sizing(v: ListSizingBehavior) -> wire::ListSizingBehavior {
    match v {
        ListSizingBehavior::Infer => wire::ListSizingBehavior::Infer,
        ListSizingBehavior::Auto => wire::ListSizingBehavior::Auto,
    }
}
fn to_wire_offset(v: ListOffset) -> wire::ListOffset {
    wire::ListOffset {
        item_ix: v.item_ix,
        offset_in_item: f32::from(v.offset_in_item),
    }
}
fn from_wire_offset(v: wire::ListOffset) -> ListOffset {
    ListOffset {
        item_ix: v.item_ix,
        offset_in_item: px(v.offset_in_item.max(0.)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestAppContext;
    use crate::{
        Context, Entity, InteractiveElement, ParentElement, Render, Role,
        StatefulInteractiveElement, View, div,
    };
    use serde::{Deserialize, Serialize};

    #[derive(Default, Serialize, Deserialize)]
    struct ListView {
        #[serde(skip)]
        state: ListState,
        #[serde(skip)]
        rendered: Vec<usize>,
        scrolls: usize,
    }

    impl View for ListView {
        const NAME: &'static str = "ListView";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                state: ListState::new(2_000, ListAlignment::Bottom, px(160.)),
                rendered: Vec::new(),
                scrolls: 0,
            }
        }
    }

    impl Render for ListView {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            self.state.set_scroll_handler(cx.listener(|view, _, _, cx| {
                view.scrolls += 1;
                cx.notify();
            }));
            list(
                "rows",
                self.state.clone(),
                cx.processor(|view, index, _, _| {
                    view.rendered.push(index);
                    div()
                        .id(format!("row-{index}"))
                        .role(Role::Button)
                        .focusable()
                        .on_click(|_, _, _| {})
                        .child(index.to_string())
                        .into_any_element()
                }),
            )
        }
    }

    fn opened() -> (TestAppContext, Entity<ListView>) {
        let mut cx = TestAppContext::new();
        let view = cx.open::<ListView>();
        (cx, view)
    }

    #[test]
    fn a_row_scrolled_in_leaves_the_other_rows_routes_alone() {
        let (mut cx, _) = opened();
        let wire::Node::List {
            request_handler,
            range_start,
            ..
        } = *cx.root()
        else {
            panic!("expected list")
        };
        let (handler, start) = (request_handler, range_start);
        cx.tick(vec![wire::Event::ListRequest {
            handler,
            request: wire::ListRequest {
                start: start - 1,
                end: start,
            },
        }]);
        let frame = cx.last_frame();
        let props = frame
            .patches
            .iter()
            .filter(|patch| matches!(patch, wire::Patch::Props { path, .. } if !path.is_empty()))
            .count();
        let inserts = frame
            .patches
            .iter()
            .filter(|patch| matches!(patch, wire::Patch::Insert { .. }))
            .count();
        assert_eq!((props, inserts), (0, 1), "{:?}", frame.patches);
    }

    #[test]
    fn a_request_inside_the_window_renders_nothing() {
        let (mut cx, _) = opened();
        let wire::Node::List {
            request_handler,
            range_start,
            ..
        } = *cx.root()
        else {
            panic!("expected list")
        };
        let (handler, start) = (request_handler, range_start);
        let ask = |start, end| wire::Event::ListRequest {
            handler,
            request: wire::ListRequest { start, end },
        };
        let renders = cx.renders();
        cx.tick(vec![ask(start, start + 2)]);
        assert!(cx.last_frame().unchanged);
        assert_eq!(cx.renders(), renders, "rows it holds render nothing");
        cx.tick(vec![ask(start - 1, start)]);
        assert_eq!(cx.renders(), renders + 1, "a row it lacks renders once");
    }

    #[test]
    fn two_thousand_items_only_evaluate_a_bounded_requested_window() {
        let (mut cx, view) = opened();
        let (handler, scroll_handler) = match cx.root() {
            wire::Node::List {
                item_count,
                range_start,
                children,
                request_handler,
                scroll_handler,
                ..
            } => {
                assert_eq!(*item_count, 2_000);
                assert_eq!(*range_start, 2_000 - INITIAL_ROWS);
                assert_eq!(children.len(), INITIAL_ROWS);
                (
                    *request_handler,
                    scroll_handler.expect("settled scroll route"),
                )
            }
            other => panic!("expected list, got {other:?}"),
        };
        view.read(|view| assert_eq!(view.rendered.len(), INITIAL_ROWS));

        cx.tick(vec![wire::Event::ListRequest {
            handler,
            request: wire::ListRequest {
                start: 0,
                end: usize::MAX,
            },
        }]);
        match cx.root() {
            wire::Node::List {
                range_start,
                children,
                ..
            } => {
                assert_eq!(*range_start, 0);
                assert_eq!(children.len(), 2_000);
            }
            other => panic!("expected list, got {other:?}"),
        }
        view.read(|view| assert_eq!(view.rendered.len(), INITIAL_ROWS + 2_000));

        cx.tick(vec![wire::Event::ListScroll {
            handler: scroll_handler,
            event: wire::ListScroll {
                visible_start: 1_990,
                visible_end: 2_000,
                count: 2_000,
                is_scrolled: true,
                is_following_tail: false,
                offset: wire::ListOffset {
                    item_ix: 1_990,
                    offset_in_item: 7.,
                },
            },
        }]);
        view.read(|view| {
            assert_eq!(view.scrolls, 1);
            assert_eq!(view.state.logical_scroll_top().item_ix, 1_990);
            assert!(!view.state.is_following_tail());
        });
    }

    #[test]
    fn revealing_a_row_grows_the_window_and_keeps_the_rows_on_screen() {
        let (mut cx, view) = opened();
        let state = view.read(|view| view.state.clone());
        let window = |cx: &TestAppContext| match cx.root() {
            wire::Node::List {
                range_start,
                children,
                ..
            } => *range_start..*range_start + children.len(),
            other => panic!("expected list, got {other:?}"),
        };
        assert_eq!(window(&cx), 2_000 - INITIAL_ROWS..2_000);
        state.scroll_to_reveal_item(2_000 - INITIAL_ROWS - 1);
        cx.app_mut().notify();
        cx.tick(vec![]);
        assert_eq!(window(&cx), 2_000 - INITIAL_ROWS - 1..2_000);
        state.scroll_to_reveal_item(100);
        cx.app_mut().notify();
        cx.tick(vec![]);
        assert_eq!(window(&cx), 100..2_000);
    }

    #[test]
    fn a_scroll_to_a_row_brings_its_rows_in_the_frame_that_carries_it() {
        let (mut cx, view) = opened();
        let state = view.read(|view| view.state.clone());
        let window = |cx: &TestAppContext| match cx.root() {
            wire::Node::List {
                range_start,
                children,
                ..
            } => *range_start..*range_start + children.len(),
            other => panic!("expected list, got {other:?}"),
        };
        let to = |cx: &mut TestAppContext, item_ix: usize| {
            state.scroll_to(ListOffset {
                item_ix,
                offset_in_item: px(0.),
            });
            cx.app_mut().notify();
            cx.tick(vec![]);
            window(cx)
        };
        assert_eq!(to(&mut cx, 100), 100..100 + INITIAL_ROWS);
        assert_eq!(to(&mut cx, 110), 100..110 + INITIAL_ROWS);
        assert_eq!(to(&mut cx, 1_999), 2_000 - INITIAL_ROWS..2_000);
    }

    #[test]
    fn a_scroll_keeps_every_row_the_host_lays_out_past_the_screen() {
        let (mut cx, view) = opened();
        cx.update(&view, |view, _, cx| {
            view.state = ListState::new(2_000, ListAlignment::Bottom, px(600.));
            cx.notify();
        });
        cx.run_until_parked();
        let wire::Node::List {
            request_handler,
            scroll_handler: Some(scroll_handler),
            ..
        } = *cx.root()
        else {
            panic!("expected list")
        };
        cx.tick(vec![wire::Event::ListRequest {
            handler: request_handler,
            request: wire::ListRequest {
                start: 1_936,
                end: 2_000,
            },
        }]);
        cx.tick(vec![wire::Event::ListScroll {
            handler: scroll_handler,
            event: wire::ListScroll {
                visible_start: 1_970,
                visible_end: 1_990,
                count: 2_000,
                is_scrolled: true,
                is_following_tail: false,
                offset: wire::ListOffset {
                    item_ix: 1_970,
                    offset_in_item: 0.,
                },
            },
        }]);
        let kept = view.read(|view| view.state.0.inner.borrow().requested.clone());
        assert_eq!(kept, 1_970 - 25..2_000, "600px of one-line rows each way");
    }

    #[test]
    fn revealing_a_row_of_an_empty_list_renders_no_row() {
        let (mut cx, view) = opened();
        let state = view.read(|view| view.state.clone());
        let rendered = view.read(|view| view.rendered.len());
        state.reset(0);
        state.scroll_to_reveal_item(0);
        cx.app_mut().notify();
        cx.tick(vec![]);
        match cx.root() {
            wire::Node::List {
                item_count,
                children,
                ..
            } => assert_eq!((*item_count, children.len()), (0, 0)),
            other => panic!("expected list, got {other:?}"),
        }
        view.read(|view| assert_eq!(view.rendered.len(), rendered));
    }

    #[test]
    fn a_splice_shifts_or_grows_the_window_and_never_blanks_the_rows_on_screen() {
        let (mut cx, view) = opened();
        let state = view.read(|view| view.state.clone());
        let wire::Node::List {
            request_handler, ..
        } = *cx.root()
        else {
            panic!("expected list")
        };
        let window = |cx: &TestAppContext| match cx.root() {
            wire::Node::List {
                range_start,
                children,
                ..
            } => *range_start..*range_start + children.len(),
            other => panic!("expected list, got {other:?}"),
        };
        cx.tick(vec![wire::Event::ListRequest {
            handler: request_handler,
            request: wire::ListRequest {
                start: 1_900,
                end: 1_912,
            },
        }]);
        assert_eq!(window(&cx), 1_900..1_912);
        let edited = |cx: &mut TestAppContext, old: Range<usize>, count: usize| {
            state.splice(old, count);
            cx.app_mut().notify();
            cx.tick(vec![]);
            window(cx)
        };
        assert_eq!(
            edited(&mut cx, 0..0, 50),
            1_950..1_962,
            "history above shifts"
        );
        assert_eq!(
            edited(&mut cx, 1_962..1_962, 3),
            1_950..1_965,
            "an arrival grows"
        );
        assert_eq!(
            edited(&mut cx, 2_053..2_053, 5),
            1_950..1_965,
            "past it, nothing"
        );
        assert_eq!(
            edited(&mut cx, 1_960..1_970, 2),
            1_950..1_962,
            "rows at its end edited: kept to the edit's end"
        );
        assert_eq!(
            edited(&mut cx, 0..2_030, 300),
            320 - INITIAL_ROWS..320,
            "every row gone: a new list's first screenful"
        );
    }

    #[test]
    fn a_page_of_history_above_a_reader_at_the_top_keeps_the_rows_on_screen() {
        let (mut cx, view) = opened();
        let state = view.read(|view| view.state.clone());
        let wire::Node::List {
            request_handler, ..
        } = *cx.root()
        else {
            panic!("expected list")
        };
        let window = |cx: &TestAppContext| match cx.root() {
            wire::Node::List {
                range_start,
                children,
                ..
            } => *range_start..*range_start + children.len(),
            other => panic!("expected list, got {other:?}"),
        };
        cx.tick(vec![wire::Event::ListRequest {
            handler: request_handler,
            request: wire::ListRequest { start: 0, end: 12 },
        }]);
        assert_eq!(window(&cx), 0..12);
        state.splice(0..0, 64);
        cx.app_mut().notify();
        cx.tick(vec![]);
        assert_eq!(
            window(&cx),
            64..76,
            "the rows on screen, where they are now"
        );

        let empty = ListState::new(0, ListAlignment::Bottom, px(0.));
        empty.splice(0..0, 100);
        assert_eq!(empty.0.inner.borrow().requested, 100 - INITIAL_ROWS..100);
    }

    #[test]
    fn state_operations_cross_once_as_native_commands_and_patch_props() {
        let (mut cx, view) = opened();
        let state = view.read(|view| view.state.clone());
        state.splice(0..0, 20);
        state.remeasure_items(100..102);
        state.scroll_to_reveal_item(1_999);
        cx.app_mut().notify();
        cx.tick(vec![]);
        match cx.root() {
            wire::Node::List {
                item_count,
                commands,
                children,
                ..
            } => {
                assert_eq!(*item_count, 2_020);
                assert_eq!(commands.len(), 3);
                assert!(!children.is_empty());
            }
            other => panic!("expected list, got {other:?}"),
        }
        state.scroll_to(ListOffset {
            item_ix: 1_999,
            offset_in_item: px(3.),
        });
        cx.app_mut().notify();
        cx.tick(vec![]);
        assert!(
            cx.last_frame()
                .patches
                .iter()
                .any(|patch| matches!(patch, wire::Patch::Props { .. })),
            "same-shape state operations update by props patches"
        );
        cx.app_mut().notify();
        cx.tick(vec![]);
        assert!(matches!(cx.root(), wire::Node::List { commands, .. } if commands.is_empty()));
    }

    #[derive(Default, Serialize, Deserialize)]
    struct Shared {
        #[serde(skip)]
        state: ListState,
    }
    impl View for Shared {
        const NAME: &'static str = "Shared";
        fn new(_: &mut Window, _: &mut Context<Self>) -> Self {
            Self {
                state: ListState::new(500, ListAlignment::Top, px(40.)),
            }
        }
    }
    impl Render for Shared {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let rows = |id: &'static str| {
                list(id, self.state.clone(), |index, _, _| {
                    div().child(index.to_string()).into_any_element()
                })
            };
            div().id("page").child(rows("first")).child(rows("second"))
        }
    }

    #[test]
    #[should_panic(
        expected = "one ListState drawn by two lists: [Name(\"page\"), Name(\"first\")] \
                    and [Name(\"page\"), Name(\"second\")]"
    )]
    fn one_state_drawn_by_two_lists_is_refused_naming_both() {
        TestAppContext::new().open::<Shared>();
    }
}
