//! The create and edit form, one page: its fields full width, the model a pick from harness's
//! records, thinking and whom it answers as segments, then its one verb. The title row's caption
//! says what a create sends; once its first op lands, how far it got is said over the verb.
use harness::Thinking;
use view::design::{Ladder, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::parts::scroller;
use crate::Agents;
use crate::create::{Answers, Changers, proposed_path};
use crate::ui::{thinking_picker, waiting};
use crate::words::{serving_words, window_short};

pub(super) fn page(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let draft = &view.draft;
    let (title, caption) = match draft.again {
        true => (
            format!("Edit {}", draft.name.text()),
            "a new version of its configuration file",
        ),
        false => (
            "Create an agent".to_owned(),
            "account, file, harness · each op confirmed",
        ),
    };
    let advance = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.advance(cx));
    let steps = draft.steps();
    let done = steps.iter().filter(|(_, done)| *done).count();
    // how far a create got, once its first op landed
    let progress = (!draft.again && draft.created.is_some()).then(|| {
        let next = steps.iter().find(|(_, done)| !done).map(|(what, _)| *what);
        let mut words = format!("{done} of {} sent", steps.len());
        if let Some(next) = next {
            words.push_str(&format!(" · next: {}", next.to_lowercase()));
        }
        div()
            .id("create-progress")
            .text_type(ladder.caption)
            .text_color(theme.muted)
            .child(words)
    });
    let body = div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .p(space::BLOCK)
        .child(fields(view, cx, theme, ladder))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("create-problem")
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .role(Role::Alert)
                .aria_live(view::accesskit::Live::Assertive)
                .child(problem)
        }))
        .children(progress)
        .child(
            div().flex().flex_col().child(
                design::button("create-next", draft.verb(), ladder, theme, advance)
                    .kind(design::Kind::Primary)
                    .busy(draft.sending.is_some()),
            ),
        );
    super::page("create-page", title, view, cx, theme, ladder)
        .caption(caption)
        .child(scroller("create-scroll").child(body))
        .into_any_element()
}

/// A field's label, over what it holds.
fn labelled(label: &'static str, held: impl IntoElement, theme: &Theme, ladder: &Ladder) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .child(
            div()
                .text_type(ladder.caption)
                .text_color(theme.muted)
                .child(label),
        )
        .child(held)
}

fn fields(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.draft;
    let mut form = div().flex().flex_col().gap(space::LG);
    if draft.created.is_none() {
        form = form.child(
            design::field(
                Input::new("create-name", &draft.name, "Name").placeholder("Scout"),
                ladder,
                theme,
            )
            .label("Name")
            .caption("Chat shows it with the agent badge."),
        );
    }
    let proposed = proposed_path(view.home(), &draft.name.text()).unwrap_or_default();
    form.child(
        design::field(
            Input::new("create-path", &draft.path, "Configuration file").placeholder(proposed),
            ladder,
            theme,
        )
        .label("Configuration file")
        .caption("Read at each model call. Only you may change it."),
    )
    .child(labelled(
        "Model",
        models(view, cx, theme, ladder),
        theme,
        ladder,
    ))
    .child(labelled(
        "Thinking",
        div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(thinking_picker(
                "create-thinking",
                draft.thinking,
                ladder,
                theme,
                cx.processor(|view: &mut Agents, thinking: Option<Thinking>, _, cx| {
                    view.draft.thinking = thinking;
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .child("Auto is the model's default."),
            ),
        theme,
        ladder,
    ))
    .child(
        design::field(
            Input::new("create-max-tokens", &draft.max_tokens, "Max output tokens")
                .placeholder("4096"),
            ladder,
            theme,
        )
        .label("Max output tokens"),
    )
    .child(
        design::field(
            Input::new("create-wait", &draft.wait, "Wait").placeholder("egress's default"),
            ladder,
            theme,
        )
        .label("Wait")
        .caption("Blocks egress waits for the answer."),
    )
    .child(
        design::field(
            Textarea::new("create-system", &draft.system, "System prompt")
                .min_h(ladder.control * 3.)
                .placeholder("What this agent does, and how"),
            ladder,
            theme,
        )
        .label("System prompt"),
    )
    .when(draft.again || !draft.scripted, |form| {
        form.child(answers(view, cx, theme, ladder))
    })
    .when(draft.again, |form| {
        form.child(changers(view, cx, theme, ladder))
    })
}

/// Harness's model records, one pick each: its id, its service and window, how many member
/// nodes serve it.
fn models(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    if let Some(shown) = waiting("create-models", &view.models, cx, theme, ladder) {
        return shown;
    }
    let models = view.models.ready().map_or(&[][..], Vec::as_slice);
    let rows = models.iter().map(|model| {
        let id = model.row.id.clone();
        let picked = view.draft.model.as_deref() == Some(id.as_str());
        let (now, tone) = serving_words(model.serving(), theme);
        let chose = id.clone();
        let choose = cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
            view.draft.model = Some(chose.clone());
            cx.notify();
        });
        pick(format!("model-{id}"), picked, theme, ladder)
            .on_click(choose)
            .child(
                design::mono(id)
                    .text_type(ladder.small)
                    .font_weight(FontWeight::MEDIUM),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_type(ladder.caption)
                    .text_color(theme.muted)
                    .child(format!(
                        "{} · {}",
                        model.row.service,
                        window_short(model.row.window)
                    )),
            )
            .child(
                div()
                    .flex_none()
                    .text_type(ladder.caption)
                    .text_color(tone)
                    .child(now),
            )
    });
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .children(rows)
        .when(models.is_empty(), |list| {
            list.child(design::quiet(
                "Harness keeps no model yet: set one on the Models page.",
                theme,
            ))
        })
        .into_any_element()
}

/// One choice of a list the form picks from: a row's height, its edge in ink once picked.
pub(super) fn pick(id: String, picked: bool, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let row = div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .gap(space::LG)
        .min_h(ladder.row)
        .px(space::LG)
        .border_1()
        .border_color(match picked {
            true => theme.foreground,
            false => theme.border,
        });
    design::pressed(row, ladder)
        .role(Role::Button)
        .aria_toggled(picked.into())
        .focusable()
}

/// One of a few choices, its list field under it once "Listed…" is picked.
#[allow(clippy::too_many_arguments)]
fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    label: &'static str,
    choices: &'static [(T, &'static str)],
    picked: Option<T>,
    listed: Option<impl IntoElement>,
    set: impl Fn(&mut Agents, T) + 'static,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let at = choices
        .iter()
        .position(|(choice, _)| Some(*choice) == picked);
    let segmented = design::segmented(
        id,
        label,
        ladder,
        theme,
        at,
        choices.iter().enumerate().map(|(index, (_, words))| {
            (
                ElementId::Name(format!("{id}-{index}").into()),
                SharedString::from(*words),
            )
        }),
        cx.processor(move |view: &mut Agents, index: usize, _, cx| {
            set(view, choices[index].0);
            cx.notify();
        }),
    );
    labelled(
        label,
        div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(segmented)
            .children(listed.map(IntoElement::into_any_element)),
        theme,
        ladder,
    )
}

const ANSWERS: &[(Answers, &str)] = &[
    (Answers::Manager, "Only me"),
    (Answers::Anyone, "Anyone"),
    (Answers::Listed, "Listed…"),
];

const CHANGERS: &[(Changers, &str)] = &[
    (Changers::Manager, "Only me"),
    (Changers::Listed, "Listed…"),
];

fn answers(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.draft;
    let listed = (draft.answers == Answers::Listed).then(|| {
        design::field(
            Input::new(
                "create-accounts",
                &draft.listed,
                "Account numbers and programs",
            )
            .placeholder("7, 12, forge"),
            ladder,
            theme,
        )
        .caption("Its manager always.")
    });
    choice(
        "create-answers",
        "Who it answers",
        ANSWERS,
        Some(draft.answers),
        listed,
        |view, answers| view.draft.answers = answers,
        cx,
        theme,
        ladder,
    )
}

/// Who may change the configuration file: its manager alone, or the accounts listed too. Left
/// as it opened, nothing is sent.
fn changers(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.draft;
    let listed = (draft.changers == Some(Changers::Listed)).then(|| {
        design::field(
            Input::new(
                "create-changers-listed",
                &draft.changers_listed,
                "Account numbers and programs",
            )
            .placeholder("7, 12"),
            ladder,
            theme,
        )
        .caption("Its manager always.")
    });
    choice(
        "create-changers",
        "Who may change the configuration",
        CHANGERS,
        draft.changers,
        listed,
        |view, changers| view.draft.changers = Some(changers),
        cx,
        theme,
        ladder,
    )
}
