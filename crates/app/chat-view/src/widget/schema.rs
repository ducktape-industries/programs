//! A widget file held to its program's schema before anything is asked: the query fits the
//! program's query, `rows` and a cursor's `next` are paths of its reply, the cursor one of the query.
use borsh_json::Interface;
use serde_json::Value;

use super::{Widget, next_beside, put};
use crate::words;

const CURSOR: &str = "{cursor}";

fn stands_for_any(value: &Value) -> bool {
    let text = value.as_str().unwrap_or_default();
    let filled_for_the_reader = text.contains("{me}") || text.contains("{room}");
    filled_for_the_reader || text == CURSOR
}

pub(crate) fn check(widget: &Widget, interface: &Interface) -> Result<(), String> {
    let source = &widget.source;
    interface
        .query_fits(&source.query, stands_for_any)
        .map_err(|fault| fault.to_string())?;
    interface
        .reply_at(&source.rows)
        .map_err(|fault| words::widget_rows_misfit(&fault.to_string()))?;
    let Some(cursor) = &source.cursor else {
        return Ok(());
    };
    let mut query = source.query.clone();
    put(&mut query, cursor, Value::from(CURSOR))
        .map_err(|why| words::widget_cursor_misfit(&why))?;
    interface
        .query_fits(&query, stands_for_any)
        .map_err(|fault| words::widget_cursor_misfit(&fault.to_string()))?;
    let next = next_beside(&source.rows);
    interface
        .reply_at(&next)
        .map_err(|_| words::widget_no_next(&next))?;
    Ok(())
}
