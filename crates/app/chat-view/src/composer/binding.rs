//! The composer as drawn: the mention menu, the host's text area, a note,
//! the Restore banner and the toolbar. Every press and key goes through
//! [`Draft::handle`], which asks the host for the edits.

use super::draft::{Draft, MentionChoice, Place, matching_choices};
use crate::words;
use std::rc::Rc;
use view::design::Ladder;
use view::prelude::*;
use view::{
    Anchor, AnchoredPositionMode, Deferred, Div, Edges, FontWeight, Point, Stateful, UniformList,
    wire,
};

/// What the composer hears: the host's text moved, a key it claimed went
/// down, or a control was pressed (`bold`, `italic`, `code`, `quote`,
/// `send`, `restore`, `mention:<token>`).
#[derive(Clone, Debug)]
pub enum Event {
    Changed(wire::TextChange),
    Key(wire::keyboard::KeyState, bool),
    Action(String),
    Placed(Place),
    // files dropped on the composer, which the host holds
    Dropped(Vec<Picked>),
}

/// What the view does after the draft handled an event: nothing more, or
/// the send the draft put in `submitted`.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Updated,
    Action(String),
}

pub type Handle<V> = Rc<dyn Fn(&mut V, Event, &mut Window, &mut Context<V>)>;
/// A press on a composer control.
pub type Click = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
pub(crate) type Presses = Rc<dyn Fn(&str, &mut Window, &mut App)>;

impl Draft {
    /// Runs `event` on the draft under the composer `key`, asking the host
    /// for the edits it needs through `window`.
    pub fn handle(
        &mut self,
        event: Event,
        key: &str,
        choices: &[MentionChoice],
        window: &mut Window,
    ) -> Outcome {
        let tag = match event {
            Event::Changed(change) => {
                self.changed(&change);
                return Outcome::Updated;
            }
            Event::Key(key, repeat) => match self.key_tag(&key, repeat, choices) {
                Some(tag) => tag,
                None => return Outcome::Updated,
            },
            Event::Action(tag) => tag,
            Event::Placed(place) => {
                self.placed = Some(place);
                return Outcome::Updated;
            }
            Event::Dropped(_) => return Outcome::Updated,
        };
        let (edits, outcome) = self.act(&tag, &format!("{key}/editor"), choices);
        for edit in edits {
            window.dispatch(edit);
        }
        outcome
    }
}

/// Dresses a mark's sign as what it does: bold, italic, code, quote.
type Face = fn(Div) -> Div;

#[derive(IntoElement)]
struct Mark {
    id: ElementId,
    sign: SharedString,
    label: SharedString,
    face: Face,
    on_click: Option<Click>,
}

impl RenderOnce for Mark {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let sign = (self.face)(div()).child(self.sign);
        let mut mark = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(self.label)
            .aria_disabled(self.on_click.is_none())
            .flex()
            .items_center()
            .justify_center()
            .size(design::size::CONTROL)
            .text_size(design::text::BODY)
            .text_color(theme.muted)
            .child(sign);
        if let Some(on_click) = self.on_click {
            mark = mark
                .hover(move |style| style.bg(theme.surface_raised).text_color(theme.foreground))
                .focusable()
                .on_click(on_click);
        }
        mark
    }
}

#[derive(IntoElement)]
struct ActionButton {
    id: ElementId,
    label: SharedString,
    primary: bool,
    on_click: Option<Click>,
}

impl RenderOnce for ActionButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        // A primary button that can't act yet reads as resting, not as ink.
        let (background, foreground, border) = match (self.primary, self.on_click.is_some()) {
            (true, true) => (theme.primary, theme.primary_foreground, theme.primary),
            (true, false) => (theme.surface_raised, theme.muted, theme.surface_raised),
            (false, _) => (theme.surface, theme.foreground, theme.border),
        };
        let mut button = div()
            .id(self.id)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .aria_disabled(self.on_click.is_none())
            .flex()
            .items_center()
            .justify_center()
            .h(design::size::CONTROL)
            .px_2()
            .border_1()
            .border_color(border)
            .bg(background)
            .text_color(foreground)
            .text_size(design::text::SECONDARY)
            .child(self.label);
        if let Some(on_click) = self.on_click {
            button = button.focusable().on_click(on_click);
            if self.primary {
                button = design::focus_shown_on_ink(button, &theme);
            }
        }
        button
    }
}

#[derive(IntoElement)]
struct MentionItem {
    id: ElementId,
    label: SharedString,
    hint: Option<SharedString>,
    selected: bool,
    on_click: Option<Click>,
}

impl RenderOnce for MentionItem {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.global::<Theme>();
        let name = match &self.hint {
            Some(hint) => format!("{}, {hint}", self.label).into(),
            None => self.label.clone(),
        };
        let hint = self.hint.map(|hint| {
            let id = ElementId::Name(format!("{}/hint", self.id).into());
            // the row is drawn at the pointer's sizes, its badge too
            design::badge(
                id,
                hint,
                &design::POINTER,
                theme.warning,
                theme.warning_soft,
            )
        });
        let mut row = div()
            .id(self.id)
            .role(Role::ListBoxOption)
            .aria_label(name)
            .aria_selected(self.selected)
            .focusable()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .min_h(design::size::ROW)
            .px_2()
            .bg(if self.selected {
                theme.accent_soft
            } else {
                theme.background
            })
            .text_color(if self.selected {
                theme.accent_foreground
            } else {
                theme.foreground
            })
            .text_size(design::text::BODY)
            .child(self.label)
            .children(hint);
        if let Some(on_click) = self.on_click {
            row = row.on_click(on_click);
        }
        row
    }
}

/// A press that acts on the draft. A pointer press focuses the control it
/// lands on, so the press hands the keys back to `editor` first: the
/// typing goes on where it was.
pub(crate) fn press<V: 'static>(
    editable: bool,
    tag: String,
    editor: &str,
    handle: &Handle<V>,
    cx: &Context<V>,
) -> Option<Click> {
    let presses = presses(editable, editor, handle, cx)?;
    Some(picks(&presses, tag))
}

pub(crate) fn picks(presses: &Presses, tag: String) -> Click {
    let presses = presses.clone();
    Box::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| presses(&tag, window, cx))
}

pub(crate) fn presses<V: 'static>(
    editable: bool,
    editor: &str,
    handle: &Handle<V>,
    cx: &Context<V>,
) -> Option<Presses> {
    if !editable {
        return None;
    }
    let handle = handle.clone();
    let editor = ElementId::Name(editor.to_owned().into());
    Some(Rc::new(cx.listener(
        move |view: &mut V, tag: &str, window: &mut Window, cx: &mut Context<V>| {
            window.focus(editor.clone());
            handle(view, Event::Action(tag.to_owned()), window, cx);
            cx.notify();
        },
    )))
}

/// The host's text area over the draft: what it shows, what it claims,
/// its lines wrapping; each design sizes it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn editor<V: 'static>(
    draft: &Draft,
    key: &str,
    label: &str,
    placeholder: &str,
    editable: bool,
    choices: &[MentionChoice],
    handle: &Handle<V>,
    cx: &Context<V>,
) -> Textarea {
    let changed = handle.clone();
    let keyed = handle.clone();
    let mut area = Textarea::new(ElementId::Name(key.to_owned().into()), &draft.field, label)
        .placeholder(placeholder)
        .read_only(!editable)
        .on_change(
            cx.listener(move |view, change: &wire::TextChange, window, cx| {
                changed(view, Event::Changed(change.clone()), window, cx);
                cx.notify();
            }),
        );
    if editable {
        area = area.claims(draft.claims(choices)).on_key(cx.listener(
            move |view, key: &KeyDownEvent, window, cx| {
                let state = wire::keyboard::KeyState::from(&key.keystroke);
                keyed(view, Event::Key(state, key.is_held), window, cx);
                cx.notify();
            },
        ));
    }
    area.w_full().whitespace_normal()
}

pub(crate) struct Opening {
    at: Point<Pixels>,
    anchor: Anchor,
    width: Pixels,
    room: Pixels,
}

pub(crate) fn opening(composer: Place, pane: Place) -> Option<Opening> {
    let margin = f32::from(design::space::SM);
    let above = composer.y - pane.y - margin;
    let below = pane.y + pane.height - (composer.y + composer.height) - margin;
    let upward = above >= below;
    let (anchor, y, room) = match upward {
        true => (Anchor::BottomLeft, composer.y, above),
        false => (Anchor::TopLeft, composer.y + composer.height, below),
    };
    let fits = room > 0.;
    fits.then(|| Opening {
        at: Point {
            x: px(composer.x),
            y: px(y),
        },
        anchor,
        width: px(composer.width),
        room: px(room),
    })
}

#[allow(clippy::too_many_arguments)]
fn mention_menu<V: 'static>(
    draft: &Draft,
    key: &str,
    editor: &str,
    editable: bool,
    choices: &[MentionChoice],
    pane: Option<Place>,
    handle: &Handle<V>,
    cx: &Context<V>,
) -> Option<Deferred> {
    let (sigil, partial) = draft.typed()?;
    let opening = opening(draft.placed?, pane?)?;
    let matches: Rc<[MentionChoice]> = matching_choices(choices, sigil, &partial)
        .into_iter()
        .cloned()
        .collect();
    let count = matches.len();
    let last = count.checked_sub(1)?;
    let selected = draft.menu_index.min(last);
    let presses = presses(editable, editor, handle, cx);
    let menu_key = key.to_owned();
    let rows = uniform_list(
        ElementId::Name(format!("{key}/mentions").into()),
        count,
        move |range, _, _| {
            range
                .map(|index| {
                    let choice = &matches[index];
                    let tag = format!("mention:{}", choice.token);
                    let on_click = presses.as_ref().map(|presses| picks(presses, tag));
                    MentionItem {
                        id: ElementId::Name(format!("{menu_key}/mention/{}", choice.token).into()),
                        label: choice.shown().into(),
                        hint: choice.hint.clone().map(Into::into),
                        selected: index == selected,
                        on_click,
                    }
                })
                .collect::<Vec<_>>()
        },
    )
    .track_scroll(&draft.menu_scroll.0);
    Some(mention_popup(key, opening, rows, cx.global::<Theme>()))
}

pub(crate) fn mention_popup(
    key: &str,
    opening: Opening,
    rows: UniformList,
    theme: &Theme,
) -> Deferred {
    let rows = rows
        .with_sizing_behavior(ListSizingBehavior::Infer)
        .flex_shrink(1.)
        .min_h(px(0.));
    let frame = div()
        .id(ElementId::Name(format!("{key}/mention-menu").into()))
        .role(Role::ListBox)
        .aria_label(words::PEOPLE)
        .w(opening.width)
        .max_h(opening.room)
        .flex()
        .flex_col()
        .border_1()
        .border_color(theme.border)
        .bg(theme.background)
        .shadow_lg()
        .occlude()
        .child(rows);
    deferred(
        anchored()
            .position(opening.at)
            .anchor(opening.anchor)
            .position_mode(AnchoredPositionMode::Window)
            .snap_to_window_with_margin(Edges::all(design::space::SM))
            .child(frame),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn view<V: 'static, F: Fn(&mut V, Event, &mut Window, &mut Context<V>) + 'static>(
    draft: &Draft,
    key: &str,
    // what the field is, for assistive technology ("New message"); the
    // hint is what to write in it ("Message #general") and is drawn
    label: &str,
    hint: &str,
    // what the commit button says: "Send" for a new message, "Save" for an
    // edit; the composer does not guess from the draft
    commit: &str,
    // a way out beside the commit, as an edit's Cancel beside its Save;
    // `None` for a composer that stays open
    cancel: Option<Click>,
    editable: bool,
    choices: &[MentionChoice],
    pane: Option<Place>,
    cx: &mut Context<V>,
    handle: F,
) -> impl IntoElement + use<V, F> {
    let handle: Handle<V> = Rc::new(handle);
    let editor_id = super::draft::editor_id(key);
    let editor = editor(
        draft, &editor_id, label, hint, editable, choices, &handle, cx,
    )
    .min_h(px(40.))
    .max_h(px(200.))
    .p(design::space::MD)
    .text_size(design::text::BODY);
    let mut rows: Vec<AnyElement> = Vec::new();

    let menu = mention_menu(draft, key, &editor_id, editable, choices, pane, &handle, cx);
    rows.extend(menu.map(IntoElement::into_any_element));
    rows.push(editor.into_any_element());
    let attaches = !super::Target::edits(key);
    let chips = chips(draft, key, editable, &handle, cx, &design::POINTER);
    rows.extend(chips.map(IntoElement::into_any_element));

    if !draft.note.is_empty() {
        rows.push(
            div()
                .mx(design::space::MD)
                .text_sm()
                .text_color(cx.global::<Theme>().danger)
                .child(draft.note.clone())
                .into_any_element(),
        );
    }
    if draft.failed_send.is_some() {
        rows.push(
            div()
                .mx(design::space::MD)
                .flex()
                .items_center()
                .gap_2()
                .p_2()
                .bg(cx.global::<Theme>().danger_soft)
                .text_color(cx.global::<Theme>().danger)
                .child(div().flex_1().child(words::UNSENT))
                .child(ActionButton {
                    id: ElementId::Name(format!("{key}/restore").into()),
                    label: words::RESTORE.into(),
                    primary: false,
                    on_click: press(editable, "restore".into(), &editor_id, &handle, cx),
                })
                .into_any_element(),
        );
    }

    let mut toolbar = div()
        .mx(design::space::XXS)
        .flex()
        .items_center()
        .gap(px(2.));
    let faces: [(&str, &str, &str, Face); 4] = [
        ("B", "Bold", "bold", |sign| {
            sign.font_weight(FontWeight::BOLD)
        }),
        ("I", "Italic", "italic", |sign| sign.italic()),
        ("</>", "Code", "code", |sign| {
            sign.font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
        }),
        ("“", "Quote", "quote", |sign| sign.text_size(px(16.))),
    ];
    if attaches {
        let attach = press(editable, "attach".into(), &editor_id, &handle, cx);
        toolbar = toolbar.child(attach_control(key, attach, cx.global::<Theme>()));
    }
    for (sign, label, tag, face) in faces {
        toolbar = toolbar.child(Mark {
            id: ElementId::Name(format!("{key}/{tag}").into()),
            sign: sign.into(),
            label: label.into(),
            face,
            on_click: press(editable, tag.into(), &editor_id, &handle, cx),
        });
    }
    let sendable = editable && draft.can_send();
    toolbar = toolbar.child(div().flex_1());
    if let Some(cancel) = cancel {
        toolbar = toolbar.child(ActionButton {
            id: ElementId::Name(format!("{key}/cancel").into()),
            label: words::CANCEL.into(),
            primary: false,
            on_click: Some(cancel),
        });
    }
    toolbar = toolbar.child(ActionButton {
        id: ElementId::Name(format!("{key}/send").into()),
        label: commit.to_owned().into(),
        primary: true,
        on_click: press(sendable, "send".into(), &editor_id, &handle, cx),
    });
    rows.push(toolbar.into_any_element());

    let column = div()
        .flex()
        .flex_col()
        .gap_1()
        .border_1()
        .border_color(cx.global::<Theme>().border)
        .bg(cx.global::<Theme>().background)
        .pb(design::space::XXS)
        .children(rows);
    let theme = *cx.global::<Theme>();
    let column = match attaches {
        true => column
            .id(ElementId::Name(format!("{key}/drop").into()))
            .on_file_drop(dropped(&handle, cx))
            .file_drag_over(move |style| style.border_color(theme.primary).bg(theme.accent_soft))
            .into_any_element(),
        false => column.into_any_element(),
    };
    let placed = handle.clone();
    sensor(ElementId::Name(key.into()), column).on_bounds(cx.listener(
        move |view, bounds: &Bounds<Pixels>, window, cx| {
            placed(view, Event::Placed(Place::from(bounds)), window, cx);
            cx.notify();
        },
    ))
}

// Files the host holds, dropped on the composer, to `handle`.
pub(crate) fn dropped<V: 'static>(
    handle: &Handle<V>,
    cx: &Context<V>,
) -> impl Fn(&Vec<Picked>, &mut Window, &mut App) + 'static {
    let handle = handle.clone();
    cx.listener(move |view: &mut V, files: &Vec<Picked>, window, cx| {
        handle(view, Event::Dropped(files.clone()), window, cx);
        cx.notify();
    })
}

// The toolbar's way to the host's picker.
fn attach_control(key: &str, on_click: Option<Click>, theme: &Theme) -> Stateful<Div> {
    let theme = *theme;
    let mut control = div()
        .id(ElementId::Name(format!("{key}/attach").into()))
        .role(Role::Button)
        .aria_label(words::ATTACH)
        .aria_disabled(on_click.is_none())
        .flex()
        .items_center()
        .justify_center()
        .size(design::size::CONTROL)
        .text_color(theme.muted)
        .child(design::icon("paperclip", design::text::BODY));
    if let Some(on_click) = on_click {
        control = control
            .hover(move |style| style.bg(theme.surface_raised).text_color(theme.foreground))
            .focusable()
            .on_click(on_click);
    }
    control
}

// The draft's files not sent yet, under the field: each one's name, how far
// it is up or why it failed, and the way to take it out.
pub(crate) fn chips<V: 'static>(
    draft: &Draft,
    key: &str,
    editable: bool,
    handle: &Handle<V>,
    cx: &Context<V>,
    ladder: &Ladder,
) -> Option<Div> {
    let files: Vec<_> = draft.attachments.iter().filter(|file| !file.sent).collect();
    if files.is_empty() {
        return None;
    }
    let theme = *cx.global::<Theme>();
    let editor_id = super::draft::editor_id(key);
    let presses = presses(editable, &editor_id, handle, cx);
    let chips = files.into_iter().map(|file| {
        let id = format!("{key}/file/{}", file.id);
        let (status, tone) = match (file.failed(), file.progress()) {
            (Some(why), _) => (why.to_owned(), theme.danger),
            (None, Some(progress)) => (words::uploading(progress), theme.muted),
            (None, None) if file.landed().is_some() => (words::size(file.size), theme.muted),
            (None, None) => (words::UPLOADING.to_owned(), theme.muted),
        };
        let edge = match file.failed() {
            Some(_) => theme.danger,
            None => theme.border,
        };
        let tag = format!("remove:{}", file.id);
        let remove = presses.as_ref().map(|presses| picks(presses, tag));
        let removable = remove.is_some();
        let remove: Click = remove.unwrap_or_else(|| Box::new(|_, _, _| {}));
        let named = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(
                div()
                    .truncate()
                    .text_type(ladder.secondary)
                    .child(file.name.clone()),
            )
            // said whole, wrapping: why a file failed is always read in full
            .child(
                div()
                    .id(ElementId::Name(format!("{id}/status").into()))
                    .text_type(ladder.caption)
                    .text_color(tone)
                    .child(status),
            );
        let remove = design::icon_button(
            ElementId::Name(format!("{id}/remove").into()),
            design::icon("x", ladder.icon),
            words::remove_file(&file.name),
            ladder,
            &theme,
            remove,
        )
        .enabled(removable);
        div()
            .id(ElementId::Name(id.into()))
            .flex()
            .items_center()
            .gap(design::space::SM)
            .max_w(px(280.))
            .pl(design::space::SM)
            .border_1()
            .border_color(edge)
            .bg(theme.surface)
            .child(named)
            .child(remove)
    });
    let row = div().flex().flex_wrap().gap(design::space::SM);
    Some(row.px(design::space::MD).children(chips))
}

#[cfg(test)]
mod tests;
