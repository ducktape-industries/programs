//! Repository settings: the default head, the force/delete flags and who
//! may write, as setting rows. Nothing the contract does not expose
//! appears here.
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::select::Writer;
use crate::state::{Menu, SettingsForm};
use crate::ui::changes::people_picker;
use crate::ui::components::{button, dropdown, id, ref_label};
use crate::ui::{PAGE_X, pending, reading, scroller, staged};
use crate::words;
use crate::{Forge, Stage};
use forge::Reply;
use view::design::Ladder;

/// The widest the rows run.
const ROWS_W: Pixels = px(664.);
/// The grant field.
const GRANT_W: Pixels = px(220.);

pub(crate) fn render(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let column = scroller("forge-settings")
        .px(PAGE_X + design::space::SM)
        .py(design::space::LG)
        .gap_0()
        .child(pending(forge, "settings", theme));
    let reply = match staged(
        forge,
        &forge.repo_query(),
        "forge-settings-repo",
        words::READING_REPOSITORY,
        cx,
        theme,
        ladder,
    ) {
        Ok(reply) => reply,
        Err(state) => return column.child(state).into_any_element(),
    };
    let Reply::Repo { repo, .. } = reply else {
        return column.into_any_element();
    };
    let Some(form) = forge.repo_settings.clone() else {
        return column.into_any_element();
    };
    let save = cx.listener(|forge, _: &ClickEvent, _, cx| forge.configure(cx));
    let rows = div()
        .id(id("forge-settings-rows"))
        .max_w(ROWS_W)
        .flex()
        .flex_col()
        .child(group("forge-settings-title", "Refs", theme))
        .child(design::setting_row(
            id("forge-settings-heads"),
            words::DEFAULT_HEAD,
            words::DEFAULT_HEAD_RULE,
            heads(forge, &form, cx, theme),
            ladder,
            theme,
        ))
        .children(flags(forge, &form, cx, theme, ladder))
        .child(
            div().flex().justify_end().py(design::space::LG).child(
                button(id("forge-settings-save"), "Save", ladder, theme, save)
                    .kind(design::Kind::Primary)
                    .enabled(forge.may_configure()),
            ),
        )
        .child(group("forge-settings-access-title", "Access", theme))
        .children(crate::policies::repo_row(forge, cx, ladder, theme))
        .child(access_row(
            "owner",
            forge.principal_name(&repo.repo.owner),
            "Owner",
            None,
            theme,
        ))
        .children(writers(forge, cx, theme, ladder))
        .child(grant_field(forge, &form, cx, theme, ladder));
    column.child(rows).into_any_element()
}

/// A group's heading, a hairline under it.
fn group(key: &str, title: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id(key.to_owned()))
        .pt(design::space::LG)
        .pb(design::space::XS)
        .border_b_1()
        .border_color(theme.border)
        .text_size(design::text::SECONDARY)
        .font_weight(FontWeight::SEMIBOLD)
        .role(Role::Heading)
        .aria_level(2)
        .child(title.to_owned())
}

/// The branches a default head may name, as a dropdown
/// ([`crate::ui::components::menu`]).
fn heads(forge: &Forge, form: &SettingsForm, cx: &mut Context<Forge>, theme: &Theme) -> AnyElement {
    dropdown(
        Menu::Head.key(),
        ref_label(&form.head),
        forge.menu == Some(Menu::Head),
        theme,
        cx.listener(|forge, event: &ClickEvent, window, cx| {
            forge.open_dropdown(Menu::Head, event, window, cx)
        }),
    )
}

/// The force and delete switches.
fn flags(
    forge: &Forge,
    form: &SettingsForm,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> [Stateful<Div>; 2] {
    let force = cx.listener(|forge, _: &ClickEvent, _, cx| {
        if let Some(form) = &mut forge.repo_settings {
            form.allow_force = !form.allow_force;
        }
        cx.notify();
    });
    let delete = cx.listener(|forge, _: &ClickEvent, _, cx| {
        if let Some(form) = &mut forge.repo_settings {
            form.allow_delete = !form.allow_delete;
        }
        cx.notify();
    });
    let configures = forge.may_configure();
    [
        design::setting_row(
            id("forge-settings-force-row"),
            words::ALLOW_FORCE_PUSHES,
            words::ALLOW_FORCE_PUSHES_RULE,
            design::switch(
                id("forge-settings-force"),
                words::ALLOW_FORCE_PUSHES,
                form.allow_force,
                configures,
                ladder,
                theme,
                force,
            ),
            ladder,
            theme,
        ),
        design::setting_row(
            id("forge-settings-delete-row"),
            words::ALLOW_REF_DELETION,
            words::ALLOW_REF_DELETION_RULE,
            design::switch(
                id("forge-settings-delete"),
                words::ALLOW_REF_DELETION,
                form.allow_delete,
                configures,
                ladder,
                theme,
                delete,
            ),
            ladder,
            theme,
        ),
    ]
}

/// One person with access: avatar, name over what they may do, and what
/// the owner can take back.
fn access_row(
    key: &str,
    name: String,
    role: &str,
    action: Option<AnyElement>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id(format!("forge-writer-{key}")))
        .flex()
        .items_center()
        .gap(design::space::LG)
        .py(design::space::MD)
        .border_b_1()
        .border_color(theme.border)
        .child(
            design::avatar(&name, design::size::AVATAR, theme)
                .border_1()
                .border_color(theme.border)
                .font_weight(FontWeight::SEMIBOLD),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::MEDIUM).child(name))
                .child(
                    div()
                        .text_size(design::text::SECONDARY)
                        .text_color(theme.muted)
                        .child(role.to_owned()),
                ),
        )
        .children(action)
}

/// Who to grant write access to, and Grant: an account number typed, or a
/// name searched and picked from the roster, which fills in its number.
fn grant_field(
    forge: &Forge,
    form: &SettingsForm,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let grant = cx.listener(|forge, _: &ClickEvent, _, cx| forge.grant(cx));
    // a typed number needs no search; a name does
    let typed = form.grant.text();
    let needle = typed.trim();
    let searching = !needle.is_empty() && forge::Principal::parse(needle).is_none();
    let matches = match searching {
        true => {
            let fill = |forge: &mut Forge, person: forge::Principal| {
                if let (Some(form), Some(number)) = (&mut forge.repo_settings, person.account()) {
                    form.grant.reset(number.to_string());
                }
            };
            people_picker(
                forge,
                "forge-grant-pick",
                needle,
                |_| false,
                fill,
                cx,
                theme,
                ladder,
            )
        }
        false => Vec::new(),
    };
    let control = div()
        .flex()
        .gap(design::space::SM)
        .items_center()
        .child(
            Input::new(
                id("forge-settings-grant-input"),
                &form.grant,
                words::GRANT_WRITE_ACCESS,
            )
            .h(design::size::CONTROL)
            .w(GRANT_W)
            .px_2()
            .border_1()
            .border_color(theme.border_strong)
            .bg(theme.background)
            .text_color(theme.foreground)
            .placeholder(words::SEARCH_MEMBERS)
            .when(forge.owns_repo(), |input| {
                input.on_submit(cx.listener(|forge, _: &(), _, cx| forge.grant(cx)))
            }),
        )
        .child(
            button(id("forge-settings-grant"), "Grant", ladder, theme, grant)
                .kind(design::Kind::Outline)
                .enabled(forge.owns_repo()),
        );
    div()
        .id(id("forge-settings-access"))
        .flex()
        .flex_col()
        .child(design::setting_row(
            id("forge-settings-grant-row"),
            words::GRANT_WRITE_ACCESS,
            words::PICK_A_WRITER,
            control,
            ladder,
            theme,
        ))
        .when(!matches.is_empty(), |access| {
            access.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .py(design::space::SM)
                    .children(matches),
            )
        })
}

fn writers(
    forge: &Forge,
    cx: &mut Context<Forge>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    match forge.writers_stage() {
        Stage::Ready(list) => writer_rows(forge, &Writer::rows(list), cx, theme)
            .into_iter()
            .map(IntoElement::into_any_element)
            .collect(),
        Stage::Loading => vec![reading(
            "forge-settings-writers",
            words::READING_WRITERS,
            theme,
        )],
        Stage::Failed(refusal) => {
            let retry = cx.listener(|forge, _: &ClickEvent, _, cx| forge.retry_writers(cx));
            vec![
                view::design::refused(
                    "forge-settings-writers",
                    design::sentence(refusal),
                    ladder,
                    theme,
                    retry,
                )
                .m_2()
                .into_any_element(),
            ]
        }
    }
}

/// One row per writer with its Revoke.
fn writer_rows(
    forge: &Forge,
    writers: &[Writer],
    cx: &mut Context<Forge>,
    theme: &Theme,
) -> Vec<Stateful<Div>> {
    writers
        .iter()
        .map(|key| {
            let revoke = cx.listener({
                let key = key.clone();
                move |forge, _: &ClickEvent, _, cx| forge.revoke(key.clone(), cx)
            });
            let owns = forge.owns_repo();
            let danger = theme.danger;
            let action = div()
                .id(id(format!("forge-settings-revoke-{}", key.key())))
                .px(design::space::SM)
                .py(design::space::XXS)
                .text_size(design::text::SECONDARY)
                .text_color(if owns { danger } else { theme.muted })
                .role(Role::Button)
                .when(!owns, |action| action.aria_disabled(true))
                .when(owns, |action| {
                    action
                        .hover(|style| style.text_decoration_1())
                        .focusable()
                        .on_click(revoke)
                })
                .child("Revoke");
            access_row(
                &key.key(),
                forge.writer_name(key),
                words::CAN_WRITE,
                Some(action.into_any_element()),
                theme,
            )
        })
        .collect()
}
