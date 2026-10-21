//! One member: who the account is, the devices it acts with, the agents it
//! manages and what it signed lately. Expo `app/(tabs)/members/[number].tsx`
//! (board 91). Read-only, as the desk's detail is.
//!
//! Reads the roster, the account's valset standing, its recent activity
//! (`Members::activity`), and the session's reader, chain and signing key.
//! Writes the page stack only: Back, and a manager or an agent opened over
//! this page (`Members::open`). Open DM and Explorer leave through the
//! host's links.
use view::design::{self, Ladder, Nav, space};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::parts::{face, name_line, quiet};
use crate::activity::Signed;
use crate::{
    Device, Group, Members, Row, explorer_account, explorer_block, name_or_number, opens,
    standing_badge, valset_badge, words,
};

pub(super) fn page(
    view: &Members,
    number: u64,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let back = cx.listener(|view, _: &ClickEvent, _, cx| view.back(cx));
    let (Some(rows), Some(row)) = (view.rows.ready(), view.selected_row()) else {
        return missing(view, number, back, cx, ladder, theme);
    };
    let explorer = explorer_account(number);
    let open_explorer = opens(explorer.clone());
    let mut body = div()
        .id("members-detail")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .pb(space::BLOCK)
        .child(head(view, row, rows, cx, ladder, theme))
        .children(bio(row, ladder))
        .child(actions(view, row, explorer, ladder, theme))
        .children(devices(view, row, ladder, theme));
    let managed = view.managed(number);
    if !managed.is_empty() {
        body = body.children(manages(view, &managed, cx, ladder, theme));
    }
    if !row.devices.is_empty() {
        body = body.children(activity(view, ladder, theme));
    }
    if row.group() == Group::Agents {
        let note = words::managed_where(&row.kind);
        body = body.child(quiet("members-managed-in-settings", note, ladder, theme));
    }
    design::page(
        "members-member",
        row.name.clone(),
        Nav::back(view.pages.depth(), back),
        ladder,
        theme,
    )
    .caption(words::account(number))
    .trailing(design::icon_button(
        "members-explorer-open",
        design::icon("compass", ladder.icon),
        words::EXPLORER,
        ladder,
        theme,
        open_explorer,
    ))
    .child(body)
    .into_any_element()
}

fn missing(
    view: &Members,
    number: u64,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let body = match &view.rows {
        Loadable::Failed(refusal) => crate::refused(refusal, cx, ladder, theme).into_any_element(),
        Loadable::Ready(_) | Loadable::Reloading(..) => design::empty_state(
            "members-no-account",
            "user-x",
            words::no_account(number),
            words::NO_ACCOUNT_WHY,
            ladder,
            theme,
        )
        .into_any_element(),
        Loadable::Idle | Loadable::Loading(_) => {
            quiet("members-loading", words::READING_ROSTER, ladder, theme)
        }
    };
    design::page(
        "members-member",
        words::account_title(number),
        Nav::back(view.pages.depth(), back),
        ladder,
        theme,
    )
    .child(body)
    .into_any_element()
}

fn head(
    view: &Members,
    row: &Row,
    rows: &[Row],
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let me = view.me == Some(row.number);
    let title = name_line(row, me, ladder.big_title, ladder, theme)
        .font_weight(FontWeight::MEDIUM)
        .id("members-detail-title")
        .role(Role::Heading)
        .aria_level(2);
    div()
        .flex()
        .gap(space::LG)
        .p(space::BLOCK)
        .pb(space::LG)
        .child(face(
            row,
            view.picture(row, cx),
            ladder.avatar_detail,
            theme,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(space::XXS)
                .child(title)
                .child(account_line(row, rows, cx, ladder, theme))
                .child(badges(row, ladder, theme)),
        )
}

fn account_line(
    row: &Row,
    rows: &[Row],
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let what = format!("{} · {}", words::account(row.number), row.kind_word());
    let devices = words::devices(row.devices.len());
    let line = div()
        .id("members-account-line")
        .flex()
        .flex_wrap()
        .items_baseline()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.caption)
        .text_color(theme.muted);
    match row.manager() {
        Some(manager) => {
            let who = name_or_number(rows, manager);
            let open = cx.listener(move |view, _: &ClickEvent, _, cx| view.open(manager, cx));
            line.child(format!("{what} · {} ", words::MANAGED_BY))
                .child(inline_link("members-manager", who, open, ladder, theme))
                .child(format!(" · {devices}"))
        }
        None if row.group() == Group::Programs => line.child(what),
        None => line.child(format!("{what} · {devices}")),
    }
}

fn badges(row: &Row, ladder: &Ladder, theme: &Theme) -> Div {
    let standing = standing_badge("members-standing", row, ladder, theme);
    div()
        .flex()
        .flex_wrap()
        .gap(space::XS)
        .children(standing)
        .children(row.seat.map(|seat| valset_badge(seat, ladder, theme)))
}

fn bio(row: &Row, ladder: &Ladder) -> Option<Stateful<Div>> {
    let bio = row.bio.clone().filter(|bio| !bio.trim().is_empty())?;
    Some(
        div()
            .id("members-bio")
            .px(space::BLOCK)
            .pb(space::LG)
            .text_type(ladder.secondary)
            .child(bio),
    )
}

fn actions(view: &Members, row: &Row, explorer: String, ladder: &Ladder, theme: &Theme) -> Div {
    let dm = view.dm_link(row);
    div()
        .flex()
        .gap(space::SM)
        .px(space::BLOCK)
        .pb(space::LG)
        .children(
            dm.map(|link| small_button("members-open-dm", words::OPEN_DM, link, ladder, theme)),
        )
        .child(small_button(
            "members-explorer",
            words::EXPLORER,
            explorer,
            ladder,
            theme,
        ))
}

fn devices(view: &Members, row: &Row, ladder: &Ladder, theme: &Theme) -> Vec<AnyElement> {
    let count = row.devices.len();
    let head = design::section_head("members-devices", words::DEVICES, ladder, theme)
        .child(count.to_string());
    let mut section = vec![head.into_any_element()];
    if row.devices.is_empty() {
        section.push(quiet(
            "members-no-devices",
            words::NO_DEVICES,
            ladder,
            theme,
        ));
    }
    let rows = row.devices.iter().enumerate();
    section.extend(rows.map(|(index, device)| device_row(view, index, device, ladder, theme)));
    section
}

fn device_row(
    view: &Members,
    index: usize,
    device: &Device,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let hex = device.hex();
    let glyph = match hex == view.signer {
        true => "smartphone",
        false => "key-round",
    };
    let label = device.title(index);
    let added = div()
        .flex_none()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.caption)
        .text_color(theme.faint)
        .child(words::added(device.added_at));
    design::row(format!("members-device-{index}"), ladder, theme)
        .min_h(ladder.row_tall)
        .child(design::lead(
            design::icon(glyph, ladder.icon_row).text_color(theme.muted),
        ))
        .child(
            design::row_words(div().truncate().child(label)).child(
                design::row_caption(design::short_hex(&hex), ladder, theme)
                    .font_family(design::fonts::FAMILY_MONO),
            ),
        )
        .child(added)
        .into_any_element()
}

fn manages(
    view: &Members,
    managed: &[&Row],
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> Vec<AnyElement> {
    let head = design::section_head("members-manages", words::MANAGES, ladder, theme)
        .child(managed.len().to_string());
    let rows = managed
        .iter()
        .map(|agent| managed_row(agent, view.picture(agent, cx), cx, ladder, theme));
    std::iter::once(head.into_any_element())
        .chain(rows)
        .collect()
}

fn managed_row(
    agent: &Row,
    picture: Option<std::sync::Arc<view::Image>>,
    cx: &mut Context<Members>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let number = agent.number;
    let about = words::agent(number);
    let life = agent.life();
    let row = design::pressed(
        design::row(format!("members-manages-{number}"), ladder, theme),
        ladder,
    )
    .min_h(ladder.row_tall)
    .role(Role::Button)
    .aria_label(agent.name.clone())
    .aria_description(format!("{about} · {life}"))
    .focusable()
    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.open(number, cx)))
    .child(design::lead(face(agent, picture, ladder.avatar, theme)))
    .child(
        design::row_words(div().truncate().child(agent.name.clone()))
            .child(design::row_caption(about, ladder, theme)),
    )
    .children(standing_badge(
        "members-manages-standing",
        agent,
        ladder,
        theme,
    ))
    .child(design::chevron(ladder, theme));
    row.into_any_element()
}

fn activity(view: &Members, ladder: &Ladder, theme: &Theme) -> Vec<AnyElement> {
    let head = design::section_head("members-activity", words::RECENT_ACTIVITY, ladder, theme);
    let body = match &view.activity {
        Loadable::Idle | Loadable::Loading(_) => vec![quiet(
            "members-activity-loading",
            words::READING_ACTIVITY,
            ladder,
            theme,
        )],
        Loadable::Failed(refusal) => vec![quiet(
            "members-activity-refused",
            design::sentence(refusal),
            ladder,
            theme,
        )],
        Loadable::Ready(recent) | Loadable::Reloading(recent, _) if recent.items.is_empty() => {
            vec![quiet(
                "members-no-activity",
                words::NO_ACTIVITY,
                ladder,
                theme,
            )]
        }
        Loadable::Ready(recent) | Loadable::Reloading(recent, _) => recent
            .items
            .iter()
            .enumerate()
            .map(|(index, signed)| signed_row(index, signed, recent.now, ladder, theme))
            .collect(),
    };
    std::iter::once(head.into_any_element())
        .chain(body)
        .collect()
}

fn signed_row(
    index: usize,
    signed: &Signed,
    now: u64,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    let place = design::grouped(signed.height);
    let open = opens(explorer_block(signed.height));
    let when = div()
        .flex()
        .items_baseline()
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.caption)
        .text_color(theme.muted)
        .child(inline_link(
            format!("members-block-{index}"),
            words::block(&place),
            open,
            ladder,
            theme,
        ))
        .child(format!(" · {}", design::ago(now, signed.time)));
    design::row(format!("members-signed-{index}"), ladder, theme)
        .min_h(ladder.row_tall)
        .child(design::row_words(div().truncate().child(signed.title().to_owned())).child(when))
        .into_any_element()
}

fn small_button(
    id: &'static str,
    label: &'static str,
    link: String,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    design::button(id, label, ladder, theme, opens(link))
        .kind(design::Kind::Outline)
        .small()
}

fn inline_link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ladder: &Ladder,
    theme: &Theme,
) -> Div {
    let label = label.into();
    let line = ladder.small.line.unwrap_or(ladder.small.size);
    div()
        .relative()
        .flex_none()
        .h(line)
        .font_family(design::fonts::FAMILY_MONO)
        .text_type(ladder.small)
        .text_color(theme.foreground)
        .text_decoration_1()
        .whitespace_nowrap()
        .child(label.clone())
        .child(
            design::hit(id, px(0.), line, px(0.), ladder)
                .role(Role::Link)
                .aria_label(label)
                .focusable()
                .on_click(click),
        )
}
