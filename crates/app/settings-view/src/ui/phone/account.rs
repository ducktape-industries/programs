//! Expo `app/account/index.tsx`: the Account screen. Its title row with the
//! network under it, the strip of panes (Account; Agents, for an account;
//! Invites), and the Account pane: who the seated key is and
//! its keys, or what stands in for them.
//!
//! Reads the seat (`view.account`), the session (the network's name, this
//! device's key), the panes there are and the one shown, and the
//! create-account form. Writes the open pane (`select_section`), a new
//! account (`submit_create_account`) and a read again after a refusal
//! (`read_account`). "Add a device" is the phone shell's row, not this
//! view's (SPEC §7.4).
use view::design::{Kind as ButtonKind, Ladder, Nav, Orientation};
use view::prelude::*;
use view::{Div, FontWeight, Stateful};

use super::agents;
use super::parts::{
    column, mono, muted_icon, name_input, name_line, named, note, problem_of, quiet,
};
use crate::Settings;
use crate::queries::{Account, Key, Seat};
use crate::state::Section;
use crate::words;

/// The page element's id; `design::page` names its title row after it.
const PAGE: &str = "settings/page";
/// This device's key, where no account names it (Expo
/// `app/account/index.tsx:97`).
const THIS_DEVICE: &str = "Phone";
/// A phone runs no node, and only the machine running one mints its
/// invites: the Invites pane is this line (the Expo app's, from the app's
/// `invite_unsupported` refusal).
const NO_INVITES: &str = "Only the machine running this node can mint its invites.";

/// The screen: the title row, the strip, the open pane under it.
pub(super) fn screen(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> impl IntoElement {
    let section = view.shown();
    let pane = match section {
        Section::Account => account_pane(view, cx, ladder, theme),
        Section::Agents => agents::pane(view, cx, ladder, theme),
        Section::Policies => crate::ui::policies::pane(view, cx, ladder, theme),
        Section::Invites => {
            quiet("settings/invite/unsupported", NO_INVITES, ladder, theme).into_any_element()
        }
    };
    // Expo's Account screen is a page with Back (`app/account/index.tsx:38`);
    // here the shell's capsule closes it, so it is a child page with no Back
    // of its own. It is the account place itself: it draws no avatar.
    let page = design::page(PAGE, "Account", Nav::Child, ladder, theme);
    // SDK gap: the network caption, swap when the SDK builds it (kit.md §2)
    let page = match words::network(&view.session) {
        Some(network) => page.caption(network.to_owned()),
        None => page,
    };
    page.child(strip(view, section, cx, ladder, theme)).child(
        div()
            .id("settings/scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pb(design::space::BLOCK)
            .child(pane),
    )
}

/// The panes under the title row (the Expo kit's `Strip`,
/// `src/kit/Choice.tsx:114`): one Tab stop, ← → open the next.
fn strip(
    view: &Settings,
    shown: Section,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let sections = view.sections();
    let at = sections
        .iter()
        .position(|section| *section == shown)
        .unwrap_or_default();
    let picked = sections.clone();
    let tabs = sections.into_iter().map(|section| {
        let on = section == shown;
        let pick = cx
            .listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.select_section(section, cx));
        let id = format!("settings/strip/{}", section.slug());
        design::item(
            design::tab(id, section.label(), on, ladder, theme, pick),
            Role::Tab,
            on,
        )
    });
    design::composite("settings/strip", Role::TabList, "Account")
        .orientation(Orientation::Horizontal)
        .active(at, picked.len())
        .wrap()
        .on_move(cx.processor(move |v: &mut Settings, index: usize, _, cx| {
            v.select_section(picked[index], cx)
        }))
        .build()
        .flex_none()
        .flex()
        .h(ladder.strip)
        .px(design::space::SM)
        .border_b_1()
        .border_color(theme.border)
        .children(tabs)
}

/// The Account pane: who the seated key is and its keys, or what stands in
/// for them while they are read, refused or missing.
fn account_pane(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> AnyElement {
    match &view.account {
        Loadable::Ready(Some(seat)) | Loadable::Reloading(Some(seat), _) => {
            seat_pane(view, seat, cx, ladder, theme).into_any_element()
        }
        Loadable::Ready(None) | Loadable::Reloading(None, _) => {
            no_account(view, ladder, theme).into_any_element()
        }
        Loadable::Failed(refusal) => {
            let retry = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.read_account(cx));
            design::refused(
                "settings/account",
                design::sentence(refusal),
                ladder,
                theme,
                retry,
            )
            .into_any_element()
        }
        Loadable::Idle | Loadable::Loading(_) => {
            quiet("settings/account/loading", words::READING, ladder, theme).into_any_element()
        }
    }
}

/// A seat: an account and its keys; a bare key and the form that gives it
/// an account; a stopped agent's key and why it does not act.
fn seat_pane(
    view: &Settings,
    seat: &Seat,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    // a signer that is not hex is no key: no row is this device's
    let mine = abi::unhex(&view.session.signer).unwrap_or_default();
    let pane = column("settings/account/data");
    match seat {
        Seat::Account(account) => pane
            .child(who(
                &account.name,
                about(account),
                view.picture(cx),
                crate::ui::photo::presses(view, account.avatar.is_some(), cx, ladder, theme),
                ladder,
                theme,
            ))
            .child(keys_head(account.keys.len(), ladder, theme))
            .children(account.keys.iter().enumerate().map(|(i, key)| {
                let label = words::key_label(key.label.as_deref(), i);
                key_row(i, &label, key, &mine, ladder, theme)
            })),
        Seat::Bare(key) => pane
            .child(who(
                words::UNREGISTERED,
                words::NO_ACCOUNT_YET.into(),
                None,
                None,
                ladder,
                theme,
            ))
            .child(keys_head(1, ladder, theme))
            .child(key_row(0, THIS_DEVICE, key, &mine, ladder, theme))
            .child(create_account(view, cx, ladder, theme)),
        Seat::Stopped { key, name, note } => pane
            .child(who(
                name,
                words::AGENT_KEY.into(),
                None,
                None,
                ladder,
                theme,
            ))
            .child(keys_head(1, ladder, theme))
            .child(key_row(0, THIS_DEVICE, key, &mine, ladder, theme))
            .child(quiet("settings/account/note", note.clone(), ladder, theme)),
    }
}

/// No seat: the session names an account identity does not hold, or no
/// key is seated at all.
fn no_account(view: &Settings, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    match view.session.account {
        Some(number) if !view.session.signer.is_empty() => quiet(
            "settings/account/none",
            format!("Identity holds no account {number}."),
            ladder,
            theme,
        ),
        _ => design::empty_state(
            "settings/account/empty",
            "user-round",
            words::NO_ACCOUNT,
            words::NO_KEY,
            ladder,
            theme,
        ),
    }
}

/// The line under an account's name: `account 7 · Person · 1 key`.
fn about(account: &Account) -> String {
    format!(
        "{} · {} · {}",
        words::account(account.number),
        words::what(&account.kind),
        words::keys_count(account.keys.len())
    )
}

/// Who the seated key is: its face, its name, one mono line under it.
// An account's picture fills the face once read; its photo presses follow.
fn who(
    name: &str,
    about: String,
    picture: Option<std::sync::Arc<view::Image>>,
    presses: Option<Div>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id("settings/who")
        .flex()
        .flex_col()
        .gap(design::space::LG)
        .p(design::space::BLOCK)
        .child(named(
            design::avatar_with(name, picture, ladder.avatar_detail, theme),
            name_line(name, ladder.big_title).id("settings/who/name"),
            about,
            ladder,
            theme,
        ))
        .children(presses)
}

fn keys_head(count: usize, ladder: &Ladder, theme: &Theme) -> Stateful<Div> {
    design::section_head("settings/keys", words::KEYS, ladder, theme).child(count.to_string())
}

/// One key: a phone for this device's, a key for the rest; its name over
/// its short hex (and this device's fingerprint); Validator at the right.
fn key_row(
    i: usize,
    label: &str,
    key: &Key,
    mine: &[u8],
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let this = key.key == mine;
    let mut caption = design::short_hex(&abi::hex(&key.key));
    if this {
        caption.push_str(&format!(" · fingerprint {}", design::fingerprint(&key.key)));
    }
    let glyph = if this { "smartphone" } else { "key-round" };
    design::row(format!("settings/key/{i}"), ladder, theme)
        .min_h(ladder.row_tall)
        .child(design::lead(muted_icon(glyph, ladder, theme)))
        .child(
            design::row_words(design::row_title(label.to_owned(), ladder, theme))
                .child(mono(caption, ladder, theme)),
        )
        .when(key.validator, |row| {
            row.child(design::badge(
                format!("settings/key/{i}/validator"),
                words::VALIDATOR,
                ladder,
                theme.success,
                theme.success_soft,
            ))
        })
}

/// A bare key's account: what it is for, its name, then Create.
fn create_account(
    view: &Settings,
    cx: &mut Context<Settings>,
    ladder: &Ladder,
    theme: &Theme,
) -> Stateful<Div> {
    let form = &view.create_account;
    let problem = problem_of(form, words::CREATE_ACCOUNT.empty);
    let name = name_input(
        "settings/account/create/name",
        form,
        words::ACCOUNT_NAME,
        words::CREATE_ACCOUNT.prompt,
    )
    // Return creates it (Expo `app/account/index.tsx:189`)
    .on_submit(cx.listener(|v: &mut Settings, _: &(), _, cx| v.submit_create_account(cx)));
    let pressed =
        cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_create_account(cx));
    let create = design::button(
        "settings/account/create/submit",
        if form.busy {
            words::CREATE_ACCOUNT.busy
        } else {
            words::CREATE_ACCOUNT.verb
        },
        ladder,
        theme,
        pressed,
    )
    .kind(ButtonKind::Primary)
    .busy(form.busy);
    column("settings/account/create")
        .gap(design::space::SM)
        .p(design::space::BLOCK)
        .child(
            div()
                .text_type(ladder.body)
                .font_weight(FontWeight::MEDIUM)
                .child(words::CREATE_ACCOUNT_TITLE),
        )
        .child(note(words::CREATE_ACCOUNT_WHY, theme.muted, ladder))
        .child(
            design::field(name, ladder, theme)
                .label(words::ACCOUNT_NAME)
                .when_some(problem, design::Field::problem),
        )
        .child(div().flex().child(create))
}
