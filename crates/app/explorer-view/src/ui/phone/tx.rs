//! Expo `app/(tabs)/explorer/tx/[hash].tsx`, board `10-transaction`: one
//! transaction over the root. Where it sits and how it ran, its hash, who
//! sent it to what, its anchor and time; its op in its program's words;
//! the runs its messages caused. Reads the transaction from the window or
//! the one the node was asked for (`located`), the accounts and the
//! programs role; asks its program to describe it (`open`); writes the
//! route (a link).
use view::design::space;
use view::prelude::*;
use view::{Div, Stateful};

use crate::decode::{amount, clip, date, preview, short};
use crate::words;
use crate::{Cx, Explorer, Note, Route, TxRow};

use super::Look;
use super::parts::{
    Tone, copy_link, data, empty, gone, key_value, linked, over, quiet, refused, scroller, section,
    time_and_age, title_of,
};

const OP_KEY_W: Pixels = px(88.);
const NEST_STEP: Pixels = px(16.);

pub(super) fn tx(view: &Explorer, height: u64, position: u32, cx: Cx, k: &Look) -> AnyElement {
    let Some(tx) = view.tx_at(height, position) else {
        return over("explorer-tx", "Transaction", view, cx, k)
            .child(missing(view, height, position, cx, k))
            .into_any_element();
    };
    view.open(tx, cx);
    let copy = copy_link(view, &Route::Tx { height, position }, cx, k);
    let page = over("explorer-tx", "Transaction", view, cx, k).caption(format!(
        "{} · {}",
        tx.target,
        clip(&title_of(tx))
    ));
    let page = match copy {
        Some(copy) => page.trailing(copy),
        None => page,
    };
    page.child(found(view, tx, cx, k)).into_any_element()
}

pub(super) fn by_hash(note: &Note, cx: Cx, k: &Look) -> AnyElement {
    let heading = words::NO_TRANSACTION;
    gone(
        "explorer-tx-unfound",
        "Transaction",
        "search-x",
        heading,
        note,
        cx,
        k,
    )
}

fn missing(view: &Explorer, height: u64, position: u32, cx: Cx, k: &Look) -> AnyElement {
    match &view.located {
        Loadable::Ready(None) | Loadable::Reloading(None, _) => empty(
            "explorer-no-tx",
            "search-x",
            words::NO_TRANSACTION,
            words::no_transaction_at(height, position),
            k,
        ),
        Loadable::Failed(refusal) => refused(&design::sentence(refusal), cx, k),
        Loadable::Idle
        | Loadable::Loading(_)
        | Loadable::Ready(Some(_))
        | Loadable::Reloading(Some(_), _) => quiet("explorer-tx-loading", words::READING_TX, k),
    }
}

fn found(view: &Explorer, tx: &TxRow, cx: Cx, k: &Look) -> AnyElement {
    let block = linked(
        "explorer-tx-block",
        words::in_block(tx.height),
        Route::Block(tx.height),
        cx,
        k,
    );
    let anchor = linked(
        "explorer-tx-anchor",
        words::block(tx.anchor),
        Route::Block(tx.anchor),
        cx,
        k,
    );
    let status = tx.receipt.as_ref().map(|run| {
        key_value(
            "explorer-field-status",
            "Status",
            status(run, "explorer-tx", k),
            k,
        )
    });
    scroller("explorer-page")
        .child(key_value("explorer-field-block", "Block", block, k))
        .children(status)
        .child(key_value(
            "explorer-field-hash",
            "Hash",
            data(abi::hex(&tx.hash)).id("tx-hash"),
            k,
        ))
        .child(key_value(
            "explorer-field-from",
            "From",
            from(view, tx, cx, k),
            k,
        ))
        .child(key_value(
            "explorer-field-program",
            "Program",
            program(view, tx, k),
            k,
        ))
        .child(key_value("explorer-field-anchor", "Anchor", anchor, k))
        .child(key_value(
            "explorer-field-time",
            "Time",
            time_and_age(tx.time, view.chain.now(), k),
            k,
        ))
        .child(section("explorer-operation-heading", "Operation", None, k))
        .child(operation(view, tx, cx, k))
        .children(messages(tx, k))
        .into_any_element()
}

fn program(view: &Explorer, tx: &TxRow, k: &Look) -> Div {
    let code = view.runs(&tx.target).map(|entry| {
        data(format!("code {}", short(entry.code.digest()))).text_color(k.theme.faint)
    });
    div()
        .flex()
        .flex_col()
        .child(data(tx.target.clone()))
        .children(code)
}

fn status(run: &Receipt, id: &str, k: &Look) -> Div {
    let refusal = match &run.outcome {
        Outcome::Applied { .. } => None,
        Outcome::Rejected(refusal) => Some(refusal),
    };
    let tone = Tone::of_run(refusal.is_none());
    let badge = design::badge(
        SharedString::from(format!("{id}-status")),
        activity::view::verdict(refusal.is_none()),
        &k.ladder,
        tone.ink(&k.theme),
        tone.ground(&k.theme),
    );
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap(space::XXS)
        .child(div().w_full().flex().justify_center().child(badge))
        .children(refusal.map(|refusal| div().child(refusal.message.clone())))
        .children(refusal.map(|refusal| {
            data(refusal.code.clone())
                .text_type(k.ladder.caption)
                .text_color(k.theme.faint)
        }))
}

fn messages(tx: &TxRow, k: &Look) -> Option<Div> {
    let run = tx.receipt.as_ref().filter(|run| !run.nested.is_empty())?;
    let mut lines = Vec::new();
    nested(&run.nested, 0, "explorer-run", k, &mut lines);
    Some(
        div()
            .child(section("explorer-messages-heading", "Messages", None, k))
            .child(
                div()
                    .id("explorer-messages")
                    .px(space::BLOCK)
                    .pb(space::LG)
                    .children(lines),
            ),
    )
}

fn nested(runs: &[Receipt], depth: usize, at: &str, k: &Look, lines: &mut Vec<AnyElement>) {
    for (index, run) in runs.iter().enumerate() {
        let id = format!("{at}-{index}");
        lines.push(
            div()
                .flex()
                .items_center()
                .gap(space::SM)
                .min_h(k.ladder.row)
                .pl(NEST_STEP * depth as f32)
                .child(
                    data(run.program.clone())
                        .text_type(k.ladder.small)
                        .flex_none(),
                )
                .child(status(run, &id, k))
                .into_any_element(),
        );
        nested(&run.nested, depth + 1, &id, k, lines);
    }
}

fn from(view: &Explorer, tx: &TxRow, cx: Cx, k: &Look) -> Div {
    let Some((account, at)) = view.holder(&tx.signer) else {
        return data(abi::hex(&tx.signer));
    };
    let which = words::key_line(account, at, &tx.signer);
    let picture = view.picture(account, cx);
    div()
        .flex()
        .items_center()
        .gap(space::SM)
        .child(design::avatar_with(
            &account.card.name,
            picture,
            k.ladder.avatar_inline,
            &k.theme,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .items_start()
                .child(linked(
                    "explorer-from",
                    account.card.name.clone(),
                    Route::Account(account.number),
                    cx,
                    k,
                ))
                .child(
                    data(which)
                        .text_type(k.ladder.caption)
                        .text_color(k.theme.faint),
                ),
        )
}

fn operation(view: &Explorer, tx: &TxRow, cx: Cx, k: &Look) -> Stateful<Div> {
    let fields = tx
        .op()
        .map(|op| op.fields.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            div()
                .flex()
                .gap(space::LG)
                .child(
                    data(field.label.clone())
                        .w(OP_KEY_W)
                        .flex_none()
                        .text_color(k.theme.muted),
                )
                .child(div().flex_1().min_w_0().child(value(
                    view,
                    &field.value,
                    &index.to_string(),
                    cx,
                    k,
                )))
        });
    div()
        .id("explorer-operation")
        .flex_none()
        .mx(space::BLOCK)
        .mb(space::LG)
        .p(space::LG)
        .flex()
        .flex_col()
        .gap(space::SM)
        .bg(k.theme.surface)
        .text_type(k.ladder.small)
        .children(fields)
}

fn value(view: &Explorer, shown: &Value, at: &str, cx: Cx, k: &Look) -> AnyElement {
    match shown {
        Value::Text(text) => div().child(clip(text)).into_any_element(),
        Value::Account(number) => account_value(view, *number, at, cx, k),
        Value::Key(key) => match view.holder(key) {
            Some((holder, _)) => account_value(view, holder.number, at, cx, k),
            None => data(short(key)).into_any_element(),
        },
        Value::Program(name) => match view.runs(name) {
            Some(_) => linked(
                format!("explorer-value-{at}"),
                name.clone(),
                Route::Program(name.clone()),
                cx,
                k,
            )
            .font_family(design::fonts::FAMILY_MONO)
            .into_any_element(),
            None => data(name.clone()).into_any_element(),
        },
        Value::Hash(bytes) => data(short(bytes)).into_any_element(),
        Value::Amount { value, decimals } => data(amount(*value, *decimals)).into_any_element(),
        Value::Time(millis) => data(date(*millis)).into_any_element(),
        Value::Bytes { len, preview: head } => data(preview(*len, head)).into_any_element(),
        Value::List(items) => div()
            .flex()
            .flex_wrap()
            .gap(space::MD)
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| value(view, item, &format!("{at}-{index}"), cx, k)),
            )
            .into_any_element(),
    }
}

fn account_value(view: &Explorer, number: u64, at: &str, cx: Cx, k: &Look) -> AnyElement {
    let face = view.account(number).map(|account| {
        let picture = view.picture(account, cx);
        design::avatar_with(
            &account.card.name,
            picture,
            k.ladder.avatar_inline,
            &k.theme,
        )
    });
    let shown = view.name_of(number);
    div()
        .flex()
        .items_center()
        .gap(space::XS)
        .children(face)
        .child(linked(
            format!("explorer-value-{at}"),
            shown,
            Route::Account(number),
            cx,
            k,
        ))
        .child(data(format!("#{number}")).text_color(k.theme.faint))
        .into_any_element()
}
