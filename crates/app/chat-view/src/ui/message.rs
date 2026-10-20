//! Message cards and their native GPUI actions.

use view::accesskit::Action;
use view::prelude::*;
use view::{Div, FontStyle, FontWeight, HighlightStyle, Stateful, UnderlineStyle};

use crate::message::{ChatMessage, SpanStyle, SystemLine};
use crate::ui::badge;
use crate::{Chat, Control, Mode, Pane};
use chat::view::Names;
use chat::{Block, Principal, Span};
mod controls;
pub(crate) mod rich;
use crate::markup::{Piece, opens, pieces};
use crate::words;
use controls::{Face, action_button, reaction_button, replies_button};
use rich::rich_line;
use view::design::Ladder;

const PENDING_OPACITY: f32 = 0.5;

#[derive(PartialEq)]
pub(crate) struct RowDeps {
    pub message: ChatMessage,
    pub pane: Pane,
    pub active: Option<usize>,
    pub set: (usize, usize),
    pub markers: (Option<String>, bool),
    pub seen: Seen,
    pub shared: std::rc::Rc<Shared>,
    pub ladder: Ladder,
    // its author's face (`Faces::dep`)
    pub face: Option<(Option<abi::BlobId>, bool)>,
}

#[derive(PartialEq)]
pub(crate) struct Seen {
    chosen: bool,
    ranged: bool,
    pointed: bool,
    reactors: Option<std::collections::BTreeMap<String, Vec<Principal>>>,
    files: Vec<Option<(Option<usize>, bool)>>,
    answering: Vec<String>,
}

impl Seen {
    pub(crate) fn of(chat: &Chat, pane: Pane, message: &ChatMessage) -> Self {
        let seq = message.seq;
        Self {
            chosen: chat
                .menu
                .as_ref()
                .is_some_and(|menu| menu.pane == pane && menu.seq == seq),
            ranged: chat.copy.is_some_and(|range| range.holds(pane, seq)),
            pointed: chat.pointer_on((pane, seq)),
            reactors: chat
                .room
                .as_ref()
                .and_then(|room| room.reactors.get(&seq))
                .cloned(),
            files: message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Attachment(file) => Some(file),
                    _ => None,
                })
                .map(|file| {
                    let shown = chat.shown_files.get(&(file.path.clone(), file.snapshot));
                    shown.map(|shown| {
                        let picture = shown.picture.as_ref();
                        let picture =
                            picture.map(|picture| std::sync::Arc::as_ptr(&picture.image) as usize);
                        (picture, shown.chunks.is_some())
                    })
                })
                .collect(),
            answering: answering_under(chat, pane, seq),
        }
    }
}

// who is answering under a message, as the channel draws it on the message's reply row
fn answering_under(chat: &Chat, pane: Pane, seq: u64) -> Vec<String> {
    match pane {
        Pane::Timeline => chat.answering_under(seq),
        Pane::Thread => Vec::new(),
    }
}

#[derive(PartialEq)]
pub(crate) struct Shared {
    names: Option<Names>,
    writable: bool,
    chain: String,
    room: String,
}

impl Shared {
    pub(crate) fn of(chat: &Chat) -> std::rc::Rc<Self> {
        std::rc::Rc::new(Self {
            names: chat.names.ready().cloned(),
            writable: chat.may_write(),
            chain: chat.session.chain_id.clone(),
            room: chat.room_id(),
        })
    }
}
const STRIP_OVERHANG: f32 = 13.;
const RUN_GAP: Pixels = design::space::SM;

struct Cells {
    active: Option<usize>,
    controls: Vec<Control>,
}

impl Cells {
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
    let picture = message
        .from
        .account()
        .and_then(|account| chat.faces.of(account, cx));
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
        .bg(if chosen {
            theme.accent_soft
        } else if ranged || chat.pointer_on(key) {
            theme.surface_raised
        } else {
            theme.background
        })
        .when(message.pending, |card| card.opacity(PENDING_OPACITY))
        .child(message_cell)
        .child(avatar(&message, picture, pane, &mut cells, cx, theme))
        .child(content(
            chat,
            message.clone(),
            pane,
            &mut cells,
            cx,
            theme,
            ladder,
        ));
    let row_hover = hovers(key, false, cx);
    let mut outer = div()
        .id(format!("chat-message-{id}-row"))
        .relative()
        .w_full()
        .pt(gap(&message))
        .on_hover(row_hover)
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

fn gap(message: &ChatMessage) -> Pixels {
    match message.show_author {
        true => RUN_GAP,
        false => px(0.),
    }
}

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

fn cell(control_id: &str, enabled: bool, control: impl IntoElement) -> AnyElement {
    div()
        .id(format!("{control_id}-cell"))
        .role(view::Role::GridCell)
        .when(!enabled, |cell| cell.aria_disabled(true))
        .child(control)
        .into_any_element()
}

fn avatar(
    message: &ChatMessage,
    picture: Option<std::sync::Arc<view::Image>>,
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
    let ringed = message.agent && picture.is_some();
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
        .when(ringed, |face| face.border_2().border_color(theme.agent));
    let face = match picture {
        Some(picture) => {
            let initial = message.initial.clone();
            let fallback = move || div().child(initial.clone()).into_any_element();
            face.child(
                img(picture)
                    .size_full()
                    .rounded(design::space::XS)
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(fallback),
            )
        }
        None => face.child(message.initial.clone()),
    };
    let Principal::Account(account) = message.from else {
        return face.into_any_element();
    };
    let control = Control::Profile(account);
    let active = cells.push(control.clone());
    let open = acts(pane, message.seq, message.rev, control, cx);
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

// the face over a draft, as a post's author wears it, with nothing to press
pub(super) fn still_face(
    chat: &Chat,
    author: &Principal,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let agent = crate::ui::answering::agent(chat, author);
    let initial = design::initial(&crate::ui::answering::name(chat, author));
    let picture = author
        .account()
        .and_then(|account| chat.faces.of(account, cx));
    let face = div()
        .size_7()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(design::space::XS)
        .bg(if agent {
            theme.agent_soft
        } else {
            theme.surface_raised
        })
        .text_color(if agent { theme.agent } else { theme.muted })
        .font_weight(view::FontWeight::SEMIBOLD)
        .text_size(design::text::CAPTION)
        .when(agent && picture.is_some(), |face| {
            face.border_2().border_color(theme.agent)
        });
    match picture {
        Some(picture) => face
            .child(
                img(picture)
                    .size_full()
                    .rounded(design::space::XS)
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => face.child(initial).into_any_element(),
    }
}

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
    let reacts = chat.offered(pane, seq).react;
    let strip_hover = hovers((pane, seq), true, cx);
    let actions = div()
        .id(format!("chat-message-{id}-actions"))
        .absolute()
        .right_2()
        .top(gap(message) - px(STRIP_OVERHANG))
        .flex()
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .occlude()
        .on_hover(strip_hover);
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
    let thumbs = reacts && cells.push(Control::ThumbsUp);
    let react = reacts && cells.push(Control::React);
    let more = cells.push(Control::More);
    actions
        .children(thread)
        .child(cell(
            action_button(
                format!("chat-message-{id}-thumbs-up"),
                "👍",
                "React with 👍",
                theme,
                reacts,
                thumbs,
                acts(pane, seq, rev, Control::ThumbsUp, cx),
            )
            .into_any_element(),
            "thumbs-up",
            reacts,
        ))
        .child(cell(
            action_button(
                format!("chat-message-{id}-react"),
                "😀",
                "Manage reactions",
                theme,
                reacts,
                react,
                acts(pane, seq, rev, Control::React, cx),
            )
            .into_any_element(),
            "react",
            reacts,
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
    let answering = answering_under(chat, pane, message.seq);
    let replies = replies(&message, pane, &answering, cells, cx, theme);
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

fn blocks(
    chat: &Chat,
    message: &ChatMessage,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Vec<AnyElement> {
    if let Some(post) = &message.system {
        return vec![program_post(chat, message, post, cells, cx, theme)];
    }
    if message.blocks.is_empty() {
        let text = div()
            .id(format!("chat-message-{}-text", message.id))
            .cursor_text()
            .child(message.body.clone());
        return vec![text.into_any_element()];
    }
    let key = format!("chat-message-{}", message.id);
    drawn(chat, &key, &message.blocks, cx, theme)
}

pub(super) fn drawn(
    chat: &Chat,
    key: &str,
    blocks: &[Block],
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Vec<AnyElement> {
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    pieces(blocks, names, &chat.session.chain_id)
        .into_iter()
        .map(|(index, piece)| match piece {
            Piece::Chat(block) => block_view(chat, key, index, block, names, cx, theme),
            Piece::Artifact(link) => artifact(format!("{key}-block-{index}"), link)
                .w_full()
                .into_any_element(),
            Piece::Markdown(block) => {
                let key = format!("{key}-block-{index}");
                div()
                    .cursor_text()
                    .child(markdown::desktop::block(&key, &block, theme, &opens(cx)))
                    .into_any_element()
            }
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

fn replies(
    message: &ChatMessage,
    pane: Pane,
    answering: &[String],
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> Option<AnyElement> {
    let mark = crate::ui::answering::mark(&message.id, answering, theme)
        .map(|mark| mark.text_size(design::text::CAPTION));
    if message.reply_count == 0 {
        return mark.map(|mark| div().flex().pt_1().child(mark).into_any_element());
    }
    if pane == Pane::Timeline {
        let active = cells.push(Control::Replies);
        let open = acts(pane, message.seq, message.rev, Control::Replies, cx);
        let id = format!("chat-message-{}-replies", message.id);
        let button = replies_button(id.clone(), message.reply_count, theme, active, open);
        return Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .pt_1()
                .child(cell(&id, true, button))
                .children(mark)
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

// A program's own post: what it says, quiet and mono, and a link to where the program itself
// shows the room it is about: once a run in the program's own room, on each line elsewhere.
fn program_post(
    chat: &Chat,
    message: &ChatMessage,
    post: &SystemLine,
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
        .child(design::mono(post.said.clone()));
    let elsewhere = post.room != chat.room_id();
    let link = (message.show_author || elsewhere)
        .then(|| crate::links::program_link(&chat.session.chain_id, &post.room))
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
            .child(words::open_in(&post.program));
        line = line.child(cell(
            &id,
            true,
            design::item(open, view::Role::Link, active),
        ));
    }
    line.into_any_element()
}

fn reactions(
    chat: &Chat,
    message: &ChatMessage,
    pane: Pane,
    cells: &mut Cells,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let (seq, rev) = (message.seq, message.rev);
    let reacts = chat.offered(pane, seq).react;
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
        let active = reacts && cells.push(control.clone());
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
            reacts,
            active,
            click,
        );
        reactions = reactions.child(cell(&id, reacts, chip));
    }
    let active = reacts && cells.push(Control::AddReaction);
    let id = format!("chat-message-{}-reaction-add", message.id);
    let add = reaction_button(
        id.clone(),
        Face::Add,
        false,
        theme,
        reacts,
        active,
        acts(pane, seq, rev, Control::AddReaction, cx),
    );
    reactions = reactions.child(cell(&id, reacts, add));
    reactions.into_any_element()
}

fn block_view(
    chat: &Chat,
    key: &str,
    index: usize,
    block: &Block,
    names: &Names,
    cx: &mut Context<Chat>,
    theme: &Theme,
) -> AnyElement {
    let id = ElementId::from(format!("{key}-block-{index}"));
    match block {
        Block::Divider => div()
            .id(id)
            .h(px(1.))
            .w_full()
            .bg(theme.border)
            .into_any_element(),
        // its language over the shared code panel, coloured by it
        Block::Code { lang, text } => {
            let lang = lang.as_deref().unwrap_or_default();
            let caption = (!lang.is_empty()).then(|| {
                div()
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted)
                    .child(lang.to_owned())
            });
            let key = format!("{key}-block-{index}");
            div()
                .id(id)
                .flex()
                .flex_col()
                .gap_1()
                .cursor_text()
                .children(caption)
                .child(markdown::desktop::code(&key, lang, text, theme))
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
        Block::Attachment(file) => {
            let id = format!("{key}-block-{index}");
            crate::ui::attachment::view(chat, id, file, cx, theme, &design::POINTER)
        }
        Block::Notice(notice) => div()
            .id(format!("{key}-block-{index}-notice"))
            .flex()
            .gap_2()
            .cursor_text()
            .text_size(design::text::SECONDARY)
            .text_color(theme.muted)
            .child(
                div()
                    .flex_none()
                    .text_color(crate::message::notice_tone(notice.status, theme))
                    .child(words::NOTICE_MARK),
            )
            .child(
                rich_line(id, &crate::message::notice_spans(notice), names, cx, theme)
                    .flex_1()
                    .min_w(px(0.)),
            )
            .into_any_element(),
        Block::Paragraph(spans) => match chat::list_item(spans) {
            Some((marker, item)) => div()
                .id(format!("{key}-block-{index}-item"))
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
