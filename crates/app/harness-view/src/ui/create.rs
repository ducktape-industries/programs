//! The create form: the ops a new agent takes, each with how far it got, the configuration
//! file it writes (where, the model and its provider from harness's routes, thinking, max output, wait, the
//! system prompt) and who it answers. Editing is the same form, filled from the agent's file.
use view::design::Ladder;
use view::design::{size, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::routes::providers;
use super::{back, caption, thinking_picker};
use crate::Agents;
use crate::create::{
    Answers, CREATE_INTRO, Changers, Draft, EDIT_INTRO, PROVIDER_PICK_HINT, VENDOR_HINT,
    proposed_path,
};
use harness::Thinking;

pub(super) fn render(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let draft = &view.draft;
    let name = draft.name.text();
    let (back_to, title, intro) = match draft.again {
        true => (name.clone(), format!("Edit {name}"), EDIT_INTRO),
        false => (
            "Agents".to_owned(),
            "Create an agent".to_owned(),
            CREATE_INTRO,
        ),
    };
    let sending = draft.sending.is_some();
    let advance = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.advance(cx));
    let cancel = cx.listener(|view: &mut Agents, _: &ClickEvent, _, cx| view.back(cx));
    let form = div()
        .flex_1()
        .min_w(px(320.))
        .max_w(px(560.))
        .flex()
        .flex_col()
        .gap(space::LG)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(space::HAIR)
                .child(design::heading("create-title", title, 1, theme))
                .child(caption(intro, theme)),
        )
        .child(fields(view, cx, theme, ladder))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("create-problem")
                .text_color(theme.danger)
                .child(problem)
        }))
        .child(
            div()
                .flex()
                .gap(space::SM)
                .child(
                    design::button("create-next", draft.verb(), ladder, theme, advance)
                        .kind(design::Kind::Primary)
                        .enabled(!sending),
                )
                .child(
                    design::button("create-cancel", "Cancel", ladder, theme, cancel)
                        .kind(design::Kind::Quiet),
                ),
        );
    div()
        .flex()
        .flex_col()
        .gap(space::BLOCK)
        .child(
            div()
                .flex()
                .child(back("create-back", back_to, cx, theme, ladder)),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_start()
                .gap(px(40.))
                .child(form)
                .when(!draft.again, |page| {
                    page.child(steps(draft, view.home(), theme).w(px(260.)))
                }),
        )
        .into_any_element()
}

fn steps(draft: &Draft, home: Option<&str>, theme: &Theme) -> view::Div {
    let created = draft
        .created
        .map(|agent| format!("Account {agent} created"));
    let committed = draft.committed.then(|| draft.path(home)).flatten();
    div()
        .flex()
        .flex_col()
        .gap(space::SM)
        .child(design::heading("create-sends", "What this sends", 2, theme))
        .child(caption("Each op is confirmed before it is signed.", theme))
        .children(
            draft
                .steps()
                .into_iter()
                .enumerate()
                .map(|(index, (what, done))| {
                    let mark = match done {
                        true => "✓".to_owned(),
                        false => (index + 1).to_string(),
                    };
                    div()
                        .id(SharedString::from(format!("create-step-{}", index + 1)))
                        .flex()
                        .items_center()
                        .gap(space::SM)
                        .child(
                            div()
                                .w(size::AVATAR)
                                .text_color(if done { theme.success } else { theme.muted })
                                .child(mark),
                        )
                        .child(what)
                }),
        )
        .children(created.map(|created| caption(created, theme)))
        .children(committed.map(|committed| caption(committed, theme)))
}

fn labelled(
    label: &'static str,
    hint: &'static str,
    field: impl IntoElement,
    theme: &Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(space::XXS)
        .child(div().font_weight(FontWeight::MEDIUM).child(label))
        .child(field)
        .child(caption(hint, theme))
}

pub(super) fn input(field: Input, theme: &Theme) -> Input {
    field
        .h(size::CONTROL)
        .w_full()
        .px(space::SM)
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .text_color(theme.foreground)
}

fn fields(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.draft;
    let named = draft.created.is_some();
    let mut form = div().flex().flex_col().gap(space::LG);
    if !named {
        form = form.child(labelled(
            "Name",
            "Chat shows it with the agent badge.",
            input(Input::new("create-name", &draft.name, "Name"), theme).placeholder("Scout"),
            theme,
        ));
    }
    form.child(labelled(
        "Configuration file",
        "Harness reads it at each model call. Only you may change it.",
        input(
            Input::new("create-path", &draft.path, "Configuration file"),
            theme,
        )
        .placeholder(proposed_path(view.home(), &draft.name.text()).unwrap_or_default()),
        theme,
    ))
    .when(!draft.again, |form| {
        form.child(model(view, cx, theme, ladder))
            .child(labelled(
                "Thinking",
                "How hard the model thinks before it answers.",
                div().flex().child(thinking_picker(
                    "create-thinking",
                    draft.thinking,
                    ladder,
                    theme,
                    cx.processor(|view: &mut Agents, thinking: Option<Thinking>, _, cx| {
                        view.draft.thinking = thinking;
                        cx.notify();
                    }),
                )),
                theme,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(space::LG)
                    .child(
                        labelled(
                            "Max output tokens",
                            "The most one answer writes.",
                            input(
                                Input::new(
                                    "create-max-tokens",
                                    &draft.max_tokens,
                                    "Max output tokens",
                                ),
                                theme,
                            )
                            .placeholder("4096"),
                            theme,
                        )
                        .w(px(200.)),
                    )
                    .child(
                        labelled(
                            "Wait",
                            "Blocks egress waits for the answer.",
                            input(Input::new("create-wait", &draft.wait, "Wait"), theme)
                                .placeholder("egress's default"),
                            theme,
                        )
                        .w(px(200.)),
                    ),
            )
    })
    .child(labelled(
        "System prompt",
        "Each paragraph is its own system block; harness adds its ducktape manual after them.",
        Textarea::new("create-system", &draft.system, "System prompt")
            .min_h(size::CONTROL * 4.)
            .w_full()
            .px(space::SM)
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.background)
            .text_color(theme.foreground)
            .placeholder("What this agent does, and how"),
        theme,
    ))
    .when(draft.again || !draft.scripted, |form| {
        form.child(answers(draft, cx, theme, ladder))
    })
    .when(draft.again, |form| {
        form.child(changers(draft, cx, theme, ladder))
    })
}

fn model(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let draft = &view.draft;
    let set = |view: &mut Agents, provider: String, _: &mut Context<Agents>| {
        view.draft.provider = Some(provider);
    };
    let picked = draft.provider.as_deref();
    let providers = providers(view, picked, "create-provider", set, cx, theme, ladder);
    div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .child(labelled(
            "Model",
            VENDOR_HINT,
            input(Input::new("create-model", &draft.vendor, "Model"), theme)
                .placeholder("claude-opus-5-5"),
            theme,
        ))
        .child(labelled("Provider", PROVIDER_PICK_HINT, providers, theme))
}

pub(super) fn pickable(id: String, picked: bool, theme: &Theme) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(SharedString::from(id))
        .flex()
        .items_center()
        .gap(space::LG)
        .px(space::SM)
        .py(space::XS)
        .border_1()
        .border_color(if picked {
            theme.foreground
        } else {
            theme.border
        })
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .aria_toggled(picked.into())
        .focusable()
}

fn answers(
    draft: &Draft,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let choices = [
        (Answers::Manager, "Only me"),
        (Answers::Anyone, "Anyone"),
        (Answers::Listed, "These accounts…"),
    ];
    let picked = choices
        .iter()
        .position(|(answers, _)| *answers == draft.answers);
    let segmented = design::segmented(
        "create-answers",
        "Who it answers",
        ladder,
        theme,
        picked,
        choices.iter().enumerate().map(|(index, (_, label))| {
            (
                ElementId::Name(format!("create-answers-{index}").into()),
                SharedString::from(*label),
            )
        }),
        cx.processor(move |view: &mut Agents, index: usize, _, cx| {
            view.draft.answers = choices[index].0;
            cx.notify();
        }),
    );
    labelled(
        "Who it answers",
        "Its manager always.",
        div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(div().flex().child(segmented))
            .when(draft.answers == Answers::Listed, |field| {
                field.child(
                    input(
                        Input::new(
                            "create-accounts",
                            &draft.listed,
                            "Account numbers and programs",
                        ),
                        theme,
                    )
                    .placeholder("7, 12, forge"),
                )
            }),
        theme,
    )
}

fn changers(
    draft: &Draft,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let choices = [
        (Changers::Manager, "Only me"),
        (Changers::Listed, "These accounts…"),
    ];
    let picked = choices
        .iter()
        .position(|(changers, _)| Some(*changers) == draft.changers);
    let segmented = design::segmented(
        "create-changers",
        "Who may change the configuration",
        ladder,
        theme,
        picked,
        choices.iter().enumerate().map(|(index, (_, label))| {
            (
                ElementId::Name(format!("create-changers-{index}").into()),
                SharedString::from(*label),
            )
        }),
        cx.processor(move |view: &mut Agents, index: usize, _, cx| {
            view.draft.changers = Some(choices[index].0);
            cx.notify();
        }),
    );
    labelled(
        "Who may change the configuration",
        "Its manager always.",
        div()
            .flex()
            .flex_col()
            .gap(space::XS)
            .child(div().flex().child(segmented))
            .when(draft.changers == Some(Changers::Listed), |field| {
                field.child(
                    input(
                        Input::new(
                            "create-changers-listed",
                            &draft.changers_listed,
                            "Account numbers and programs",
                        ),
                        theme,
                    )
                    .placeholder("7, 12"),
                )
            }),
        theme,
    )
}
