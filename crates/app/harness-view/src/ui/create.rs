//! The create form: the ops a new agent takes, each with how far it got, the configuration
//! file it writes (where, the model from harness's records, max output, wait, thinking, the
//! system prompt) and who it answers. Editing is the same form, its fields filled from the agent's
//! file, saved as a new version of it.
use view::design::Ladder;
use view::design::{size, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::{back, caption, thinking_picker, waiting};
use crate::Agents;
use crate::create::{Answers, CREATE_INTRO, Changers, Draft, EDIT_INTRO, proposed_path};
use crate::words::{serving_words, window_short};
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

fn input(field: Input, theme: &Theme) -> Input {
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
    .child(models(view, cx, theme, ladder))
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
                        Input::new("create-max-tokens", &draft.max_tokens, "Max output tokens"),
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

/// Harness's model records, one press each: the id the file names, its wire, its service and
/// how many member nodes serve it now.
fn models(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> impl IntoElement + use<> {
    let list = match waiting("create-models", &view.models, cx, theme, ladder) {
        Some(shown) => shown,
        None => {
            let models = view.models.ready().map_or(&[][..], Vec::as_slice);
            let rows = models.iter().map(|model| {
                let id = model.row.id.clone();
                let picked = view.draft.model.as_deref() == Some(id.as_str());
                let (now, tone) = serving_words(model.serving, theme);
                let pick = id.clone();
                pickable(format!("model-{id}"), picked, theme)
                    .on_click(
                        cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| {
                            view.draft.model = Some(pick.clone());
                            cx.notify();
                        }),
                    )
                    .child(
                        design::mono(id)
                            .flex_none()
                            .w(px(150.))
                            .truncate()
                            .font_weight(FontWeight::MEDIUM),
                    )
                    .child(div().flex_none().w(px(140.)).child(model.row.wire.label()))
                    .child(
                        caption(
                            format!("{} · {}", model.row.service, window_short(model.row.window)),
                            theme,
                        )
                        .flex_1()
                        .min_w_0()
                        .truncate(),
                    )
                    .child(caption(now, theme).flex_none().text_color(tone))
            });
            div()
                .flex()
                .flex_col()
                .children(rows)
                .when(models.is_empty(), |list| {
                    list.child(design::quiet(
                        "Harness keeps no model yet: set one on the Models page.",
                        theme,
                    ))
                })
                .into_any_element()
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(space::XXS)
        .child(div().font_weight(FontWeight::MEDIUM).child("Model"))
        .child(list)
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

/// Who may change the configuration file: its manager alone, or the accounts listed too. An
/// edit picks it; left as it opened, nothing is sent.
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
