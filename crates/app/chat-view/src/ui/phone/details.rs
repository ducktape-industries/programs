//! Channel details (Expo `app/(tabs)/chat/[channel]/details.tsx`, board 53): its name and facts,
//! who can edit it, its members, and for whoever may change it adding and removing a member,
//! rename and archive, each a sheet over the page; sends membership, rename and archive ops.
use view::prelude::*;
use view::{Div, Stateful};

use super::parts::{self, face, quiet};
use crate::words;
use crate::{Chat, Details, DetailsSheet, Room};
use chat::view::Names;
use chat::{ChannelInfo, Principal};
use view::FontWeight;
use view::design::Ladder;

pub(super) fn screen(
    chat: &Chat,
    room: &Room,
    back: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let page = design::page(
        "chat-details-page",
        words::CHANNEL_DETAILS,
        design::Nav::back(chat.pages().depth(), back),
        ladder,
        theme,
    );
    let (Some(info), true) = (chat.info(&room.id), chat.details.is_some()) else {
        return page
            .child(quiet(
                "chat-details-loading",
                "Loading the channel…",
                theme,
                ladder,
            ))
            .into_any_element();
    };
    let dm = chat::dm_peers(&info.channel.id).is_some();
    let agents = crate::ui::notices::agents(chat, dm, true, cx, theme, ladder);
    let agents_head = (!agents.is_empty()).then(|| {
        design::section_head(
            "chat-details-agents-head",
            words::AGENTS_NOTICES,
            ladder,
            theme,
        )
    });
    let body = div()
        .id("chat-details-scroll")
        .flex_1()
        .min_h(px(0.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .child(head(chat, info, theme, ladder))
        .children(crate::policies::room_row(chat, cx, ladder, theme))
        .child(design::section_head(
            "chat-details-notices-head",
            words::NOTIFICATIONS,
            ladder,
            theme,
        ))
        .child(crate::ui::notices::choices(
            chat,
            &info.channel.id,
            chat::dm_peers(&info.channel.id).is_some(),
            cx,
            theme,
            ladder,
        ))
        .children(agents_head)
        .children(agents)
        .children(rules(chat, theme, ladder))
        .children(crate::ui::widget::section(chat, cx, theme, ladder))
        .children(members(chat, room, info, cx, theme, ladder))
        .children(owner_actions(chat, info, cx, theme, ladder));
    page.caption(format!("#{}", info.channel.name))
        .children(parts::room_notice(chat, cx, theme, ladder))
        .child(body)
        .into_any_element()
}

fn rules(chat: &Chat, theme: &Theme, ladder: &Ladder) -> Vec<AnyElement> {
    let Some(said) = crate::ui::rules::said(chat, theme, ladder) else {
        return Vec::new();
    };
    let head = design::section_head("chat-details-rules-head", words::RULES, ladder, theme);
    vec![
        head.into_any_element(),
        div().p(design::space::BLOCK).child(said).into_any_element(),
    ]
}

fn head(chat: &Chat, info: &ChannelInfo, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let channel = &info.channel;
    let policy = words::policy(channel);
    let owner = chat.names.ready().map_or_else(
        || Names::empty().member(&channel.owner),
        |names| names.member(&channel.owner),
    );
    div()
        .id("chat-details-head")
        .flex()
        .flex_col()
        .gap(design::space::XXS)
        .p(design::space::BLOCK)
        .child(
            div()
                .text_type(ladder.heading)
                .font_weight(FontWeight::MEDIUM)
                .child(format!("#{}", channel.name)),
        )
        .child(
            div()
                .text_type(ladder.secondary)
                .text_color(theme.muted)
                .child(format!(
                    "{policy} · created {} · owner {owner}",
                    design::day(channel.created_at)
                )),
        )
}

fn members(
    chat: &Chat,
    room: &Room,
    info: &ChannelInfo,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let roster = chat.roster();
    let count = (!roster.is_empty()).then_some(roster.len());
    let mut rows = vec![
        design::section_head("chat-details-members-head", words::MEMBERS, ladder, theme)
            .children(count.map(|count| count.to_string()))
            .into_any_element(),
    ];
    if room.members.ready().is_none() {
        rows.push(
            quiet(
                "chat-details-members-loading",
                "Loading members…",
                theme,
                ladder,
            )
            .into_any_element(),
        );
    } else if roster.is_empty() {
        rows.push(
            quiet("chat-details-no-members", words::NO_MEMBERS, theme, ladder).into_any_element(),
        );
    }
    let changes = chat.may_change_room();
    rows.extend(
        roster
            .into_iter()
            .map(|member| member_row(chat, info, member, changes, cx, theme, ladder)),
    );
    if changes {
        let add = cx.listener(|chat, _: &ClickEvent, _window, cx| {
            chat.open_details_sheet(DetailsSheet::Add);
            cx.notify();
        });
        rows.push(
            action_row("chat-details-add", "plus", theme, ladder)
                .on_click(add)
                .child(
                    div()
                        .flex_1()
                        .text_type(ladder.body)
                        .text_color(theme.muted)
                        .child("Add a member…"),
                )
                .into_any_element(),
        );
    }
    rows
}

fn member_row(
    chat: &Chat,
    info: &ChannelInfo,
    (principal, name): (Principal, String),
    changes: bool,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let me = chat.me();
    let notes = [
        (principal == info.channel.owner).then_some("owner"),
        (Some(&principal) == me.as_ref()).then_some("you"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let agent = chat
        .names
        .ready()
        .is_some_and(|names| crate::message::agent(names, &principal));
    let removable = match &principal {
        Principal::Account(account) if changes && Some(&principal) != me.as_ref() => Some(*account),
        _ => None,
    };
    let id = format!(
        "chat-details-member-{}",
        crate::names::mention_token(&principal)
    );
    let picture = principal
        .account()
        .and_then(|account| chat.faces.of(account, cx));
    design::row(id.clone(), ladder, theme)
        .when(!notes.is_empty(), |row| row.min_h(ladder.row_tall))
        .child(design::lead(face(
            &name,
            agent,
            picture,
            ladder.avatar,
            theme,
        )))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(div().truncate().text_type(ladder.body).child(name.clone()))
                .when(!notes.is_empty(), |words| {
                    words.child(
                        div()
                            .truncate()
                            .text_type(ladder.caption)
                            .text_color(theme.muted)
                            .child(notes),
                    )
                }),
        )
        .when(agent, |row| {
            row.child(design::badge(
                format!("{id}-agent"),
                words::AGENT,
                ladder,
                theme.agent,
                theme.agent_soft,
            ))
        })
        .when_some(removable, |row, account| {
            row.role(Role::Button)
                .aria_label(name)
                .focusable()
                .on_click(cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                    chat.open_details_sheet(DetailsSheet::Remove(account));
                    cx.notify();
                }))
        })
        .into_any_element()
}

fn owner_actions(
    chat: &Chat,
    info: &ChannelInfo,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    if !chat.may_change_room() {
        return Vec::new();
    }
    let rename = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        chat.open_details_sheet(DetailsSheet::Rename);
        cx.notify();
    });
    let archived = info.channel.archived;
    let archive = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.set_archived(!archived, cx);
    });
    let (icon, title, caption) = match archived {
        true => (
            "archive-restore",
            words::UNARCHIVE_CHANNEL,
            "It takes messages again",
        ),
        false => (
            "archive",
            words::ARCHIVE_CHANNEL,
            "Keeps history, takes no new messages",
        ),
    };
    vec![
        design::section_head("chat-details-owner-head", "Channel actions", ladder, theme)
            .into_any_element(),
        action_row("chat-details-rename", "pencil", theme, ladder)
            .on_click(rename)
            .child(design::row_words(design::row_title(
                words::RENAME_CHANNEL,
                ladder,
                theme,
            )))
            .child(design::chevron(ladder, theme))
            .into_any_element(),
        action_row("chat-details-archive", icon, theme, ladder)
            .min_h(ladder.row_tall)
            .on_click(archive)
            .child(
                design::row_words(design::row_title(title, ladder, theme))
                    .child(design::row_caption(caption, ladder, theme)),
            )
            .into_any_element(),
    ]
}

fn action_row(
    id: &'static str,
    icon: &'static str,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    design::row(id, ladder, theme)
        .role(Role::Button)
        .focusable()
        .child(design::lead(
            design::icon(icon, ladder.icon_row).text_color(theme.muted),
        ))
}

pub(super) fn sheet(
    chat: &Chat,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Option<AnyElement> {
    let details = chat.details.as_ref()?;
    let which = details.sheet?;
    let empty = Names::empty();
    let names = chat.names.ready().unwrap_or(&empty);
    let (title, body) = match which {
        DetailsSheet::Add => (
            "Add a member".to_owned(),
            add_body(chat, details, names, cx, theme, ladder),
        ),
        DetailsSheet::Rename => (
            words::RENAME_CHANNEL.to_owned(),
            rename_body(details, cx, theme, ladder),
        ),
        DetailsSheet::Notices(whose) => (
            crate::ui::notices::heard(chat, whose),
            crate::ui::notices::sheet_rows(chat, whose, cx, theme, ladder),
        ),
        DetailsSheet::Remove(account) => {
            let remove = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.close_details_sheet();
                cx.notify();
                chat.set_member(Principal::Account(account), false, cx);
            });
            let row = design::sheet_row(
                "chat-details-remove",
                "Remove from this channel",
                ladder,
                theme,
                remove,
            )
            .icon("user-minus")
            .danger();
            (
                names.member(&Principal::Account(account)),
                vec![row.into_any_element()],
            )
        }
    };
    let close = cx.listener(|chat, _: &(), _window, cx| {
        chat.close_details_sheet();
        cx.notify();
    });
    let head = design::sheet_head("chat-details-sheet-head", theme).child(
        div()
            .text_type(ladder.body)
            .font_weight(FontWeight::MEDIUM)
            .child(title.clone()),
    );
    Some(
        design::sheet("chat-details-sheet", title, ladder, theme)
            .on_dismiss(close)
            .child(div().flex().flex_col().child(head).children(body))
            .into_any_element(),
    )
}

fn add_body(
    chat: &Chat,
    details: &Details,
    names: &Names,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let add = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.add_typed(cx);
    });
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.add_typed(cx);
    });
    let input = Input::new(
        "chat-details-add/account",
        &details.member_draft,
        words::ACCOUNT_NUMBER,
    )
    .virtual_keyboard(VirtualKeyboard::Numeric)
    .autocorrect(false)
    .autocapitalize(Autocapitalize::None)
    .on_submit(submit);
    let field = design::field(input, ladder, theme)
        .label(words::ACCOUNT_NUMBER)
        .mono();
    let form = div()
        .flex()
        .flex_col()
        .gap(design::space::LG)
        .p(design::space::BLOCK)
        .child(field)
        .child(
            design::button("chat-details-add/submit", "Add", ladder, theme, add)
                .kind(design::Kind::Primary),
        );
    let seated: Vec<Principal> = chat
        .roster()
        .into_iter()
        .map(|(principal, _)| principal)
        .collect();
    let people = names
        .people()
        .filter(|account| !seated.contains(&Principal::Account(*account)))
        .map(|account| {
            let pick = cx.listener(move |chat, _: &ClickEvent, _window, cx| {
                chat.close_details_sheet();
                cx.notify();
                chat.set_member(Principal::Account(account), true, cx);
            });
            let name = names.member(&Principal::Account(account));
            design::sheet_row(
                format!("chat-details-person-{account}"),
                name,
                ladder,
                theme,
                pick,
            )
            .hint(words::account(account))
            .into_any_element()
        });
    std::iter::once(form.into_any_element())
        .chain(people)
        .collect()
}

fn rename_body(
    details: &Details,
    cx: &mut Context<Chat>,
    theme: &Theme,
    ladder: &Ladder,
) -> Vec<AnyElement> {
    let rename = cx.listener(|chat, _: &ClickEvent, _window, cx| {
        cx.notify();
        chat.rename_from_sheet(cx)
    });
    let submit = cx.listener(|chat, _: &(), _window, cx| {
        cx.notify();
        chat.rename_from_sheet(cx)
    });
    let input = Input::new("chat-details-rename/name", &details.name_draft, words::NAME)
        .autocorrect(false)
        .autocapitalize(Autocapitalize::None)
        .on_submit(submit);
    vec![
        div()
            .flex()
            .flex_col()
            .gap(design::space::LG)
            .p(design::space::BLOCK)
            .child(design::field(input, ladder, theme).label(words::NAME))
            .child(
                design::button(
                    "chat-details-rename/submit",
                    words::RENAME,
                    ladder,
                    theme,
                    rename,
                )
                .kind(design::Kind::Primary),
            )
            .into_any_element(),
    ]
}
