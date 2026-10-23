//! Many transactions through the explorer's host-driven lists: a block's, read a range at a time,
//! and a program's, read a page at a time by its cursor.
use super::*;
use view::methods::{TxPlace, TxRange};
use view::wire::Node;

type Held = Rc<RefCell<Vec<Block>>>;

fn many_blocks() -> Vec<Block> {
    let mut blocks = chain(12);
    let txs = (0..WINDOW)
        .map(|index| {
            let mut row = tx(0, ADA, "chat", post("design", "many transactions"));
            row.hash[..8].copy_from_slice(&(index as u64).to_le_bytes());
            row
        })
        .collect();
    blocks.last_mut().unwrap().txs = txs;
    blocks
}

fn served(page: u32) -> (TestAppContext, Held, Rc<RefCell<u64>>, Asked) {
    let mut cx = TestAppContext::new();
    let tip = Rc::new(RefCell::new(12));
    node(&mut cx, tip.clone());
    let held: Held = Rc::new(RefCell::new(many_blocks()));
    let indexed = held.clone();
    index(&cx, move || indexed.borrow().clone());
    let facts = held.clone();
    serve(&cx, move || facts.borrow().clone());
    let answered = held.clone();
    let asked = answer_once(&cx, move || answered.borrow().clone(), page);
    (cx, held, tip, asked)
}

fn many_transactions(touch: bool, page: u32) -> (TestAppContext, Asked) {
    let (mut cx, _, _, asked) = served(page);
    cx.open::<Explorer>();
    cx.run_until_parked();
    cx.simulate_touch(touch);
    cx.run_until_parked();
    cx.simulate_click("explorer-block-12");
    cx.run_until_parked();
    (cx, asked)
}

fn chat_transactions(cx: &mut TestAppContext) {
    cx.open::<Explorer>();
    cx.run_until_parked();
    for key in [
        "explorer-tab-programs",
        "explorer-program-chat",
        "explorer-program-txs",
    ] {
        cx.simulate_click(key);
        cx.run_until_parked();
    }
}

fn to_chat(cx: &TestAppContext) -> Vec<TxRange> {
    let asked = cx.host().requests::<ChainTxs>();
    asked
        .into_iter()
        .filter(|range| range.target.as_deref() == Some("chat"))
        .collect()
}

fn rows_of(held: &Held, asked: &[TxRange]) -> usize {
    let held = held.borrow();
    let pages = asked.iter().map(|range| view::rig::tx_page(&held, range));
    pages.map(|page| page.txs.len()).sum()
}

fn says_read(cx: &TestAppContext, rows: usize) {
    let caption = format!("{rows} transactions and more");
    let texts = cx.texts();
    assert!(
        texts.iter().any(|text| text.starts_with(&caption)),
        "{caption:?} in {texts:?}"
    );
}

fn only_visible_transaction_rows(touch: bool) {
    let (mut cx, _) = many_transactions(touch, u32::MAX);
    assert!(cx.find("explorer-block").is_some());
    let Some(Node::UniformList {
        count, children, ..
    }) = cx.find("explorer-block-txs-rows")
    else {
        panic!("a many-transaction block must build rows lazily on both designs");
    };
    assert_eq!(*count, WINDOW);
    assert!(children.len() < *count, "only visible rows are lowered");
    let end = WINDOW;
    cx.simulate_range("explorer-block-txs", end - end / 10..end);
    cx.run_until_parked();
    assert!(
        cx.find("explorer-tx-12-0").is_some(),
        "the last range is reachable"
    );
    assert!(cx.find(&format!("explorer-tx-12-{}", end / 2)).is_none());
    let Some(Node::UniformList { children, .. }) = cx.find("explorer-block-txs-rows") else {
        panic!("the rows are still a list");
    };
    let lowered = children.len();
    let asked = cx.host().requests::<ChainTxs>();
    assert!(
        asked.iter().all(|range| range.limit as usize <= lowered),
        "each read asks no more than the {lowered} rows the host lowers: {asked:?}"
    );
    assert!(cx.host().requests::<ChainBlock>().is_empty());
    assert!(cx.host().requests::<ChainBlocks>().is_empty());
    cx.simulate_click("explorer-tx-12-0");
    cx.run_until_parked();
    assert!(
        cx.find("explorer-tx").is_some(),
        "a range row opens its transaction"
    );
}

#[test]
fn a_many_transaction_block_builds_only_visible_rows_on_desktop() {
    only_visible_transaction_rows(false);
}

#[test]
fn a_many_transaction_block_builds_only_visible_rows_on_phone() {
    only_visible_transaction_rows(true);
}

fn short_pages_are_followed(touch: bool) {
    let (cx, _) = many_transactions(touch, 2);
    let Some(Node::UniformList { children, .. }) = cx.find("explorer-block-txs-rows") else {
        panic!("a many-transaction block's rows are a list");
    };
    let lowered = children.len();
    assert!(lowered > 2, "the range spans more than one page: {lowered}");
    let reading: Vec<usize> = (0..lowered)
        .filter(|index| cx.find(&format!("explorer-tx-reading-{index}")).is_some())
        .collect();
    assert!(
        reading.is_empty(),
        "a node answering 2 rows a page still fills all {lowered} rows: {reading:?} are still reading"
    );
    let second = Some(TxPlace {
        height: 12,
        position: (WINDOW - 2) as u32,
    });
    let asked = cx.host().requests::<ChainTxs>();
    assert!(
        asked.iter().any(|range| range.before == second),
        "the second page starts where the first ended: {asked:?}"
    );
}

#[test]
fn a_short_page_is_followed_until_the_range_is_filled_on_desktop() {
    short_pages_are_followed(false);
}

#[test]
fn a_short_page_is_followed_until_the_range_is_filled_on_phone() {
    short_pages_are_followed(true);
}

fn scrolled_out_rows_are_dropped(touch: bool) {
    let (mut cx, asked) = many_transactions(touch, u32::MAX);
    let Some(Node::UniformList { children, .. }) = cx.find("explorer-block-txs-rows") else {
        panic!("a many-transaction block's rows are a list");
    };
    let first = 0..children.len();
    let top = Some(TxPlace {
        height: 12,
        position: WINDOW as u32,
    });
    let from_the_top = |cx: &TestAppContext| {
        let asked = cx.host().requests::<ChainTxs>();
        asked.iter().filter(|range| range.before == top).count()
    };
    let opened = from_the_top(&cx);
    assert!(opened > 0, "the block's first rows are read from its top");
    cx.simulate_range("explorer-block-txs", WINDOW - WINDOW / 10..WINDOW);
    cx.run_until_parked();
    asked.borrow_mut().clear();
    cx.simulate_range("explorer-block-txs", 0..first.end / 2);
    cx.run_until_parked();
    let Some(Node::UniformList { children, .. }) = cx.find("explorer-block-txs-rows") else {
        panic!("the rows are still a list");
    };
    assert!(
        children.len() <= first.end,
        "the rows lowered on the way back were all read when the block opened: {} of {first:?}",
        children.len()
    );
    assert!(
        from_the_top(&cx) > opened,
        "the rows {first:?} scrolled out of view are dropped, so scrolling back reads them again"
    );
    assert!(cx.find(&format!("explorer-tx-12-{}", WINDOW - 1)).is_some());
}

#[test]
fn rows_scrolled_out_of_a_many_transaction_block_are_dropped_on_desktop() {
    scrolled_out_rows_are_dropped(false);
}

#[test]
fn rows_scrolled_out_of_a_many_transaction_block_are_dropped_on_phone() {
    scrolled_out_rows_are_dropped(true);
}

#[test]
fn a_programs_list_reads_its_next_page_from_its_cursor() {
    let (mut cx, held, _, _) = served(u32::MAX);
    chat_transactions(&mut cx);
    let first = to_chat(&cx);
    let read = rows_of(&held, &first);
    assert!(read > 0, "the first page is read: {first:?}");
    says_read(&cx, read);
    cx.simulate_range("explorer-transactions", read - 1..read + 1);
    cx.run_until_parked();
    let asked = to_chat(&cx);
    let Some(next) = asked.get(first.len()) else {
        panic!("the rows past the {read} read ask the next page: {asked:?}");
    };
    let cursor = {
        let held = held.borrow();
        view::rig::tx_page(&held, first.last().unwrap()).next
    };
    assert!(cursor.is_some(), "the first page ends with a cursor");
    assert_eq!(
        next.before, cursor,
        "the next page starts where the last ended"
    );
    says_read(&cx, read + rows_of(&held, &asked[first.len()..]));
}

#[test]
fn a_page_asked_before_a_programs_list_restarts_at_a_new_head_is_dropped() {
    let (mut cx, held, tip, _) = served(u32::MAX);
    let heads = cx.host().stream::<ChainHeads>();
    chat_transactions(&mut cx);
    let read = rows_of(&held, &to_chat(&cx));
    cx.host().never::<ChainTxs>();
    cx.simulate_range("explorer-transactions", 0..read + 1);
    let stale = to_chat(&cx).len() - 1;
    assert!(
        to_chat(&cx)[stale].before.is_some(),
        "the next page is in flight: {:?}",
        to_chat(&cx)
    );
    let answered = held.clone();
    answer_once(&cx, move || answered.borrow().clone(), u32::MAX);
    let mut head = chain(13).pop().unwrap();
    head.txs = vec![tx(0xd4, ADA, "chat", post("design", "a new head"))];
    held.borrow_mut().push(head);
    *tip.borrow_mut() = 13;
    heads.send(Head {
        height: 13,
        time: T0 + 13_000,
        id: [113; 32],
    });
    cx.run_until_parked();
    cx.host().answer_held::<ChainTxs>();
    cx.run_until_parked();
    let asked = to_chat(&cx);
    let Some(restart) = asked.iter().rposition(|range| range.before.is_none()) else {
        panic!("a new head reads the list again from its top: {asked:?}");
    };
    assert!(restart > stale, "{asked:?}");
    says_read(&cx, rows_of(&held, &asked[restart..]));
}
