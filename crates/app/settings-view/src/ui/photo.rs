//! Change photo and Remove photo beside the reader's face, and the crop sheet:
//! the picked picture in a round frame the reader drags, a zoom slider, Cancel
//! and Save. Both designs draw these, each at its own ladder.
use view::design::{Kind, Ladder};
use view::prelude::*;
use view::{CursorStyle, Div, Stateful};

use crate::Settings;
use crate::photo::{Crop, FRAME};
use crate::state::Form;
use crate::words;

const SHEET: &str = "settings/photo";
const THUMB: Pixels = design::space::BLOCK;
const STEP: Pixels = design::space::SM;

pub(crate) fn presses(
    view: &Settings,
    number: u64,
    has_picture: bool,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<Div> {
    if !view.reads_files && !has_picture {
        return None;
    }
    let reading = view.photo_opening(number);
    let change = view.reads_files.then(|| {
        let pressed =
            cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.change_photo(number, cx));
        let label = match reading {
            true => words::OPENING,
            false => words::CHANGE_PHOTO,
        };
        design::button("settings/photo/change", label, ladder, theme, pressed)
            .kind(Kind::Outline)
            .busy(view.photo_of == Some(number) && view.change_photo.busy)
    });
    let remove = has_picture.then(|| {
        let pressed =
            cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.remove_photo(number, cx));
        let label = match view.photo_removing(number) {
            true => words::REMOVING,
            false => words::REMOVE_PHOTO,
        };
        design::button("settings/photo/remove", label, ladder, theme, pressed)
            .kind(Kind::Quiet)
            .busy(view.photo_removing(number))
    });
    let stopped = view
        .photo_stopped(number)
        .and_then(|form| refused("settings/photo/refused", form, ladder, theme));
    Some(
        div()
            .flex()
            .flex_col()
            .gap(design::space::SM)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(design::space::SM)
                    .children(change)
                    .children(remove),
            )
            .children(stopped),
    )
}

fn refused(id: &str, form: &Form, ladder: &Ladder, theme: &Theme) -> Option<Stateful<Div>> {
    let problem = form.problem.as_ref()?;
    Some(
        div()
            .id(id.to_owned())
            .text_type(ladder.secondary)
            .text_color(theme.danger)
            .child(words::problem_text(problem, "")),
    )
}

pub(crate) fn sheet(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let photo = view.photo.as_ref()?;
    let size = photo.size();
    let crop = photo.crop;
    let saving = view.change_photo.busy;
    let cancel = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.cancel_photo(cx));
    let save = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.save_photo(cx));
    let save_label = match saving {
        true => words::SAVING,
        false => words::SAVE,
    };
    let verbs = design::form_verbs(
        design::button(
            "settings/photo/cancel",
            words::CANCEL,
            ladder,
            theme,
            cancel,
        )
        .kind(Kind::Quiet),
        design::button("settings/photo/save", save_label, ladder, theme, save)
            .kind(Kind::Primary)
            .busy(saving),
    );
    let body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(design::space::LG)
        .p(design::SHEET_INSET)
        .child(frame(view, photo.preview.clone(), size, crop, cx, theme))
        .children(zoom(size, crop, cx, theme))
        .child(
            div()
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(words::PHOTO_HELP),
        )
        .children(refused(
            "settings/photo/save-refused",
            &view.change_photo,
            ladder,
            theme,
        ))
        .child(div().w_full().child(verbs));
    let dismissed = cx.listener(|v: &mut Settings, _: &(), _, cx| v.cancel_photo(cx));
    let sheet = design::sheet(SHEET, words::PHOTO_TITLE, ladder, theme)
        .on_dismiss(dismissed)
        .child(design::sheet_title(
            "settings/photo/head",
            words::PHOTO_TITLE,
            ladder,
            theme,
        ))
        .child(body);
    Some(sheet.into_any_element())
}

fn frame(
    view: &Settings,
    preview: std::sync::Arc<Image>,
    (width, height): (u32, u32),
    crop: Crop,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> impl IntoElement {
    let scale = FRAME * crop.zoom / width.min(height) as f32;
    let (x, y) = crop.centre;
    let picture = img(preview)
        .absolute()
        .left(px(FRAME / 2. - x * scale))
        .top(px(FRAME / 2. - y * scale))
        .w(px(width as f32 * scale))
        .h(px(height as f32 * scale))
        .object_fit(ObjectFit::Fill);
    let ring = div()
        .absolute()
        .left(px(-FRAME / 2.))
        .top(px(-FRAME / 2.))
        .size(px(FRAME * 2.))
        .rounded_full()
        .border(px(FRAME / 2.))
        .border_color(theme.background);
    let window = div()
        .id("settings/photo/picture")
        .relative()
        .size(px(FRAME))
        .overflow_hidden()
        .bg(theme.surface)
        .child(picture)
        .child(ring);
    let dragged =
        cx.listener(|v: &mut Settings, delta: &(Pixels, Pixels), _, cx| v.drag_photo(*delta, cx));
    let stepped = cx.listener(|v: &mut Settings, event: &KeyDownEvent, _, cx| {
        let none = px(0.);
        let by = match event.keystroke.key.as_str() {
            "left" => (STEP, none),
            "right" => (-STEP, none),
            "up" => (none, STEP),
            "down" => (none, -STEP),
            _ => return,
        };
        v.drag_photo(by, cx)
    });
    let pinched =
        cx.listener(|v: &mut Settings, event: &PinchEvent, _, cx| v.pinch_photo(event.delta, cx));
    let busy = view.change_photo.busy;
    div()
        .id("settings/photo/pinch")
        .when(!busy, |frame| frame.on_pinch(pinched))
        .child(
            resize_handle("settings/photo/frame", window)
                .on_drag(dragged)
                .cursor(CursorStyle::OpenHand)
                .role(Role::Splitter)
                .aria_label(words::PHOTO_FRAME)
                .focusable()
                .tab_stop(true)
                .on_key_down(stepped),
        )
}

fn zoom(
    size: (u32, u32),
    crop: Crop,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> Option<impl IntoElement> {
    let most = Crop::most(size);
    if most <= 1. {
        return None;
    }
    let travel = FRAME - f32::from(THUMB);
    let at = crop.zoom.ln() / most.ln();
    let slid = cx.listener(move |v: &mut Settings, delta: &(Pixels, Pixels), _, cx| {
        v.slide_zoom(f32::from(delta.0), travel, cx)
    });
    let stepped = cx.listener(move |v: &mut Settings, event: &KeyDownEvent, _, cx| {
        let by = match event.keystroke.key.as_str() {
            "left" | "down" => -STEP,
            "right" | "up" => STEP,
            _ => return,
        };
        v.slide_zoom(f32::from(by), travel, cx)
    });
    let thumb = resize_handle(
        "settings/photo/zoom",
        div()
            .size(THUMB)
            .rounded_full()
            .bg(theme.foreground)
            .border_2()
            .border_color(theme.background),
    )
    .on_drag(slid)
    .cursor(CursorStyle::ResizeLeftRight)
    .role(Role::Splitter)
    .aria_label(words::ZOOM)
    .aria_orientation(view::design::Orientation::Horizontal)
    .aria_numeric_value(f64::from(crop.zoom))
    .aria_min_numeric_value(1.)
    .aria_max_numeric_value(f64::from(most))
    .focusable()
    .tab_stop(true)
    .on_key_down(stepped);
    let half = f32::from(THUMB) / 2.;
    Some(
        div()
            .id("settings/photo/track")
            .relative()
            .w(px(FRAME))
            .h(THUMB)
            .child(
                div()
                    .absolute()
                    .left(px(half))
                    .right(px(half))
                    .top(px(half) - design::space::HAIR / 2.)
                    .h(design::space::HAIR)
                    .bg(theme.border_strong),
            )
            .child(div().absolute().top_0().left(px(at * travel)).child(thumb)),
    )
}
