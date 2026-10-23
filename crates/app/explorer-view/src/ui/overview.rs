//! The Overview tab: the head, the latest blocks and transactions.
use super::*;

const STAT_MIN_W: Pixels = px(200.);
const PANEL_MIN_W: Pixels = px(320.);
const TXS_MIN_W: Pixels = px(624.);

fn side_by_side(view: &Explorer) -> bool {
    let gutter = f32::from(design::size::SCROLLBAR);
    view.width - gutter >= f32::from(PANEL_MIN_W + TXS_MIN_W)
}

pub(super) fn overview(view: &Explorer, cx: Cx, theme: &Theme) -> AnyElement {
    let side = side_by_side(view);
    let latest = div()
        .id("explorer-latest")
        .flex()
        .when(side, |row| row.flex_wrap().flex_1())
        .when(!side, |column| column.flex_col())
        .child(latest_blocks(view, side, cx, theme))
        .child(latest_txs(view, side, cx, theme));
    div()
        .id("explorer-overview")
        .flex()
        .flex_col()
        .flex_1()
        .child(stats(view, theme))
        .child(latest)
        .into_any_element()
}

fn stats(view: &Explorer, theme: &Theme) -> impl IntoElement {
    let status = view.status.ready();
    let dash = || words::NO_VALUE.to_owned();
    let (height, cadence) = match status {
        Some(status) => (
            grouped(status.height),
            words::block_every(status.block_time_ms),
        ),
        None => (dash(), String::new()),
    };
    let (epoch, next) = match (status, view.epoch_left()) {
        (Some(status), Some(left)) => (grouped(status.epoch), words::next_in(left)),
        _ => (dash(), String::new()),
    };
    let validators = view
        .validators
        .ready()
        .map_or_else(dash, |keys| grouped(keys.len() as u64));
    let accounts = view
        .accounts
        .ready()
        .map_or_else(dash, |accounts| grouped(accounts.list.len() as u64));
    let pair = || {
        div()
            .flex()
            .flex_1()
            .min_w(STAT_MIN_W * 2.)
            .border_b_1()
            .border_color(theme.border)
    };
    div()
        .id("explorer-stats")
        .flex()
        .flex_wrap()
        .child(
            pair()
                .child(stat(
                    "explorer-stat-height",
                    "Height",
                    height,
                    cadence,
                    theme,
                ))
                .child(stat("explorer-stat-epoch", "Epoch", epoch, next, theme)),
        )
        .child(
            pair()
                .child(stat(
                    "explorer-stat-validators",
                    "Validators",
                    validators,
                    String::new(),
                    theme,
                ))
                .child(stat(
                    "explorer-stat-accounts",
                    "Accounts",
                    accounts,
                    String::new(),
                    theme,
                )),
        )
}

fn stat(
    id: &'static str,
    label: &'static str,
    value: String,
    note: String,
    theme: &Theme,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_w(STAT_MIN_W)
        .flex()
        .flex_col()
        .gap_1()
        .px_5()
        .py_4()
        .border_r_1()
        .border_color(theme.border)
        .child(
            mono(label)
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        )
        .child(div().text_size(design::text::TITLE).child(value))
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(note),
        )
}

fn latest_blocks(view: &Explorer, side: bool, cx: Cx, theme: &Theme) -> impl IntoElement {
    let all = link(
        "explorer-all-blocks".into(),
        words::ALL_BLOCKS.into(),
        Route::Blocks,
        cx,
        theme,
    )
    .text_size(design::text::SECONDARY)
    .into_any_element();
    let list = rows("explorer-latest-blocks-list", "Latest blocks", view);
    let blocks = block_lines(list, view, LATEST).whole(view, cx, theme);
    div()
        .id("explorer-latest-blocks")
        .when(side, |panel| {
            panel
                .flex_1()
                .min_w(PANEL_MIN_W)
                .max_w(LATEST_BLOCKS_W)
                .border_r_1()
                .border_color(theme.border)
        })
        .child(heading(
            "explorer-latest-blocks-heading",
            "Latest activity",
            Some(all),
            theme,
        ))
        .child(blocks.build(cx))
}

fn latest_txs(view: &Explorer, side: bool, cx: Cx, theme: &Theme) -> impl IntoElement {
    let all = link(
        "explorer-all-txs".into(),
        words::ALL_TRANSACTIONS.into(),
        Route::Transactions(None),
        cx,
        theme,
    )
    .text_size(design::text::SECONDARY)
    .into_any_element();
    let list = rows("explorer-latest-txs-list", "Latest transactions", view);
    let txs = latest(view, list, cx, theme);
    div()
        .id("explorer-latest-txs")
        .when(side, |panel| panel.flex_1().min_w(TXS_MIN_W))
        .child(heading(
            "explorer-latest-txs-heading",
            "Latest transactions",
            Some(all),
            theme,
        ))
        .child(txs)
}

// The newest of the window's transactions.
fn latest(view: &Explorer, list: Rows, cx: Cx, theme: &Theme) -> AnyElement {
    if view.chain.transactions() == 0 {
        let blocks = view.chain.blocks.len() as u64;
        return quiet("explorer-no-txs", words::no_transactions_in(blocks), theme);
    }
    tx_rows(list, view, |view| {
        (0..LATEST)
            .filter_map(|index| view.shown_at(&TxList::Window, index))
            .collect()
    })
    .whole(view, cx, theme)
    .build(cx)
    .into_any_element()
}
