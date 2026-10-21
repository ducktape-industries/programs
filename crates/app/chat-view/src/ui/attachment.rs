//! A file a message carries, as both designs draw it: the picture itself (a click opens it whole
//! over the room, a phone's tap in the system's viewer), else a row with its name, Save
//! (`file.save`) and its size once read, else whether its read is out, failed or found nothing.
use chat::message::Attachment;
use view::Img;
use view::design::Ladder;
use view::prelude::*;

use super::button;
use super::dialogs::INSET;
use crate::attach::{Picture, Unread};
use crate::{Chat, words};

const FILE_W: Pixels = px(360.);
const PICTURE_H: Pixels = px(240.);
const WHOLE: &str = "chat-picture";
pub(crate) const WHOLE_OVERLAY: &str = "chat-picture-overlay";

fn fitted(picture: &Picture, room: (f32, f32)) -> Img {
    let (width, height) = view::picture::fit(picture.size, room);
    img(picture.image.clone())
        .w(px(width))
        .h(px(height))
        .flex_none()
        .object_fit(ObjectFit::Contain)
}

pub(crate) fn view(
    chat: &Chat,
    id: String,
    file: &Attachment,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let key = (file.path.clone(), file.snapshot);
    let shown = chat.shown_files.get(&key);
    if let Some(picture) = shown.and_then(|shown| shown.picture.as_ref()) {
        let (name, theme) = (file.name.clone(), *theme);
        let touch = ladder.touch;
        let open = {
            let file = file.clone();
            cx.listener(move |chat, _: &ClickEvent, _, cx| match touch {
                true => chat.save_file(&file, cx),
                false => chat.enlarge(&file, cx),
            })
        };
        let picture = fitted(picture, (FILE_W.into(), PICTURE_H.into()))
            .max_w_full()
            .with_fallback(move || design::quiet(name.clone(), &theme).into_any_element())
            .id(format!("{id}/picture"));
        return div()
            .id(id)
            .self_start()
            .max_w_full()
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
    let unread = chat.unread_files.get(&key).map(|(_, unread)| *unread);
    let (said, tone) = match (shown, unread) {
        (Some(_), _) => (words::size(file.size), theme.muted),
        (None, Some(Unread::Absent)) => (words::FILE_MISSING.to_owned(), theme.danger),
        (None, Some(Unread::Failed | Unread::Refused)) => {
            (words::FILE_FAILED.to_owned(), theme.danger)
        }
        (None, None | Some(Unread::Reading | Unread::ReadingAgain | Unread::Retrying)) => {
            (words::FILE_READING.to_owned(), theme.muted)
        }
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
                .id(ElementId::Name(format!("{id}/status").into()))
                .text_type(ladder.caption)
                .text_color(tone)
                .child(said),
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
    let picture = shown.picture.as_ref()?;
    let inset = 2. * f32::from(INSET);
    let bar = f32::from(design::space::SM) + f32::from(ladder.hit);
    let (width, height) = chat.layout.viewport;
    let room = (width - inset, height - inset - bar);
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
    let picture = fitted(picture, room)
        .id(format!("{WHOLE}/image"))
        .role(Role::Image)
        .aria_label(file.name.clone());
    let buttons = div().flex().gap(design::space::SM).child(save).child(open);
    let whole = div()
        .id(WHOLE)
        .flex()
        .flex_col()
        .items_center()
        .gap(design::space::SM)
        .child(picture)
        .child(buttons);
    Some(whole.into_any_element())
}
