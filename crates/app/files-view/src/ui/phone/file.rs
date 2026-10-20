//! A file's screen: its name over its path, its writes behind the title row's control, what it is
//! and how big, then its picture, a markdown file drawn, its text, where a link points, or that it
//! is neither; nearest Expo screen `mobile/app/(tabs)/forge/[repo]/code/[...path].tsx`.
use files::Node;
use view::design::space;

use super::*;
use crate::state::{Holds, Opened, name_of};

pub(super) fn file(view: &Files, path: &str, cx: Cx, k: &Kit) -> AnyElement {
    let page = page("files-file", name_of(path).to_owned(), view, cx, k).caption(path.to_owned());
    let body = match view.file_at(path) {
        Some(file) => opened(file, cx, k),
        None => match view.file.failed() {
            Some(refusal) => refused("files-file", refusal, cx, k),
            None => quiet("files-file-reading", words::READING_FILE, k),
        },
    };
    let policy = crate::policies::row(view, cx, &k.ladder, &k.theme);
    page.child(scroller("files-page").children(policy).child(body))
        .into_any_element()
}

fn opened(file: &Opened, cx: Cx, k: &Kit) -> AnyElement {
    let Some(entry) = file.file() else {
        let name = name_of(&file.path).to_owned();
        return design::empty_state(
            "files-gone",
            "file-question-mark",
            name,
            words::GONE,
            &k.ladder,
            &k.theme,
        )
        .into_any_element();
    };
    let facts = div()
        .flex()
        .flex_col()
        .child(key_value(
            words::KIND,
            words::kind(&entry.node, file.holds()),
            k,
        ))
        .child(key_value(words::SIZE, words::size(file.size()), k));
    let shown = match (&entry.node, file.holds(), file.text()) {
        (Node::Symlink { target }, _, _) => {
            key_value(words::TARGET, data(target.clone(), k), k).into_any_element()
        }
        (_, Holds::Picture, _) => picture(file, k),
        (_, _, Some(text)) => text_block(text, file, cx, k),
        (_, _, None) => quiet("files-not-text", words::not_text(file.size()), k),
    };
    div()
        .flex()
        .flex_col()
        .child(facts)
        .child(shown)
        .into_any_element()
}

// The text in the data face, or a markdown file drawn, and, read only in
// part, how much shows.
fn text_block(text: &str, file: &Opened, cx: Cx, k: &Kit) -> AnyElement {
    let cut = (!file.whole).then(|| {
        div()
            .pb(space::SM)
            .text_type(k.ladder.caption)
            .text_color(k.theme.muted)
            .child(words::cut(file.size()))
    });
    let shown = match &file.doc {
        Some(doc) => {
            let links = crate::actions::links(&file.path, cx);
            markdown::phone::markdown("files-markdown", doc, &links, &k.ladder, &k.theme)
        }
        None => data(text.to_owned(), k).into_any_element(),
    };
    div()
        .id("files-text")
        .p(space::BLOCK)
        .flex()
        .flex_col()
        .children(cut)
        .child(shown)
        .into_any_element()
}

fn picture(file: &Opened, k: &Kit) -> AnyElement {
    let theme = k.theme;
    let ladder = k.ladder;
    match words::picture(file) {
        Ok(picture) => div()
            .p(space::BLOCK)
            .child(
                img(picture.clone())
                    .id("files-picture")
                    .max_w_full()
                    .object_fit(ObjectFit::ScaleDown)
                    .role(Role::Image)
                    .aria_label(name_of(&file.path).to_owned())
                    .with_fallback(move || {
                        quiet(
                            "files-picture-note",
                            words::NOT_DRAWN,
                            &Kit { theme, ladder },
                        )
                    }),
            )
            .into_any_element(),
        Err(sentence) => quiet("files-picture-note", sentence, k),
    }
}
