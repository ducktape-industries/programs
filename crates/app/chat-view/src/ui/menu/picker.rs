//! The reaction picker: a search field that takes the keys, the reader's
//! frequent row, one tab of emoji at a time, or the search's matches.
use view::prelude::*;
use view::{Div, Stateful};

use super::{
    CAPTION, CELL, COLUMNS, GRID_ROWS, PICKER_GAP, PICKER_INSET, Press, SEARCH, STACK_GAP, TABS,
    focus_key,
};
use crate::words;
use crate::{Chat, Menu, Mode, emoji};
use view::design::Ladder;

pub(super) fn reactions(
    chat: &Chat,
    menu: &Menu,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let picker = div()
        .id("chat-reaction-picker")
        .size_full()
        .flex()
        .flex_col()
        .gap(px(STACK_GAP))
        .p(px(PICKER_INSET))
        .child(search_field(chat, menu, cx, theme));
    let body = match chat.picker.query.is_blank() {
        true => browse(chat, menu.seq, cx, theme, ladder),
        false => matches(chat, menu.seq, cx, theme),
    };
    picker.children(body).into_any_element()
}

fn pick(chat: &Chat, seq: u64, emoji: &str, cx: &mut Context<Chat>) -> Option<Press> {
    let emoji = emoji.to_owned();
    chat.may_write().then(|| {
        Box::new(cx.listener(move |chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.react(seq, emoji.clone(), true, cx)
        })) as Press
    })
}

fn search_field(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> Input {
    let key = focus_key(menu.pane, Mode::Reactions);
    let search = Input::new(key, &chat.picker.query, "Find an emoji to react with")
        .h(px(SEARCH))
        .w_full()
        .px_2()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .placeholder(words::SEARCH_EMOJI);
    let first = emoji::search(&chat.picker.query.text()).first().copied();
    match first.filter(|_| chat.may_write()) {
        Some(first) => {
            let seq = menu.seq;
            search.on_submit(cx.listener(move |chat, _: &(), _, cx| {
                cx.notify();
                chat.react(seq, first.into(), true, cx)
            }))
        }
        None => search,
    }
}

fn browse(
    chat: &Chat,
    seq: u64,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let frequent = emoji::frequent(&chat.recent_emoji);
    let frequent = grid(
        "chat-reaction-frequent",
        words::FREQUENTLY_USED,
        frequent
            .iter()
            .map(|emoji| (format!("chat-reaction-{emoji}"), emoji.clone()))
            .collect(),
        seq,
        chat,
        cx,
        theme,
    );
    let tab = chat.picker.tab.min(emoji::CATEGORIES.len() - 1);
    let category = &emoji::CATEGORIES[tab];
    let cells = grid(
        "chat-reaction-grid",
        category.name,
        category
            .emoji
            .iter()
            .map(|(emoji, _)| {
                (
                    format!("chat-reaction-{}-{emoji}", category.name),
                    (*emoji).to_owned(),
                )
            })
            .collect(),
        seq,
        chat,
        cx,
        theme,
    );
    vec![
        caption(words::FREQUENTLY_USED, theme).into_any_element(),
        frequent.into_any_element(),
        tabs(tab, cx, theme, ladder).into_any_element(),
        caption(category.name, theme).into_any_element(),
        cells.into_any_element(),
    ]
}

fn tabs(chosen: usize, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> impl IntoElement {
    let mut tabs = design::composite("chat-reaction-tabs", Role::TabList, words::EMOJI_CATEGORIES)
        .orientation(design::Orientation::Horizontal)
        .wrap()
        .active(chosen, emoji::CATEGORIES.len())
        .on_move(cx.processor(|chat, index: usize, _, cx| {
            chat.picker.tab = index;
            cx.notify();
        }))
        .build()
        .h(px(TABS))
        .flex()
        .border_b_1()
        .border_color(theme.border);
    for (index, category) in emoji::CATEGORIES.iter().enumerate() {
        let open = cx.listener(move |chat, _: &ClickEvent, _, cx| {
            chat.picker.tab = index;
            cx.notify();
        });
        let id = format!("chat-reaction-tab-{}", category.name);
        tabs = tabs.child(design::item(
            design::tab(id, category.glyph, index == chosen, ladder, theme, open)
                .flex_1()
                .h_full()
                .justify_center()
                .text_size(design::text::SECTION)
                .aria_label(category.name)
                .cursor_pointer(),
            Role::Tab,
            index == chosen,
        ));
    }
    tabs
}

fn matches(chat: &Chat, seq: u64, cx: &mut Context<Chat>, theme: &Theme) -> Vec<AnyElement> {
    let found = emoji::search(&chat.picker.query.text());
    let count = match found.len() {
        0 => words::NO_EMOJI_MATCH.to_owned(),
        n => design::plural(n as u64, "match", "matches"),
    };
    let cells = grid(
        "chat-reaction-results",
        "Matches",
        found
            .iter()
            .map(|emoji| (format!("chat-reaction-{emoji}"), (*emoji).to_owned()))
            .collect(),
        seq,
        chat,
        cx,
        theme,
    );
    let scroll = div()
        .id("chat-reaction-results-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .child(cells);
    vec![
        caption(&count, theme).into_any_element(),
        scroll.into_any_element(),
    ]
}

fn grid(
    id: &'static str,
    label: &str,
    cells: Vec<(String, String)>,
    seq: u64,
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Stateful<Div> {
    let at = match chat.picker.cursor {
        Some((grid, at)) if grid == id => at.min(cells.len().saturating_sub(1)),
        _ => 0,
    };
    let emoji: Vec<String> = cells.iter().map(|(_, emoji)| emoji.clone()).collect();
    let mut grid = design::composite(id, Role::Grid, label.to_owned())
        .grid(COLUMNS as usize)
        .active(at, cells.len())
        .on_move(cx.processor(move |chat, index: usize, _, cx| {
            chat.picker.cursor = Some((id, index));
            cx.notify();
        }))
        .on_press(cx.processor(move |chat, index: usize, _, cx| {
            if chat.may_write() {
                cx.notify();
                chat.react(seq, emoji[index].clone(), true, cx);
            }
        }))
        .build()
        .flex()
        .flex_col()
        .gap(px(PICKER_GAP));
    for (row, cells) in cells.chunks(COLUMNS as usize).enumerate() {
        let mut line = div()
            .id(format!("{id}-row-{row}"))
            .role(Role::Row)
            .flex()
            .gap(px(PICKER_GAP));
        for (column, (cell_id, emoji)) in cells.iter().enumerate() {
            let index = row * COLUMNS as usize + column;
            let press = pick(chat, seq, emoji, cx);
            line = line.child(
                div()
                    .id(format!("{cell_id}-cell"))
                    .role(Role::GridCell)
                    .child(Reaction::new(
                        cell_id.clone(),
                        emoji,
                        press,
                        index == at,
                        theme,
                    )),
            );
        }
        grid = grid.child(line);
    }
    grid
}

fn caption(text: &str, theme: &Theme) -> impl IntoElement {
    div()
        .h(px(CAPTION))
        .text_size(design::text::CAPTION)
        .font_family(design::fonts::FAMILY_MONO)
        .text_color(theme.muted)
        .child(text.to_uppercase())
}

pub(super) fn picker_size() -> (f32, f32) {
    let columns = COLUMNS as f32;
    let grid = GRID_ROWS * CELL + (GRID_ROWS - 1.) * PICKER_GAP;
    (
        PICKER_INSET * 2. + columns * CELL + (columns - 1.) * PICKER_GAP,
        PICKER_INSET * 2. + SEARCH + CAPTION + CELL + TABS + CAPTION + grid + 5. * STACK_GAP,
    )
}

#[derive(IntoElement)]
struct Reaction {
    id: ElementId,
    emoji: String,
    press: Option<Press>,
    active: bool,
    theme: Theme,
}
impl Reaction {
    fn new(
        id: impl Into<ElementId>,
        emoji: &str,
        press: Option<Press>,
        active: bool,
        theme: &Theme,
    ) -> Self {
        Self {
            id: id.into(),
            emoji: emoji.into(),
            press,
            active,
            theme: *theme,
        }
    }
}
impl RenderOnce for Reaction {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let enabled = self.press.is_some();
        let cell = div()
            .id(self.id)
            .w(px(CELL))
            .h(px(CELL))
            .flex()
            .items_center()
            .justify_center()
            .text_size(design::text::TITLE)
            .role(Role::Button)
            .aria_label(words::react_with(&self.emoji))
            .aria_disabled(!enabled)
            .child(self.emoji);
        match self.press {
            Some(press) => design::item(
                cell.cursor_pointer()
                    .hover(|s| s.bg(self.theme.surface_raised))
                    .on_click(press),
                Role::Button,
                self.active,
            )
            .into_any_element(),
            None => cell.opacity(0.4).into_any_element(),
        }
    }
}
