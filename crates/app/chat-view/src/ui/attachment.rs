//! A file a message carries, as both designs draw it: the picture itself
//! (a click opens it whole over the room, a phone's tap in the system's
//! viewer), else a row with its name, size and Save (`file.save`).
use chat::message::Attachment;
use view::design::Ladder;
use view::prelude::*;

use super::button;
use super::dialogs::INSET;
use crate::{Chat, words};

// the most room a file takes in a message's column
const FILE_W: Pixels = px(360.);
const PICTURE_H: Pixels = px(240.);
const WHOLE: &str = "chat-picture";
pub(crate) const WHOLE_OVERLAY: &str = "chat-picture-overlay";

pub(crate) fn view(
    chat: &Chat,
    id: String,
    file: &Attachment,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let shown = chat.shown_files.get(&(file.path.clone(), file.snapshot));
    if let Some(picture) = shown.and_then(|shown| shown.picture.clone()) {
        let (name, theme) = (file.name.clone(), *theme);
        // a click opens it whole over the room; on a phone a tap hands it
        // to the system's viewer (Quick Look, Android's), whose share saves it
        let touch = ladder.touch;
        let open = {
            let file = file.clone();
            cx.listener(move |chat, _: &ClickEvent, _, cx| match touch {
                true => chat.save_file(&file, cx),
                false => chat.enlarge(&file, cx),
            })
        };
        let picture = img(picture)
            .max_w(FILE_W)
            .max_h(PICTURE_H)
            .object_fit(ObjectFit::ScaleDown)
            .with_fallback(move || design::quiet(name.clone(), &theme).into_any_element());
        return div()
            .id(id)
            .role(Role::Button)
            .aria_label(words::open(&file.name))
            .focusable()
            .on_click(open)
            .child(picture)
            .into_any_element();
    }
    let saves = shown.is_some_and(|shown| shown.chunks.is_some());
    let save = {
        let file = file.clone();
        cx.listener(move |chat, _: &ClickEvent, _, cx| chat.save_file(&file, cx))
    };
    let save = design::icon_button(
        ElementId::Name(format!("{id}/save").into()),
        design::icon("download", ladder.icon),
        words::SAVE,
        ladder,
        theme,
        save,
    )
    .enabled(saves);
    let glyph = match file.content_type.starts_with("image/") {
        true => "image",
        false => "file",
    };
    let named = div()
        .flex_1()
        .min_w(px(0.))
        .flex()
        .flex_col()
        .child(
            div()
                .truncate()
                .text_type(ladder.body)
                .child(file.name.clone()),
        )
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(words::size(file.size)),
        );
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(design::space::SM)
        .max_w(FILE_W)
        .p(design::space::SM)
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(design::icon(glyph, ladder.icon).text_color(theme.muted))
        .child(named)
        .child(save)
        .into_any_element()
}

// The picture a click opened, whole over the room and fit to the pane, with
// Save and Open in Files; Escape or a press outside it closes it.
pub(crate) fn over(
    chat: &Chat,
    screen: AnyElement,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let dismiss = cx.listener(|chat, _: &(), _window, cx| {
        chat.enlarged = None;
        cx.notify();
    });
    let enlarged = chat.enlarged.as_ref();
    let label = enlarged.map(|file| file.name.clone()).unwrap_or_default();
    let whole = enlarged.and_then(|file| whole(chat, file, cx, theme, ladder));
    modal_overlay(WHOLE_OVERLAY, label, screen, whole)
        .flex()
        .items_center()
        .justify_center()
        .p(INSET)
        .backdrop(hsla(0., 0., 0., 0.55))
        .on_dismiss(dismiss)
        .into_any_element()
}

fn whole(
    chat: &Chat,
    file: &Attachment,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let shown = chat.shown_files.get(&(file.path.clone(), file.snapshot))?;
    let picture = shown.picture.clone()?;
    let inset: f32 = INSET.into();
    let (width, height) = chat.layout.viewport;
    let room = |side: f32| px((side - 2. * inset).max(0.));
    let save = {
        let file = file.clone();
        cx.listener(move |chat, _: &ClickEvent, _, cx| chat.save_file(&file, cx))
    };
    let open = {
        let path = file.path.clone();
        cx.listener(move |chat, _: &ClickEvent, _, cx| chat.open_in_files(&path, cx))
    };
    let save = button(format!("{WHOLE}/save"), words::SAVE, ladder, theme, save)
        .enabled(shown.chunks.is_some());
    let open = button(
        format!("{WHOLE}/open"),
        words::OPEN_IN_FILES,
        ladder,
        theme,
        open,
    );
    let picture = img(picture)
        .id(format!("{WHOLE}/image"))
        .max_w(room(width))
        .min_h(px(0.))
        .flex_shrink(1.)
        .object_fit(ObjectFit::ScaleDown)
        .role(Role::Image)
        .aria_label(file.name.clone());
    let buttons = div().flex().gap(design::space::SM).child(save).child(open);
    let whole = div()
        .id(WHOLE)
        .flex()
        .flex_col()
        .items_center()
        .gap(design::space::SM)
        .max_h(room(height))
        .child(picture)
        .child(buttons);
    Some(whole.into_any_element())
}
