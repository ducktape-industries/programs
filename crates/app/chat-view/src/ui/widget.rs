//! A widget in its place: the line under the room's title, the pinned card over the messages and
//! the side pane on the desktop; the same line and card in the phone's room, the side widget as a
//! section of its details. Reads the widgets the open room names; writes the fold and the pane.
mod sections;

use serde_json::Value;
use view::design::Ladder;
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::widget::{Kind, Scope, Shape, Shown, Slot, fill, holds};
use crate::{Chat, words};

fn scope(chat: &Chat, count: Option<usize>) -> Scope<'_> {
    Scope {
        me: chat.my_account(),
        names: chat.names.ready(),
        count,
        now: chat.widgets.now,
    }
}

fn key(slot: Slot, part: impl std::fmt::Display) -> String {
    format!("chat-widget-{}-{part}", slot.name())
}

pub(crate) fn line(chat: &Chat, theme: &Theme, ladder: &Ladder) -> Option<AnyElement> {
    let shown = chat.widget(Slot::Header)?;
    let said = match shown {
        Shown::Loading => words::WIDGET_LOADING.to_owned(),
        Shown::Failed { why, .. } => why.clone(),
        Shown::Ready {
            widget,
            reply,
            rows,
        } => summary(chat, &widget.view, reply, rows),
    };
    Some(
        div()
            .id(key(Slot::Header, "line"))
            .text_type(ladder.secondary)
            .text_color(theme.muted)
            .truncate()
            .child(format!("{}: {said}", shown.title()))
            .into_any_element(),
    )
}

fn summary(chat: &Chat, shape: &Shape, reply: &Value, rows: &[Value]) -> String {
    let rows = shape.rows(rows);
    let said = match shape.kind {
        Kind::Stat => figure(chat, shape, reply, rows.len()),
        Kind::Columns => shape
            .columns(&rows)
            .iter()
            .map(|(value, items)| format!("{value} {}", items.len()))
            .collect::<Vec<_>>()
            .join(" · "),
        Kind::Table => design::plural(rows.len() as u64, "row", "rows"),
        Kind::List if shape.group_by.is_some() => shape
            .sections(&rows)
            .iter()
            .map(|(value, items)| match shape.icons.get(value) {
                Some(icon) => format!("{icon} {value} {}", items.len()),
                None => format!("{value} {}", items.len()),
            })
            .collect::<Vec<_>>()
            .join(" · "),
        Kind::List | Kind::Checklist => {
            let (item, scope) = (shape.item.as_deref().unwrap_or_default(), scope(chat, None));
            let items: Vec<String> = rows.iter().map(|row| fill(item, row, &scope)).collect();
            items.join(" · ")
        }
    };
    match said.trim().is_empty() {
        true => shape.nothing(),
        false => said,
    }
}

fn figure(chat: &Chat, shape: &Shape, reply: &Value, count: usize) -> String {
    let scope = scope(chat, Some(count));
    let value = fill(shape.value.as_deref().unwrap_or_default(), reply, &scope);
    match &shape.label {
        Some(label) => format!("{value} {}", fill(label, reply, &scope)),
        None => value,
    }
}

pub(crate) fn pinned(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let shown = chat.widget(Slot::Pinned)?;
    let room = &chat.room.as_ref()?.id;
    let open = !chat.widgets.folded.contains(room);
    let fold = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.fold_widget();
        cx.notify();
    });
    let head = div()
        .id(key(Slot::Pinned, "fold"))
        .flex()
        .items_center()
        .gap_2()
        .role(Role::Button)
        .focusable()
        .cursor_pointer()
        .aria_expanded(open)
        .on_click(fold)
        .child(if open { "▾" } else { "▸" })
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(shown.title().to_owned()),
        );
    let body = open.then(|| {
        div()
            .id(key(Slot::Pinned, "body"))
            .max_h(px(chat.layout.viewport.1 * 0.4))
            .overflow_y_scroll()
            .child(body(chat, Slot::Pinned, cx, theme, ladder))
    });
    Some(
        div()
            .id(key(Slot::Pinned, "card"))
            .mx_3()
            .my_2()
            .p_2()
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .text_type(ladder.body)
            .flex()
            .flex_col()
            .gap_2()
            .child(head)
            .children(body)
            .into_any_element(),
    )
}

pub(crate) fn side_button(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    chat.widget_path(Slot::Side)?;
    let title = chat.widget(Slot::Side).map_or(words::WIDGET, Shown::title);
    let toggle = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_widget_side();
        cx.notify();
    });
    let button = design::button("chat-room-widget", title.to_owned(), ladder, theme, toggle);
    Some(button.into_any_element())
}

pub(crate) fn side(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let close = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.toggle_widget_side();
        cx.notify();
    });
    let title = chat.widget(Slot::Side).map_or(words::WIDGET, Shown::title);
    div()
        .id(key(Slot::Side, "pane"))
        .w(px(chat.layout.clamped().details))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .p_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .id(key(Slot::Side, "title"))
                        .flex_1()
                        .text_size(design::text::SECTION)
                        .font_weight(FontWeight::SEMIBOLD)
                        .role(Role::Heading)
                        .aria_level(2)
                        .child(title.to_owned()),
                )
                .child(design::button(
                    key(Slot::Side, "close"),
                    words::close_widget(title),
                    ladder,
                    theme,
                    close,
                )),
        )
        .child(
            div()
                .id(key(Slot::Side, "scroll"))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .p_3()
                .child(body(chat, Slot::Side, cx, theme, ladder)),
        )
}

pub(crate) fn section(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let shown = chat.widget(Slot::Side)?;
    Some(
        div()
            .id(key(Slot::Side, "section"))
            .flex()
            .flex_col()
            .gap(design::space::XS)
            .p(design::space::BLOCK)
            .child(
                div()
                    .id(key(Slot::Side, "title"))
                    .text_type(ladder.heading)
                    .font_weight(FontWeight::MEDIUM)
                    .role(Role::Heading)
                    .aria_level(2)
                    .child(shown.title().to_owned()),
            )
            .child(body(chat, Slot::Side, cx, theme, ladder))
            .into_any_element(),
    )
}

pub(crate) fn body(
    chat: &Chat,
    slot: Slot,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let quiet = |part: &str, said: String, color: Hsla| {
        div()
            .id(key(slot, part))
            .text_type(ladder.secondary)
            .text_color(color)
            .child(said)
            .into_any_element()
    };
    let Some(shown) = chat.widget(slot) else {
        return div().into_any_element();
    };
    let (widget, reply, rows) = match shown {
        Shown::Loading => return quiet("loading", words::WIDGET_LOADING.into(), theme.muted),
        Shown::Failed { why, .. } => return quiet("failed", why.clone(), theme.danger),
        Shown::Ready {
            widget,
            reply,
            rows,
        } => (widget, reply, rows),
    };
    let shape = &widget.view;
    let rows = shape.rows(rows);
    if rows.is_empty() && !matches!(shape.kind, Kind::Stat | Kind::Columns) {
        return quiet("empty", shape.nothing(), theme.muted);
    }
    let draw = Draw {
        chat,
        slot,
        path: chat.widget_path(slot).unwrap_or_default(),
        shape,
        theme,
        ladder,
    };
    match shape.kind {
        Kind::List if shape.in_parts() => draw.parted(&rows, cx),
        Kind::List => draw.list(&rows, cx),
        Kind::Checklist => draw.checklist(&rows, cx),
        Kind::Columns => draw.columns(&rows, cx),
        Kind::Table => draw.table(&widget.title, &rows, cx),
        Kind::Stat => draw.stat(reply, rows.len()),
    }
}

struct Draw<'a> {
    chat: &'a Chat,
    slot: Slot,
    // the file the widget is read from: what the reader's folds are kept by
    path: &'a str,
    shape: &'a Shape,
    theme: &'a Theme,
    ladder: &'a Ladder,
}

impl Draw<'_> {
    fn item(&self, row: &Value) -> String {
        let item = self.shape.item.as_deref().unwrap_or_default();
        fill(item, row, &scope(self.chat, None))
    }

    fn entry(
        &self,
        part: String,
        said: String,
        label: Option<String>,
        row: &Value,
        cx: &mut Context<Chat>,
    ) -> AnyElement {
        let theme = self.theme;
        let entry = div()
            .id(key(self.slot, part))
            .px_1()
            .py(design::space::XXS)
            .child(said);
        let Some(target) = self.shape.target(row, &scope(self.chat, None)) else {
            return entry.into_any_element();
        };
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            chat.open_widget_target(target.clone(), cx);
        });
        let entry = entry
            .role(Role::Link)
            .focusable()
            .cursor_pointer()
            .hover(|style| style.bg(theme.surface_raised))
            .on_click(open);
        match label {
            Some(label) => entry.aria_label(label).into_any_element(),
            None => entry.into_any_element(),
        }
    }

    fn list(&self, rows: &[&Value], cx: &mut Context<Chat>) -> AnyElement {
        let entries: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(at, row)| self.entry(at.to_string(), self.item(row), None, row, cx))
            .collect();
        div().flex().flex_col().children(entries).into_any_element()
    }

    fn checklist(&self, rows: &[&Value], cx: &mut Context<Chat>) -> AnyElement {
        let entries: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(at, row)| {
                let done = self.shape.checked.as_ref().is_some_and(|f| holds(f, row));
                let item = self.item(row);
                let said = format!("{} {item}", if done { "☑" } else { "☐" });
                let label = words::checked(&item, done);
                self.entry(at.to_string(), said, Some(label), row, cx)
            })
            .collect();
        div().flex().flex_col().children(entries).into_any_element()
    }

    fn columns(&self, rows: &[&Value], cx: &mut Context<Chat>) -> AnyElement {
        let theme = self.theme;
        let mut columns = Vec::new();
        for (index, (value, items)) in self.shape.columns(rows).into_iter().enumerate() {
            let entries: Vec<AnyElement> = items
                .iter()
                .enumerate()
                .map(|(at, row)| self.entry(format!("{index}-{at}"), self.item(row), None, row, cx))
                .collect();
            let head = div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("{value} · {}", items.len()));
            columns.push(
                div()
                    .id(key(self.slot, format!("column-{index}")))
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .w(px(COLUMN_W))
                    .gap_1()
                    .p_2()
                    .bg(theme.surface_raised)
                    .child(head)
                    .children(entries),
            );
        }
        div()
            .id(key(self.slot, "columns"))
            .flex()
            .gap_2()
            .overflow_x_scroll()
            .children(columns)
            .into_any_element()
    }

    fn table(&self, title: &str, rows: &[&Value], cx: &mut Context<Chat>) -> AnyElement {
        let scope = scope(self.chat, None);
        let columns = &self.shape.columns;
        let cell = |id: String, role: Role, child: AnyElement| {
            div()
                .id(id)
                .flex_1()
                .min_w(px(0.))
                .px_1()
                .role(role)
                .child(child)
        };
        let header = columns.iter().enumerate().map(|(index, column)| {
            let title = div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(column.title.clone());
            cell(
                key(self.slot, format!("head-{index}")),
                Role::ColumnHeader,
                title.into_any_element(),
            )
        });
        let header = div()
            .id(key(self.slot, "head"))
            .flex()
            .role(Role::Row)
            .children(header);
        let mut body = Vec::new();
        for (at, row) in rows.iter().enumerate() {
            let mut cells = Vec::new();
            for (index, column) in columns.iter().enumerate() {
                let said = fill(&column.value, row, &scope);
                let child = match index {
                    0 => self.entry(format!("{at}-link"), said, None, row, cx),
                    _ => div().child(said).into_any_element(),
                };
                cells.push(cell(
                    key(self.slot, format!("{at}-{index}")),
                    Role::Cell,
                    child,
                ));
            }
            body.push(
                div()
                    .id(key(self.slot, format!("row-{at}")))
                    .flex()
                    .role(Role::Row)
                    .children(cells),
            );
        }
        div()
            .id(key(self.slot, "table"))
            .flex()
            .flex_col()
            .role(Role::Table)
            .aria_label(title.to_owned())
            .child(header)
            .children(body)
            .into_any_element()
    }

    fn stat(&self, reply: &Value, count: usize) -> AnyElement {
        let scope = scope(self.chat, Some(count));
        let value = fill(
            self.shape.value.as_deref().unwrap_or_default(),
            reply,
            &scope,
        );
        let label = self.shape.label.as_ref().map(|label| {
            div()
                .text_type(self.ladder.secondary)
                .text_color(self.theme.muted)
                .child(fill(label, reply, &scope))
        });
        div()
            .id(key(self.slot, "stat"))
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(design::text::TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(value),
            )
            .children(label)
            .into_any_element()
    }
}

const COLUMN_W: f32 = 200.;
