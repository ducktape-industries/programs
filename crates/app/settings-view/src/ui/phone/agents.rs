//! Expo `src/account/Agents.tsx`: the Agents pane and its sheets. The pane
//! lists every agent this account manages, then the rows that open the two
//! add sheets (create an agent, add a key to one). An agent's row opens its
//! one sheet, in three modes: its actions, its rename, whom it answers.
//!
//! Reads the account's agents (`view.account`), the open sheet
//! (`view.sheet`) and each form: create, add key, rename, standing, whom it
//! answers. Writes the open sheet (`open_sheet`, `close_sheet`,
//! `open_answers`, `open_rename`, `cancel_rename_agent`) and each op
//! through its submit (`submit_create_agent`, `submit_agent_key`,
//! `submit_rename_agent`, `set_standing`, `pick_ownership`,
//! `submit_listed_person`, `remove_listed_person`).
use identity::{Ownership, Party, Standing};
use view::design::{Kind as ButtonKind, Ladder, SHEET_INSET};
use view::prelude::*;
use view::{Div, Stateful};

use super::parts::{
    column, mono, muted_icon, name_input, name_line, named, note, problem_of, quiet,
};
use crate::Settings;
use crate::queries::HARNESS;
use crate::queries::{Agent, Seat};
use crate::state::{Choice, Form, Sheet};
use crate::ui::shared::{agent_face, ownership_choices};
use crate::words;

const REVOKED_OPACITY: f32 = 0.5;
const CREATE_NAME: &str = "settings/agents/create/name";
const ANSWERS_DETENTS: [f32; 1] = [0.85];

pub(super) fn pane(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let Some(Some(Seat::Account(account))) = view.account.ready() else {
        return div().into_any_element();
    };
    let rows: Vec<_> = account
        .agents
        .iter()
        .map(|agent| agent_row(view, agent, cx, ladder, theme))
        .collect();
    let create = cx.listener(|v: &mut Settings, _: &ClickEvent, window, cx| {
        v.open_sheet(Sheet::CreateAgent, cx);
        window.focus(CREATE_NAME);
    });
    column("settings/agents")
        .child(quiet(
            "settings/agents/help",
            words::AGENTS_HELP,
            ladder,
            theme,
        ))
        .child(
            design::section_head("settings/agents/list", "Agents", ladder, theme)
                .child(account.agents.len().to_string()),
        )
        .children(rows)
        .child(design::section_head(
            "settings/agents/add",
            words::ADD,
            ladder,
            theme,
        ))
        .child(add_row(
            "settings/agents/create/open",
            "plus",
            words::CREATE_AGENT_TITLE,
            words::CONNECT_WHY,
            create,
            ladder,
            theme,
        ))
        .into_any_element()
}

fn add_row(
    id: &str,
    glyph: &str,
    title: &str,
    about: &str,
    open: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    design::row(id.to_owned(), ladder, theme)
        .min_h(ladder.row_tall)
        .role(Role::Button)
        .focusable()
        .on_click(open)
        .child(design::lead(muted_icon(glyph, ladder, theme)))
        .child(
            design::row_words(design::row_title(title.to_owned(), ladder, theme))
                .child(design::row_caption(about.to_owned(), ladder, theme)),
        )
        .child(design::chevron(ladder, theme))
}

fn agent_row(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let number = agent.number;
    let revoked = agent.standing() == Standing::Revoked;
    let keys = words::account_keys(number, agent.keys);
    let about = match revoked || agent.harnessed().is_some() {
        true => keys,
        false => format!("{keys} · answers {}", words::answers(&agent.ownership)),
    };
    let tone = if revoked {
        theme.faint
    } else {
        theme.foreground
    };
    let open = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.open_sheet(Sheet::Actions(number), cx)
    });
    let face = agent_avatar(agent, view.picture(agent.avatar, cx), ladder.avatar, theme);
    design::row(format!("settings/agents/{number}"), ladder, theme)
        .min_h(ladder.row_tall)
        .role(Role::Button)
        .focusable()
        .on_click(open)
        .child(design::lead(face))
        .child(
            design::row_words(
                design::row_title(agent.name.clone(), ladder, theme).text_color(tone),
            )
            .child(design::row_caption(about, ladder, theme)),
        )
        .children(words::runs_badge(&agent.kind).map(|runs| {
            design::badge(
                format!("settings/agents/{number}/runs"),
                runs,
                ladder,
                theme.muted,
                theme.surface_raised,
            )
        }))
        .child(standing_badge(agent, ladder, theme))
        .child(design::chevron(ladder, theme))
}

fn standing_badge(agent: &Agent, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    let standing = agent.standing();
    let (foreground, background) = match standing {
        Standing::Active => (theme.success, theme.success_soft),
        Standing::Suspended => (theme.warning, theme.warning_soft),
        Standing::Revoked => (theme.danger, theme.danger_soft),
    };
    design::badge(
        format!("settings/agents/{}/standing", agent.number),
        words::standing(standing),
        ladder,
        foreground,
        background,
    )
}

fn agent_avatar(
    agent: &Agent,
    picture: Option<std::sync::Arc<view::Image>>,
    face: design::Face,
    theme: &Theme,
) -> Div {
    agent_face(agent, picture, face, theme).when(agent.standing() == Standing::Revoked, |face| {
        face.opacity(REVOKED_OPACITY)
    })
}

pub(super) fn sheet(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let open = view.sheet?;
    let close = cx.listener(|v: &mut Settings, _: &(), _, cx| v.close_sheet(cx));
    let (id, label, body) = match open {
        Sheet::Actions(number) | Sheet::Rename(number) | Sheet::Answers(number) => {
            let agent = view.agent(number)?;
            (
                format!("settings/agents/{number}/sheet"),
                agent.name.clone(),
                agent_sheet(view, agent, open, cx, ladder, theme),
            )
        }
        Sheet::CreateAgent => (
            "settings/agents/create/sheet".to_owned(),
            words::CREATE_AGENT_TITLE.to_owned(),
            create_agent(view, cx, ladder, theme),
        ),
        Sheet::AddKey => (
            "settings/agents/key/sheet".to_owned(),
            words::ADD_KEY_TITLE.to_owned(),
            add_key(view, cx, ladder, theme),
        ),
    };
    let sheet = design::sheet(id, label, ladder, theme).on_dismiss(close);
    let sheet = match open {
        Sheet::Answers(_) => sheet.detents(ANSWERS_DETENTS),
        Sheet::Actions(_) | Sheet::Rename(_) | Sheet::CreateAgent | Sheet::AddKey => sheet,
    };
    Some(sheet.child(body).into_any_element())
}

fn agent_sheet(
    view: &Settings,
    agent: &Agent,
    open: Sheet,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let body = match (agent.standing(), open) {
        (Standing::Revoked, _) => None,
        (_, Sheet::Rename(_)) => rename(view, agent, cx, ladder, theme),
        (_, Sheet::Answers(_)) => Some(answers_form(view, agent, cx, ladder, theme)),
        _ => Some(actions(view, agent, cx, ladder, theme)),
    };
    let number = agent.number;
    let about = format!(
        "{} · {}",
        words::account(number),
        words::standing(agent.standing())
    );
    let face = agent_avatar(
        agent,
        view.picture(agent.avatar, cx),
        ladder.avatar_sheet,
        theme,
    );
    let head = design::sheet_head(format!("settings/agents/{number}/head"), theme).child(named(
        face,
        name_line(&agent.name, ladder.body),
        about,
        ladder,
        theme,
    ));
    let content = div().flex().flex_col().child(head).children(body);
    match open {
        Sheet::Answers(_) => div()
            .id(format!("settings/agents/{number}/scroll"))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(content)
            .into_any_element(),
        _ => content.into_any_element(),
    }
}

fn actions(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let number = agent.number;
    let listing = view
        .answering
        .get(&number)
        .is_some_and(|answering| answering.listing);
    let rename = cx.listener(move |v: &mut Settings, _: &ClickEvent, window, cx| {
        v.open_rename(number, cx);
        window.focus(rename_id(number));
    });
    let answers =
        cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.open_answers(number, cx));
    let revoke = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.set_standing(identity::Op::Revoke { account: number }, cx)
    });
    let change = view.reads_files.then(|| {
        let pressed =
            cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.change_photo(number, cx));
        let label = match view.photo_opening(number) {
            true => words::OPENING,
            false => words::CHANGE_PHOTO,
        };
        let id = format!("settings/agents/{number}/photo/change");
        design::sheet_row(id, label, ladder, theme, pressed).icon("camera")
    });
    let remove = agent.avatar.is_some().then(|| {
        let pressed =
            cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.remove_photo(number, cx));
        let label = match view.photo_removing(number) {
            true => words::REMOVING,
            false => words::REMOVE_PHOTO,
        };
        let id = format!("settings/agents/{number}/photo/remove");
        design::sheet_row(id, label, ladder, theme, pressed).icon("trash")
    });
    let photo_refused = view
        .photo_stopped(number)
        .and_then(|form| form.problem.as_ref())
        .map(|problem| {
            div().px(SHEET_INSET).child(note(
                words::problem_text(problem, ""),
                theme.danger,
                ladder,
            ))
        });
    let chain = view.session.chain_id.clone();
    let agent_id = number.to_string();
    let opens = |id: String, label: String, link: Option<String>, glyph: &'static str| {
        link.map(|link| {
            let pressed =
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| cx.host().open_link(&link);
            design::sheet_row(id, label, ladder, theme, pressed).icon(glyph)
        })
    };
    if let Some(script) = agent.harnessed() {
        let link = ducklink::mint(&chain, &script.calls, &[agent_id.as_str()]);
        return column(format!("settings/agents/{number}/actions"))
            .children(change)
            .children(remove)
            .children(opens(
                format!("settings/agents/{number}/open"),
                words::open_in(&script.calls),
                link,
                "bot",
            ))
            .children(photo_refused)
            .into_any_element();
    }
    let keyed = cx
        .listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.open_key_for(number, true, cx));
    let key = design::sheet_row(
        format!("settings/agents/{number}/key/open"),
        words::ADD_A_KEY,
        ladder,
        theme,
        keyed,
    )
    .icon("key-round");
    let harness = opens(
        format!("settings/agents/{number}/harness"),
        words::RUN_ON_HARNESS.to_owned(),
        ducklink::mint(&chain, HARNESS, &["create", agent_id.as_str()])
            .filter(|_| agent.script().is_none()),
        "bot",
    );
    column(format!("settings/agents/{number}/actions"))
        .child(
            design::sheet_row(
                format!("settings/agents/{number}/rename/open"),
                words::RENAME_AGENT,
                ladder,
                theme,
                rename,
            )
            .icon("pencil"),
        )
        .children(change)
        .children(remove)
        .child(key)
        .children(harness)
        .child(
            design::sheet_row(
                format!("settings/agents/{number}/answers/open"),
                words::WHO_IT_ANSWERS,
                ladder,
                theme,
                answers,
            )
            .icon("users")
            .hint(Choice::shown(&agent.ownership, listing).label().to_owned()),
        )
        .child(standing_row(view, agent, cx, ladder, theme))
        .child(
            design::sheet_row(
                format!("settings/agents/{number}/revoke"),
                words::REVOKE,
                ladder,
                theme,
                revoke,
            )
            .icon("x")
            .hint("final")
            .danger(),
        )
        .children(view.agent_standing.problem.as_ref().map(|problem| {
            div().px(SHEET_INSET).child(note(
                words::problem_text(problem, ""),
                theme.danger,
                ladder,
            ))
        }))
        .children(photo_refused)
        .into_any_element()
}

fn rename_id(number: u64) -> String {
    format!("settings/agents/{number}/name")
}

fn standing_row(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let (slug, word, op) = agent.standing_toggle();
    let glyph = match agent.standing() {
        Standing::Suspended => "play",
        Standing::Active | Standing::Revoked => "clock",
    };
    let press =
        cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.set_standing(op.clone(), cx));
    design::sheet_row(
        format!("settings/agents/{}/{slug}", agent.number),
        if view.agent_standing.busy {
            words::WORKING
        } else {
            word
        },
        ladder,
        theme,
        press,
    )
    .icon(glyph)
}

fn rename(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Option<AnyElement> {
    let number = agent.number;
    let form = view.rename_agent.get(&number)?;
    let label = words::rename_field(&agent.name);
    let problem = problem_of(form, words::RENAME.empty);
    let input = name_input(rename_id(number), form, label.clone(), words::RENAME.prompt);
    let save = cx
        .listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.submit_rename_agent(number, cx));
    let cancel = cx
        .listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.cancel_rename_agent(number, cx));
    let form = column(format!("settings/agents/{number}/rename/form"))
        .gap(design::space::LG)
        .p(SHEET_INSET)
        .child(
            design::field(input, ladder, theme)
                .label(label)
                .when_some(problem, design::Field::problem),
        )
        .child(verbs(
            &format!("settings/agents/{number}/rename"),
            words::RENAME.verb,
            form.busy.then_some(words::RENAME.busy),
            save,
            cancel,
            ladder,
            theme,
        ));
    Some(form.into_any_element())
}

fn answers_form(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let number = agent.number;
    let answering = view.answering.get(&number);
    let listing = answering.is_some_and(|answering| answering.listing);
    let refused = answering.and_then(|answering| problem_of(&answering.pick, ""));
    let shown = Choice::shown(&agent.ownership, listing);
    let writing = view.writing();
    let people = match (shown, answering) {
        (Choice::Specific, Some(answering)) => {
            Some(people(agent, &answering.form, writing, cx, ladder, theme))
        }
        _ => None,
    };
    let done = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.open_sheet(Sheet::Actions(number), cx)
    });
    let saving = match writing {
        true => format!(" · {}", words::SAVING),
        false => String::new(),
    };
    column(format!("settings/agents/{number}/answering"))
        .gap(design::space::LG)
        .p(SHEET_INSET)
        .pb(design::space::BLOCK)
        .child(note(
            format!("{}{saving}", words::WHO_IT_ANSWERS),
            theme.muted,
            ladder,
        ))
        .child(ownership_choices(agent, shown, cx, ladder, theme))
        .when(agent.ownership == Ownership::Anyone, |block| {
            block.child(note(words::ANYONE_WARNING, theme.warning, ladder))
        })
        .children(refused.map(|refused| note(refused, theme.danger, ladder)))
        .children(people)
        .child(
            div().flex().child(
                design::button(
                    format!("settings/agents/{number}/answers/done"),
                    "Done",
                    ladder,
                    theme,
                    done,
                )
                .kind(ButtonKind::Quiet),
            ),
        )
        .into_any_element()
}

fn people(
    agent: &Agent,
    form: &Form,
    writing: bool,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let busy = writing;
    let problem = problem_of(form, words::ADD_PERSON.empty);
    let number = agent.number;
    let listed = agent.ownership.listed();
    let label = words::person_field(&agent.name);
    let input = name_input(
        format!("settings/agents/{number}/answers/name"),
        form,
        label.clone(),
        words::ADD_PERSON.prompt,
    )
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None);
    let add = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.submit_listed_person(number, cx)
    });
    column(format!("settings/agents/{number}/answers/people"))
        .gap(design::space::SM)
        .children(
            listed
                .iter()
                .map(|party| person_row(agent, party, busy, cx, ladder, theme)),
        )
        .when(listed.is_empty(), |people| {
            people.child(note(words::NOBODY_LISTED, theme.muted, ladder))
        })
        .child(
            design::field(input, ladder, theme)
                .label(label)
                .when_some(problem, design::Field::problem),
        )
        .child(
            div().flex().child(
                design::button(
                    format!("settings/agents/{number}/answers/add"),
                    if busy {
                        words::ADD_PERSON.busy
                    } else {
                        words::ADD_PERSON.verb
                    },
                    ladder,
                    theme,
                    add,
                )
                .kind(ButtonKind::Outline)
                .busy(busy),
            ),
        )
}

fn person_row(
    agent: &Agent,
    party: &Party,
    busy: bool,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let number = agent.number;
    let id = words::party_id(party);
    let removed = party.clone();
    let remove = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.remove_listed_person(number, &removed, cx)
    });
    div()
        .id(format!("settings/agents/{number}/answers/{id}"))
        .flex()
        .items_center()
        .gap(design::space::SM)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .child(div().text_type(ladder.body).child(agent.party_name(party)))
                .child(mono(words::party(party), ladder, theme)),
        )
        .child(
            design::button(
                format!("settings/agents/{number}/answers/{id}/remove"),
                words::REMOVE,
                ladder,
                theme,
                remove,
            )
            .kind(ButtonKind::Quiet)
            .small()
            .enabled(!busy),
        )
}

fn create_agent(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let form = &view.create_agent;
    let problem = problem_of(form, words::CREATE_AGENT.empty);
    let input = name_input(
        CREATE_NAME,
        form,
        words::AGENT_NAME,
        words::CREATE_AGENT.prompt,
    )
    .on_submit(cx.listener(|v: &mut Settings, _: &(), _, cx| v.submit_create_agent(cx)));
    let create = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_create_agent(cx));
    let cancel = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.close_sheet(cx));
    add_sheet(
        "settings/agents/create",
        words::CREATE_AGENT_TITLE,
        design::field(input, ladder, theme)
            .label(words::AGENT_NAME)
            .when_some(problem, design::Field::problem),
        verbs(
            "settings/agents/create",
            words::CREATE_AGENT.verb,
            form.busy.then_some(words::CREATE_AGENT.busy),
            create,
            cancel,
            ladder,
            theme,
        ),
        ladder,
        theme,
    )
}

fn add_key(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let form = &view.agent_key;
    let request = Textarea::new(
        "settings/agents/key/request",
        &form.text,
        words::KEY_REQUEST,
    )
    .placeholder(words::ADD_KEY.prompt)
    .read_only(form.busy)
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None)
    .suggestions(false);
    let add = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_agent_key(cx));
    let cancel = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.close_sheet(cx));
    add_sheet(
        "settings/agents/key",
        words::ADD_KEY_TITLE,
        design::field(request, ladder, theme)
            .mono()
            .label(words::KEY_REQUEST)
            .caption(view.key_for.map_or_else(
                || words::ADD_KEY_WHY.to_owned(),
                |(n, made)| words::key_for(n, made),
            ))
            .when_some(
                problem_of(form, words::ADD_KEY.empty),
                design::Field::problem,
            ),
        verbs(
            "settings/agents/key",
            words::ADD_KEY.verb,
            form.busy.then_some(words::ADD_KEY.busy),
            add,
            cancel,
            ladder,
            theme,
        ),
        ladder,
        theme,
    )
}

fn add_sheet(
    id: &str,
    title: &str,
    field: impl IntoElement,
    verbs: Div,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .child(design::sheet_title(
            format!("{id}/head"),
            title.to_owned(),
            ladder,
            theme,
        ))
        .child(
            column(format!("{id}/form"))
                .gap(design::space::LG)
                .p(SHEET_INSET)
                .child(field)
                .child(verbs),
        )
        .into_any_element()
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
