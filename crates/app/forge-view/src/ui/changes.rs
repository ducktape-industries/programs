//! Changes: the filter rail with "Needs my judgment" on top, the list, and
//! the form that opens or edits one.
use std::collections::BTreeMap;
use std::rc::Rc;

use view::prelude::*;
use view::{Div, wire};

use crate::Forge;
use crate::state::{ChangeForm, Filter};
use crate::ui::components::{badge, button, empty_state, heading, id, quiet};
use crate::ui::diff::Route;
use crate::ui::{pending, staged};
use crate::words;
use forge::{ChangeState, ChangeSummary, Judgment, Reply, ReviewCounts};
use view::design::Ladder;

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let ladder = *ladder;
    let mut column = div()
        .id(id("forge-changes"))
        .flex()
        .flex_col()
        .flex_1()
        .min_h(px(0.))
        .child(filters(forge, cx, theme, &ladder))
        .child(pending(forge, "changes", theme));
    if let Some(form) = forge.change_form() {
        column = column.child(self::form(form, forge, cx, theme, &ladder));
    }
    let Some(query) = forge.changes_query(&forge.repo_name()) else {
        return column
            .child(empty_state(
                id("forge-changes-nobody"),
                "user-round",
                words::NO_ACCOUNT,
                words::CREATE_AN_ACCOUNT,
                &ladder,
                theme,
            ))
            .into_any_element();
    };
    let reply = match staged(
        forge,
        &query,
        "forge-changes-list",
        words::READING_CHANGES,
        cx,
        theme,
        &ladder,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let Some(shared) = forge.shared(&query) else {
        return column.into_any_element();
    };
    // the changes shown, by their place in the reply: the title search
    // keeps them; a render reads none of the rest
    let needle = forge.change_search.text().trim().to_lowercase();
    let shown: Vec<usize> = (0..listed(reply))
        .filter(|at| {
            needle.is_empty()
                || summary_at(reply, *at)
                    .is_some_and(|(summary, _)| summary.title.to_lowercase().contains(&needle))
        })
        .collect();
    if shown.is_empty() {
        return column
            .child(empty_state(
                id("forge-changes-empty"),
                "git-pull-request",
                if forge.filter == Filter::Judgment {
                    words::NOTHING_WAITS
                } else {
                    words::NO_CHANGES_HERE
                },
                if forge.filter == Filter::Judgment {
                    words::NOTHING_ASKED
                } else {
                    words::COMPARE_TO_OPEN
                },
                &ladder,
                theme,
            ))
            .into_any_element();
    }
    // the names of the people who wrote them: few, each read once
    let mut authors = BTreeMap::new();
    for (summary, _) in shown.iter().filter_map(|at| summary_at(reply, *at)) {
        if !authors.contains_key(&summary.author) {
            authors.insert(
                summary.author.clone(),
                forge.principal_name(&summary.author),
            );
        }
    }
    // one Tab stop: ↑ ↓ walk the changes (a virtual list, scrolled to the
    // one moved to), Enter opens the active one
    let count = shown.len();
    let shown = Rc::new(shown);
    let (at, _) = crate::ui::components::cursor(forge, "forge-changes-list");
    let marks = Rc::new(Marks {
        authors,
        active: at.min(count.saturating_sub(1)),
        walked: crate::ui::components::walked(forge, "forge-changes-list"),
    });
    let pressed = (shared.clone(), shown.clone());
    let list = crate::ui::components::list(
        "forge-changes-list",
        "Changes",
        count,
        Some(&forge.changes_scroll),
        forge,
        cx,
        move |forge, index, _, cx| {
            let (reply, shown) = &pressed;
            let n = shown
                .get(index)
                .and_then(|at| summary_at(reply, *at))
                .map(|(summary, _)| summary.n);
            if n.is_some() {
                forge.open_change(n, cx)
            }
        },
    );
    let open: Route<u64> =
        Rc::new(cx.listener(|forge, n: &u64, _, cx| forge.open_change(Some(*n), cx)));
    let theme = *theme;
    let rows = crate::ui::components::rows(
        "forge-changes-rows",
        count,
        None,
        Some(&forge.changes_scroll),
        move |index| match shown.get(index).and_then(|at| summary_at(&shared, *at)) {
            Some((summary, judgment)) => {
                change_row(summary, judgment, index, &marks, &open, &theme, &ladder)
            }
            None => div().into_any_element(),
        },
    );
    column.child(list.child(rows)).into_any_element()
}

/// How many changes a changes or judgment reply lists.
fn listed(reply: &Reply) -> usize {
    match reply {
        Reply::Changes { page, .. } => page.items.len(),
        Reply::Judgment { page, .. } => page.items.len(),
        _ => 0,
    }
}

/// Change `at` of a changes or judgment reply, and what it asks of me when
/// listed by judgment.
fn summary_at(reply: &Reply, at: usize) -> Option<(&ChangeSummary, Option<&Judgment>)> {
    match reply {
        Reply::Changes { page, .. } => page.items.get(at).map(|summary| (summary, None)),
        Reply::Judgment { page, .. } => page
            .items
            .get(at)
            .map(|judgment| (&judgment.change, Some(judgment))),
        _ => None,
    }
}

/// What marks the changes' rows: their authors' names, the row the arrows
/// are on, and whether they have walked the list.
struct Marks {
    authors: BTreeMap<forge::Principal, String>,
    active: usize,
    walked: bool,
}

/// The title search field.
const SEARCH_W: Pixels = px(220.);
/// A row's state dot.
const DOT: Pixels = px(6.);

/// One change: its state dot, title and number, `from → into · author`
/// under it, and on the right its verdicts, its comments, and what it asks
/// of me when listed by judgment.
fn change_row(
    summary: &ChangeSummary,
    judgment: Option<&Judgment>,
    index: usize,
    marks: &Marks,
    open: &Route<u64>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let n = summary.n;
    let open = {
        let open = open.clone();
        move |_: &ClickEvent, window: &mut Window, app: &mut App| open(&n, window, app)
    };
    let author = marks
        .authors
        .get(&summary.author)
        .cloned()
        .unwrap_or_default();
    let (active, walked) = (index == marks.active, marks.walked);
    let dot = match summary.state {
        ChangeState::Open => theme.success,
        ChangeState::Merged => theme.agent,
        ChangeState::Closed => theme.faint,
    };
    let theme_ = *theme;
    // as wide as the list: its counts on the right, its rule across
    let mut line = div()
        .id(id(format!("forge-change-{n}")))
        .w_full()
        .flex()
        .items_center()
        .gap(design::space::LG)
        .px(crate::ui::PAGE_X)
        .py(design::space::MD)
        .border_b_1()
        .border_color(theme.border)
        .hover(move |style| style.bg(theme_.surface))
        .when(walked && active, |line| line.bg(theme_.surface))
        .aria_selected(false)
        .on_click(open)
        .child(
            div()
                .id(id(format!("forge-change-state-{n}")))
                .size(DOT)
                .flex_none()
                .rounded_full()
                .bg(dot)
                .role(Role::Image)
                .aria_label(words::change_state(summary.state)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(design::space::HAIR)
                .child(
                    div()
                        .flex()
                        .gap(design::space::XS)
                        .min_w(px(0.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .font_weight(view::FontWeight::MEDIUM)
                                .child(summary.title.clone()),
                        )
                        .child(div().text_color(theme.muted).child(format!("#{n}"))),
                )
                .child(
                    design::mono(format!(
                        "{} · {author}",
                        words::endpoints(
                            &crate::select::revision_name(&summary.from),
                            &summary.into
                        )
                    ))
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted),
                ),
        );
    if let Some(judgment) = judgment {
        if judgment.requested {
            line = line.child(badge(
                id(format!("forge-change-requested-{n}")),
                words::REVIEW_REQUESTED,
                ladder,
                theme.accent_foreground,
                theme.accent_soft,
            ));
        }
        if judgment.replies.is_some() {
            line = line.child(badge(
                id(format!("forge-change-unread-{n}")),
                words::NEW_REPLY,
                ladder,
                theme.warning,
                theme.warning_soft,
            ));
        }
    }
    let line = line.child(verdicts(&summary.verdicts, n, theme)).child(
        div()
            .w(px(90.))
            .flex()
            .justify_end()
            .child(quiet(words::comments(summary.comment_count), theme)),
    );
    design::item(line, Role::ListBoxOption, active).into_any_element()
}

pub(crate) fn state_chip(state: ChangeState, n: u64, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let (foreground, background) = match state {
        ChangeState::Open => (theme.success, theme.success_soft),
        ChangeState::Merged => (theme.agent, theme.agent_soft),
        ChangeState::Closed => (theme.muted, theme.surface_raised),
    };
    badge(
        id(format!("forge-change-state-{n}")),
        words::change_state(state),
        ladder,
        foreground,
        background,
    )
    .into_any_element()
}

/// `1 approved · 1 asked for changes`, when anyone has reviewed.
fn verdicts(counts: &ReviewCounts, n: u64, theme: &Theme) -> AnyElement {
    if counts.approve + counts.request_changes + counts.comment == 0 {
        return div().into_any_element();
    }
    div()
        .id(id(format!("forge-change-verdicts-{n}")))
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(format!(
            "{} approved · {} asked for changes",
            counts.approve, counts.request_changes
        ))
        .into_any_element()
}

/// The state as one segmented choice (Open with its count), the lists
/// about me as quiet buttons, and the title search on the right.
fn filters(forge: &Forge, cx: &mut Context<Forge>, theme: &Theme, ladder: &Ladder) -> AnyElement {
    const STATES: [Filter; 3] = [Filter::Open, Filter::Merged, Filter::Closed];
    let states = design::segmented(
        id("forge-filter-states"),
        "Change state",
        ladder,
        theme,
        STATES.iter().position(|state| *state == forge.filter),
        STATES.map(|filter| {
            let label = match (filter, forge.open_changes()) {
                (Filter::Open, Some(count)) => format!("Open {count}"),
                _ => filter.label().to_owned(),
            };
            (id(format!("forge-filter-{}", filter.slug())), label.into())
        }),
        cx.processor(|forge, picked: usize, _, cx| forge.set_filter(STATES[picked], cx)),
    );
    let mut bar = div()
        .id(id("forge-filters"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::MD)
        .px(crate::ui::PAGE_X)
        .py(design::space::LG)
        .border_b_1()
        .border_color(theme.border)
        .child(states)
        .child(div().w(px(1.)).h(px(18.)).bg(theme.border));
    for filter in [Filter::Judgment, Filter::Authored, Filter::Involves] {
        let pick = cx.listener(move |forge, _: &ClickEvent, _, cx| forge.set_filter(filter, cx));
        bar = bar.child(
            button(
                id(format!("forge-filter-{}", filter.slug())),
                filter.label(),
                ladder,
                theme,
                pick,
            )
            .kind(design::Kind::Quiet)
            .selected(forge.filter == filter)
            .enabled(forge.me_principal().is_some()),
        );
    }
    // the Input sits in a box of its own: pushed right, it keeps its width
    bar.child(
        div().ml_auto().flex_none().w(SEARCH_W).child(
            Input::new(
                id("forge-changes-search"),
                &forge.change_search,
                "Search changes",
            )
            .h(design::size::CONTROL)
            .w(SEARCH_W)
            .px_2()
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.background)
            .text_color(theme.foreground)
            .placeholder(words::SEARCH_TITLES),
        ),
    )
    .into_any_element()
}

/// The Change draft: a comparison turned into a title, a body and reviewers.
pub(crate) fn form(
    form: &ChangeForm,
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let submit = cx.listener(|forge, _: &ClickEvent, _, cx| forge.submit_change(cx));
    let cancel = cx.listener(|forge, _: &ClickEvent, _, cx| forge.cancel_change(cx));
    let mut card = div()
        .id(id("forge-change-form"))
        .flex()
        .flex_col()
        .gap_2()
        .m_3()
        .p_3()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface)
        .child(heading(
            id("forge-change-form-title"),
            match form.edit {
                Some(n) => words::edit_change(n),
                None => words::NEW_CHANGE.to_owned(),
            },
            2,
            theme,
        ))
        .child(quiet(words::endpoints(&form.from, &form.into), theme))
        .child(form_fields(form, forge, cx, theme))
        .child(reviewers(form, forge, cx, theme, ladder));
    if !form.error.is_empty() {
        card = card.child(
            div()
                .id(id("forge-change-error"))
                .text_size(design::text::SECONDARY)
                .text_color(theme.danger)
                .child(form.error.clone()),
        );
    }
    card.child(
        div()
            .flex()
            .gap_2()
            .child(div().flex_1())
            .child(button(
                id("forge-change-cancel"),
                "Cancel",
                ladder,
                theme,
                cancel,
            ))
            .child(
                button(
                    id("forge-change-submit"),
                    if form.edit.is_some() { "Save" } else { "Open" },
                    ladder,
                    theme,
                    submit,
                )
                .kind(design::Kind::Primary)
                .enabled(forge.may_write()),
            ),
    )
    .into_any_element()
}

/// The draft's title and body fields.
fn form_fields(form: &ChangeForm, forge: &Forge, cx: &mut Context<Forge>, theme: &Theme) -> Div {
    // a title typed again takes the last refusal with it
    let title = cx.listener(|forge, _: &wire::TextChange, _, cx| {
        if let Some(form) = &mut forge.form {
            form.error.clear();
        }
        cx.notify();
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            Input::new(id("forge-change-title"), &form.title, "Change title")
                .h(design::size::CONTROL)
                .w_full()
                .px_2()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground)
                .placeholder(words::TITLE_PROMPT)
                .on_change(title)
                .when(forge.may_write(), |title| {
                    title.on_submit(cx.listener(|forge, _: &(), _, cx| forge.submit_change(cx)))
                }),
        )
        .child(
            Textarea::new(id("forge-change-body"), &form.body, "Change body")
                .placeholder(words::BODY_PROMPT)
                .min_h(design::size::CONTROL * 4.)
                .w_full()
                .px_2()
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.background)
                .text_color(theme.foreground),
        )
}

/// The Change's reviewers, picked from the roster.
fn reviewers(
    form: &ChangeForm,
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    if forge.names.ready().is_none() {
        return quiet(words::READING_ROSTER, theme);
    }
    let toggle = |forge: &mut Forge, key: forge::Principal| {
        if let Some(form) = &mut forge.form {
            if let Some(at) = form.reviewers.iter().position(|held| *held == key) {
                form.reviewers.remove(at);
            } else {
                form.reviewers.push(key);
            }
        }
    };
    div()
        .id(id("forge-change-reviewers"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .child(quiet("Reviewers", theme))
        .children(people_picker(
            forge,
            "forge-reviewer",
            "",
            |key| form.reviewers.contains(key),
            toggle,
            cx,
            theme,
            ladder,
        ))
        .into_any_element()
}

/// The identity picker: the accounts chat's mentions offer
/// ([`chat::view::Names::people`]) whose name holds `needle`, each a
/// button `{key}-{number}` that `pick`s it.
/// Nothing while the roster is still being read.
#[allow(
    clippy::too_many_arguments,
    reason = "the ladder rides beside the theme into every sized helper; the phone rework of this view reshapes the row"
)]
pub(crate) fn people_picker(
    forge: &Forge,
    key: &str,
    needle: &str,
    picked: impl Fn(&forge::Principal) -> bool,
    pick: impl Fn(&mut Forge, forge::Principal) + Clone + 'static,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let Some(names) = forge.names.ready() else {
        return Vec::new();
    };
    let needle = needle.trim().to_lowercase();
    names
        .people()
        .map(forge::Principal::Account)
        .filter(|person| {
            let name = names.name(person).unwrap_or_default().to_lowercase();
            needle.is_empty() || name.contains(&needle)
        })
        .map(|person| {
            let number = person.account().unwrap_or_default();
            let chosen = picked(&person);
            let label = names.member(&person);
            let click = cx.listener({
                let pick = pick.clone();
                move |forge, _: &ClickEvent, _, cx| {
                    pick(forge, person.clone());
                    cx.notify();
                }
            });
            button(id(format!("{key}-{number}")), label, ladder, theme, click)
                .selected(chosen)
                .into_any_element()
        })
        .collect()
}
