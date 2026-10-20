//! The style table of the tree being generated: a generator hands a node
//! the id of a hostile style and the entry waits here for the frame, or
//! for the patch frame after it, to carry.
use super::*;
use std::cell::RefCell;

struct Table {
    next: u32,
    waiting: Vec<Style>,
}

thread_local! {
    static TABLE: RefCell<Table> = const {
        RefCell::new(Table {
            next: 0,
            waiting: Vec::new(),
        })
    };
}

pub(super) fn start_table() {
    TABLE.with_borrow_mut(|table| {
        *table = Table {
            next: 1,
            waiting: plain(),
        }
    });
}

pub(super) fn gen_style(rng: &mut Rng) -> StyleId {
    let style = gen_native_style(rng);
    TABLE.with_borrow_mut(|table| {
        table.waiting.push(Style::new(&style));
        table.next += 1;
        StyleId(table.next - 1)
    })
}

pub(super) fn take_styles() -> Vec<Style> {
    TABLE.with_borrow_mut(|table| std::mem::take(&mut table.waiting))
}

pub(super) fn sanitized(frame: &mut Frame) -> Result<Styles, Refused> {
    let mut styles = Styles::default();
    view_wire::sanitize(frame, &mut styles).map_err(|refused| refused.reason().clone())?;
    Ok(styles)
}
