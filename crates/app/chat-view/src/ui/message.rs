//! Message cards and their native GPUI actions.

use view::accesskit::Action;
use view::prelude::*;
use view::{Div, FontStyle, FontWeight, HighlightStyle, Stateful, UnderlineStyle};

use crate::message::{ChatMessage, SpanStyle};
use crate::ui::badge;
use crate::{Chat, Control, Mode, Pane};
use chat::view::Names;
use chat::{Block, Principal, Span};
mod controls;
pub(crate) mod rich;
use crate::message::plain_line;
use crate::words;
use controls::{Face, action_button, reaction_button, replies_button};
use rich::rich_line;
use view::design::Ladder;

const PENDING_OPACITY: f32 = 0.5;
/// How far the action strip hangs over the card's top edge: half of the
/// strip, which is 26 tall (24 px buttons inside a 1 px border).
const STRIP_OVERHANG: f32 = 13.;
/// The quiet above a run's first message, outside its card: the card's
/// ground (hover, chosen) hugs the message, and the strip hangs over the
/// card, not over the gap.
const RUN_GAP: Pixels = design::space::SM;

/// The cells of the message row being drawn: the message is cell 0, each
/// enabled control the next in paint order. `active` is the cell the
/// pane's arrows are on, when this is their row; the controls are handed
/// back so Enter knows what the active cell does.
struct Cells {
    active: Option<usize>,
    controls: Vec<Control>,
}

impl Cells {
    /// Records `control` as the next cell; whether it is the active one.
    fn push(&mut self, control: Control) -> bool {
        self.controls.push(control);
        self.active == Some(self.controls.len())
    }
}

/// A message's row of the pane's grid, and its controls in paint order.
/// The row is `{id}-row`: its first cell is the message, then one cell
/// per control, the card's (block link, program link, chips, `+`,
/// replies) and the action strip's. `active`: the cell the arrows are on,
/// when this is their row. `set`: the row's place among the pane's
/// messages, 1-based, and their count — said by the row itself, since the
/// host positions only a list item's own node, and a message under a day
/// or unread marker is wrapped.
#[allow(
    clippy::too_many_arguments,
    reason = "the ladder rides beside the theme into every sized helper; the phone rework of this view reshapes the row"
)]
pub fn card(
    chat: &Chat,
    message: ChatMessage,
    pane: Pane,
    active: Option<usize>,
    set: (usize, usize),
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> (AnyElement, Vec<Control>) {
    let id = message.id.clone();
    let seq = message.seq;
    let press = press(pane, seq, cx);
    let mut cells = Cells {
        active,
        controls: Vec::new(),
    };
    let chosen = !message.deleted
        && seq > 0
        && chat
            .menu
            .as_ref()
            .is_some_and(|menu| menu.pane == pane && menu.seq == seq);
    let ranged = !message.deleted && chat.copy.is_some_and(|range| range.holds(pane, seq));
    let key = (pane, seq);
    // cell 0 of the row: the message, the cell the arrows land on and
    // claimed while they are on it. It lies under the whole card, as a
    // cell may not hold the controls' cells drawn over it. The pointer's
    // click is here; the keys' Enter comes through the grid
    let message_cell = div()
        .id(format!("chat-message-{id}"))
        .absolute()
        .inset_0()
        .role(view::Role::GridCell)
        .aria_label(format!("{}: {}", message.author, message.body))
        .when(cells.active == Some(0), |cell| {
            cell.aria_active_descendant()
        })
        .on_click(press);
    let card = div()
        .id(format!("chat-message-{id}-card"))
        .relative()
        .flex()
        .gap(design::space::MD)
        .px_4()
        .pt(if message.show_author {
            design::space::XXS
        } else {
            design::space::HAIR
        })
        .pb(design::space::HAIR)
        // the pointer on the strip is not on the card (the strip occludes
        // it): the card keeps its hover ground from state, not from
        // `hover`, so it does not drop while the pointer is on the strip
        .bg(if chosen {
            theme.accent_soft
        } else if ranged || chat.pointer_on(key) {
            theme.surface_raised
        } else {
            theme.background
        })
        .when(message.pending, |card| card.opacity(PENDING_OPACITY))
        .child(message_cell)
        .child(avatar(&message, pane, &mut cells, cx, theme))
        .child(content(
            chat,
            message.clone(),
            pane,
            &mut cells,
            cx,
            theme,
            ladder,
        ));
    // Controls are siblings of the selection target: their native click must
    // not also replace the opened menu with the message-selection toolbar.
    // The row says when the pointer is over it, and only that row (and a
    // chosen one) carries the action strip: drawn invisible under every
    // row, the strips were most of each frame the view sends — over half
    // its bytes in a busy room — and every frame is paid for in fuel.
    let row_hover = hovers(key, false, cx);
    let mut outer = div()
        .id(format!("chat-message-{id}-row"))
        .relative()
        .w_full()
        .pt(gap(&message))
        .on_hover(row_hover)
        // a row of the pane's grid; it never claims
        .role(view::Role::Row)
        .aria_label(format!(
            "Select message, shows its actions: {}: {}",
            message.author, message.body
        ))
        .aria_position_in_set(set.0)
        .aria_size_of_set(set.1)
        .child(card);
    if !message.pending && !message.deleted && seq > 0 {
        outer = pointed(outer, pane, seq, message.rev, cx);
    }
    if !message.pending && !message.deleted && (chosen || chat.pointer_on(key)) {
        let strip = action_strip(chat, &message, pane, &mut cells, cx, theme);
        outer = outer.child(strip);
    }
    (outer.into_any_element(), cells.controls)
}

// A right press on the message's row opens its menu at the pointer, as
// ⋯ opens it under the strip; assistive tech's "show menu" opens it where
// the keys do ([`Chat::key_spot`]).
fn pointed(
    row: Stateful<Div>,
    pane: Pane,
    seq: u64,
    rev: u32,
    cx: &mut Context<Chat>,
) -> Stateful<Div> {
    let open = cx.listener(move |chat, event: &MouseDownEvent, window, cx| {
        chat.layout.press = (event.position.x.into(), event.position.y.into());
        chat.open_menu(pane, seq, rev, Mode::More, window, cx);
        chat.menu_at_pointer = chat
            .menu
            .as_ref()
            .is_some_and(|menu| menu.seq == seq && menu.mode == Mode::More);
        cx.notify();
    });
    let show = cx.listener(move |chat, _: &(), window, cx| {
        chat.layout.press = chat.key_spot(pane);
        chat.open_menu(pane, seq, rev, Mode::More, window, cx);
        cx.notify();
    });
    row.on_mouse_down(MouseButton::Right, open)
        .on_a11y_action(Action::ShowContextMenu, move |_, window, app| {
            show(&(), window, app)
        })
}

/// The row's quiet above its card: a run's first message has one.
fn gap(message: &ChatMessage) -> Pixels {
    match message.show_author {
        true => RUN_GAP,
        false => px(0.),
    }
}

/// A control's click: it does what Enter on its cell does
/// ([`Chat::act`]); the control consumes the press, so the card beneath
/// does not hear it.
fn acts(
    pane: Pane,
    seq: u64,
    rev: u32,
    control: Control,
    cx: &mut Context<Chat>,
) -> impl Fn(&ClickEvent, &mut Window, &mut view::App) + 'static {
    cx.listener(move |chat, event: &ClickEvent, window, cx| {
        let position = event.position();
        chat.layout.press = (position.x.into(), position.y.into());
        cx.notify();
        chat.act(pane, seq, rev, control.clone(), window, cx);
    })
}

/// A press on the card selects the message (with shift held, the copy
/// range grows to it); a control on it consumes its own press, so the
/// card never hears it.
fn press(
    pane: Pane,
    seq: u64,
    cx: &mut Context<Chat>,
) -> impl Fn(&ClickEvent, &mut Window, &mut view::App) + 'static {
    cx.listener(move |chat, event: &ClickEvent, _window, cx| {
        let position = event.position();
        let at = (position.x.into(), position.y.into());
        cx.notify();
        chat.layout.press = at;
        chat.press_message(pane, seq, event.modifiers().shift);
    })
}

/// The row, or its strip, takes `hovered` as the pointer enters it and
/// gives it up as the pointer leaves, if it still holds it: the host says
/// the row's leave and the strip's enter in either order
/// ([`Chat::pointer_on`]).
fn hovers(
    key: (Pane, u64),
    strip: bool,
    cx: &mut Context<Chat>,
) -> impl Fn(&bool, &mut Window, &mut view::App) + 'static {
    let at = Some(crate::state::Hovered { key, strip });
    cx.listener(move |chat, over: &bool, _window, cx| {
        if *over && chat.hovered != at {
            chat.hovered = at;
            cx.notify();
        } else if !*over && chat.hovered == at {
            chat.hovered = None;
            cx.notify();
        }
    })
}

/// The grid cell of the row that holds `control`, whose id is
/// `control_id`: `{control_id}-cell`. A disabled control's cell says so,
/// as the arrows skip it.
fn cell(control_id: &str, enabled: bool, control: impl IntoElement) -> AnyElement {
    div()
        .id(format!("{control_id}-cell"))
        .role(view::Role::GridCell)
        .when(!enabled, |cell| cell.aria_disabled(true))
        .child(control)
        .into_any_element()
}

/// A run's first message wears its author's initials, which open the
/// author's identity page as a mention does; the rest keep the column.
fn avatar(
    message: &ChatMessage,
    pane: Pane,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    if !message.show_author {
        return div()
            .w_7()
            .h(design::space::XXS)
            .flex_shrink_0()
            .into_any_element();
    }
    let id = format!("chat-message-{}-avatar", message.id);
    let face = div()
        .id(id.clone())
        .size_7()
        .flex()
        .items_center()
        .justify_center()
        .rounded(design::space::XS)
        .bg(if message.agent {
            theme.agent_soft
        } else {
            theme.surface_raised
        })
        .text_color(if message.agent {
            theme.agent
        } else {
            theme.muted
        })
        .font_weight(view::FontWeight::SEMIBOLD)
        .text_size(design::text::CAPTION)
        .child(message.initial.clone());
    let Principal::Account(account) = message.from else {
        return face.into_any_element();
    };
    let control = Control::Profile(account);
    let active = cells.push(control.clone());
    let open = acts(pane, message.seq, message.rev, control, cx);
    // an agent's face opens its card in place; a person's opens the profile link
    let (label, role) = match message.agent {
        true => (
            format!("Open {}’s card", message.author),
            view::Role::Button,
        ),
        false => (
            format!("Open {}’s profile", message.author),
            view::Role::Link,
        ),
    };
    let face = face
        .cursor_pointer()
        .aria_label(label)
        .on_click(open)
        .consumes_click();
    cell(&id, true, design::item(face, role, active))
}

/// The strip over a row the pointer is on (or a chosen one): thread,
/// 👍, the picker and the "More" menu.
fn action_strip(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let (id, seq) = (&message.id, message.seq);
    let rev = message.rev;
    let writable = chat.may_write();
    let strip_hover = hovers((pane, seq), true, cx);
    let actions = div()
        .id(format!("chat-message-{id}-actions"))
        .absolute()
        .right_2()
        // Half over the card's top edge, as Slack hangs it: the message
        // under it stays where it is. The wire keeps a negative inset. The
        // upper half lies over the row above, whose hover would otherwise take the strip
        // away as the pointer reached it: the strip occludes, so no row
        // is hovered while the pointer is on it, and says itself that it
        // is (`hovered`), which keeps it drawn.
        .top(gap(message) - px(STRIP_OVERHANG))
        .flex()
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .occlude()
        .on_hover(strip_hover);
    // each button is a cell of the row, beside the card
    let cell = |button: AnyElement, key: &str, enabled: bool| {
        cell(&format!("chat-message-{id}-{key}"), enabled, button)
    };
    let thread = (pane == Pane::Timeline && message.reply_count == 0).then(|| {
        let active = cells.push(Control::Thread);
        let open = acts(pane, seq, rev, Control::Thread, cx);
        let id = format!("chat-message-{id}-thread");
        cell(
            action_button(id, "💬", "Open thread", theme, true, active, open).into_any_element(),
            "thread",
            true,
        )
    });
    // a disabled button's cell is not counted: the arrows skip it
    let thumbs = writable && cells.push(Control::ThumbsUp);
    let react = writable && cells.push(Control::React);
    let more = cells.push(Control::More);
    actions
        .children(thread)
        .child(cell(
            action_button(
                format!("chat-message-{id}-thumbs-up"),
                "👍",
                "React with 👍",
                theme,
                writable,
                thumbs,
                acts(pane, seq, rev, Control::ThumbsUp, cx),
            )
            .into_any_element(),
            "thumbs-up",
            writable,
        ))
        .child(cell(
            action_button(
                format!("chat-message-{id}-react"),
                "😀",
                "Manage reactions",
                theme,
                writable,
                react,
                acts(pane, seq, rev, Control::React, cx),
            )
            .into_any_element(),
            "react",
            writable,
        ))
        .child(cell(
            action_button(
                format!("chat-message-{id}-more"),
                "⋯",
                "More message actions",
                theme,
                true,
                more,
                acts(pane, seq, rev, Control::More, cx),
            )
            .into_any_element(),
            "more",
            true,
        ))
        .into_any_element()
}

/// The card's content: the header, the blocks, the marks, the reactions and
/// the way into the thread, in that order (the controls' cell order too).
fn content(
    chat: &Chat,
    message: ChatMessage,
    pane: Pane,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let header = match message.show_author {
        true => Some(header(&message, cells, theme, ladder)),
        false => None,
    };
    let blocks = blocks(chat, &message, cells, cx, theme);
    let reactions = match message.reactions.is_empty() {
        true => None,
        false => Some(reactions(chat, &message, pane, cells, cx, theme)),
    };
    let replies = replies(&message, pane, cells, cx, theme);
    div()
        .id(format!("chat-message-{}-contents", message.id))
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .gap_1()
        .children(header)
        .children(blocks)
        .children(edited_mark(&message, theme))
        .children(reactions)
        .children(replies)
        .into_any_element()
}

/// The message's blocks, or its plain body where it has none.
fn blocks(
    chat: &Chat,
    message: &ChatMessage,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Vec<AnyElement> {
    if let Some((program, code)) = &message.system {
        return vec![program_post(chat, message, program, code, cells, cx, theme)];
    }
    if message.blocks.is_empty() {
        let text = div()
            .id(format!("chat-message-{}-text", message.id))
            .cursor_text()
            .child(message.body.clone());
        return vec![text.into_any_element()];
    }
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    let blocks = message.blocks.iter().enumerate();
    blocks
        .map(|(index, block)| {
            block_view(message, index, block, names, cx, theme).into_any_element()
        })
        .collect()
}

fn edited_mark(message: &ChatMessage, theme: &Theme) -> Option<AnyElement> {
    message.edited.then(|| {
        div()
            .text_size(design::text::CAPTION)
            .text_color(theme.muted)
            .child(words::EDITED)
            .into_any_element()
    })
}

/// A root's replies: in the timeline, the button into its thread; atop
/// the thread pane, the count over a rule.
fn replies(
    message: &ChatMessage,
    pane: Pane,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Option<AnyElement> {
    if message.reply_count == 0 {
        return None;
    }
    if pane == Pane::Timeline {
        let active = cells.push(Control::Replies);
        let open = acts(pane, message.seq, message.rev, Control::Replies, cx);
        let id = format!("chat-message-{}-replies", message.id);
        let button = replies_button(id.clone(), message.reply_count, theme, active, open);
        return Some(
            div()
                .flex()
                .pt_1()
                .child(cell(&id, true, button))
                .into_any_element(),
        );
    }
    let separator = div()
        .id(format!("chat-message-{}-reply-separator", message.id))
        .flex()
        .items_center()
        .gap_2()
        .pt_1()
        .child(
            div()
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .child(design::plural(message.reply_count, "reply", "replies")),
        )
        .child(div().h(px(1.)).flex_1().bg(theme.border));
    Some(separator.into_any_element())
}

/// A run's first message names its author, what the author is (an agent
/// and its manager, a program), when it was posted and its block.
fn header(message: &ChatMessage, cells: &mut Cells, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let mut header = div()
        .id(format!("chat-message-{}-header", message.id))
        .flex()
        .items_center()
        .gap_1()
        .child(
            div()
                .text_size(design::text::BODY)
                .font_weight(view::FontWeight::MEDIUM)
                .child(message.author.clone()),
        );
    if let Some(label) = &message.badge {
        let (foreground, background) = match message.agent {
            true => (theme.agent, theme.agent_soft),
            false => (theme.muted, theme.surface_raised),
        };
        header = header.child(badge(
            format!("chat-message-{}-badge", message.id),
            label.clone(),
            ladder,
            foreground,
            background,
        ));
    }
    if message.time > 0 {
        let clock = design::clock(message.time);
        header = header.child(
            div()
                .id(format!("chat-message-{}-time", message.id))
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .whitespace_nowrap()
                .child(match message.height > 0 {
                    true => format!("{clock} ·"),
                    false => clock,
                }),
        );
    }
    if message.height > 0 {
        // the link opens Explorer at its block, and the card under it
        // stays unchosen
        let href = crate::links::block_link(message.height);
        let active = cells.push(Control::Height(href.clone()));
        let id = format!("chat-message-{}-height", message.id);
        let label = format!("block {}", design::grouped(message.height));
        let link = design::link(id.clone(), label.clone(), href, ladder, theme)
            .aria_label(format!("Open {label} in Explorer"));
        header = header.child(cell(
            &id,
            true,
            design::item(link, view::Role::Link, active),
        ));
    }
    header.into_any_element()
}

/// A program's own post: its event code, quiet and mono, and (on the first
/// of a run) a link to where the program itself shows the room.
fn program_post(
    chat: &Chat,
    message: &ChatMessage,
    program: &str,
    code: &str,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let mut line = div()
        .id(format!("chat-message-{}-program", message.id))
        .flex()
        .items_center()
        .gap(design::space::SM)
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(design::mono(code.to_owned()));
    // one link per run of the program's lines: the room is the same
    let link = message
        .show_author
        .then(|| crate::links::program_link(&chat.session.chain_id, &chat.room_id()))
        .flatten();
    if let Some(link) = link {
        let active = cells.push(Control::ProgramOpen(link.clone()));
        let open = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
            chat.open_link(link.clone(), cx);
        });
        let theme = *theme;
        let id = format!("chat-message-{}-program-open", message.id);
        let open = div()
            .id(id.clone())
            .text_color(theme.accent)
            .cursor_pointer()
            .hover(move |style| style.text_decoration_1())
            .on_click(open)
            .consumes_click()
            .child(words::open_in(program));
        line = line.child(cell(
            &id,
            true,
            design::item(open, view::Role::Link, active),
        ));
    }
    line.into_any_element()
}

/// The reactions under a message, and the `+` that opens the picker.
fn reactions(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let (seq, rev) = (message.seq, message.rev);
    let writable = chat.may_write();
    let reactors = chat.room.as_ref().and_then(|room| room.reactors.get(&seq));
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    let mut reactions = div()
        .id(format!("chat-message-{}-reactions", message.id))
        .flex()
        .flex_wrap()
        .gap_1();
    for reaction in &message.reactions {
        let control = Control::Reaction {
            emoji: reaction.emoji.clone(),
            add: !reaction.reacted_by_me,
        };
        let active = writable && cells.push(control.clone());
        let id = format!("chat-message-{}-reaction-{}", message.id, reaction.emoji);
        let who = reactors
            .and_then(|reactors| reactors.get(&reaction.emoji))
            .into_iter()
            .flatten()
            .map(|principal| names.author(principal))
            .collect::<Vec<_>>();
        let face = Face::Emoji {
            emoji: &reaction.emoji,
            count: reaction.count,
            who: who.join(", "),
        };
        let click = acts(pane, seq, rev, control, cx);
        let chip = reaction_button(
            id.clone(),
            face,
            reaction.reacted_by_me,
            theme,
            writable,
            active,
            click,
        );
        reactions = reactions.child(cell(&id, writable, chip));
    }
    // the card under the `+` would otherwise take the same click and put
    // the row's toolbar over the picker just opened: the `+` consumes it
    let active = writable && cells.push(Control::AddReaction);
    let id = format!("chat-message-{}-reaction-add", message.id);
    let add = reaction_button(
        id.clone(),
        Face::Add,
        false,
        theme,
        writable,
        active,
        acts(pane, seq, rev, Control::AddReaction, cx),
    );
    reactions = reactions.child(cell(&id, writable, add));
    reactions.into_any_element()
}

fn block_view(
    message: &ChatMessage,
    index: usize,
    block: &Block,
    names: &Names,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = ElementId::from(format!("chat-message-{}-block-{index}", message.id));
    match block {
        Block::Divider => div()
            .id(id)
            .h(px(1.))
            .w_full()
            .bg(theme.border)
            .into_any_element(),
        Block::Code { lang, text } => {
            let mut code = div()
                .id(id)
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .bg(theme.surface)
                .cursor_text();
            if let Some(lang) = lang.as_ref().filter(|lang| !lang.is_empty()) {
                code = code.child(
                    div()
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted)
                        .child(lang.clone()),
                );
            }
            code.child(plain_line(
                format!("chat-message-{}-block-{index}-code", message.id).into(),
                text,
                true,
            ))
            .into_any_element()
        }
        Block::Quote(spans) => div()
            .border_l_2()
            .border_color(theme.border_strong)
            .pl_2()
            .cursor_text()
            .text_color(theme.muted)
            .child(rich_line(id, spans, names, cx, theme))
            .into_any_element(),
        Block::Paragraph(spans) => match chat::list_item(spans) {
            // a list item: its marker in a hanging gutter, the item beside it
            Some((marker, item)) => div()
                .id(format!("chat-message-{}-block-{index}-item", message.id))
                .cursor_text()
                .flex()
                .gap_2()
                .child(
                    div()
                        .min_w(px(16.))
                        .text_color(theme.muted)
                        .child(match marker {
                            chat::ListMarker::Bullet => "•".to_owned(),
                            chat::ListMarker::Ordered(number) => format!("{number}."),
                        }),
                )
                // the item takes what the gutter leaves of the row and wraps
                // there: a line is as wide as its parent (`w_full`), and a
                // flex item is no narrower than its unwrapped words unless
                // its floor is 0
                .child(
                    rich_line(id, &item, names, cx, theme)
                        .flex_1()
                        .min_w(px(0.)),
                )
                .into_any_element(),
            None => rich_line(id, spans, names, cx, theme)
                .cursor_text()
                .into_any_element(),
        },
    }
}
