//! The Account screen: a left menu (Account, Agents, Policies, Invites) and the one
//! pane it has open. `render` reads the state and changes nothing; presses
//! land in `actions.rs`.
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use crate::Settings;
use crate::queries::Seat;
use crate::state::{Form, Section, TTL};
use crate::words;
use view::design::Ladder;

mod account;
pub(crate) mod phone;
pub(crate) mod photo;
mod policies;
mod shared;

const NAV_W: Pixels = px(200.);
const PANE_W: Pixels = px(720.);
const FIELD_W: Pixels = px(220.);

pub(crate) fn render(view: &Settings, cx: &mut Context<Settings>) -> impl IntoElement {
    let theme = *cx.global::<Theme>();
    let ladder = &design::POINTER;
    let section = view.shown();
    let body = match section {
        Section::Account => account::account(view, cx, &theme, ladder),
        Section::Agents => account::agents_pane(view, cx, &theme, ladder),
        Section::Policies => policies::pane(view, cx, ladder, &theme),
        Section::Invites => invites(view, cx, &theme, ladder),
    };
    let sheet = photo::sheet(view, cx, ladder, &theme)
        .or_else(|| policies::sheet(view, cx, ladder, &theme));
    div()
        .id("settings")
        .flex()
        .size_full()
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(nav(view, section, cx, &theme))
        .child(
            div()
                .id("settings/scroll")
                .flex_1()
                .min_w(px(0.))
                .overflow_y_scroll()
                .px(design::space::XL + design::space::SM)
                .py(design::space::XL)
                .child(
                    column("settings/pane")
                        .max_w(PANE_W)
                        .gap_0()
                        .child(eyebrow("settings/title", section.label(), &theme))
                        .child(body),
                ),
        )
        .children(sheet)
}

fn nav(
    view: &Settings,
    shown: Section,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> Stateful<Div> {
    let theme = *theme;
    let sections = view.sections();
    let at = sections
        .iter()
        .position(|section| *section == shown)
        .unwrap_or_default();
    let picked = sections.clone();
    design::composite("settings/nav", Role::TabList, "Settings")
        .active(at, sections.len())
        .wrap()
        .on_move(cx.processor(move |v: &mut Settings, index: usize, _, cx| {
            v.select_section(picked[index], cx)
        }))
        .build()
        .w(NAV_W)
        .flex_none()
        .flex()
        .flex_col()
        .gap(design::space::HAIR)
        .px(design::space::SM)
        .py(design::space::LG)
        .border_r_1()
        .border_color(theme.border)
        .children(sections.into_iter().map(|section| {
            let on = section == shown;
            let pick = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
                v.select_section(section, cx)
            });
            let tab = div()
                .id(format!("settings/nav/{}", section.slug()))
                .h(px(30.))
                .px(design::space::LG)
                .flex()
                .items_center()
                .text_color(if on { theme.foreground } else { theme.muted })
                .when(on, |item| item.bg(theme.surface_raised))
                .when(!on, |item| {
                    item.hover(move |style| style.text_color(theme.foreground))
                })
                .aria_selected(on)
                .on_click(pick)
                .child(section.label());
            design::item(tab, Role::Tab, on)
        }))
}

fn invites(
    view: &Settings,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let network = words::network(&view.session).unwrap_or("this network");
    let choices = design::segmented(
        "settings/ttl",
        "Expires after",
        ladder,
        theme,
        Some(view.ttl),
        TTL.into_iter().map(|days| {
            (
                format!("settings/ttl/{days}").into(),
                design::plural(days, "day", "days").into(),
            )
        }),
        cx.processor(|v: &mut Settings, i: usize, _, cx| {
            v.ttl = i;
            cx.notify();
        }),
    );
    let mint = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.mint_invite(cx));
    let minting = view.invite.is_loading();
    let body = column("settings/network/body")
        .gap_0()
        .child(design::setting_row(
            "settings/invite/ttl",
            "Expires after",
            format!("An invite lets one node join {network} as a resident."),
            choices,
            ladder,
            theme,
        ))
        .child(design::setting_row(
            "settings/invite/help",
            "Mint an invite",
            "Send it to them any way you like.",
            submit(
                "settings/invite/mint",
                "Mint invite",
                "Minting…",
                minting,
                true,
                theme,
                mint,
            ),
            ladder,
            theme,
        ));
    let minted = match &view.invite {
        Loadable::Ready(invite) | Loadable::Reloading(invite, _) => {
            let copy = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.copy_invite(cx));
            let (copied, tone) = match &view.copied {
                Loadable::Ready(()) | Loadable::Reloading((), _) => {
                    ("Copied".to_owned(), theme.success)
                }
                Loadable::Failed(refusal) => (design::sentence(refusal), theme.danger),
                Loadable::Idle | Loadable::Loading(_) => (String::new(), theme.muted),
            };
            column("settings/invite/minted")
                .py(design::space::MD)
                .border_b_1()
                .border_color(theme.border)
                .child(eyebrow("settings/invite/minted-label", "Minted", theme).mb_0())
                .children(invite.notes.iter().enumerate().map(|(i, note)| {
                    secondary(format!("settings/invite/note/{i}"), &note.message, theme)
                }))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(design::space::SM)
                        .child(
                            div()
                                .id("settings/invite/blob")
                                .flex_1()
                                .min_w(px(0.))
                                .p(design::space::LG)
                                .bg(theme.surface)
                                .border_1()
                                .border_color(theme.border)
                                .font_family(design::fonts::FAMILY_MONO)
                                .text_size(design::text::SECONDARY)
                                .child(invite.invite.clone()),
                        )
                        .child(button("settings/invite/copy", "Copy invite", theme).on_click(copy)),
                )
                .child(secondary("settings/invite/copied", copied, theme).text_color(tone))
                .into_any_element()
        }
        Loadable::Failed(refusal) => refusal_line("invite", &design::sentence(refusal), theme)
            .mt(design::space::MD)
            .into_any_element(),
        Loadable::Loading(_) => secondary("settings/invite/loading", "Minting invite…", theme)
            .py(design::space::MD)
            .into_any_element(),
        Loadable::Idle => secondary("settings/invite/empty", "No invite minted yet.", theme)
            .py(design::space::MD)
            .into_any_element(),
    };
    body.child(minted).into_any_element()
}

fn eyebrow(id: impl Into<String>, text: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .mb(design::space::BLOCK)
        .font_family(design::fonts::FAMILY_MONO)
        .text_size(design::text::CAPTION)
        .text_color(theme.muted)
        .child(text.to_owned())
}

fn group(id: impl Into<String>, title: &str, count: Option<usize>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .flex()
        .items_baseline()
        .gap(design::space::SM)
        .pt(design::space::XL)
        .pb(design::space::XXS)
        .border_b_1()
        .border_color(theme.border)
        .text_size(design::text::SECONDARY)
        .font_weight(FontWeight::SEMIBOLD)
        .role(Role::Heading)
        .aria_level(2)
        .child(title.to_owned())
        .children(count.map(|count| {
            div()
                .font_family(design::fonts::FAMILY_MONO)
                .text_size(design::text::CAPTION)
                .text_color(theme.muted)
                .font_weight(FontWeight::NORMAL)
                .child(count.to_string())
        }))
}

fn column(id: impl Into<String>) -> Stateful<Div> {
    div().id(id.into()).flex().flex_col().gap_2().w_full()
}

fn problem(key: &str, form: &Form, empty: &str, theme: &Theme) -> Option<Stateful<Div>> {
    let problem = form.problem.as_ref()?;
    Some(refusal_line(key, &words::problem_text(problem, empty), theme).my(design::space::SM))
}

fn refusal_line(key: &str, sentence: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(format!("settings/{key}/refused"))
        .bg(theme.danger_soft)
        .border_1()
        .border_color(theme.danger)
        .px_3()
        .py_2()
        .child(
            div()
                .id(format!("settings/{key}/why"))
                .w_full()
                .child(sentence.to_owned()),
        )
}

fn secondary(id: impl Into<String>, text: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id.into())
        .text_size(design::text::SECONDARY)
        .text_color(theme.muted)
        .child(text.into())
}

fn field(id: &str, name: &str, hint: &str, form: &Form, theme: &Theme) -> Input {
    Input::new(id.to_owned(), &form.text, name.to_owned())
        .h(design::size::CONTROL)
        .w(FIELD_W)
        .px_2()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.background)
        .placeholder(hint.to_owned())
        .disabled(form.busy)
}

fn submit(
    id: &str,
    label: &str,
    busy_label: &str,
    busy: bool,
    primary: bool,
    theme: &Theme,
    pressed: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let button = match primary {
        true => self::primary(id.to_owned(), if busy { busy_label } else { label }, theme),
        false => self::button(id.to_owned(), if busy { busy_label } else { label }, theme),
    };
    button
        .aria_disabled(busy)
        .when(busy, |b| b.opacity(0.5).tab_stop(false))
        .when(!busy, |b| b.on_click(pressed))
}

fn button(id: impl Into<String>, label: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    let theme = *theme;
    div()
        .id(id.into())
        .h(design::size::CONTROL)
        .px(design::space::MD)
        .flex()
        .flex_none()
        .items_center()
        .whitespace_nowrap()
        .text_size(design::text::SECONDARY)
        .bg(theme.background)
        .border_1()
        .border_color(theme.border_strong)
        .hover(move |style| style.bg(theme.surface))
        .role(Role::Button)
        .focusable()
        .child(label.into())
}

fn primary(id: impl Into<String>, label: impl Into<String>, theme: &Theme) -> Stateful<Div> {
    let button = button(id, label, theme)
        .bg(theme.primary)
        .border_color(theme.primary)
        .text_color(theme.primary_foreground)
        .hover(|style| style.opacity(0.9));
    design::focus_shown_on_ink(button, theme)
}
