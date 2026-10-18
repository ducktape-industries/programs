//! The Programs tab: what the programs role runs, lists and will change, who
//! changes each name, and the presses the reader's account or voices
//! allow (`acting.rs`). A form opens under the list it changes, outside
//! it: a list takes the arrows and Enter, a form's fields need them. One
//! program, or one view-only entry, opens as a page ([`program`]): what its
//! code declares first, then the code, who changes it and what is coming.
use super::*;
use crate::Network;
use crate::acting::{self, Asked, Edit, FormField, Path};
use crate::decode::{self, bytes_preview};
use crate::state::{Declared, Unlisted, blob_ref};
use ::programs::{NOTICE, Pending};
use view::design::Ladder;

pub(super) fn programs(view: &Explorer, cx: Cx, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let network = match &view.network {
        Loadable::Ready(network) | Loadable::Reloading(network, _) => network,
        Loadable::Failed(refusal) => return failed(&design::sentence(refusal), cx, theme, ladder),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-programs-loading", words::READING_ROLE, theme);
        }
    };
    if network.is_empty() {
        return empty_state(
            "explorer-empty",
            "package",
            "No programs",
            words::RUNS_NOTHING,
            ladder,
            theme,
        )
        .into_any_element();
    }
    let add = view.adds().then(|| {
        press("explorer-add", "Add", cx, theme, |view, cx| {
            view.open_form(Edit::Add, Path::Account, cx)
        })
    });
    let count = caption(
        plural(network.programs.len() as u64, "program", "programs"),
        theme,
    );
    let running = running(view, network, cx, theme, ladder);
    let views = listed(view, network, cx, theme, ladder);
    let scheduled: Vec<_> = network
        .changes
        .iter()
        .enumerate()
        .map(|(index, change)| scheduled(view, index, change, cx, theme, ladder))
        .collect();
    let nothing_scheduled = scheduled
        .is_empty()
        .then(|| quiet("explorer-no-changes", words::NOTHING_SCHEDULED, theme));
    let running_form = form(view, |edit| !edits_a_view(edit), cx, theme, ladder);
    let views_form = form(view, edits_a_view, cx, theme, ladder);
    div()
        .id("explorer-list")
        .child(heading(
            "explorer-running-header",
            "Running",
            Some(beside([Some(count), add])),
            theme,
        ))
        .child(running)
        .children(running_form)
        .when_some(views, |list, views| {
            list.child(heading(
                "explorer-views-header",
                "Views",
                Some(caption(
                    plural(network.views.len() as u64, "view", "views"),
                    theme,
                )),
                theme,
            ))
            .child(views)
            .children(views_form)
        })
        .child(heading(
            "explorer-scheduled-header",
            "Scheduled",
            None,
            theme,
        ))
        .children(scheduled)
        .children(nothing_scheduled)
        .child(access(view, theme))
        .into_any_element()
}

fn edits_a_view(edit: &Edit) -> bool {
    matches!(
        edit,
        Edit::Upgrade { view: true, .. } | Edit::Remove { view: true, .. }
    )
}

/// What sits on a heading's right, in a row.
fn beside(parts: impl IntoIterator<Item = Option<AnyElement>>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_3()
        .children(parts.into_iter().flatten())
        .into_any_element()
}

fn running(
    view: &Explorer,
    network: &Network,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let list = rows("explorer-programs-list", "Programs", view);
    network
        .programs
        .iter()
        .fold(list, |list, entry| {
            let route = Route::Program(entry.program.clone());
            let row = row(
                SharedString::from(format!("explorer-program-{}", entry.program)).into(),
                entry.program.clone(),
                route.clone(),
                cx,
                theme,
            )
            .child(named(network, &entry.program, theme, ladder))
            .child(
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(plural(
                        entry.params.len() as u64,
                        "param byte",
                        "param bytes",
                    )),
            )
            .children(stamp(network, &entry.program, theme))
            .child(mono(short(entry.code.digest())).text_color(theme.muted))
            .children(presses(view, network, &entry.program, false, cx, theme));
            list.row(route, row, theme)
        })
        .build(cx)
}

/// The view-only entries, each a row that opens its page; `None` where the
/// programs role lists none.
fn listed(
    view: &Explorer,
    network: &Network,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<Stateful<Div>> {
    if network.views.is_empty() {
        return None;
    }
    let list = rows("explorer-views-list", "Views", view);
    let list = network.views.iter().fold(list, |list, listed| {
        let route = Route::Program(listed.name.clone());
        let row = row(
            SharedString::from(format!("explorer-view-{}", listed.name)).into(),
            listed.name.clone(),
            route.clone(),
            cx,
            theme,
        )
        .child(named(network, &listed.name, theme, ladder))
        .child(muted(words::VIEW_ONLY, theme))
        .child(mono(short(listed.view.digest())).text_color(theme.muted))
        .children(presses(view, network, &listed.name, true, cx, theme));
        list.row(route, row, theme)
    });
    Some(list.build(cx))
}

// The commit a program's code was built from, on its row; nothing for
// code that names none.
fn stamp(network: &Network, name: &str, theme: &Theme) -> Option<AnyElement> {
    let source @ ::programs::Provenance::Commit(_) = network.source_of(name)? else {
        return None;
    };
    let built = mono(words::built_from(source)).text_color(theme.muted);
    let id = SharedString::from(format!("explorer-built-{name}"));
    Some(built.id(id).into_any_element())
}

fn named(network: &Network, name: &str, theme: &Theme, ladder: &Ladder) -> Div {
    let hook = network.is_hook(name);
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .overflow_hidden()
        .child(mono(name.to_owned()))
        .children(hook.then(|| chip(format!("explorer-hook-{name}"), "hook", theme, ladder)))
        .child(changers(network, name, theme))
}

fn changers(network: &Network, name: &str, theme: &Theme) -> AnyElement {
    match network.fills_a_role(name) {
        true => muted(words::VALIDATORS_VOTE, theme),
        false => muted(words::ANY_ACCOUNT, theme),
    }
}

pub(super) fn program(
    view: &Explorer,
    name: &str,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let network = match &view.network {
        Loadable::Ready(network) | Loadable::Reloading(network, _) => network,
        Loadable::Failed(refusal) => return failed(&design::sentence(refusal), cx, theme, ladder),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-program-loading", words::READING_ROLE, theme);
        }
    };
    let entry = view.runs(name);
    let Some(code) = view.code_of(name) else {
        return empty_state(
            "explorer-no-program",
            "search-x",
            words::nothing_called(name),
            words::NO_PROGRAM_HERE,
            ladder,
            theme,
        )
        .into_any_element();
    };
    let listed = entry.is_none();
    let hook = network.is_hook(name);
    let role = network.role_of(name);
    let blob = view.manifest.ready().and_then(|blob| blob.as_ref());
    let manifest = blob
        .and_then(|blob| blob.view.as_ref())
        .and_then(|view| view.manifest.as_ref());
    let title = div()
        .flex()
        .items_center()
        .gap_3()
        .pr_5()
        .child(titled(words::entry_kind(listed), name.to_owned(), theme))
        .children(hook.then(|| chip("explorer-program-hook".into(), "hook", theme, ladder)))
        .children(role.map(|role| chip("explorer-program-role".into(), role, theme, ladder)))
        .child(div().flex_1())
        .children(copy_button(
            view,
            &Route::Program(name.to_owned()),
            cx,
            theme,
        ))
        .children(presses(view, network, name, listed, cx, theme));
    let page_form = form(
        view,
        |edit| match edit {
            Edit::Upgrade { name: edited, .. } | Edit::Remove { name: edited, .. } => {
                edited == name
            }
            Edit::Add => false,
        },
        cx,
        theme,
        ladder,
    );
    let count = manifest.map(|manifest| {
        caption(
            plural(
                manifest.capabilities.len() as u64,
                "capability",
                "capabilities",
            ),
            theme,
        )
    });
    let id = blob_ref(&code);
    let copy = {
        let id = id.clone();
        press("explorer-copy-code", "Copy", cx, theme, move |view, cx| {
            view.copy(id.clone(), words::CODE_ID, cx)
        })
    };
    let code_field = div()
        .flex_1()
        .flex()
        .items_center()
        .gap_3()
        .min_w_0()
        .child(
            mono(id)
                .id("explorer-code-id")
                .text_size(design::text::SECONDARY)
                .overflow_hidden(),
        )
        .child(copy);
    let sizes = blob.map(|blob| words::code_size(blob, listed));
    let built = network.source_of(name).map(|source| match source {
        ::programs::Provenance::Commit(_) => mono(words::built_from(source)).into_any_element(),
        _ => muted(&words::built_from(source), theme),
    });
    let params = entry.map(|entry| {
        if entry.params.is_empty() {
            muted(words::NO_PARAMS, theme)
        } else {
            mono(bytes_preview(&entry.params)).into_any_element()
        }
    });
    let about = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(changers(network, name, theme));
    let declares = manifest.map(|manifest| {
        let mut words = manifest.name.clone();
        if !manifest.description.is_empty() {
            words = format!("{words} · {}", manifest.description);
        }
        words
    });
    let talks_to = manifest
        .filter(|manifest| !manifest.targets.is_empty())
        .map(|manifest| manifest.targets.join(", "));
    let blocks = view.chain.blocks.len() as u64;
    let sent = view.sent_to(name);
    // no window yet: the way to its transactions, without a count of nothing
    let transactions = (!listed).then(|| {
        line("explorer-program-transactions".into(), theme).child(link(
            "explorer-program-txs".into(),
            words::to_transactions(sent, blocks),
            Route::Transactions(Some(name.to_owned())),
            cx,
            theme,
        ))
    });
    let scheduled: Vec<AnyElement> = network
        .changes
        .iter()
        .enumerate()
        .filter(|(_, pending)| pending.change.program() == name)
        .map(|(index, pending)| scheduled(view, index, pending, cx, theme, ladder))
        .collect();
    let nothing_scheduled = scheduled.is_empty().then(|| {
        quiet(
            "explorer-program-no-changes",
            words::NOTHING_SCHEDULED_FOR_IT,
            theme,
        )
    });
    div()
        .id("explorer-program")
        .flex()
        .flex_col()
        .child(title)
        .children(page_form)
        .child(heading(
            "explorer-capabilities-header",
            "Capabilities",
            count,
            theme,
        ))
        .children(capabilities(view, listed, cx, theme, ladder))
        .child(heading("explorer-about-header", "About", None, theme))
        .child(field("Code", code_field, theme))
        .children(built.map(|built| field("Built from", built, theme)))
        .children(sizes.map(|sizes| field("Size", muted(&sizes, theme), theme)))
        .children(params.map(|params| field("Params", params, theme)))
        .child(field("Changed by", about, theme))
        .children(declares.map(|words| field("Declares", muted(&words, theme), theme)))
        .children(manifest.map(|manifest| {
            field(
                "Min width",
                muted(&words::min_width(manifest.min_width), theme),
                theme,
            )
        }))
        .children(talks_to.map(|targets| field("Talks to", mono(targets), theme)))
        .children(manifest.map(|manifest| {
            field(
                "Wire",
                mono(manifest.wire_id.clone()).text_color(theme.muted),
                theme,
            )
        }))
        .child(heading(
            "explorer-program-scheduled-header",
            "Scheduled",
            None,
            theme,
        ))
        .children(scheduled)
        .children(nothing_scheduled)
        .children(transactions)
        .into_any_element()
}

/// The capabilities the code's view declares, a line each, or why none are
/// shown: unread yet, refused (with a retry), code the node does not hold,
/// a program that draws nothing, a manifest this app cannot read.
fn capabilities(
    view: &Explorer,
    listed: bool,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let manifest = match view.declared(listed) {
        Declared::Reading => {
            return vec![quiet(
                "explorer-manifest-loading",
                words::READING_CODE,
                theme,
            )];
        }
        Declared::Refused(refusal) => {
            return vec![failed(&design::sentence(refusal), cx, theme, ladder)];
        }
        Declared::Nothing(why) => {
            let id = match why {
                Unlisted::NoCode => "explorer-no-code",
                Unlisted::NoView => "explorer-no-view",
                Unlisted::NoListedView | Unlisted::Unreadable => "explorer-unread-view",
                Unlisted::AsksNothing => "explorer-no-capabilities",
            };
            return vec![quiet(id, why.sentence(), theme)];
        }
        Declared::Asks(manifest) => manifest,
    };
    manifest
        .capabilities
        .iter()
        .map(|capability| {
            line(
                SharedString::from(format!("explorer-capability-{}", capability.as_str())).into(),
                theme,
            )
            .child(
                mono(capability.as_str().to_owned())
                    .w(LABEL_W)
                    .flex_shrink_0(),
            )
            .child(div().child(words::allows(*capability)))
            .into_any_element()
        })
        .collect()
}

fn muted(text: &str, theme: &Theme) -> AnyElement {
    div()
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(text.to_owned())
        .into_any_element()
}

fn chip(id: String, label: &str, theme: &Theme, ladder: &Ladder) -> AnyElement {
    design::badge(
        SharedString::from(id),
        label.to_owned(),
        ladder,
        theme.muted,
        theme.surface,
    )
    .into_any_element()
}

/// [`Explorer::edits`] as presses beside a row or a title.
fn presses(
    view: &Explorer,
    network: &Network,
    name: &str,
    listed: bool,
    cx: Cx,
    theme: &Theme,
) -> Vec<AnyElement> {
    view.edits(network, name, listed)
        .into_iter()
        .map(|offer| {
            let (edit, path) = (offer.edit, offer.path);
            press(edit.press_id(), offer.word, cx, theme, move |view, cx| {
                view.open_form(edit.clone(), path, cx)
            })
        })
        .collect()
}

fn scheduled(
    view: &Explorer,
    index: usize,
    pending: &Pending,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let change = &pending.change;
    let (foreground, background) = match change.code() {
        None => (theme.danger, theme.danger_soft),
        Some(_) => (theme.accent_foreground, theme.accent_soft),
    };
    let by = match pending.by {
        Some(number) => format!("by {}", view.name_of(number)),
        None => words::BY_VOTE.into(),
    };
    let window = pending.by.and(view.status.ready()).map(|status| {
        let blocks = pending.height.saturating_sub(status.height);
        format!(
            "validators can cancel for {} min",
            decode::minutes(blocks, status.block_time_ms)
        )
    });
    let program = change.program();
    let cancel = view.cancels(pending).map(|path| {
        let op = acting::cancel(pending);
        let label = path.cancel_word();
        press(
            format!("explorer-cancel-{index}"),
            label,
            cx,
            theme,
            move |view, cx| view.press(path, op.clone(), cx),
        )
    });
    line(ElementId::named_usize("explorer-change", index), theme)
        .child(design::badge(
            ElementId::named_usize("explorer-change-verb", index),
            change.verb(),
            ladder,
            foreground,
            background,
        ))
        .child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .gap_2()
                .child(mono(program.to_string()))
                .child(muted(&by, theme))
                .children(window.map(|window| muted(&window, theme))),
        )
        .child(
            div()
                .text_size(design::text::SECONDARY)
                .text_color(theme.muted)
                .child(format!("at {}", pending.height)),
        )
        .children(
            change
                .code()
                .map(|code| mono(short(code.digest())).text_color(theme.muted)),
        )
        .children(cancel)
        .into_any_element()
}

fn access(view: &Explorer, theme: &Theme) -> Div {
    let minutes = view
        .status
        .ready()
        .map(|status| decode::minutes(NOTICE, status.block_time_ms));
    let notice = caption(words::account_wait(minutes), theme);
    div()
        .child(heading(
            "explorer-access-header",
            "Access",
            Some(notice),
            theme,
        ))
        .child(quiet(
            "explorer-access-rule",
            words::OPEN_TO_ACCOUNTS,
            theme,
        ))
}

/// The open form, when `here` says it belongs under this list.
fn form(
    view: &Explorer,
    here: impl Fn(&Edit) -> bool,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let form = view.acting.form.as_ref().filter(|form| here(&form.edit))?;
    let network = view.network.ready()?;
    let tip = view.status.ready().map_or(0, |status| status.height);
    let title = form.title();
    let field = |field: FormField| {
        Input::new(field.id(), form.text(field), field.label())
            .h(design::size::CONTROL)
            .w(px(field_width(field)))
            .px_2()
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.background)
            .text_size(design::text::SECONDARY)
            .placeholder(field.hint())
            .into_any_element()
    };
    let kind = |view_kind: bool, label: &str| {
        let pick = cx.listener(move |view: &mut Explorer, _: &ClickEvent, _, cx| {
            view.add_view(view_kind, cx)
        });
        design::button(
            SharedString::from(format!("explorer-form-{label}")),
            label.to_owned(),
            ladder,
            theme,
            pick,
        )
        .selected(form.view == view_kind)
        .into_any_element()
    };
    let fields: Vec<AnyElement> = form
        .asks()
        .into_iter()
        .flat_map(|asked| match asked {
            Asked::Field(asked) => vec![field(asked)],
            Asked::Kind => vec![kind(false, "program"), kind(true, "view")],
        })
        .collect();
    let notes = form.notes(network, tip);
    let submit = form.verb();
    let send = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.submit_form(cx));
    let close = cx.listener(|view: &mut Explorer, _: &ClickEvent, _, cx| view.close_form(cx));
    Some(
        div()
            .id("explorer-form")
            .flex()
            .flex_col()
            .gap_2()
            .px_5()
            .py_3()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .child(design::heading("explorer-form-title", title, 3, theme))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .children(fields),
            )
            .children(notes.into_iter().map(|note| {
                div()
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.muted)
                    .child(note)
            }))
            .children(form.problem.as_ref().map(|problem| {
                div()
                    .id("explorer-form-problem")
                    .text_size(design::text::SECONDARY)
                    .text_color(theme.danger)
                    .child(problem.sentence.clone())
            }))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        design::button("explorer-form-submit", submit, ladder, theme, send)
                            .kind(design::Kind::Primary)
                            .enabled(!view.acting.busy),
                    )
                    .child(
                        design::button("explorer-form-close", "Close", ladder, theme, close)
                            .kind(design::Kind::Outline),
                    ),
            )
            .into_any_element(),
    )
}

/// How wide a field is drawn, for what it takes.
fn field_width(field: FormField) -> f32 {
    match field {
        FormField::Height => 140.,
        FormField::Name | FormField::Params => 160.,
        FormField::Code => 280.,
    }
}

/// A list row that opens nothing.
fn line(id: ElementId, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_4()
        .h(ROW_H)
        .px_5()
        .border_b_1()
        .border_color(theme.border)
}
