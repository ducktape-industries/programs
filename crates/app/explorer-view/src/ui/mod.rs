//! The explorer's pages under a pointer: the desktop tree, which the root
//! picks when the device is not touch first (`lib.rs`; the phone's is
//! `ui/phone/`). Square corners, rows split by one-pixel rules, hashes and
//! numbers in the data face.
use std::ops::Range;

use view::prelude::*;
use view::{Div, FontWeight, Paged, Stateful};

use crate::chain::{Line, lines};
use crate::decode::{ago, date, grouped, plural, short};
use crate::state::landed;
use crate::words::{self, unindexed};
use crate::{BlockRow, Cx, Explorer, Route, TxRow};
use design::{empty_state, mono};

/// The rows each Overview panel draws.
const LATEST: usize = 12;

/// The bar's height, each line of it once the search wraps under the tabs, and a section heading's.
const BAR_H: Pixels = px(44.);
/// The search field grows to this; the tabs keep their room first.
const SEARCH_W: Pixels = px(360.);
/// The search field keeps this much of the bar: past it, it wraps under the tabs.
const SEARCH_MIN_W: Pixels = px(160.);
/// A list row's height, and a detail field's least.
const ROW_H: Pixels = px(40.);
/// The smallest box a pointer presses, each way (the door's AX-017).
const PRESS_TARGET: Pixels = px(24.);
/// The signer column.
const SIGNER_W: Pixels = px(180.);
/// A block list's height column.
const HEIGHT_W: Pixels = px(72.);
/// The age column: `59s`, `3h`.
const AGE_W: Pixels = px(36.);
/// A transaction row's outcome mark.
const MARK: Pixels = px(6.);
/// Each level a nested run steps in.
const NEST_W: Pixels = px(20.);
/// A transaction row's short hash.
/// A detail page's field labels.
const LABEL_W: Pixels = px(110.);
/// An operation's field labels.
const OP_LABEL_W: Pixels = px(90.);
/// The Accounts list's device count.
const DEVICES_W: Pixels = px(90.);
/// The Accounts list's transaction count.
const COUNT_W: Pixels = px(60.);
/// An account page's side column: devices and programs used.
const SIDE_W: Pixels = px(380.);
/// The Overview's latest blocks at their widest, beside the latest transactions.
const LATEST_BLOCKS_W: Pixels = px(420.);
/// The avatar an account page opens with.
const PAGE_AVATAR: Pixels = px(40.);

mod accounts;
mod blocks;
mod overview;
pub(crate) mod phone;
mod programs;
mod transactions;
use accounts::{account, accounts};
use blocks::{block, blocks};
use overview::overview;
use programs::{program, programs};
use transactions::{transactions, tx};
use view::design::Ladder;

pub fn render(view: &Explorer, cx: Cx) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    let mut root = div()
        .id("explorer")
        .flex()
        .flex_col()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(bar(view, cx, &theme, ladder));
    if let Some(note) = &view.note {
        let note = words::note(note);
        root = root.child(
            div()
                .id("explorer-note")
                .px_5()
                .py_2()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(note),
        );
    }
    let scroller = div().id("explorer-page").flex_1().flex().flex_col();
    let scroller = match view.pages.top() {
        Route::Transactions(_) | Route::Blocks | Route::Accounts => scroller.min_h_0(),
        _ => scroller.overflow_y_scroll(),
    };
    root.child(scroller.child(page(view, cx, &theme, ladder)))
        .into_any_element()
}

fn bar(view: &Explorer, cx: Cx, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let tabs = [
        ("overview", "Overview", Route::Overview),
        ("blocks", "Blocks", Route::Blocks),
        ("transactions", "Transactions", Route::Transactions(None)),
        ("accounts", "Accounts", Route::Accounts),
        ("programs", "Programs", Route::Programs),
    ];
    let submit = cx.listener(|view: &mut Explorer, _: &(), _, cx| view.search(cx));
    let shown = view.pages.top().tab();
    let routes: Vec<Route> = tabs.iter().map(|(_, _, route)| route.clone()).collect();
    let pressed = routes.clone();
    let tabs = tabs.into_iter().map(|(key, label, route)| {
        let active = shown == route.tab();
        let go = cx
            .listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
        design::item(
            design::tab(
                format!("explorer-tab-{key}"),
                label,
                active,
                ladder,
                theme,
                go,
            )
            .h_full()
            .mx_1(),
            Role::Tab,
            active,
        )
    });
    // on a narrow window the search wraps under the tabs, a line of its own
    div()
        .id("explorer-bar")
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .px_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            // one Tab stop; ← → open the next tab's page as it was left (a
            // click, or Enter or Space on the active tab, opens its list)
            design::composite("explorer-tabs", Role::TabList, "Pages")
                .orientation(design::Orientation::Horizontal)
                .wrap()
                .active(shown, routes.len())
                .on_move(
                    cx.processor(move |view: &mut Explorer, index: usize, _, cx| {
                        let page = view.left[index].clone();
                        view.go(page.unwrap_or_else(|| routes[index].clone()), cx)
                    }),
                )
                .on_press(
                    cx.processor(move |view: &mut Explorer, index: usize, _, cx| {
                        view.go(pressed[index].clone(), cx)
                    }),
                )
                .build()
                .h(BAR_H)
                .flex()
                .items_center()
                .children(tabs),
        )
        .child(
            div()
                .flex_1()
                .min_w(SEARCH_MIN_W)
                .max_w(SEARCH_W)
                .h(BAR_H)
                .flex()
                .items_center()
                .child(
                    Input::new("explorer-search", &view.search, words::SEARCH)
                        .w_full()
                        .h(design::size::CONTROL)
                        .px_2()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        .text_size(design::text::SECONDARY)
                        .placeholder(words::SEARCH_HINT)
                        .on_submit(submit),
                ),
        )
}

fn page(view: &Explorer, cx: Cx, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let without_chain = matches!(
        view.pages.top(),
        Route::Programs | Route::Program(_) | Route::Accounts
    );
    if view.chain.blocks.is_empty() && !without_chain {
        return match &view.chain.failed {
            Some(sentence) => failed(sentence, cx, theme, ladder),
            None => quiet("explorer-loading", words::READING_CHAIN, theme),
        };
    }
    match view.pages.top().clone() {
        Route::Overview => overview(view, cx, theme, ladder),
        Route::Blocks => blocks(view, cx, theme),
        Route::Block(height) => block(view, height, cx, theme, ladder),
        Route::Transactions(program) => transactions(view, program, cx, theme, ladder),
        Route::Tx { height, position } => tx(view, height, position, cx, theme, ladder),
        Route::Accounts => accounts(view, cx, theme, ladder),
        Route::Account(number) => account(view, number, cx, theme, ladder),
        Route::Programs => programs(view, cx, theme, ladder),
        Route::Program(name) => program(view, &name, cx, theme, ladder),
    }
}

// ---------- pieces ----------

fn quiet(id: &'static str, text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    design::quiet(text, theme)
        .id(id)
        .px_5()
        .py_4()
        .into_any_element()
}

fn failed(sentence: &str, cx: Cx, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let retry = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.read_all(cx));
    design::refused("explorer", sentence.to_owned(), ladder, theme, retry)
        .m_5()
        .into_any_element()
}

/// A section's title row: its name, then what sits on its right.
fn heading(id: &str, title: &str, right: Option<AnyElement>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.to_string()))
        .flex()
        .items_center()
        .gap_3()
        .h(BAR_H)
        .px_5()
        .border_b_1()
        .border_color(theme.border)
        .child(design::heading(format!("{id}-title"), title.to_owned(), 2, theme).flex_1())
        .children(right)
}

fn caption(text: impl Into<SharedString>, theme: &Theme) -> AnyElement {
    mono(text)
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .into_any_element()
}

/// A link that goes somewhere inside the explorer, in a box no smaller
/// than [`PRESS_TARGET`] with the text centred down it.
fn link(id: String, text: String, route: Route, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    div()
        .id(SharedString::from(id))
        .min_w(PRESS_TARGET)
        .min_h(PRESS_TARGET)
        .flex()
        .items_center()
        .text_color(theme.link)
        .hover(|s| s.underline())
        .role(Role::Link)
        .focusable()
        .on_click(go)
        .child(text)
}

/// A clickable row of a list: an option of the [`Rows`] it goes in, which
/// holds the focus and the arrows; nothing is ever selected, the press
/// navigates.
fn row(id: ElementId, label: String, route: Route, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let go =
        cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| view.go(route.clone(), cx));
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_4()
        .w_full()
        .h(ROW_H)
        .px_5()
        .border_b_1()
        .border_color(theme.border)
        .hover(|s| s.bg(theme.hover))
        .aria_label(label)
        .aria_selected(false)
        .on_click(go)
}

/// A list of [`row`]s: one Tab stop, ↑ ↓ walk the rows, Home/End reach the
/// ends, Enter opens the active row. The active row is the view's cursor in
/// this list, else the first. Rows handed over built ([`Rows::row`]) are
/// drawn whole, with notes (a run of empty blocks) between them unroled;
/// rows the list paints when asked ([`Rows::shown`]) are drawn as a
/// `uniform_list` that scrolls for its page: the screenful the host shows
/// and a margin, scrolled to the row the arrows move to ahead of the frame
/// that claims it.
struct Rows {
    id: &'static str,
    label: &'static str,
    active: usize,
    scroll: UniformListScrollHandle,
    routes: Vec<Route>,
    children: Vec<AnyElement>,
    paint: Option<Painter>,
    /// the line a painted list ends with while its listing reads on
    /// ([`Rows::more`])
    more: Option<&'static str>,
}

/// Paints a range of a list's rows, each a [`row`], off the view they are
/// read from: the host asks for a window at a time.
type Painter = Box<dyn Fn(&Explorer, Range<usize>, Cx, &Theme) -> Vec<Stateful<Div>>>;

fn rows(id: &'static str, label: &'static str, view: &Explorer) -> Rows {
    let active = match view.cursor {
        Some((list, index)) if list == id => index,
        _ => 0,
    };
    Rows {
        id,
        label,
        active,
        scroll: view.scroll.clone(),
        routes: Vec::new(),
        children: Vec::new(),
        paint: None,
        more: None,
    }
}

/// A row as its list's option: roled, and the claim when it is the active
/// one.
fn option(row: Stateful<Div>, active: bool, theme: &Theme) -> AnyElement {
    design::item(row, Role::ListBoxOption, active)
        .when(active, |row| row.bg(theme.surface_raised))
        .into_any_element()
}

impl Rows {
    fn row(mut self, route: Route, row: Stateful<Div>, theme: &Theme) -> Self {
        let active = self.routes.len() == self.active;
        self.routes.push(route);
        self.children.push(option(row, active, theme));
        self
    }
    /// `routes.len()` rows the host asks for by index, painted as it shows
    /// them. They scroll for their page, which is no scroller then
    /// ([`render`]): the list reaches the pane's edge, where the host keeps
    /// the bar's gutter beside the rows, as a page that scrolls is given
    /// one.
    fn shown(
        mut self,
        routes: Vec<Route>,
        paint: impl Fn(&Explorer, Range<usize>, Cx, &Theme) -> Vec<Stateful<Div>> + 'static,
    ) -> Self {
        self.routes = routes;
        self.paint = Some(Box::new(paint));
        self
    }
    /// The line the painted rows end with, a row of their height that is
    /// no option: the listing goes on past them. The painter is asked for
    /// its index too, which is how it hears the list reached its end.
    fn more(mut self, line: Option<&'static str>) -> Self {
        self.more = line;
        self
    }
    /// The painted rows built now, drawn whole: for a list in a page that
    /// scrolls (the Overview's panels, a block's transactions, an account's
    /// activity), which is laid out by its rows; a list sized by its box
    /// has no size of its own to give.
    fn whole(mut self, view: &Explorer, cx: Cx, theme: &Theme) -> Self {
        if let Some(paint) = self.paint.take() {
            let rows = paint(view, 0..self.routes.len(), cx, theme);
            let active = self.active;
            self.children.extend(
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| option(row, index == active, theme)),
            );
        }
        self
    }
    fn build(self, cx: Cx) -> Stateful<Div> {
        let Self {
            id,
            label,
            active,
            scroll,
            routes,
            children,
            paint,
            more,
        } = self;
        let count = routes.len();
        let active = active.min(count.saturating_sub(1));
        let mut list = design::composite(id, Role::ListBox, label).active(active, count);
        if paint.is_some() {
            list = list.track_scroll(&scroll);
        }
        let list = list
            .on_move(
                cx.processor(move |view: &mut Explorer, index: usize, _, cx| {
                    view.cursor = Some((id, index));
                    cx.notify();
                }),
            )
            .on_press(
                cx.processor(move |view: &mut Explorer, index: usize, _, cx| {
                    view.go(routes[index].clone(), cx)
                }),
            )
            .build()
            .children(children);
        let Some(paint) = paint else {
            return list;
        };
        let theme = *cx.global::<Theme>();
        let window = uniform_list(
            SharedString::from(format!("{id}-rows")),
            count + usize::from(more.is_some()),
            cx.processor(move |view: &mut Explorer, range: Range<usize>, _, cx| {
                let mut rows: Vec<AnyElement> = paint(view, range.clone(), cx, &theme)
                    .into_iter()
                    .zip(range.clone())
                    .map(|(row, index)| option(row, index == active, &theme))
                    .collect();
                if let Some(line) = more.filter(|_| range.end > count) {
                    rows.push(
                        div()
                            .id(SharedString::from(format!("{id}-more")))
                            .h(ROW_H)
                            .px_5()
                            .flex()
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .text_color(theme.faint)
                            .child(line)
                            .into_any_element(),
                    );
                }
                rows
            }),
        )
        .track_scroll(&scroll)
        .flex_1()
        .min_h_0();
        list.flex_1().min_h_0().flex().flex_col().child(window)
    }
}

/// Who signed: the account holding the key, or the key itself.
fn signer(view: &Explorer, key: &[u8], cx: Cx, theme: &Theme) -> impl IntoElement {
    let (name, number, picture) = match view.holder(key) {
        Some((account, _)) => (
            account.card.name.clone(),
            Some(account.number),
            view.picture(account, cx),
        ),
        None => (short(key), None, None),
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .w(SIGNER_W)
        .flex_shrink_0()
        .child(design::avatar_with(
            &name,
            picture,
            design::size::AVATAR,
            theme,
        ))
        .child(div().truncate().child(name))
        .children(number.map(|number| {
            mono(format!("#{number}"))
                .text_size(design::text::CAPTION)
                .text_color(theme.faint)
        }))
}

fn block_row(block: &BlockRow, now: u64, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let id = SharedString::from(format!("explorer-block-{}", block.height)).into();
    row(
        id,
        format!("Block {}", block.height),
        Route::Block(block.height),
        cx,
        theme,
    )
    .when(block.txs == 0, |row| row.text_color(theme.faint))
    .child(
        mono(grouped(block.height))
            .w(HEIGHT_W)
            .when(block.txs > 0, |height| {
                height.font_weight(FontWeight::SEMIBOLD)
            }),
    )
    .child(mono(short(&block.id)).flex_1().text_color(theme.muted))
    .child(
        div()
            .text_color(theme.muted)
            .child(words::tx_count(block.txs as u64)),
    )
    .child(
        mono(ago(now, block.time))
            .w(AGE_W)
            .flex()
            .justify_end()
            .text_color(theme.faint),
    )
}

fn line_route(line: &Line) -> Route {
    match line {
        Line::Block(block) => Route::Block(block.height),
        Line::Empty { newest, .. } => Route::Block(*newest),
    }
}

fn line_row(line: &Line, now: u64, cx: Cx, theme: &Theme) -> Stateful<Div> {
    match line {
        Line::Block(block) => block_row(block, now, cx, theme),
        Line::Empty { newest, oldest } => {
            let text = words::empty_run(*oldest, *newest);
            row(
                SharedString::from(format!("explorer-empty-{newest}")).into(),
                text.clone(),
                Route::Block(*newest),
                cx,
                theme,
            )
            .text_color(theme.faint)
            .child(mono(text))
        }
    }
}

fn block_lines(list: Rows, view: &Explorer, rows: usize) -> Rows {
    let routes = lines(&view.chain.blocks, rows)
        .iter()
        .map(line_route)
        .collect();
    list.shown(routes, move |view, range, cx, theme| {
        let now = view.chain.now();
        let lines = lines(&view.chain.blocks, rows);
        range
            .filter_map(|index| lines.get(index))
            .map(|line| line_row(line, now, cx, theme))
            .collect()
    })
}

/// `tx_row`s as a [`Rows`] list. `select` picks the transactions, in
/// order, off the view: once here for their routes, and again for each
/// window the host asks for, since a row borrows the view it is read from.
fn tx_rows(
    list: Rows,
    view: &Explorer,
    select: impl Fn(&Explorer) -> Vec<&TxRow> + 'static,
) -> Rows {
    let routes = select(view).iter().map(|tx| tx_route(tx)).collect();
    list.shown(routes, move |view, range, cx, theme| {
        let txs = select(view);
        range
            .map(|index| tx_row(view, txs[index], cx, theme))
            .collect()
    })
}

/// Its id is its place on the chain, which a frame the node landed again
/// (the same hash) does not share.
fn tx_row(view: &Explorer, tx: &TxRow, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let (height, position) = (tx.height, tx.index);
    let id = SharedString::from(format!("explorer-tx-{height}-{position}"));
    let place = format!("{} · {position}", grouped(height));
    let applied = tx
        .receipt
        .as_ref()
        .map(|run| matches!(run.outcome, Outcome::Applied { .. }));
    let label = match applied {
        Some(applied) => activity::view::label(&tx.target, applied, &grouped(height), position),
        None => format!("{}, block {place}", tx.target),
    };
    let color = applied.map(|applied| verdict(applied, theme).1);
    let mark = div()
        .size(MARK)
        .flex_shrink_0()
        .children(color.map(|color| {
            div()
                .id(SharedString::from(format!(
                    "explorer-tx-mark-{height}-{position}"
                )))
                .size_full()
                .bg(color)
        }));
    row(ElementId::Name(id), label, tx_route(tx), cx, theme)
        .child(mark)
        .child(div().flex_1().truncate().child(tx.target.clone()))
        .child(signer(view, &tx.signer, cx, theme))
        .child(mono(place).text_color(theme.muted))
}

fn tx_route(tx: &TxRow) -> Route {
    Route::Tx {
        height: tx.height,
        position: tx.index,
    }
}

/// The frames `select` picks of those `listing` holds, in order, as a
/// [`Rows`] list. A list painted past them (its [`Rows::more`] line) reads
/// the listing's next page, and the next while that line stays in view:
/// until the rows fill the list or the listing ends.
fn frame_rows(
    list: Rows,
    listing: &Entity<Paged<activity::Frame>>,
    select: impl Fn(&[activity::Frame]) -> Vec<&activity::Frame> + 'static,
) -> Rows {
    let routes = listing.read(|listing| {
        let picked = select(listing.rows());
        picked.into_iter().map(frame_route).collect()
    });
    let listing = listing.clone();
    list.shown(routes, move |view, range, cx, theme| {
        let (rows, picked) = listing.read(|listing| {
            let picked = select(listing.rows());
            let rows = picked.get(range.start..range.end.min(picked.len()));
            let rows = rows.unwrap_or_default().iter();
            let rows = rows
                .map(|frame| frame_row(view, frame, cx, theme))
                .collect();
            (rows, picked.len())
        });
        if range.end > picked {
            listing.update(cx, read_older);
        }
        rows
    })
}

/// The line over a list whose listing is behind the rows written above
/// it: Show newest reads it anew from its first page.
fn newer(cx: Cx, theme: &Theme) -> impl IntoElement {
    let show = |view: &mut Explorer, cx: &mut Context<Explorer>| view.show_newest(cx);
    div()
        .id("explorer-newer")
        .flex()
        .items_center()
        .gap_3()
        .px_5()
        .py_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .text_color(theme.muted)
        .child(div().flex_1().child(words::NEWER))
        .child(press(
            "explorer-show-newest",
            words::SHOW_NEWEST,
            cx,
            theme,
            show,
        ))
}

/// The listing's next page, if it goes on: asked as the page past the
/// rows held, which [`Paged::show`] reads once.
fn read_older(listing: &mut Paged<activity::Frame>, cx: &mut Context<Paged<activity::Frame>>) {
    let held = listing.rows().len();
    listing.show(held..held + 1, cx);
}

fn frame_route(frame: &activity::Frame) -> Route {
    Route::Tx {
        height: frame.height,
        position: frame.position,
    }
}

fn frame_row(view: &Explorer, frame: &activity::Frame, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let (height, position) = (frame.height, frame.position);
    let id = SharedString::from(format!("explorer-tx-{height}-{position}"));
    let now = view.chain.now();
    let (_, color, _) = verdict(frame.applied, theme);
    let label = activity::view::label(&frame.target, frame.applied, &grouped(height), position);
    let mark = div()
        .id(SharedString::from(format!(
            "explorer-tx-mark-{height}-{position}"
        )))
        .size(MARK)
        .flex_shrink_0()
        .bg(color);
    row(ElementId::Name(id), label, frame_route(frame), cx, theme)
        .child(mark)
        .child(div().flex_1().truncate().child(frame.target.clone()))
        .child(signer(view, &frame.signer, cx, theme))
        .child(mono(grouped(height)).text_color(theme.muted))
        .child(
            mono(ago(now, frame.time))
                .w(AGE_W)
                .flex()
                .justify_end()
                .text_color(theme.faint),
        )
}

/// A run's outcome in a word, its colour and its soft background.
fn outcome(run: &Receipt, theme: &Theme) -> (&'static str, Hsla, Hsla) {
    let applied = matches!(run.outcome, Outcome::Applied { .. });
    verdict(applied, theme)
}

fn verdict(applied: bool, theme: &Theme) -> (&'static str, Hsla, Hsla) {
    let word = activity::view::verdict(applied);
    if applied {
        (word, theme.success, theme.success_soft)
    } else {
        (word, theme.danger, theme.danger_soft)
    }
}

/// A label and its value, one row of a detail page.
fn field(label: &str, value: impl IntoElement, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .min_h(ROW_H)
        .px_5()
        .gap_4()
        .border_b_1()
        .border_color(theme.border)
        .child(
            mono(label.to_string())
                .w(LABEL_W)
                .flex_shrink_0()
                .text_color(theme.muted),
        )
        .child(value)
}

fn titled(kind: &str, title: String, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .px_5()
        .py_4()
        .child(
            mono(kind.to_string())
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        )
        .child(
            div()
                .id("explorer-title")
                .text_size(design::text::TITLE)
                .role(Role::Heading)
                .aria_level(1)
                .child(title),
        )
}

/// A press on a page: an outlined button that takes its own click and
/// Enter or Space, so a row it sits in does not open too.
fn press(
    id: impl Into<SharedString>,
    label: &str,
    cx: Cx,
    theme: &Theme,
    pressed: impl Fn(&mut Explorer, &mut Context<Explorer>) + 'static,
) -> AnyElement {
    let click = cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| pressed(view, cx));
    div()
        .id(id.into())
        .flex_shrink_0()
        .px_2()
        .h(design::size::CONTROL)
        .flex()
        .items_center()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .text_color(theme.foreground)
        .hover(|s| s.bg(theme.hover))
        .role(Role::Button)
        .focusable()
        .on_click(click)
        .consumes_click()
        .consumes_keys(["enter", "space"])
        .child(label.to_owned())
        .into_any_element()
}

/// "Copy link": this page's `duck://<chain>/explorer/…` link, onto the
/// clipboard. Absent while the session names no chain.
fn copy_button(view: &Explorer, route: &Route, cx: Cx, theme: &Theme) -> Option<AnyElement> {
    let link = view.link(route)?;
    let copy = cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
        view.copy(link.clone(), words::LINK, cx)
    });
    Some(
        div()
            .id("explorer-copy-link")
            .px_3()
            .h(design::size::CONTROL)
            .flex()
            .items_center()
            .border_1()
            .border_color(theme.border)
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .hover(|s| s.bg(theme.hover).text_color(theme.foreground))
            .role(Role::Button)
            .focusable()
            .on_click(copy)
            .child("Copy link")
            .into_any_element(),
    )
}
