//! The small controls on a message card: the action strip's buttons, a
//! reaction, and the way into a thread.
use crate::words;
use view::prelude::*;

/// An action strip button's height, the smallest box a pointer presses
/// (the door's AX-017): the strip, borders and all, is 26. Its width is
/// the kit's row height, already over 24.
const STRIP_BUTTON_HEIGHT: f32 = 24.;
/// A reaction chip's height, the thread button's: the smallest box a
/// pointer presses (the door's AX-017), and a chip is no narrower.
const REACTION_HEIGHT: f32 = 24.;
/// The thread button's height under a root.
const REPLIES_HEIGHT: f32 = 24.;
// A reaction tooltip's widest line, before its names wrap.
const REACTORS_WIDTH: f32 = 280.;

/// `active`: the grid's arrows are on this button (it is never a Tab stop
/// of its own; the pane's message list is).
pub(super) fn action_button(
    id: impl Into<ElementId>,
    label: impl Into<String>,
    accessible: &str,
    theme: &Theme,
    enabled: bool,
    active: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut view::App) + 'static,
) -> impl IntoElement {
    let control = div()
        .id(id)
        .w(design::size::ROW)
        .h(px(STRIP_BUTTON_HEIGHT))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.background)
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.faint
        })
        .role(view::Role::Button)
        .aria_label(accessible)
        .aria_disabled(!enabled)
        .text_size(design::text::SECONDARY)
        .child(label.into());
    if enabled {
        design::item(
            control
                .cursor_pointer()
                .hover(|s| s.bg(theme.surface_raised))
                .on_click(click)
                .consumes_click(),
            Role::Button,
            active,
        )
    } else {
        control
    }
}

// What a reaction button shows: an emoji and how many chose it, named on
// hover (`who`, empty until read), or the `+` that opens the picker.
pub(super) enum Face<'a> {
    Emoji {
        emoji: &'a str,
        count: u64,
        who: String,
    },
    Add,
}

pub(super) fn reaction_button(
    id: impl Into<ElementId>,
    face: Face,
    mine: bool,
    theme: &Theme,
    enabled: bool,
    active: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut view::App) + 'static,
) -> impl IntoElement {
    let mut control = div()
        .id(id)
        .h(px(REACTION_HEIGHT))
        .min_w(px(REACTION_HEIGHT))
        .px(design::space::XS)
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .border_1()
        // the reader's own wear the strong line: accent_soft is the
        // hover grey in the calm palette, so a fill alone can't say "mine"
        .border_color(if mine { theme.accent } else { theme.border })
        .bg(if mine {
            theme.accent_soft
        } else {
            theme.background
        })
        .text_color(if enabled {
            theme.foreground
        } else {
            theme.muted
        })
        .role(view::Role::Button)
        .aria_disabled(!enabled)
        .text_size(design::text::SECONDARY);
    control = match face {
        // the count in the data face, as every count here is
        Face::Emoji { emoji, count, who } => {
            let control = control
                // one name whether it is yours or not: the toggle says which
                .aria_label(format!("{emoji} reaction"))
                .aria_description(match who.is_empty() {
                    true => count.to_string(),
                    false => format!("{count}: {who}"),
                })
                .aria_toggled(mine.into())
                .child(emoji.to_owned())
                .child(
                    div()
                        .font_family(design::fonts::FAMILY_MONO)
                        .text_size(design::text::CAPTION)
                        .child(count.to_string()),
                );
            match who.is_empty() {
                true => control,
                false => reactors_tip(control, who.into(), theme),
            }
        }
        // `+` is the picker's method, not a toggle
        Face::Add => control.aria_label(words::ADD_REACTION).child("+"),
    };
    if enabled {
        design::item(
            control
                .cursor_pointer()
                .hover(|style| {
                    style
                        .bg(theme.surface_raised)
                        .border_color(theme.border_strong)
                })
                .on_click(click)
                .consumes_click(),
            Role::Button,
            active,
        )
    } else {
        control
    }
}

// `chip` names who chose it on hover.
fn reactors_tip<E: StatefulInteractiveElement>(chip: E, who: SharedString, theme: &Theme) -> E {
    let theme = *theme;
    chip.tooltip(move |_, cx| {
        cx.new(|_| WhoChose {
            who: who.clone(),
            theme,
        })
        .into()
    })
}

// A reaction's tooltip: who chose it.
struct WhoChose {
    who: SharedString,
    theme: Theme,
}

impl Render for WhoChose {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("chat-reactors")
            .max_w(px(REACTORS_WIDTH))
            .px(design::space::SM)
            .py(design::space::XXS)
            .border_1()
            .border_color(self.theme.border_strong)
            .bg(self.theme.background)
            .text_color(self.theme.foreground)
            .text_size(design::text::CAPTION)
            .child(self.who.clone())
    }
}

/// Under a message with replies: how many, and the way into them, drawn as
/// the button it is.
pub(super) fn replies_button(
    id: impl Into<ElementId>,
    count: u64,
    theme: &Theme,
    active: bool,
    click: impl Fn(&ClickEvent, &mut Window, &mut view::App) + 'static,
) -> impl IntoElement {
    let replies = design::plural(count, "reply", "replies");
    let button = div()
        .id(id)
        .h(px(REPLIES_HEIGHT))
        .px_2()
        .flex()
        .items_center()
        .gap(design::space::XS)
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .text_size(design::text::SECONDARY)
        .text_color(theme.foreground)
        .cursor_pointer()
        .hover(|style| {
            style
                .bg(theme.surface_raised)
                .border_color(theme.border_strong)
        })
        .active(|style| style.bg(theme.accent_soft))
        .aria_label(words::open_thread(&replies))
        .on_click(click)
        .consumes_click()
        .child(replies)
        .child(div().text_color(theme.muted).child("Open thread →"));
    design::item(button, Role::Button, active)
}
