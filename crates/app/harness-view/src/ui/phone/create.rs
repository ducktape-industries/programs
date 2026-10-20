//! The create and edit form, one page: its fields full width, the model named and its provider a
//! pick from harness's routes, thinking and whom it answers as segments, then its one verb. The title row's caption
//! says what a create sends; once its first op lands, how far it got is said over the verb.
use harness::Thinking;
use view::Div;
use view::design::{Ladder, space};
use view::prelude::*;

use super::parts::scroller;
use crate::Agents;
use crate::create::{Answers, Changers, PROVIDER_PICK_HINT, VENDOR_HINT, proposed_path};
use crate::ui::routes::providers;
use crate::ui::thinking_picker;

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
            "its prompt, file and lists; the setup edits its front matter",
        ),
        false => (
            "Create an agent".to_owned(),
            "account, file, harness · each op confirmed",
        ),
    };
    let advance = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.advance(cx));
    let steps = draft.steps();
    let done = steps.iter().filter(|(_, done)| *done).count();
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
    .when(!draft.again, |form| {
        let set = |view: &mut Agents, provider: String, _: &mut Context<Agents>| {
            view.draft.provider = Some(provider);
        };
        let picked = draft.provider.as_deref();
        let providers = providers(view, picked, "create-provider", set, cx, theme, ladder);
        form.child(
            design::field(
                Input::new("create-model", &draft.vendor, "Model").placeholder("claude-opus-5-5"),
                ladder,
                theme,
            )
            .label("Model")
            .caption(VENDOR_HINT),
        )
        .child(labelled(
            "Provider",
            div()
                .flex()
                .flex_col()
                .gap(space::XS)
                .child(providers)
                .child(
                    div()
                        .text_type(ladder.caption)
                        .text_color(theme.muted)
                        .child(PROVIDER_PICK_HINT),
                ),
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
    })
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
