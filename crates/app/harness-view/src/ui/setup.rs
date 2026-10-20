//! The front-matter editor both trees draw where an agent's setup shows: the model, its provider
//! from harness's routes, thinking, max output and wait, the server tools, the rooms it hears every message of
//! (a finder adds more), settle and compact_at, then Cancel and Save.
use harness::{ServerTool, Thinking};
use view::Div;
use view::design::{Ladder, space};
use view::prelude::*;

use super::routes::providers;
use super::{pick, thinking_picker};
use crate::Agents;
use crate::create::{PROVIDER_PICK_HINT, VENDOR_HINT};
use crate::setup::Press;

fn pressing(
    press: Press,
    cx: &mut Context<Agents>,
) -> impl Fn(&ClickEvent, &mut view::Window, &mut App) + 'static {
    cx.listener(move |view: &mut Agents, _: &ClickEvent, _, cx| view.press_setup(press.clone(), cx))
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

pub(crate) fn editor(
    view: &Agents,
    cx: &mut Context<Agents>,
    theme: &Theme,
    ladder: &Ladder,
) -> Div {
    let draft = &view.setup_draft;
    let thinking = thinking_picker(
        "setup-thinking",
        draft.thinking,
        ladder,
        theme,
        cx.processor(|view: &mut Agents, thinking: Option<Thinking>, _, cx| {
            view.press_setup(Press::Thinking(thinking), cx)
        }),
    );
    let thinking = match ladder.touch {
        true => div().flex().flex_col().child(thinking),
        false => div().flex().child(thinking),
    };
    let tools: Vec<_> = ServerTool::ALL
        .iter()
        .map(|tool| {
            let picked = draft.server_tools.contains(tool);
            pick(format!("setup-tool-{}", tool.word()), picked, theme, ladder)
                .on_click(pressing(Press::Tool(*tool), cx))
                .child(design::mono(tool.word()).text_type(ladder.small))
        })
        .collect();
    let number = |id: &'static str,
                  field: &TextField,
                  label: &'static str,
                  caption: &'static str,
                  placeholder: &'static str| {
        design::field(
            Input::new(id, field, label).placeholder(placeholder),
            ladder,
            theme,
        )
        .label(label)
        .caption(caption)
    };
    let save = design::button(
        "setup-save",
        "Save",
        ladder,
        theme,
        pressing(Press::Save, cx),
    )
    .kind(design::Kind::Primary)
    .busy(draft.sending.is_some());
    let cancel = design::button(
        "setup-cancel",
        "Cancel",
        ladder,
        theme,
        pressing(Press::Close, cx),
    )
    .kind(design::Kind::Quiet);
    let set = |view: &mut Agents, provider: String, cx: &mut Context<Agents>| {
        view.press_setup(Press::Provider(provider), cx)
    };
    let picked = draft.provider.as_deref();
    let providers = providers(view, picked, "setup-provider", set, cx, theme, ladder);
    let rooms = rooms(view, cx, theme, ladder);
    div()
        .flex()
        .flex_col()
        .gap(space::LG)
        .child(number(
            "setup-model",
            &draft.vendor,
            "Model",
            VENDOR_HINT,
            "claude-opus-5-5",
        ))
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
        .child(labelled("Thinking", thinking, theme, ladder))
        .child(number(
            "setup-max-tokens",
            &draft.max_tokens,
            "Max output tokens",
            "The most one answer writes.",
            "4096",
        ))
        .child(number(
            "setup-wait",
            &draft.wait,
            "Wait",
            "Blocks egress waits for the answer; empty: egress's default.",
            "egress's default",
        ))
        .child(labelled(
            "Server tools",
            div().flex().flex_col().gap(space::XS).children(tools),
            theme,
            ladder,
        ))
        .child(labelled("Listens to", rooms, theme, ladder))
        .child(number(
            "setup-settle",
            &draft.settle,
            "Settle (ms)",
            "How long its general input waits for more before a turn; empty: no wait.",
            "10000",
        ))
        .child(number(
            "setup-compact-at",
            &draft.compact_at,
            "Compacts at",
            "The tokens of context at which its conversations start compacting; empty: when the window fills.",
            "650000",
        ))
        .children(draft.problem.clone().map(|problem| {
            div()
                .id("setup-problem")
                .text_type(ladder.secondary)
                .text_color(theme.danger)
                .child(problem)
        }))
        .child(design::form_verbs(cancel, save))
}

fn rooms(view: &Agents, cx: &mut Context<Agents>, theme: &Theme, ladder: &Ladder) -> Div {
    let draft = &view.setup_draft;
    let rooms = view.rooms.borrow();
    let named = |id: &str| {
        let name = rooms.get(id);
        name.map_or_else(|| id.to_owned(), |name| format!("#{name}"))
    };
    let listed: Vec<_> = draft
        .listen
        .iter()
        .map(|id| {
            pick(format!("setup-room-{id}"), true, theme, ladder)
                .on_click(pressing(Press::Listen(id.clone()), cx))
                .child(named(id))
        })
        .collect();
    let typed = draft.rooms.text().trim().to_lowercase();
    let mut found: Vec<(&String, &String)> = match typed.is_empty() {
        false => rooms
            .iter()
            .filter(|(id, name)| {
                !draft.listen.contains(*id) && name.to_lowercase().contains(&typed)
            })
            .collect(),
        true => Vec::new(),
    };
    found.sort_by_key(|(_, name)| *name);
    let found: Vec<_> = found
        .into_iter()
        .map(|(id, name)| {
            pick(format!("setup-room-{id}"), false, theme, ladder)
                .on_click(pressing(Press::Listen(id.clone()), cx))
                .child(format!("#{name}"))
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(space::XS)
        .children(listed)
        .child(
            design::field(
                Input::new("setup-rooms", &draft.rooms, "Find a room").placeholder("a room's name"),
                ladder,
                theme,
            )
            .caption("The rooms it hears every message of: a press adds one or drops it."),
        )
        .children(found)
}
