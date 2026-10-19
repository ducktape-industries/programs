//! Expo `app/(tabs)/explorer/program/[name].tsx` (mobile dev 266633c):
//! one program, or one view-only entry, over the root. Its hook and role,
//! what its code's view asks of the host, the code and its size, the
//! params, who changes it, what the view declares, what is scheduled for
//! it, the way to its transactions. The reader's presses stay on the
//! Programs page's sheet. Reads the programs role, the code's manifest
//! (`manifest`), the window; writes the route and the clipboard (Copy).
use view::design::space;
use view::prelude::*;
use view::wire::manifest::Manifest;
use view::{Div, Stateful};

use crate::decode::{bytes_preview, plural};
use crate::state::{Declared, blob_ref};
use crate::words;
use crate::{Cx, Explorer, Network, Route};

use super::Look;
use super::parts::{
    Press, aside, copy_link, data, empty, key_value, opens, over, quiet, refused, scroller,
    section, small,
};

pub(super) fn program(view: &Explorer, name: &str, cx: Cx, k: &Look) -> AnyElement {
    let code = view.code_of(name);
    let copy = copy_link(view, &Route::Program(name.to_owned()), cx, k);
    let page = over("explorer-program", name.to_owned(), view, cx, k);
    let page = if code.is_some() {
        page.caption(words::entry_kind(view.runs(name).is_none()))
    } else {
        page
    };
    let page = match copy {
        Some(copy) => page.trailing(copy),
        None => page,
    };
    let body = match (&*view.network, code) {
        (Loadable::Failed(refusal), _) => refused(&design::sentence(refusal), cx, k),
        (Loadable::Idle | Loadable::Loading(_), _) => {
            quiet("explorer-program-loading", words::READING_ROLE, k)
        }
        (Loadable::Ready(_) | Loadable::Reloading(..), None) => empty(
            "explorer-no-program",
            "search-x",
            words::nothing_called(name),
            words::NO_PROGRAM_HERE,
            k,
        ),
        (Loadable::Ready(network) | Loadable::Reloading(network, _), Some(code)) => {
            detail(view, network, name, &code, cx, k)
        }
    };
    page.child(body).into_any_element()
}

/// The page under its title row (Expo `Detail`).
fn detail(
    view: &Explorer,
    network: &Network,
    name: &str,
    code: &abi::BlobId,
    cx: Cx,
    k: &Look,
) -> AnyElement {
    let listed = view.runs(name).is_none();
    scroller("explorer-page")
        .children(standing(network, name, k))
        .children(capabilities(view, listed, cx, k))
        .child(section("explorer-about-header", "About", None, k))
        .children(about(view, network, name, code, cx, k))
        .children(scheduled(view, network, name, k))
        .children((!listed).then(|| transactions(view, name, cx, k)))
        .into_any_element()
}

/// The hook and the role it fills, as badges; neither, nothing.
fn standing(network: &Network, name: &str, k: &Look) -> Option<Div> {
    let hook = network.is_hook(name);
    let role = network.role_of(name);
    if !hook && role.is_none() {
        return None;
    }
    let badge = |id: &'static str, label: String| {
        design::badge(id, label, &k.ladder, k.theme.muted, k.theme.surface_raised)
    };
    Some(
        div()
            .flex()
            .gap(space::SM)
            .px(space::BLOCK)
            .pt(space::MD)
            .children(hook.then(|| badge("explorer-hook", "hook".into())))
            .children(role.map(|role| badge("explorer-role", role.to_owned()))),
    )
}

/// The manifest the code's view carries, once read and readable.
fn view_manifest(view: &Explorer) -> Option<&Manifest> {
    view.manifest
        .ready()
        .and_then(Option::as_ref)
        .and_then(|blob| blob.view.as_ref())
        .and_then(|carried| carried.manifest.as_ref())
}

/// What the code's view asks of the host, a row each, under its head with
/// their count; or why none is shown (Expo `Capabilities`).
fn capabilities(view: &Explorer, listed: bool, cx: Cx, k: &Look) -> Vec<AnyElement> {
    let count = view_manifest(view).map(|manifest| {
        aside(plural(
            manifest.capabilities.len() as u64,
            "capability",
            "capabilities",
        ))
    });
    let head = section("explorer-capabilities-header", "Capabilities", count, k);
    let rows = match view.declared(listed) {
        Declared::Reading => vec![quiet("explorer-manifest-loading", words::READING_CODE, k)],
        Declared::Refused(refusal) => vec![refused(&design::sentence(refusal), cx, k)],
        Declared::Nothing(why) => vec![quiet("explorer-unlisted", why.sentence(), k)],
        Declared::Asks(manifest) => manifest
            .capabilities
            .iter()
            .map(|capability| {
                key_value(
                    format!("explorer-capability-{}", capability.as_str()),
                    capability.as_str(),
                    div().child(words::allows(*capability)),
                    k,
                )
                .into_any_element()
            })
            .collect(),
    };
    std::iter::once(head.into_any_element())
        .chain(rows)
        .collect()
}

fn about(
    view: &Explorer,
    network: &Network,
    name: &str,
    code: &abi::BlobId,
    cx: Cx,
    k: &Look,
) -> Vec<AnyElement> {
    let blob = view.manifest.ready().and_then(Option::as_ref);
    let declared = view_manifest(view);
    let entry = view.runs(name);
    let mut rows = vec![key_value(
        "explorer-field-code",
        "Code",
        code_and_copy(code, cx, k),
        k,
    )];
    rows.extend(network.source_of(name).map(|source| {
        let built = words::built_from(source);
        let built = match source {
            ::programs::Provenance::Commit(_) => data(built),
            _ => div().child(built),
        };
        key_value("explorer-field-built", "Built from", built, k)
    }));
    rows.extend(blob.map(|blob| {
        key_value(
            "explorer-field-size",
            "Size",
            div().child(words::code_size(blob, entry.is_none())),
            k,
        )
    }));
    rows.extend(
        entry.map(|entry| key_value("explorer-field-params", "Params", params(&entry.params), k)),
    );
    rows.push(key_value(
        "explorer-field-changed-by",
        "Changed by",
        div().child(words::changed_by(network.fills_a_role(name))),
        k,
    ));
    rows.extend(declared.into_iter().flat_map(|manifest| {
        let says = if manifest.description.is_empty() {
            manifest.name.clone()
        } else {
            format!("{} · {}", manifest.name, manifest.description)
        };
        [
            Some(key_value(
                "explorer-field-declares",
                "Declares",
                div().child(says),
                k,
            )),
            Some(key_value(
                "explorer-field-min-width",
                "Min width",
                div().child(words::min_width(manifest.min_width)),
                k,
            )),
            Some(key_value(
                "explorer-field-wire",
                "Wire",
                data(manifest.wire_id.clone()),
                k,
            )),
        ]
        .into_iter()
        .flatten()
    }));
    rows.into_iter()
        .map(IntoElement::into_any_element)
        .collect()
}

/// The code id in the data face, and a small Copy beside it.
fn code_and_copy(code: &abi::BlobId, cx: Cx, k: &Look) -> Div {
    let id = blob_ref(code);
    let copy: Press = {
        let id = id.clone();
        Box::new(
            cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
                view.copy(id.clone(), words::CODE_ID, cx)
            }),
        )
    };
    div()
        .flex()
        .items_center()
        .gap(space::SM)
        .child(data(id).flex_1().min_w_0())
        .child(small(
            "explorer-copy-code",
            "Copy",
            design::Kind::Outline,
            Some(copy),
            k,
        ))
}

/// The params it runs with: "none", or their length and ends in the data
/// face.
fn params(params: &[u8]) -> Div {
    if params.is_empty() {
        div().child(words::NO_PARAMS)
    } else {
        data(bytes_preview(params))
    }
}

/// What is scheduled for it, under its head, each row as the Programs
/// page says it; none, a quiet line.
fn scheduled(view: &Explorer, network: &Network, name: &str, k: &Look) -> Vec<AnyElement> {
    let head = section("explorer-program-scheduled-header", "Scheduled", None, k);
    let rows: Vec<AnyElement> = network
        .changes
        .iter()
        .filter(|pending| pending.change.program() == name)
        .map(|pending| super::programs::scheduled_row(view, network, pending, k).into_any_element())
        .collect();
    let none = rows.is_empty().then(|| {
        quiet(
            "explorer-program-no-changes",
            words::NOTHING_SCHEDULED_FOR_IT,
            k,
        )
    });
    std::iter::once(head.into_any_element())
        .chain(rows)
        .chain(none)
        .collect()
}

/// The way to its transactions: how many the window holds, and over how
/// many blocks.
fn transactions(view: &Explorer, name: &str, cx: Cx, k: &Look) -> Stateful<Div> {
    let blocks = view.chain.blocks.len() as u64;
    let title = words::to_transactions(view.sent_to(name), blocks);
    let row = design::row("explorer-program-txs", &k.ladder, &k.theme);
    opens(
        row,
        title.clone(),
        Route::Transactions(Some(name.to_owned())),
        cx,
        k,
    )
    .child(div().flex_1().truncate().child(title))
}
