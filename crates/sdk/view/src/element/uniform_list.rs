use super::*;
use crate::list::MARGIN_ROWS;
use gpui::Pixels;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Weak;

type UniformProcessor = Box<dyn Fn(Range<usize>, &mut Window, &mut App) -> Vec<AnyElement>>;

/// A handle for controlling a view uniform list across frames.
#[derive(Clone, Default)]
pub struct UniformListScrollHandle(Rc<RefCell<UniformListScrollState>>);

#[derive(Default)]
struct UniformListScrollState {
    pub(crate) request: Option<wire::list::UniformListScrollRequest>,
    pub(crate) revision: u64,
    pub(crate) y_flipped: bool,
    pub(crate) top_index: usize,
    pub(crate) scrollable: bool,
    pub(crate) scrolled_to_end: Option<bool>,
}

impl UniformListScrollHandle {
    pub fn new() -> Self {
        Self::default()
    }

    fn request(&self, index: usize, strategy: ScrollStrategy, offset: usize, strict: bool) {
        let strategy = match strategy {
            ScrollStrategy::Top => wire::list::UniformListScrollStrategy::Top,
            ScrollStrategy::Center => wire::list::UniformListScrollStrategy::Center,
            ScrollStrategy::Bottom => wire::list::UniformListScrollStrategy::Bottom,
            ScrollStrategy::Nearest => wire::list::UniformListScrollStrategy::Nearest,
        };
        let mut state = self.0.borrow_mut();
        state.request = Some(wire::list::UniformListScrollRequest {
            index,
            strategy,
            offset,
            strict,
        });
        state.revision = state.revision.wrapping_add(1);
    }

    pub fn scroll_to_item(&self, index: usize, strategy: ScrollStrategy) {
        self.request(index, strategy, 0, false);
    }

    pub fn scroll_to_item_strict(&self, index: usize, strategy: ScrollStrategy) {
        self.request(index, strategy, 0, true);
    }

    pub fn scroll_to_item_with_offset(
        &self,
        index: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.request(index, strategy, offset, false);
    }

    pub fn scroll_to_item_strict_with_offset(
        &self,
        index: usize,
        strategy: ScrollStrategy,
        offset: usize,
    ) {
        self.request(index, strategy, offset, true);
    }

    pub fn y_flipped(&self) -> bool {
        self.0.borrow().y_flipped
    }

    pub fn logical_scroll_top_index(&self) -> usize {
        self.0.borrow().top_index
    }

    pub fn is_scrollable(&self) -> bool {
        self.0.borrow().scrollable
    }

    pub fn is_scrolled_to_end(&self) -> Option<bool> {
        self.0.borrow().scrolled_to_end
    }

    pub fn scroll_to_bottom(&self) {
        self.scroll_to_item(usize::MAX, ScrollStrategy::Bottom);
    }
}

/// A GPUI-shaped uniform list. The host owns layout and virtualization; the
/// view lowers the rows of a window around what the host shows, sized
/// from its viewport, so the first frame holds a screenful and a row the
/// view scrolls to is in the frame that carries the scroll.
pub struct UniformList {
    count: usize,
    processor: UniformProcessor,
    pub(crate) interactivity: Box<Interactivity>,
    measure_index: usize,
    sizing: wire::list::UniformListSizing,
    horizontal_sizing: wire::list::UniformListHorizontalSizing,
    scroll: Option<UniformListScrollHandle>,
    y_flipped: bool,
}

/// A uniform-height list of `count` rows, built a range at a time by
/// `processor`.
///
/// Each row is a scope of its own, under the list's `id`: a row is filed
/// under the id of its root element, else under its index, and the ids
/// inside a row never meet another row's. Two rows named by one key are
/// refused, naming the id and its scope.
///
/// The host draws the list's scroll bar in a gutter it keeps beside the
/// list, outside the list's own box: no view makes room for it.
pub fn uniform_list<R: IntoElement>(
    id: impl Into<ElementId>,
    count: usize,
    processor: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
) -> UniformList {
    let mut interactivity = Box::<Interactivity>::default();
    interactivity.id = Some(id.into());
    interactivity.base_style.overflow.y = Some(Overflow::Scroll);
    UniformList {
        count,
        processor: Box::new(move |range, window, app| {
            processor(range, window, app)
                .into_iter()
                .map(IntoElement::into_any_element)
                .collect()
        }),
        interactivity,
        measure_index: 0,
        sizing: wire::list::UniformListSizing::Auto,
        horizontal_sizing: wire::list::UniformListHorizontalSizing::FitList,
        scroll: None,
        y_flipped: false,
    }
}

impl UniformList {
    pub fn with_width_from_item(mut self, item_index: Option<usize>) -> Self {
        self.measure_index = item_index.unwrap_or(0);
        self
    }

    pub fn with_sizing_behavior(mut self, behavior: ListSizingBehavior) -> Self {
        self.sizing = match behavior {
            ListSizingBehavior::Infer => wire::list::UniformListSizing::Infer,
            ListSizingBehavior::Auto => wire::list::UniformListSizing::Auto,
        };
        self
    }

    pub fn with_horizontal_sizing_behavior(
        mut self,
        behavior: ListHorizontalSizingBehavior,
    ) -> Self {
        self.horizontal_sizing = match behavior {
            ListHorizontalSizingBehavior::FitList => {
                self.interactivity.base_style.overflow.x = None;
                wire::list::UniformListHorizontalSizing::FitList
            }
            ListHorizontalSizingBehavior::Unconstrained => {
                self.interactivity.base_style.overflow.x = Some(Overflow::Scroll);
                wire::list::UniformListHorizontalSizing::Unconstrained
            }
        };
        self
    }

    pub fn track_scroll(mut self, handle: &UniformListScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }

    pub fn y_flipped(mut self, y_flipped: bool) -> Self {
        self.y_flipped = y_flipped;
        self
    }
}

impl Styled for UniformList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.interactivity.base_style
    }
}

impl crate::InteractiveElement for UniformList {
    fn interactivity(&mut self) -> &mut Interactivity {
        &mut self.interactivity
    }
}

impl Element for UniformList {
    fn id(&self) -> Option<ElementId> {
        self.interactivity.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        let count = self.count;
        let path = lowering.current_path().to_vec();
        let id = path
            .last()
            .cloned()
            .expect("uniform list lowers inside its authored scope");
        let measure_index = self.measure_index.min(count.saturating_sub(1));
        let (scroll_request, revision) = match &self.scroll {
            Some(handle) => {
                let mut state = handle.0.borrow_mut();
                state.y_flipped = self.y_flipped;
                (state.request.take(), state.revision)
            }
            None => (None, 0),
        };
        let scroll = self.scroll.as_ref().map(|handle| &handle.0);
        lowering.opaque();
        let viewport = lowering.window.viewport_size().height;
        let (route, ranges) = lowering.app.inner.uniform_lists.route(
            &path,
            count,
            measure_index,
            scroll,
            viewport,
            scroll_request.as_ref(),
        );
        let mut indices = Vec::new();
        let mut children = Vec::new();
        for range in ranges {
            if range.is_empty() {
                continue;
            }
            let rendered = (self.processor)(range.clone(), lowering.window, lowering.app);
            for (index, child) in range.zip(rendered) {
                let index = index as u32;
                if indices.contains(&index) {
                    continue;
                }
                indices.push(index);
                children.push(lowering.lower_row(index as usize, child));
            }
        }
        let style = lowering.style(&self.interactivity.base_style);
        let interactivity = self.interactivity.into_wire(lowering);
        wire::Node::UniformList {
            id,
            path,
            route,
            style,
            interactivity,
            count,
            measure_index,
            sizing: self.sizing,
            horizontal_sizing: self.horizontal_sizing,
            y_flipped: self.y_flipped,
            scroll_request,
            revision,
            indices,
            children,
        }
    }
}

impl IntoElement for UniformList {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::prelude::FluentBuilder for UniformList {}

struct UniformListRoute {
    route: u32,
    count: usize,
    measure_index: usize,
    window: Range<usize>,
    shown: Option<Range<usize>>,
    item_height: Option<f32>,
    scroll: Option<Weak<RefCell<UniformListScrollState>>>,
    seen: u64,
}

pub(crate) struct UniformLists {
    lists: RefCell<HashMap<Vec<wire::ElementIdWire>, UniformListRoute>>,
    next_route: Cell<u32>,
    frame: Cell<u64>,
}

impl Default for UniformLists {
    fn default() -> Self {
        Self {
            lists: RefCell::default(),
            next_route: Cell::new(1),
            frame: Cell::new(0),
        }
    }
}

impl UniformLists {
    pub(crate) fn begin_frame(&self) {
        self.frame.set(self.frame.get() + 1);
    }

    pub(crate) fn end_frame(&self) {
        let frame = self.frame.get();
        self.lists.borrow_mut().retain(|_, list| list.seen == frame);
    }

    fn route(
        &self,
        path: &[wire::ElementIdWire],
        count: usize,
        measure_index: usize,
        scroll: Option<&Rc<RefCell<UniformListScrollState>>>,
        viewport: Pixels,
        request: Option<&wire::list::UniformListScrollRequest>,
    ) -> (u32, Vec<Range<usize>>) {
        let mut lists = self.lists.borrow_mut();
        let state = lists.entry(path.to_vec()).or_insert_with(|| {
            let route = self.next_route.get();
            self.next_route.set(route.wrapping_add(1).max(1));
            UniformListRoute {
                route,
                count,
                measure_index,
                window: 0..0,
                shown: None,
                item_height: None,
                scroll: None,
                seen: 0,
            }
        });
        state.seen = self.frame.get();
        state.count = count;
        state.measure_index = measure_index;
        state.scroll = scroll.map(Rc::downgrade);
        let held = clamped(state.window.clone(), count);
        let rows = viewport_row_count(viewport, state.item_height);
        let (start, end) = state.shown.as_ref().map_or((0, 0), |s| (s.start, s.end));
        let start = start.min(count.saturating_sub(rows));
        let shown = clamped(start..end.max(start + rows), count);
        let target = match request {
            Some(request) => rows_after_scroll_request(request, &shown, rows, count),
            None => shown,
        };
        if request.is_some() {
            state.shown = Some(target.clone());
        }
        state.window = window(&held, target, count);
        let measurement = measure_index..measure_index.saturating_add(1).min(count);
        let mut ranges = Vec::new();
        if !state.window.contains(&measure_index) {
            ranges.push(measurement);
        }
        if !state.window.is_empty() {
            ranges.push(state.window.clone());
        }
        (state.route, ranges)
    }

    pub(crate) fn request_range(
        &self,
        path: Vec<wire::ElementIdWire>,
        route: u32,
        start: usize,
        end: usize,
        item_height: f32,
    ) -> bool {
        let mut lists = self.lists.borrow_mut();
        let Some(state) = lists.get_mut(&path) else {
            return false;
        };
        if state.route != route {
            return false;
        }
        if item_height.is_finite() && item_height > 0. {
            state.item_height = Some(item_height);
        }
        let shown = clamped(start..end, state.count);
        if shown.is_empty() {
            return false;
        }
        state.shown = Some(shown.clone());
        let held = clamped(state.window.clone(), state.count);
        let holds = !held.is_empty() && held.start <= shown.start && shown.end <= held.end;
        state.window = match holds {
            true => {
                held.start.max(shown.start.saturating_sub(MARGIN_ROWS))
                    ..held.end.min(shown.end.saturating_add(MARGIN_ROWS))
            }
            false => window(&(0..0), shown, state.count),
        };
        !holds
    }

    pub(crate) fn update_state(
        &self,
        path: &[wire::ElementIdWire],
        route: u32,
        top_index: usize,
        scrollable: bool,
        scrolled_to_end: Option<bool>,
    ) {
        let lists = self.lists.borrow();
        let Some(state) = lists.get(path).filter(|state| state.route == route) else {
            return;
        };
        if let Some(scroll) = state.scroll.as_ref().and_then(Weak::upgrade) {
            let mut scroll = scroll.borrow_mut();
            scroll.top_index = top_index;
            scroll.scrollable = scrollable;
            scroll.scrolled_to_end = scrolled_to_end;
        }
    }
}

fn viewport_row_count(viewport: Pixels, item_height: Option<f32>) -> usize {
    let item_height = item_height.unwrap_or(f32::from(crate::design::size::ROW));
    (f32::from(viewport) / item_height).ceil().max(1.) as usize
}

fn rows_after_scroll_request(
    request: &wire::list::UniformListScrollRequest,
    shown: &Range<usize>,
    rows: usize,
    count: usize,
) -> Range<usize> {
    use wire::list::UniformListScrollStrategy as Strategy;
    let index = request.index.min(count.saturating_sub(1));
    let above = index <= shown.start.saturating_add(request.offset);
    let below = index.saturating_add(1) >= shown.end;
    let start = match request.strategy {
        _ if !(above || below || request.strict) => return shown.clone(),
        Strategy::Nearest if above => index.saturating_sub(request.offset),
        Strategy::Nearest if below => (index + 1).saturating_sub(rows),
        Strategy::Nearest => return shown.clone(),
        Strategy::Top => index.saturating_sub(request.offset),
        Strategy::Center => index.saturating_sub(rows / 2),
        Strategy::Bottom => (index + 1).saturating_sub(rows),
    };
    let start = start.min(count.saturating_sub(rows));
    start..(start + rows).min(count)
}

fn window(held: &Range<usize>, target: Range<usize>, count: usize) -> Range<usize> {
    if !held.is_empty() && held.start <= target.start && target.end <= held.end {
        return held.clone();
    }
    let band = target.start.saturating_sub(MARGIN_ROWS)..target.end.saturating_add(MARGIN_ROWS);
    let band = clamped(band, count);
    let apart = held.is_empty() || band.end < held.start || held.end < band.start;
    if apart {
        return band;
    }
    held.start.min(band.start)..held.end.max(band.end)
}

fn clamped(range: Range<usize>, count: usize) -> Range<usize> {
    let start = range.start.min(count);
    start..range.end.clamp(start, count)
}
