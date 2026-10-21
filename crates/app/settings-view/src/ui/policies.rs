//! The Policies pane, alike on both designs (each passes its ladder): the reader's policies in
//! words, what new items in each scope use, and the sheets that make, edit, remove and switch a
//! policy and set a default. It reads the state and writes only through `policies.rs`.
use ownership::{Party, Policy, PolicyId, Scope};
use policy_view::{Holdings, KINDS, PROGRAMS};
use view::design::{Kind as ButtonKind, Ladder, SHEET_INSET};
use view::prelude::*;
use view::{Div, ElementId, Stateful, Svg};

use crate::Settings;
use crate::policies::problem_text;
use crate::state::{Choice, Form, PolicyForm, PolicySheet};
use crate::words;

const SHEET: &str = "settings/policies/sheet";
const HELP: &str = "A policy says who may change what you own: your channels, messages, \
    repositories, changes, reviews, models and agents' conversations. You always may.";
const READING: &str = "Reading your policies…";
const NONE_YET: &str = "No policies yet: what you own is open to anyone until you make one.";
const DEFAULTS_HELP: &str =
    "What each new item of yours links to. A row left on Inherit takes the wider one's.";
const NAME: &str = "Name";
const NAME_EMPTY: &str = "Name the policy.";
const WHO_WRITES: &str = "Who may change items on it, beside you";
const PERSON: &str = "Add someone it lets write";
const KEPT: &str = "Removed, it still governs the items on it, nothing new links to it, and \
    your defaults that name it are cleared. To move its items too, use Move everything on it \
    to… instead.";
const MOVED: &str = "Pick the policy they move to. This one is removed once they have moved.";
const NOWHERE: &str = "You have no other policy to move them to. Make one first.";

pub(crate) fn pane(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    match &view.holdings {
        Loadable::Ready(holdings) | Loadable::Reloading(holdings, _) => {
            listed(holdings, cx, ladder, theme).into_any_element()
        }
        Loadable::Failed(refusal) => {
            let retry =
                cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.refresh_policies(cx));
            design::refused(
                "settings/policies",
                design::sentence(refusal),
                ladder,
                theme,
                retry,
            )
            .into_any_element()
        }
        Loadable::Idle | Loadable::Loading(_) => {
            line("settings/policies/loading", READING, ladder, theme).into_any_element()
        }
    }
}

fn listed(
    holdings: &Holdings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let new = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| {
        v.open_policy_sheet(PolicySheet::Edit(None), cx)
    });
    let policies: Vec<_> = holdings
        .policies
        .iter()
        .map(|policy| policy_row(holdings, policy, cx, ladder, theme))
        .collect();
    let defaults: Vec<_> = scopes(holdings)
        .into_iter()
        .map(|scope| default_row(holdings, scope, cx, ladder, theme))
        .collect();
    let none = holdings
        .policies
        .is_empty()
        .then(|| line("settings/policies/none", NONE_YET, ladder, theme));
    div()
        .id("settings/policies")
        .flex()
        .flex_col()
        .w_full()
        .child(line("settings/policies/help", HELP, ladder, theme))
        .child(
            design::section_head("settings/policies/list", "Your policies", ladder, theme)
                .child(holdings.policies.len().to_string()),
        )
        .children(none)
        .children(policies)
        .child(
            design::row("settings/policies/new", ladder, theme)
                .min_h(ladder.row_tall)
                .role(Role::Button)
                .focusable()
                .on_click(new)
                .child(design::lead(glyph("plus", ladder, theme)))
                .child(design::row_words(design::row_title(
                    "New policy",
                    ladder,
                    theme,
                )))
                .child(design::chevron(ladder, theme)),
        )
        .child(design::section_head(
            "settings/policies/defaults",
            "New items use",
            ladder,
            theme,
        ))
        .child(line(
            "settings/policies/defaults/help",
            DEFAULTS_HELP,
            ladder,
            theme,
        ))
        .children(defaults)
}

fn policy_row(
    holdings: &Holdings,
    policy: &Policy,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let id = policy.id;
    let open = holdings.open == Some(id);
    let built_in = open || holdings.only_me == Some(id);
    let title = match built_in {
        true => format!("{} (built in)", policy.name),
        false => policy.name.clone(),
    };
    let who = holdings.writers(policy);
    let caption = match (built_in, policy.switched_to) {
        (true, _) => format!("{who} · never changes"),
        (false, Some(to)) => format!("Removed · its items are on {}", name_of(holdings, Some(to))),
        (false, None) if policy.retired => format!("{who} · removed, its items keep it"),
        (false, None) => who,
    };
    let row = design::row(format!("settings/policies/{id}"), ladder, theme)
        .min_h(ladder.row_tall)
        .child(design::lead(glyph(
            if open { "lock-open" } else { "shield" },
            ladder,
            theme,
        )))
        .child(
            design::row_words(design::row_title(title, ladder, theme))
                .child(design::row_caption(caption, ladder, theme)),
        )
        .when(policy.retired, |row| row.opacity(0.6));
    let settled = built_in || policy.switched_to.is_some();
    if settled {
        return row;
    }
    let press = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.open_policy_sheet(PolicySheet::Actions(id), cx)
    });
    row.role(Role::Button)
        .focusable()
        .on_click(press)
        .child(design::chevron(ladder, theme))
}

fn scopes(holdings: &Holdings) -> Vec<Scope> {
    let mut programs: Vec<String> = PROGRAMS.iter().map(|program| program.to_string()).collect();
    let mut kinds: Vec<String> = KINDS.iter().map(|kind| kind.name.to_owned()).collect();
    for default in &holdings.defaults {
        match &default.scope {
            Scope::Program(program) if !programs.contains(program) => {
                programs.push(program.clone())
            }
            Scope::Kind(kind) if !kinds.contains(kind) => kinds.push(kind.clone()),
            _ => {}
        }
    }
    std::iter::once(Scope::Global)
        .chain(programs.into_iter().map(Scope::Program))
        .chain(kinds.into_iter().map(Scope::Kind))
        .collect()
}

fn slug(scope: &Scope) -> String {
    match scope {
        Scope::Global => "all".to_owned(),
        Scope::Program(program) => format!("program-{program}"),
        Scope::Kind(kind) => format!("kind-{kind}"),
    }
}

fn default_row(
    holdings: &Holdings,
    scope: Scope,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let caption = match holdings.default_of(&scope) {
        Some(id) => name_of(holdings, Some(id)),
        None => format!(
            "Inherit · {}",
            name_of(holdings, holdings.inherited(&scope))
        ),
    };
    let title = policy_view::scope(&scope);
    let id = format!("settings/policies/default/{}", slug(&scope));
    let press = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.open_policy_sheet(PolicySheet::Default(scope.clone()), cx)
    });
    design::row(id, ladder, theme)
        .min_h(ladder.row_tall)
        .role(Role::Button)
        .focusable()
        .on_click(press)
        .child(
            design::row_words(design::row_title(title, ladder, theme))
                .child(design::row_caption(caption, ladder, theme)),
        )
        .child(design::chevron(ladder, theme))
}

fn name_of(holdings: &Holdings, id: Option<PolicyId>) -> String {
    match id {
        None => ownership::ONLY_ME.to_owned(),
        Some(id) => holdings
            .policy(id)
            .map_or_else(|| format!("#{id}"), |policy| policy.name.clone()),
    }
}

pub(crate) fn sheet(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let open = view.policy_sheet.clone()?;
    let holdings = view.holdings.ready()?;
    let (label, body) = match &open {
        PolicySheet::Actions(id) => {
            let policy = holdings.policy(*id)?;
            let body = actions(holdings, policy, cx, ladder, theme);
            (policy.name.clone(), body)
        }
        PolicySheet::Edit(id) => {
            let title = match id {
                Some(id) => format!("Edit {}", holdings.policy(*id)?.name),
                None => "New policy".to_owned(),
            };
            let body = edit(view, &title, id.is_some(), cx, ladder, theme);
            (title, body)
        }
        PolicySheet::Remove(id) => {
            let policy = holdings.policy(*id)?;
            let title = format!("Remove {}", policy.name);
            let body = remove(view, policy, &title, cx, ladder, theme);
            (title, body)
        }
        PolicySheet::Move(id) => {
            let policy = holdings.policy(*id)?;
            let title = format!("Move everything on {}", policy.name);
            let body = switch(view, holdings, policy, &title, cx, ladder, theme);
            (title, body)
        }
        PolicySheet::Default(scope) => {
            let title = policy_view::scope(scope);
            let body = default(view, holdings, scope, &title, cx, ladder, theme);
            (title, body)
        }
    };
    let close = cx.listener(|v: &mut Settings, _: &(), _, cx| v.close_policy_sheet(cx));
    let sheet = design::sheet(SHEET, label, ladder, theme)
        .on_dismiss(close)
        .child(body);
    Some(sheet.into_any_element())
}

fn actions(
    holdings: &Holdings,
    policy: &Policy,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let id = policy.id;
    let to = |sheet: PolicySheet| {
        cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
            v.open_policy_sheet(sheet.clone(), cx)
        })
    };
    let head = design::sheet_head(format!("settings/policies/{id}/head"), theme)
        .child(div().text_type(ladder.body).child(policy.name.clone()))
        .child(note(holdings.writers(policy), theme.muted, ladder));
    let edit = design::sheet_row(
        format!("settings/policies/{id}/edit"),
        "Edit",
        ladder,
        theme,
        to(PolicySheet::Edit(Some(id))),
    )
    .icon("pencil");
    let switch = design::sheet_row(
        format!("settings/policies/{id}/move"),
        "Move everything on it to…",
        ladder,
        theme,
        to(PolicySheet::Move(id)),
    )
    .icon("arrow-right-left");
    let remove = design::sheet_row(
        format!("settings/policies/{id}/remove"),
        words::REMOVE,
        ladder,
        theme,
        to(PolicySheet::Remove(id)),
    )
    .icon("trash")
    .danger();
    let live = !policy.retired;
    div()
        .flex()
        .flex_col()
        .child(head)
        .when(live, |column| column.child(edit))
        .child(switch)
        .when(live, |column| column.child(remove))
        .into_any_element()
}

fn edit(
    view: &Settings,
    title: &str,
    editing: bool,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let form = &view.policy_form;
    let busy = form.name.busy;
    let name = Input::new(
        "settings/policies/edit/name".to_owned(),
        &form.name.text,
        NAME.to_owned(),
    )
    .placeholder("Team")
    .disabled(busy);
    let name = match busy {
        true => name,
        false => name.on_submit(cx.listener(|v: &mut Settings, _: &(), _, cx| v.save_policy(cx))),
    };
    let picked = Choice::ALL.iter().position(|choice| *choice == form.grant);
    let grants = design::segmented(
        "settings/policies/edit/write",
        WHO_WRITES,
        ladder,
        theme,
        picked,
        Choice::ALL.map(|choice| {
            (
                format!("settings/policies/edit/write/{}", choice.slug()).into(),
                choice.label().into(),
            )
        }),
        cx.processor(|v: &mut Settings, index: usize, _, cx| v.pick_grant(Choice::ALL[index], cx)),
    );
    let people = (form.grant == Choice::Specific).then(|| people(form, cx, ladder, theme));
    let save = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.save_policy(cx));
    let cancel = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.close_policy_sheet(cx));
    let verb = if editing { "Save" } else { "Create policy" };
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            "settings/policies/edit/head",
            title.to_owned(),
            ladder,
            theme,
        ))
        .child(
            div()
                .id("settings/policies/edit/form")
                .flex()
                .flex_col()
                .gap(design::space::LG)
                .p(SHEET_INSET)
                .child(
                    design::field(name, ladder, theme)
                        .label(NAME)
                        .when_some(problem_text(&form.name, NAME_EMPTY), design::Field::problem),
                )
                .child(note(WHO_WRITES, theme.muted, ladder))
                .child(grants)
                .children(people)
                .child(verbs(
                    "settings/policies/edit",
                    verb,
                    busy.then_some(words::SAVING),
                    save,
                    cancel,
                    ladder,
                    theme,
                )),
        )
        .into_any_element()
}

fn people(
    form: &PolicyForm,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let busy = form.person.busy;
    let rows: Vec<_> = form
        .listed
        .iter()
        .map(|party| {
            let id = party_id(party);
            let removed = party.clone();
            let remove = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
                v.remove_policy_person(&removed, cx)
            });
            div()
                .id(format!("settings/policies/edit/people/{id}"))
                .flex()
                .items_center()
                .gap(design::space::SM)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_type(ladder.body)
                        .child(policy_view::party(party, &form.names)),
                )
                .child(
                    design::button(
                        format!("settings/policies/edit/people/{id}/remove"),
                        words::REMOVE,
                        ladder,
                        theme,
                        remove,
                    )
                    .kind(ButtonKind::Quiet)
                    .small(),
                )
        })
        .collect();
    let input = Input::new(
        "settings/policies/edit/person".to_owned(),
        &form.person.text,
        PERSON.to_owned(),
    )
    .placeholder(words::ADD_PERSON.prompt)
    .disabled(busy)
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None);
    let input = match busy {
        true => input,
        false => {
            input.on_submit(cx.listener(|v: &mut Settings, _: &(), _, cx| v.add_policy_person(cx)))
        }
    };
    let add = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.add_policy_person(cx));
    let label = match busy {
        true => words::ADD_PERSON.busy,
        false => words::ADD_PERSON.verb,
    };
    div()
        .id("settings/policies/edit/people")
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .children(rows)
        .when(form.listed.is_empty(), |people| {
            people.child(note(words::NOBODY_LISTED, theme.muted, ladder))
        })
        .child(design::field(input, ladder, theme).label(PERSON).when_some(
            problem_text(&form.person, words::ADD_PERSON.empty),
            design::Field::problem,
        ))
        .child(
            div().flex().child(
                design::button(
                    "settings/policies/edit/people/add",
                    label,
                    ladder,
                    theme,
                    add,
                )
                .kind(ButtonKind::Outline)
                .busy(busy),
            ),
        )
}

fn party_id(party: &Party) -> String {
    match party {
        Party::Account(number) => number.to_string(),
        Party::Program(program) => format!("program-{program}"),
    }
}

fn items(view: &Settings) -> String {
    match &view.policy_items {
        Loadable::Ready(count) | Loadable::Reloading(count, _) => format!(
            "On it now: {}.",
            design::plural(*count as u64, "item", "items")
        ),
        Loadable::Failed(refusal) => design::sentence(refusal),
        Loadable::Idle | Loadable::Loading(_) => "Counting the items on it…".to_owned(),
    }
}

fn remove(
    view: &Settings,
    policy: &Policy,
    title: &str,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let id = policy.id;
    let busy = view.policy_write.busy;
    let submit =
        cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.retire_policy(id, cx));
    let cancel = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.close_policy_sheet(cx));
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            "settings/policies/remove/head",
            title.to_owned(),
            ladder,
            theme,
        ))
        .child(
            div()
                .id("settings/policies/remove/form")
                .flex()
                .flex_col()
                .gap(design::space::LG)
                .p(SHEET_INSET)
                .child(note(items(view), theme.foreground, ladder))
                .child(note(KEPT, theme.muted, ladder))
                .children(refused(&view.policy_write, ladder, theme))
                .child(verbs(
                    "settings/policies/remove",
                    words::REMOVE,
                    busy.then_some(words::WORKING),
                    submit,
                    cancel,
                    ladder,
                    theme,
                )),
        )
        .into_any_element()
}

fn switch(
    view: &Settings,
    holdings: &Holdings,
    policy: &Policy,
    title: &str,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let from = policy.id;
    let targets: Vec<_> = holdings
        .live()
        .filter(|target| target.id != from)
        .map(|target| {
            let to = target.id;
            let press = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
                v.switch_policy(from, to, cx)
            });
            design::sheet_row(
                format!("settings/policies/move/{to}"),
                target.name.clone(),
                ladder,
                theme,
                press,
            )
            .hint(holdings.writers(target))
        })
        .collect();
    let nowhere = targets.is_empty();
    let busy = view.policy_write.busy;
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            "settings/policies/move/head",
            title.to_owned(),
            ladder,
            theme,
        ))
        .child(
            div()
                .id("settings/policies/move/about")
                .flex()
                .flex_col()
                .gap(design::space::SM)
                .px(SHEET_INSET)
                .py(design::space::LG)
                .child(note(items(view), theme.foreground, ladder))
                .child(note(
                    if nowhere { NOWHERE } else { MOVED },
                    theme.muted,
                    ladder,
                ))
                .when(busy, |about| {
                    about.child(note(words::WORKING, theme.muted, ladder))
                }),
        )
        .children(targets)
        .children(refused(&view.policy_write, ladder, theme))
        .into_any_element()
}

fn default(
    view: &Settings,
    holdings: &Holdings,
    scope: &Scope,
    title: &str,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let chosen = holdings.default_of(scope);
    let pick = |policy: Option<PolicyId>| {
        let scope = scope.clone();
        cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
            v.set_default(scope.clone(), policy, cx)
        })
    };
    let in_use = |shown: bool, hint: String| match shown {
        true => format!("In use · {hint}"),
        false => hint,
    };
    let inherit = design::sheet_row(
        "settings/policies/default/inherit",
        "Inherit",
        ladder,
        theme,
        pick(None),
    )
    .hint(in_use(
        chosen.is_none(),
        name_of(holdings, holdings.inherited(scope)),
    ));
    let rows: Vec<_> = holdings
        .live()
        .map(|policy| {
            let id = policy.id;
            design::sheet_row(
                format!("settings/policies/default/pick/{id}"),
                policy.name.clone(),
                ladder,
                theme,
                pick(Some(id)),
            )
            .hint(in_use(chosen == Some(id), holdings.writers(policy)))
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            "settings/policies/default/head",
            title.to_owned(),
            ladder,
            theme,
        ))
        .child(inherit)
        .children(rows)
        .children(refused(&view.policy_write, ladder, theme))
        .into_any_element()
}

fn line(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .px(design::space::BLOCK)
        .py(design::space::SM)
        .text_type(ladder.secondary)
        .text_color(theme.muted)
        .child(text.into())
}

fn note(text: impl Into<SharedString>, tone: Hsla, ladder: &Ladder) -> Div {
    div()
        .text_type(ladder.secondary)
        .text_color(tone)
        .child(text.into())
}

fn refused(form: &Form, ladder: &Ladder, theme: &Theme) -> Option<Div> {
    let said = problem_text(form, "")?;
    Some(
        div()
            .px(SHEET_INSET)
            .pb(design::space::SM)
            .child(note(said, theme.danger, ladder)),
    )
}

fn glyph(name: &str, ladder: &Ladder, theme: &Theme) -> Svg {
    design::icon(name, ladder.icon_row).text_color(theme.muted)
}

fn verbs(
    base: &str,
    verb: &str,
    busy: Option<&str>,
    pressed: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    design::form_verbs(
        design::button(
            format!("{base}/cancel"),
            words::CANCEL,
            ladder,
            theme,
            cancel,
        )
        .kind(ButtonKind::Quiet),
        design::button(
            format!("{base}/submit"),
            busy.unwrap_or(verb),
            ladder,
            theme,
            pressed,
        )
        .kind(ButtonKind::Primary)
        .busy(busy.is_some()),
    )
}
