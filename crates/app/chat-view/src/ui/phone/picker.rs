//! The full reaction picker (Expo `src/chat/Actions.tsx` `Picker`, design
//! §4.5, board 42): a sheet with the search field, Frequently used, the
//! eight category tabs and the chosen one's grid; or, while the field
//! holds a search, its results. A pick reacts and closes. Reads the open
//! menu, the picker's query and tab and the reader's recent reactions;
//! writes the query and the tab, and reacts.
use view::Div;
use view::prelude::*;

use crate::words;
use crate::{Chat, Mode, emoji};
use view::FontWeight;
use view::design::Ladder;

pub(super) const CELL: Pixels = px(44.);
const CELL_EMOJI: design::Type = design::Type::lined(24., 30.);
const DETENTS: [f32; 2] = [0.6, 1.0];
const TAB_WIDTH: Pixels = px(36.);
const TAB_GLYPH: design::Type = design::Type::lined(20., 26.);

pub(super) struct Cell {
    pub id: ElementId,
    pub emoji: String,
    pub mine: bool,
}

pub(super) fn sheet(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let menu = chat
        .menu
        .as_ref()
        .filter(|menu| menu.mode == Mode::Reactions)?;
    let close = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_menu();
        cx.notify();
    });
    let query = chat.picker.query.text();
    let body = match query.trim().is_empty() {
        true => browse(chat, menu.seq, cx, theme, ladder),
        false => results(chat, &query, menu.seq, cx, theme, ladder),
    };
    let picker = div()
        .id("chat-reaction-picker")
        .size_full()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(
            div()
                .px(design::space::BLOCK)
                .pb(design::space::SM)
                .child(field(chat, theme, ladder)),
        )
        .children(body);
    Some(
        design::sheet("chat-picker-sheet", words::ADD_REACTION, ladder, theme)
            .detents(DETENTS)
            .on_dismiss(close)
            .child(picker)
            .into_any_element(),
    )
}

fn field(chat: &Chat, theme: &Theme, ladder: &Ladder) -> Div {
    let input = Input::new(
        "chat-reaction-search",
        &chat.picker.query,
        words::SEARCH_EMOJI,
    )
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None)
    .placeholder(words::SEARCH_EMOJI);
    design::field(input, ladder, theme)
        .lead("search")
        .into_element()
}

fn browse(
    chat: &Chat,
    seq: u64,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let tab = chat.picker.tab.min(emoji::CATEGORIES.len() - 1);
    let category = &emoji::CATEGORIES[tab];
    let frequent = emoji::frequent(&chat.recent_emoji);
    let chosen = category
        .emoji
        .iter()
        .take(emoji::PER_TAB)
        .map(|(emoji, _)| (*emoji).to_owned());
    vec![
        caption(words::FREQUENTLY_USED, theme, ladder),
        grid(chat, "frequent", frequent, seq, cx, theme).into_any_element(),
        tabs(tab, cx, theme, ladder),
        caption(category.name, theme, ladder),
        grid(chat, category.name, chosen.collect(), seq, cx, theme)
            .pb(design::space::XL)
            .into_any_element(),
    ]
}

fn results(
    chat: &Chat,
    query: &str,
    seq: u64,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let found: Vec<String> = emoji::search(query)
        .into_iter()
        .map(str::to_owned)
        .collect();
    let count = match found.len() {
        0 => words::NO_EMOJI_MATCH.to_owned(),
        found => format!("Results · {found}"),
    };
    vec![
        caption(&count, theme, ladder),
        grid(chat, "results", found, seq, cx, theme).into_any_element(),
    ]
}

fn caption(text: &str, theme: &Theme, ladder: &Ladder) -> AnyElement {
    div()
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .text_type(ladder.caption)
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.muted)
        .child(text.to_owned())
        .into_any_element()
}

fn grid(
    chat: &Chat,
    section: &str,
    emoji: Vec<String>,
    seq: u64,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Div {
    let room = chat.layout.viewport.0 - 2. * f32::from(design::space::BLOCK);
    let fits = ((room / f32::from(CELL)).floor() as usize).max(2);
    let gap = px((room - fits as f32 * f32::from(CELL)) / (fits - 1) as f32);
    let cells: Vec<AnyElement> = emoji
        .into_iter()
        .map(|emoji| {
            let pick = emoji.clone();
            let press = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                match reacted(chat, seq, &pick) {
                    true => chat.close_menu(),
                    false => chat.react(seq, pick.clone(), true, cx),
                }
            });
            let id = format!("chat-reaction-{section}-{emoji}").into();
            cell(
                Cell {
                    id,
                    emoji,
                    mine: false,
                },
                press,
                theme,
            )
        })
        .collect();
    let mut rows = Vec::new();
    let mut cells = cells.into_iter().peekable();
    while cells.peek().is_some() {
        let row: Vec<AnyElement> = cells.by_ref().take(fits - 1).collect();
        let id = format!("chat-reaction-{section}-row-{}", rows.len());
        rows.push(div().id(id).flex().gap(gap).children(row));
    }
    div()
        .flex()
        .flex_col()
        .gap(design::space::XXS)
        .px(design::space::BLOCK)
        .children(rows)
}

fn reacted(chat: &Chat, seq: u64, emoji: &str) -> bool {
    let Some(menu) = &chat.menu else {
        return false;
    };
    chat.messages(menu.pane)
        .iter()
        .any(|message| message.seq == seq && message.reacted(emoji))
}

fn tabs(chosen: usize, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let tabs: Vec<AnyElement> = emoji::CATEGORIES
        .iter()
        .enumerate()
        .map(|(index, category)| {
            let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.picker.tab = index;
                cx.notify();
            });
            div()
                .id(format!("chat-reaction-tab-{}", category.name))
                .w(TAB_WIDTH)
                .h(ladder.hit)
                .flex()
                .items_center()
                .justify_center()
                .border_b_2()
                .border_color(match index == chosen {
                    true => theme.foreground,
                    false => Hsla::transparent_black(),
                })
                .text_type(TAB_GLYPH)
                .role(Role::Tab)
                .aria_label(category.name)
                .aria_selected(index == chosen)
                .focusable()
                .on_click(open)
                .child(category.glyph)
                .into_any_element()
        })
        .collect();
    div()
        .id("chat-reaction-tabs")
        .flex()
        .justify_between()
        .px(design::space::BLOCK)
        .bg(theme.background)
        .border_b_1()
        .border_color(theme.border)
        .role(Role::TabList)
        .aria_label(words::EMOJI_CATEGORIES)
        .children(tabs)
        .into_any_element()
}

pub(super) fn cell(
    cell: Cell,
    press: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    theme: &Theme,
) -> AnyElement {
    div()
        .id(cell.id)
        .size(CELL)
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(match cell.mine {
            true => theme.accent,
            false => Hsla::transparent_black(),
        })
        .when(cell.mine, |cell| cell.bg(theme.accent_soft))
        .text_type(CELL_EMOJI)
        .role(Role::Button)
        .aria_label(words::react_with(&cell.emoji))
        .focusable()
        .on_click(press)
        .child(cell.emoji)
        .into_any_element()
}
