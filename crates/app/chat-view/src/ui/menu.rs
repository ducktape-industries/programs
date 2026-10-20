//! Message menus at the pointer and the edit composer under its stream.
use crate::emoji;
use crate::words;
use crate::{CANCEL_DELETE, CONFIRM_DELETE, Chat, Menu, Mode, Pane};
use view::prelude::*;
use view::{Anchor, AnchoredPositionMode, Edges, Point};

const ROW: f32 = design::height::CONTROL as f32;
const ROW_GAP: f32 = design::spacing::HAIR as f32;
const MENU_INSET: f32 = design::spacing::XS as f32;
const CELL: f32 = 32.;
const PICKER_GAP: f32 = design::spacing::HAIR as f32;
const PICKER_INSET: f32 = design::spacing::SM as f32;
const COLUMNS: u16 = 8;
const GRID_ROWS: f32 = emoji::PER_TAB.div_ceil(COLUMNS as usize) as f32;
const SEARCH: f32 = design::height::CONTROL as f32;
const CAPTION: f32 = 14.;
const GLYPH_W: f32 = 20.;
const TABS: f32 = design::height::CONTROL as f32;
const STACK_GAP: f32 = design::spacing::XS as f32;
const STRIP_CLEAR: f32 = 26. + design::spacing::XS as f32;
type Press = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub mod card;
mod picker;
use picker::{picker_size, reactions};
use view::design::Ladder;

fn prefix(pane: Pane) -> &'static str {
    match pane {
        Pane::Timeline => "chat-room-message-",
        Pane::Thread => "chat-thread-message-",
    }
}
pub fn focus_key(pane: Pane, mode: Mode) -> String {
    let suffix = match mode {
        Mode::Reactions => "reaction-focus",
        Mode::Delete => return CANCEL_DELETE.into(),
        _ => "action-focus",
    };
    format!("{}{suffix}", prefix(pane))
}

pub fn floating(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let menu = chat.menu.as_ref()?;
    let (at, size) = popup_geometry(menu, more_items(chat, menu).len())?;
    let content = message_menu(chat, menu, cx, theme, ladder);
    let id = match menu.mode {
        Mode::Reactions => format!("{}reaction-frame", prefix(menu.pane)),
        Mode::Delete => format!("{}delete-frame", prefix(menu.pane)),
        _ => focus_key(menu.pane, menu.mode),
    };
    let frame = match menu.mode {
        Mode::Reactions => div().id(id),
        Mode::Delete => div()
            .id(id)
            .role(Role::AlertDialog)
            .aria_label(words::DELETE_QUESTION),
        Mode::Profile => div().id(format!("{}card-frame", prefix(menu.pane))),
        _ => {
            let items = more_items(chat, menu);
            let (pane, seq, rev) = (menu.pane, menu.seq, menu.rev);
            let run = items.clone();
            design::composite(id, Role::Menu, words::MESSAGE_ACTIONS)
                .active(
                    chat.menu_cursor.min(items.len().saturating_sub(1)),
                    items.len(),
                )
                .on_move(cx.processor(|chat, index: usize, _, cx| {
                    chat.menu_cursor = index;
                    cx.notify();
                }))
                .on_press(cx.processor(move |chat, index: usize, window, cx| {
                    cx.notify();
                    act(run[index], pane, seq, rev, chat, window, cx)
                }))
                .build()
        }
    };
    let frame = design::focus_shown(frame, theme, |style| style.shadow_lg())
        .when_some(size, |frame, (w, h)| {
            frame.w(px(w)).h(px(h)).overflow_hidden()
        })
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .occlude()
        .child(content);
    let popup = anchored()
        .position(Point {
            x: px(at.0),
            y: px(at.1),
        })
        .position_mode(AnchoredPositionMode::Window)
        .snap_to_window_with_margin(Edges::all(design::space::SM));
    let popup = match menu.mode {
        Mode::More if !chat.menu_at_pointer => popup.anchor(Anchor::TopRight).offset(Point {
            x: px(0.),
            y: px(STRIP_CLEAR),
        }),
        _ => popup.anchor(Anchor::TopLeft),
    };
    Some(popup.child(frame).into_any_element())
}

type Pair = (f32, f32);

fn popup_geometry(menu: &Menu, items: usize) -> Option<(Pair, Option<Pair>)> {
    let size = match menu.mode {
        Mode::More => Some(menu_size(items)),
        Mode::Reactions => Some(picker_size()),
        Mode::Delete | Mode::Profile => None,
        Mode::Toolbar | Mode::Editing | Mode::Reactors => return None,
    };
    Some((menu.at, size))
}

pub fn editing(chat: &Chat, pane: Pane, cx: &mut Context<Chat>) -> Option<AnyElement> {
    let target = chat.editing()?;
    if chat.menu.as_ref()?.pane != pane {
        return None;
    }
    Some(
        div()
            .id("chat-message-editing")
            .px_4()
            .py_1()
            .child(crate::ui::room::composer(
                chat,
                target,
                words::EDIT_MESSAGE,
                true,
                cx,
            ))
            .into_any_element(),
    )
}

fn message_menu(
    chat: &Chat,
    menu: &Menu,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match menu.mode {
        Mode::Toolbar | Mode::More => actions(chat, menu, cx, theme),
        Mode::Reactions => reactions(chat, menu, cx, theme, ladder),
        Mode::Delete => delete(chat, cx, theme),
        Mode::Profile => card::card(chat, menu, cx, theme, ladder),
        Mode::Editing | Mode::Reactors => div().into_any_element(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    Reply,
    React,
    CopyText,
    CopyLink,
    WorkRecord,
    WhoCanEdit,
    Edit,
    Delete,
}

fn more_items(chat: &Chat, menu: &Menu) -> Vec<Action> {
    let (pane, seq) = (menu.pane, menu.seq);
    let offers = chat.offered(pane, seq);
    let open = chat
        .room
        .as_ref()
        .and_then(|room| room.thread.as_ref())
        .is_some_and(|thread| thread.root == seq);
    [
        (Action::Reply, pane == Pane::Timeline && !open),
        (Action::React, offers.react),
        (Action::CopyText, text_of(chat, pane, seq).is_some()),
        (Action::CopyLink, chat.message_link(seq).is_some()),
        (Action::WorkRecord, work_record(chat, pane, seq).is_some()),
        (Action::WhoCanEdit, chat.message_item(pane, seq).is_some()),
        (Action::Edit, offers.edit),
        (Action::Delete, offers.delete),
    ]
    .into_iter()
    .filter_map(|(action, offered)| offered.then_some(action))
    .collect()
}

fn actions(chat: &Chat, menu: &Menu, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let items: Vec<Item> = more_items(chat, menu)
        .into_iter()
        .enumerate()
        .map(|(index, action)| Item {
            role: Role::MenuItem,
            active: index == chat.menu_cursor,
            ..action_item(action, chat, menu, cx, theme)
        })
        .collect();
    div()
        .id("chat-menu-actions")
        .flex()
        .flex_col()
        .gap(px(ROW_GAP))
        .p(px(MENU_INSET))
        .children(items)
        .into_any_element()
}

fn action_item(
    action: Action,
    _chat: &Chat,
    menu: &Menu,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Item {
    let (pane, seq, rev) = (menu.pane, menu.seq, menu.rev);
    let press = Some(
        Box::new(cx.listener(move |chat, _: &ClickEvent, window, cx| {
            cx.notify();
            act(action, pane, seq, rev, chat, window, cx)
        })) as Press,
    );
    match action {
        Action::Reply => Item::new(
            "chat-menu-reply",
            "↩",
            words::REPLY_IN_THREAD,
            press,
            *theme,
        ),
        Action::React => Item::new(
            "chat-menu-add-reaction",
            "😀",
            words::ADD_REACTION,
            press,
            *theme,
        ),
        Action::CopyText => Item::new("chat-menu-copy-text", "📄", words::COPY_TEXT, press, *theme),
        Action::CopyLink => Item::new("chat-menu-copy-link", "🔗", words::COPY_LINK, press, *theme),
        Action::WorkRecord => Item::new(
            "chat-menu-work-record",
            "📋",
            "View work record",
            press,
            *theme,
        ),
        Action::WhoCanEdit => Item::new(
            "chat-menu-who-can-edit",
            "🔒",
            policy_view::item::WHO,
            press,
            *theme,
        ),
        Action::Edit => Item::new("chat-menu-edit", "✎", words::EDIT_MESSAGE, press, *theme),
        Action::Delete => {
            let (fg, bg) = (theme.danger, theme.background);
            Item::toned(
                "chat-menu-delete",
                words::DELETE_MESSAGE,
                press,
                *theme,
                fg,
                bg,
            )
            .glyph("🗑")
        }
    }
}

fn act(
    action: Action,
    pane: Pane,
    seq: u64,
    rev: u32,
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
) {
    match action {
        Action::Reply => chat.reply_in_thread(seq, cx),
        Action::React => chat.open_menu(pane, seq, rev, Mode::Reactions, window, cx),
        Action::WhoCanEdit => chat.who_can_edit(pane, seq, cx),
        Action::Edit => chat.open_menu(pane, seq, rev, Mode::Editing, window, cx),
        Action::Delete => chat.open_menu(pane, seq, rev, Mode::Delete, window, cx),
        Action::CopyText => {
            let text = text_of(chat, pane, seq);
            chat.close_menu();
            if let Some(text) = text {
                chat.copy_text(text, words::MESSAGE_TEXT, cx);
            }
        }
        Action::CopyLink => {
            let link = chat.message_link(seq);
            chat.close_menu();
            match link {
                Some(link) => chat.copy_text(link, words::MESSAGE_LINK, cx),
                None => cx.host().log("no message link: the session names no chain"),
            }
        }
        Action::WorkRecord => {
            let link = work_record(chat, pane, seq);
            chat.close_menu();
            if let Some(link) = link {
                chat.open_link(link, cx);
            }
        }
    }
}

fn text_of(chat: &Chat, pane: Pane, seq: u64) -> Option<String> {
    let messages = chat.messages(pane);
    let message = messages.iter().find(|message| message.seq == seq)?;
    (!message.deleted && !message.body.is_empty()).then(|| message.body.clone())
}

fn work_record(chat: &Chat, pane: Pane, seq: u64) -> Option<String> {
    chat.work_record(&chat.author(pane, seq)?)
}

fn delete(_chat: &Chat, cx: &mut Context<Chat>, theme: &Theme) -> AnyElement {
    let cancel = cx.listener(|chat, _: &ClickEvent, _, cx| {
        chat.close_menu();
        cx.notify();
    });
    let confirm = Some({
        Box::new(cx.listener(|chat, _: &ClickEvent, _, cx| {
            cx.notify();
            chat.delete_armed(cx)
        })) as Press
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .child(
            div()
                .text_size(design::text::BODY)
                .child(words::DELETE_QUESTION),
        )
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child("It leaves the room for everyone."),
        )
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(Item::text(
                    CANCEL_DELETE,
                    words::CANCEL,
                    Some(Box::new(cancel)),
                    *theme,
                ))
                .child(Item::toned(
                    CONFIRM_DELETE,
                    words::DELETE,
                    confirm,
                    *theme,
                    theme.danger,
                    theme.danger_soft,
                )),
        )
        .into_any_element()
}

fn menu_size(items: usize) -> (f32, f32) {
    let n = items as f32;
    (220., MENU_INSET * 2. + n * ROW + (n - 1.).max(0.) * ROW_GAP)
}
#[derive(IntoElement)]
struct Item {
    id: ElementId,
    role: Role,
    active: bool,
    glyph: Option<String>,
    label: String,
    press: Option<Press>,
    theme: Theme,
    fg: Hsla,
    bg: Hsla,
}
impl Item {
    fn new(
        id: impl Into<ElementId>,
        glyph: &str,
        label: &str,
        press: Option<Press>,
        theme: Theme,
    ) -> Self {
        Self {
            id: id.into(),
            role: Role::Button,
            active: false,
            glyph: Some(glyph.into()),
            label: label.into(),
            press,
            fg: theme.foreground,
            bg: theme.background,
            theme,
        }
    }
    fn glyph(mut self, glyph: &str) -> Self {
        self.glyph = Some(glyph.into());
        self
    }
    fn text(id: impl Into<ElementId>, label: &str, press: Option<Press>, theme: Theme) -> Self {
        Self::toned(id, label, press, theme, theme.foreground, theme.surface)
    }
    fn toned(
        id: impl Into<ElementId>,
        label: &str,
        press: Option<Press>,
        theme: Theme,
        fg: Hsla,
        bg: Hsla,
    ) -> Self {
        Self {
            id: id.into(),
            role: Role::Button,
            active: false,
            glyph: None,
            label: label.into(),
            press,
            theme,
            fg,
            bg,
        }
    }
}
impl RenderOnce for Item {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let enabled = self.press.is_some();
        let mut row = div()
            .id(self.id)
            .w_full()
            .h(px(ROW))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .bg(self.bg)
            .text_color(if enabled { self.fg } else { self.theme.muted })
            .role(self.role)
            .aria_label(self.label.clone())
            .aria_disabled(!enabled);
        if let Some(glyph) = self.glyph {
            row = row.child(
                div()
                    .w(px(GLYPH_W))
                    .h(px(ROW))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(design::text::BODY)
                    .whitespace_nowrap()
                    .child(glyph),
            );
        }
        row = row
            .when(self.active, |row| row.bg(self.theme.surface_raised))
            .child(div().whitespace_nowrap().child(self.label));
        match (self.press, self.role) {
            (Some(press), Role::MenuItem) => design::item(
                row.cursor_pointer()
                    .hover(|s| s.bg(self.theme.surface_raised))
                    .active(|s| s.bg(self.theme.accent_soft))
                    .on_click(press),
                Role::MenuItem,
                self.active,
            )
            .into_any_element(),
            (Some(press), _) => row
                .focusable()
                .cursor_pointer()
                .hover(|s| s.bg(self.theme.surface_raised))
                .active(|s| s.bg(self.theme.accent_soft))
                .on_click(press)
                .into_any_element(),
            (None, _) => row.into_any_element(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menu_and_picker_dimensions() {
        let at = |mode| Menu {
            pane: Pane::Timeline,
            seq: 1,
            rev: 2,
            mode,
            at: (617., 449.),
        };
        let sized = |mode, items| {
            let (spot, size) = popup_geometry(&at(mode), items).expect("a popup");
            assert_eq!(spot, (617., 449.));
            size
        };
        let (two, three) = (sized(Mode::More, 2).unwrap(), sized(Mode::More, 3).unwrap());
        assert_eq!(two.0, three.0, "one width");
        assert!(three.1 > two.1, "a row more is taller");
        assert_eq!(sized(Mode::Reactions, 0), Some(picker_size()));
        assert_eq!(sized(Mode::Delete, 0), None);
        for mode in [Mode::Toolbar, Mode::Editing] {
            assert_eq!(popup_geometry(&at(mode), 0), None);
        }
    }
}
