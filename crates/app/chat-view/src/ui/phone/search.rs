//! Search, a page of its own (Expo `app/(tabs)/chat/search.tsx`, board 55): Back, the field, Clear;
//! the one scope; then the rooms whose names hold it and the messages, newest first, the searched
//! words in medium. A room, or a hit landed on, opens over this page.
use view::prelude::*;
use view::{Div, Stateful};
use view::{FontWeight, HighlightStyle};

use super::parts::face;
use crate::words;
use crate::{Chat, Hits};
use chat::MsgRow;
use view::design::Ladder;

const CHIP: Pixels = px(32.);

pub(super) fn screen(
    chat: &Chat,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let asked = !chat.search.query.is_empty();
    div()
        .id("chat-search-page")
        .size_full()
        .flex()
        .flex_col()
        .bg(theme.background)
        .child(title_row(chat, back, cx, theme, ladder))
        .child(scope(theme, ladder))
        .when(asked, |page| page.child(results(chat, cx, theme, ladder)))
        .into_any_element()
}

fn title_row(
    chat: &Chat,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.search_submit(cx)
    });
    let clear = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.search_empty();
        cx.notify();
    });
    let input = Input::new("search", &chat.search.draft, words::SEARCH_MESSAGES)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .virtual_keyboard(VirtualKeyboard::Search)
        .input_action(TextInputAction::Search)
        .placeholder(words::SEARCH_MESSAGES)
        .on_submit(submit);
    let field = div()
        .flex_1()
        .min_w(px(0.))
        .child(design::field(input, ladder, theme).lead("search"));
    div()
        .id("chat-search-title")
        .flex_none()
        .h(ladder.header)
        .flex()
        .items_center()
        .gap(design::space::XXS)
        .px(design::space::SM)
        .pr(design::CAPSULE_CLEARANCE)
        .bg(theme.background)
        .border_b_1()
        .border_color(theme.border)
        .child(design::icon_button(
            "chat-search-back",
            design::icon("arrow-left", ladder.icon),
            "Back",
            ladder,
            theme,
            back,
        ))
        .child(field)
        .child(design::icon_button(
            "chat-search-clear",
            design::icon("x", ladder.icon),
            "Clear",
            ladder,
            theme,
            clear,
        ))
}

fn scope(theme: &Theme, ladder: &Ladder) -> Div {
    div()
        .flex()
        .py(design::space::SM)
        .px(design::space::BLOCK)
        .child(
            div()
                .id("chat-search-scope")
                .h(CHIP)
                .flex()
                .items_center()
                .px(design::space::MD)
                .border_1()
                .border_color(theme.primary)
                .bg(theme.primary)
                .text_type(ladder.secondary)
                .text_color(theme.primary_foreground)
                .child("All rooms"),
        )
}

// What an asked search shows, in one scroll: the rooms whose names hold it, then the messages.
fn results(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    div()
        .id("chat-search-results")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .children(rooms(chat, cx, theme, ladder))
        .children(hits(chat, cx, theme, ladder))
}

// The rooms whose names hold the search, under Rooms and how many, a row each that opens it over
// this page; nothing when no room's name does.
fn rooms(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Vec<AnyElement> {
    let found = chat.found_rooms();
    if found.is_empty() {
        return Vec::new();
    }
    let head = design::section_head("chat-search-rooms-head", words::ROOMS, ladder, theme)
        .child(found.len().to_string());
    let mut rows = vec![head.into_any_element()];
    for info in found {
        let room = info.channel.id.clone();
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            cx.notify();
            chat.choose(room.clone(), cx)
        });
        let name = info.channel.name.clone();
        let hash = design::icon("hash", ladder.icon_row).text_color(theme.muted);
        let row = design::row(
            format!("chat-search-room-{}", info.channel.id),
            ladder,
            theme,
        )
        .role(Role::Button)
        .aria_label(name.clone())
        .focusable()
        .on_click(open)
        .child(design::lead(hash))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_type(ladder.body)
                .text_color(theme.foreground)
                .child(name),
        );
        rows.push(row.into_any_element());
    }
    rows
}

/// What an asked search shows: its refusal with Retry, or its hits; nothing
/// while it is on its way.
fn hits(chat: &Chat, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Option<AnyElement> {
    match &chat.search.hits {
        Loadable::Idle | Loadable::Loading(_) => None,
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|chat, _: &ClickEvent, _window, cx| {
                cx.notify();
                chat.search_now(cx)
            });
            Some(
                design::refused(
                    "chat-search",
                    design::sentence(refusal),
                    ladder,
                    theme,
                    retry,
                )
                .into_any_element(),
            )
        }
        Loadable::Ready(hits) | Loadable::Reloading(hits, _) => {
            Some(list(chat, hits, cx, theme, ladder).into_any_element())
        }
    }
}

// Messages and how many, newest first; a row per hit; More results under them while a page follows.
fn list(chat: &Chat, hits: &Hits, cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Div {
    let count = format!("{} · newest first", hits.rows.len());
    div()
        .flex()
        .flex_col()
        .child(design::section_head("chat-search-head", "Messages", ladder, theme).child(count))
        .children(
            hits.rows
                .iter()
                .map(|row| hit(chat, row, cx, theme, ladder)),
        )
        .when(hits.has_more, |list| list.child(more(cx, theme, ladder)))
}

// The way to the next page of hits, under the ones shown.
fn more(cx: &mut Context<Chat>, theme: &Theme, ladder: &Ladder) -> Div {
    let more = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.search_more(cx)
    });
    div().flex().p(design::space::BLOCK).child(design::button(
        "chat-search-more",
        words::MORE_RESULTS,
        ladder,
        theme,
        more,
    ))
}

fn hit(
    chat: &Chat,
    row: &MsgRow,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let empty = chat::view::Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    let message = crate::message::chat_message(row.clone(), names, &chat.rooms());
    let (id, seq) = (row.channel_id.clone(), row.seq);
    let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.open_at(id.clone(), seq, cx)
    });
    let place = format!(
        " in {} · {}",
        chat.room_name(&row.channel_id),
        design::day(row.time)
    );
    let title = div()
        .flex()
        .items_baseline()
        .min_w(px(0.))
        .text_type(ladder.body)
        .child(div().flex_none().child(message.author.clone()))
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(place),
        );
    let words = div()
        .truncate()
        .text_type(ladder.secondary)
        .text_color(theme.muted)
        .child(marked(&message.body, &chat.search.query, theme));
    let picture = message
        .from
        .account()
        .and_then(|account| chat.faces.of(account, cx));
    design::row(
        format!("chat-search-hit-{}-{seq}", row.channel_id),
        ladder,
        theme,
    )
    .min_h(ladder.row_tall)
    .role(Role::Button)
    .aria_label(format!("{}: {}", message.author, message.body))
    .focusable()
    .on_click(open)
    .child(design::lead(face(
        &message.author,
        message.agent,
        picture,
        ladder.avatar,
        theme,
    )))
    .child(
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(title)
            .child(words),
    )
    .into_any_element()
}

fn marked(body: &str, query: &str, theme: &Theme) -> StyledText {
    let words: Vec<&str> = match query.trim_start().starts_with('#') {
        true => Vec::new(),
        false => query.split_whitespace().collect(),
    };
    let style = HighlightStyle {
        font_weight: Some(FontWeight::MEDIUM),
        color: Some(theme.foreground),
        ..Default::default()
    };
    let mut highlights = Vec::new();
    let mut at = 0;
    while at < body.len() {
        let found = words.iter().find_map(|word| {
            let end = at + word.len();
            body.get(at..end)
                .filter(|part| part.eq_ignore_ascii_case(word))
                .map(|_| end)
        });
        match found {
            Some(end) => {
                highlights.push((at..end, style));
                at = end;
            }
            None => at += body[at..].chars().next().map_or(1, char::len_utf8),
        }
    }
    StyledText::new(body.to_owned()).with_highlights(highlights)
}
