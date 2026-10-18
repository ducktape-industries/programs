//! A file a message carries, as both designs draw it: the picture itself
//! when it is one the app draws, else a row with its name, its size and
//! Save, which hands the host its chunks for the system's save dialog.
use chat::message::Attachment;
use view::design::Ladder;
use view::prelude::*;

use crate::{Chat, words};

// the most room a file takes in a message's column
const FILE_W: Pixels = px(360.);
const PICTURE_H: Pixels = px(240.);

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
        return img(picture)
            .id(id)
            .max_w(FILE_W)
            .max_h(PICTURE_H)
            .object_fit(ObjectFit::ScaleDown)
            .role(Role::Image)
            .aria_label(file.name.clone())
            .with_fallback(move || design::quiet(name.clone(), &theme).into_any_element())
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
