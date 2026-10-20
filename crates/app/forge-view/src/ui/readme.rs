//! The README tab: the root README rendered whole, the repository's front
//! page. The Code tab is where the tree lives.
use view::prelude::*;
use view::{Div, Stateful};

use crate::Forge;
use crate::markdown::links;
use crate::ui::code::no_commits;
use crate::ui::components::{empty_state, id, path_text, quiet};
use crate::ui::staged;
use crate::words;
use forge::{BlobView, Content, Query, Reply};
use view::design::Ladder;

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let page = div()
        .id(id("forge-readme"))
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    let Some(root) = forge.tree_query(Vec::new()) else {
        return page
            .child(match forge.refs() {
                Some(_) => no_commits(forge, theme, ladder),
                None => quiet(words::RESOLVING_REF, theme),
            })
            .into_any_element();
    };
    if let Err(state) = staged(
        forge,
        &root,
        "forge-readme-tree",
        words::READING_TREE,
        cx,
        theme,
        ladder,
    ) {
        return page.child(state).into_any_element();
    }
    let Some((name, oid)) = forge.readme() else {
        return page
            .child(empty_state(
                id("forge-readme-none"),
                "file-text",
                words::NO_README,
                "This ref carries no README at its root. The Code tab has its files.",
                ladder,
                theme,
            ))
            .into_any_element();
    };
    let query = Query::Blob {
        repo: forge.repo_name(),
        oid: oid.clone(),
        range: None,
    };
    let reply = match staged(
        forge,
        &query,
        "forge-readme-blob",
        words::READING_README,
        cx,
        theme,
        ladder,
    ) {
        Ok(reply) => reply,
        Err(state) => return page.child(state).into_any_element(),
    };
    let Reply::Blob { blob, .. } = reply else {
        return page.into_any_element();
    };
    page.child(document(forge, &name, &oid, blob, cx, theme))
        .into_any_element()
}

const MEASURE: Pixels = px(880.);

fn document(
    forge: &Forge,
    name: &[u8],
    oid: &str,
    blob: &BlobView,
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Stateful<Div> {
    let body = if matches!(blob.content, Content::Text) {
        markdown::desktop::render_blocks(
            "forge-readme-body",
            &forge.blob_cache.doc(oid, &blob.bytes),
            theme,
            &links(Vec::new(), cx),
        )
    } else {
        quiet(words::README_NOT_TEXT, theme)
    };
    div()
        .id(id("forge-readme-page"))
        .w_full()
        .max_w(MEASURE)
        .px_6()
        .py_5()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .id(id("forge-readme-title"))
                .pb_2()
                .border_b_1()
                .border_color(theme.border)
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(path_text(name)),
        )
        .child(body)
}
