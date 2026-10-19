//! The Transactions tab and one transaction.
use super::*;
use crate::decode::{amount, clip, preview};
use view::design::Ladder;

/// Every transaction, newest first, or those sent to `program`: the rows
/// read so far, and a line that reads on past them as it comes into view.
/// Activity lists no transaction by program, so the program's are the
/// rows held that went to it, its pages read on until they fill the list,
/// activity ends or `load::LOOK_BACK` more rows are
/// read, when Look further back reads on; the caption says how far back
/// that looked. Rows written above a listing read on past its first page
/// show as Newer transactions (`Listing::behind`).
pub(super) fn transactions(
    view: &Explorer,
    program: Option<String>,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let frames = match landed(view.frames.as_ref()) {
        Ok(frames) => frames,
        Err(Some(refusal)) => return failed(&design::sentence(&refusal), cx, theme, ladder),
        Err(None) => return quiet("explorer-txs-loading", "Reading transactions…", theme),
    };
    let (matching, held, more) = frames.rows.read(|frames| {
        let matching = sent_to(frames.rows(), program.as_deref()).count();
        (
            matching,
            frames.rows().len(),
            frames.count() > frames.rows().len(),
        )
    });
    // a program's list reads back by itself as far as `reach`, then asks
    let further = program.is_some() && more && held >= view.reach;
    let reading_on = more && !further;
    let list = rows("explorer-transactions-list", "Transactions", view);
    let rows = (matching > 0 || reading_on).then(|| {
        let program = program.clone();
        frame_rows(list, &frames.rows, move |frames| {
            sent_to(frames, program.as_deref()).collect()
        })
        .more(reading_on.then_some(words::READING_OLDER))
        .build(cx)
    });
    let title = match &program {
        Some(program) => words::transactions_to(program),
        None => "Transactions".into(),
    };
    // a count is a total only once activity is read to its end, and only
    // while nothing was written above the rows read
    let count = match (more, &program) {
        _ if frames.behind => None,
        (false, _) => Some(plural(matching as u64, "transaction", "transactions")),
        (true, Some(_)) => Some(words::of_the_newest(matching as u64, held as u64)),
        (true, None) => None,
    };
    let empty = (matching == 0 && !more && !frames.behind)
        .then(|| quiet("explorer-no-txs", "No transactions recorded.", theme));
    let further = further.then(|| {
        let look = |view: &mut Explorer, cx: &mut Context<Explorer>| view.look_further(cx);
        div().px_5().py_2().child(press(
            "explorer-look-further",
            words::LOOK_FURTHER,
            cx,
            theme,
            look,
        ))
    });
    div()
        .id("explorer-transactions")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        // the heading ends where the rows do, short of the gutter the host
        // keeps beside the list, as it does under a page that scrolls
        .child(
            heading(
                "explorer-transactions-heading",
                &title,
                count.map(|count| caption(count, theme)),
                theme,
            )
            .mr(design::size::SCROLLBAR),
        )
        .children(frames.behind.then(|| newer(cx, theme)))
        .children(rows)
        .children(empty)
        .children(further)
        .into_any_element()
}

fn sent_to<'a, 'p>(
    frames: &'a [activity::Frame],
    program: Option<&'p str>,
) -> impl Iterator<Item = &'a activity::Frame> + use<'a, 'p> {
    frames
        .iter()
        .filter(move |frame| program.is_none_or(|program| frame.target == program))
}

pub(super) fn tx(
    view: &Explorer,
    height: u64,
    position: u32,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(tx) = view.tx_at(height, position) else {
        return match &view.located {
            Loadable::Ready(None) | Loadable::Reloading(None, _) => empty_state(
                "explorer-no-tx",
                "search-x",
                words::NO_TRANSACTION,
                words::no_transaction_at(height, position),
                ladder,
                theme,
            )
            .into_any_element(),
            Loadable::Failed(refusal) => failed(&design::sentence(refusal), cx, theme, ladder),
            Loadable::Idle
            | Loadable::Loading(_)
            | Loadable::Ready(Some(_))
            | Loadable::Reloading(Some(_), _) => {
                quiet("explorer-tx-loading", words::READING_TX, theme)
            }
        };
    };
    view.open(tx, cx);
    let now = view.chain.now();
    let code = view
        .runs(&tx.target)
        .map(|entry| short(entry.code.digest()));
    let program = div()
        .flex()
        .gap_2()
        .child(mono(tx.target.clone()))
        .children(code.map(|code| mono(format!("code {code}")).text_color(theme.faint)));
    let title = clip(&tx.title());
    let copy = copy_button(view, &Route::Tx { height, position }, cx, theme);
    div()
        .id("explorer-tx")
        .child(
            div()
                .flex()
                .items_center()
                .border_b_1()
                .border_color(theme.border)
                .child(div().flex_1().child(titled("Transaction", title, theme)))
                .children(copy.map(|copy| div().px_5().child(copy))),
        )
        .child(field(
            "Block",
            link(
                "explorer-tx-block".into(),
                words::in_block(tx.height),
                Route::Block(tx.height),
                cx,
                theme,
            ),
            theme,
        ))
        .children(
            tx.receipt
                .as_ref()
                .map(|run| field("Status", status(run, "explorer-tx", theme, ladder), theme)),
        )
        .child(field("Hash", mono(abi::hex(&tx.hash)), theme))
        .child(field("From", from(view, tx, cx, theme), theme))
        .child(field("Program", program, theme))
        .child(field(
            "Anchor",
            link(
                "explorer-tx-anchor".into(),
                words::block(tx.anchor),
                Route::Block(tx.anchor),
                cx,
                theme,
            ),
            theme,
        ))
        .child(field(
            "Time",
            div()
                .flex()
                .gap_2()
                .child(date(tx.time))
                .child(mono(words::age(now, tx.time)).text_color(theme.faint)),
            theme,
        ))
        .child(div().h(design::space::LG))
        .child(
            div()
                .id("explorer-operation-heading")
                .px_5()
                .py_2()
                .text_size(design::text::SECTION)
                .font_weight(FontWeight::SEMIBOLD)
                .role(Role::Heading)
                .aria_level(2)
                .child("Operation"),
        )
        .child(operation(view, tx, cx, theme))
        .children(
            tx.receipt
                .as_ref()
                .and_then(|run| messages(run, theme, ladder)),
        )
        .into_any_element()
}

/// A run's outcome as a badge and, where it was rejected, the refusal's
/// sentence and reason. `id` keeps the badge's id its own.
fn status(run: &Receipt, id: &str, theme: &Theme, ladder: &Ladder) -> Div {
    let (word, color, soft) = outcome(run, theme);
    let refusal = match &run.outcome {
        Outcome::Applied { .. } => None,
        Outcome::Rejected(refusal) => Some(refusal),
    };
    let refusal = refusal.map(|refusal| {
        div()
            .flex()
            .gap_2()
            .min_w_0()
            .child(div().truncate().child(refusal.message.clone()))
            .child(mono(refusal.code.clone()).text_color(theme.faint))
    });
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .child(design::badge(
            format!("{id}-status"),
            word,
            ladder,
            color,
            soft,
        ))
        .children(refusal)
}

/// The runs a transaction's messages caused, each under the run that sent
/// it; `None` where it sent none.
fn messages(run: &Receipt, theme: &Theme, ladder: &Ladder) -> Option<impl IntoElement> {
    if run.nested.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    nested(&run.nested, 0, "explorer-run", theme, ladder, &mut rows);
    Some(
        div()
            .child(
                div()
                    .id("explorer-messages-heading")
                    .px_5()
                    .py_2()
                    .text_size(design::text::SECTION)
                    .font_weight(FontWeight::SEMIBOLD)
                    .role(Role::Heading)
                    .aria_level(2)
                    .child("Messages"),
            )
            .child(div().id("explorer-messages").mx_5().mb_4().children(rows)),
    )
}

/// `runs` and theirs below them, one row each, stepped in by `depth`.
fn nested(
    runs: &[Receipt],
    depth: usize,
    at: &str,
    theme: &Theme,
    ladder: &Ladder,
    rows: &mut Vec<AnyElement>,
) {
    for (index, run) in runs.iter().enumerate() {
        let id = format!("{at}-{index}");
        rows.push(
            div()
                .flex()
                .items_center()
                .gap_4()
                .min_h(ROW_H)
                .pl(NEST_W * depth as f32)
                .border_b_1()
                .border_color(theme.border)
                .child(mono(run.program.clone()).flex_shrink_0())
                .child(status(run, &id, theme, ladder))
                .into_any_element(),
        );
        nested(&run.nested, depth + 1, &id, theme, ladder, rows);
    }
}

/// Who signed: the account holding the key and which of its keys, or the
/// key alone.
fn from(view: &Explorer, tx: &TxRow, cx: Cx, theme: &Theme) -> Div {
    let Some((account, at)) = view.holder(&tx.signer) else {
        return div().child(mono(abi::hex(&tx.signer)));
    };
    let picture = view.picture(account, cx);
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(design::avatar_with(
            &account.card.name,
            picture,
            design::size::AVATAR,
            theme,
        ))
        .child(link(
            "explorer-from".into(),
            account.card.name.clone(),
            Route::Account(account.number),
            cx,
            theme,
        ))
        .child(mono(words::key_line(account, at, &tx.signer)).text_color(theme.faint))
}

/// The op's fields, as its program described them; empty until it has.
fn operation(view: &Explorer, tx: &TxRow, cx: Cx, theme: &Theme) -> impl IntoElement {
    let fields: Vec<AnyElement> = tx
        .op()
        .map(|op| op.fields.as_slice())
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, field)| {
            div()
                .flex()
                .items_center()
                .gap_4()
                .child(
                    mono(field.label.clone())
                        .w(OP_LABEL_W)
                        .flex_shrink_0()
                        .text_color(theme.muted),
                )
                .child(div().flex_1().child(value(
                    view,
                    &field.value,
                    &format!("{index}"),
                    cx,
                    theme,
                )))
                .into_any_element()
        })
        .collect();
    div()
        .id("explorer-operation")
        .mx_5()
        .mb_4()
        .p_4()
        .flex()
        .flex_col()
        .gap_1()
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .children(fields)
}

/// One described value, shown as what it is: an account by its name, a
/// key by the account holding it, a program as a link to its
/// transactions, a hash short. `at` keeps each link's id its own.
fn value(view: &Explorer, shown: &Value, at: &str, cx: Cx, theme: &Theme) -> AnyElement {
    match shown {
        Value::Text(text) => div().child(clip(text)).into_any_element(),
        Value::Account(number) => account(view, *number, at, cx, theme),
        Value::Key(key) => match view.holder(key) {
            Some((holder, _)) => account(view, holder.number, at, cx, theme),
            None => mono(short(key)).into_any_element(),
        },
        Value::Program(name) => match view.runs(name) {
            Some(_) => link(
                format!("explorer-value-{at}"),
                name.clone(),
                Route::Program(name.clone()),
                cx,
                theme,
            )
            .into_any_element(),
            None => mono(name.clone()).into_any_element(),
        },
        Value::Hash(bytes) => mono(short(bytes)).into_any_element(),
        Value::Amount { value, decimals } => mono(amount(*value, *decimals)).into_any_element(),
        Value::Time(millis) => mono(date(*millis)).into_any_element(),
        Value::Bytes { len, preview: head } => mono(preview(*len, head)).into_any_element(),
        Value::List(items) => div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_3()
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| value(view, item, &format!("{at}-{index}"), cx, theme)),
            )
            .into_any_element(),
    }
}

/// An account as its avatar and name, a link to its page; its number
/// where the window has no such account.
fn account(view: &Explorer, number: u64, at: &str, cx: Cx, theme: &Theme) -> AnyElement {
    let face = view.account(number).map(|account| {
        let picture = view.picture(account, cx);
        design::avatar_with(&account.card.name, picture, design::size::AVATAR_SM, theme)
    });
    let shown = view.name_of(number);
    div()
        .flex()
        .items_center()
        .gap_2()
        .children(face)
        .child(link(
            format!("explorer-value-{at}"),
            shown,
            Route::Account(number),
            cx,
            theme,
        ))
        .child(mono(format!("#{number}")).text_color(theme.faint))
        .into_any_element()
}
