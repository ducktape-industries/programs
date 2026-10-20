//! A file: the path as crumbs, its name over its kind and size, the writes the reader may make to
//! it, then what it holds: its picture, a markdown file drawn, its text, where a link points, or
//! how big it is. Split, the same without the crumbs, beside the folder it is in.
use files::Node;

use super::*;
use crate::state::{Holds, Opened, name_of};

const PICTURE_H: Pixels = px(560.);

pub(super) fn file(view: &Files, path: &str, cx: Cx, k: &Kit) -> AnyElement {
    div()
        .id("files-file")
        .flex()
        .flex_col()
        .child(crumbs(path, cx, k))
        .child(detail(view, path, cx, k))
        .into_any_element()
}

pub(super) fn detail(view: &Files, path: &str, cx: Cx, k: &Kit) -> AnyElement {
    let page = div().id("files-file-detail").flex().flex_col();
    let Some(file) = view.file_at(path) else {
        let waiting = match view.file.failed() {
            Some(refusal) => refused("files-file", refusal, cx, k),
            None => reading("files-file-reading", words::READING_FILE, k),
        };
        return page.child(waiting).into_any_element();
    };
    let Some(entry) = file.file() else {
        return page
            .child(design::empty_state(
                "files-gone",
                "file-question-mark",
                name_of(path).to_owned(),
                words::GONE,
                &k.ladder,
                &k.theme,
            ))
            .into_any_element();
    };
    let under = format!(
        "{} · {}",
        words::kind(&entry.node, file.holds()),
        words::size(file.size())
    );
    page.child(title("files-file-title", name_of(path), Some(under), k))
        .children(writes(view, cx, k))
        .children(crate::policies::row(view, cx, &k.ladder, &k.theme))
        .child(body(file, &entry.node, cx, k))
        .into_any_element()
}

fn body(file: &Opened, node: &Node, cx: Cx, k: &Kit) -> AnyElement {
    let body = div().id("files-file-body").px_5().py_3();
    if let Node::Symlink { target } = node {
        return body
            .child(line(words::TARGET, data(target.clone()), k))
            .into_any_element();
    }
    if file.holds() == Holds::Picture {
        return body.child(picture(file, k)).into_any_element();
    }
    let Some(text) = file.text() else {
        return body
            .text_color(k.theme.muted)
            .child(words::not_text(file.size()))
            .into_any_element();
    };
    let cut = (!file.whole).then(|| {
        div()
            .pb_2()
            .text_size(design::text::SECONDARY)
            .text_color(k.theme.muted)
            .child(words::cut(file.size()))
    });
    if let Some(doc) = &file.doc {
        let links = crate::actions::links(&file.path, cx);
        return body
            .children(cut)
            .child(markdown::desktop::render_blocks(
                "files-markdown",
                doc,
                &k.theme,
                &links,
            ))
            .into_any_element();
    }
    body.children(cut)
        .child(
            div()
                .id("files-text")
                .cursor_text()
                .p_3()
                .border_1()
                .border_color(k.theme.border)
                .bg(k.theme.surface)
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::MONO)
                .child(text.to_owned()),
        )
        .into_any_element()
}

fn picture(file: &Opened, k: &Kit) -> AnyElement {
    let theme = k.theme;
    let quiet = move |sentence: String| {
        design::quiet(sentence, &theme)
            .id("files-picture-note")
            .into_any_element()
    };
    match words::picture(file) {
        Ok(picture) => img(picture.clone())
            .id("files-picture")
            .max_w_full()
            .max_h(PICTURE_H)
            .object_fit(ObjectFit::ScaleDown)
            .role(Role::Image)
            .aria_label(name_of(&file.path).to_owned())
            .with_fallback(move || quiet(words::NOT_DRAWN.to_owned()))
            .into_any_element(),
        Err(sentence) => quiet(sentence),
    }
}
