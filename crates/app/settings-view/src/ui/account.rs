//! The Account and Agents panes: who the seated key is and its keys, the
//! form that gives a bare key an account, and a person's agents with what
//! their manager does to them.
use super::*;
use crate::queries::{Account, Agent, HARNESS, Key};
use crate::state::{Answering, Choice};
use identity::{Ownership, Standing};
use view::design::Ladder;

const AVATAR_XL: Pixels = px(40.);

pub(super) fn account(
    view: &Settings,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match &*view.account {
        Loadable::Ready(Some(Seat::Bare(key))) | Loadable::Reloading(Some(Seat::Bare(key)), _) => {
            column("settings/account/data")
                .gap_0()
                .child(who(
                    words::UNREGISTERED,
                    words::NO_ACCOUNT_YET,
                    None,
                    None,
                    theme,
                ))
                .child(group("settings/keys", words::KEYS, Some(1), theme))
                .child(key_row(0, "Host key", key, theme, ladder))
                .child(create_account(view, cx, theme, ladder))
                .into_any_element()
        }
        Loadable::Ready(Some(Seat::Stopped { key, name, note }))
        | Loadable::Reloading(Some(Seat::Stopped { key, name, note }), _) => {
            column("settings/account/data")
                .gap_0()
                .child(who(name, words::AGENT_KEY, None, None, theme))
                .child(group("settings/keys", words::KEYS, Some(1), theme))
                .child(key_row(0, "Host key", key, theme, ladder))
                .child(secondary("settings/account/note", note, theme).pt(design::space::MD))
                .into_any_element()
        }
        Loadable::Ready(Some(Seat::Account(account)))
        | Loadable::Reloading(Some(Seat::Account(account)), _) => {
            let kind = identity::view::kind(&account.kind, |_| None);
            let about = format!("{} · {kind}", words::account(account.number));
            let keys = account.keys.iter().enumerate().map(|(i, key)| {
                key_row(i, key.label.as_deref().unwrap_or("Key"), key, theme, ladder)
            });
            let picture = view.picture(account.avatar, cx);
            let has_picture = account.avatar.is_some();
            let presses =
                super::photo::presses(view, account.number, has_picture, cx, ladder, theme);
            column("settings/account/data")
                .gap_0()
                .child(who(&account.name, &about, picture, presses, theme))
                .child(group(
                    "settings/keys",
                    words::KEYS,
                    Some(account.keys.len()),
                    theme,
                ))
                .children(keys)
                .child(
                    secondary(
                        "settings/keys/help",
                        "Add a device from the account menu.",
                        theme,
                    )
                    .pt(design::space::MD),
                )
                .into_any_element()
        }
        Loadable::Ready(None) | Loadable::Reloading(None, _) => design::empty_state(
            "settings/account/empty",
            "user-round",
            words::NO_ACCOUNT,
            words::NO_KEY,
            ladder,
            theme,
        )
        .p_0()
        .into_any_element(),
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
            secondary("settings/account/loading", words::READING, theme).into_any_element()
        }
    }
}

fn who(
    name: &str,
    about: &str,
    picture: Option<std::sync::Arc<Image>>,
    presses: Option<Div>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id("settings/who")
        .flex()
        .items_center()
        .gap(design::space::LG)
        .pb(design::space::LG)
        .border_b_1()
        .border_color(theme.border)
        .child(
            design::avatar_with(name, picture, AVATAR_XL, theme)
                .border_1()
                .border_color(theme.border),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(design::space::HAIR)
                .child(
                    div()
                        .id("settings/who/name")
                        .text_size(design::text::TITLE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name.to_owned()),
                )
                .child(
                    design::mono(about.to_owned())
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted),
                ),
        )
        .children(presses)
}

fn key_row(i: usize, label: &str, key: &Key, theme: &Theme, ladder: &Ladder) -> Stateful<Div> {
    let hex = design::short_hex(&abi::hex(&key.key));
    div()
        .id(format!("settings/key/{i}"))
        .flex()
        .items_center()
        .gap(design::space::LG)
        .py(design::space::MD)
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap(design::space::HAIR)
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(label.to_owned()),
                )
                .child(
                    design::mono(hex)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted),
                ),
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

fn create_account(
    view: &Settings,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let form = &view.create_account;
    let pressed =
        cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_create_account(cx));
    let mut name = field(
        "settings/account/create/name",
        words::CREATE_ACCOUNT.prompt,
        words::ACCOUNT_NAME,
        form,
        theme,
    );
    if !form.busy {
        name = name
            .on_submit(cx.listener(|v: &mut Settings, _: &(), _, cx| v.submit_create_account(cx)));
    }
    let control = div()
        .flex()
        .gap(design::space::SM)
        .child(name)
        .child(submit(
            "settings/account/create/submit",
            words::CREATE_ACCOUNT.verb,
            words::CREATE_ACCOUNT.busy,
            form.busy,
            true,
            theme,
            pressed,
        ));
    column("settings/account/create")
        .gap_0()
        .child(design::setting_row(
            "settings/account/create/row",
            words::CREATE_ACCOUNT_TITLE,
            words::CREATE_ACCOUNT_WHY,
            control,
            ladder,
            theme,
        ))
        .children(problem(
            "account/create",
            form,
            words::CREATE_ACCOUNT.empty,
            theme,
        ))
        .into_any_element()
}

pub(super) fn agents_pane(
    view: &Settings,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    match view.account.ready() {
        Some(Some(Seat::Account(account))) => agents(view, account, cx, theme, ladder),
        _ => div().into_any_element(),
    }
}

fn agents(
    view: &Settings,
    account: &Account,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let create = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_create_agent(cx));
    let add = cx.listener(|v: &mut Settings, _: &ClickEvent, _, cx| v.submit_agent_key(cx));
    let rows: Vec<AnyElement> = account
        .agents
        .iter()
        .map(|agent| self::agent(view, agent, cx, theme, ladder))
        .collect();
    let create_control = div()
        .flex()
        .gap(design::space::SM)
        .child(field(
            "settings/agents/create/name",
            words::CREATE_AGENT.prompt,
            words::AGENT_NAME,
            &view.create_agent,
            theme,
        ))
        .child(submit(
            "settings/agents/create/submit",
            words::CREATE_AGENT.verb,
            words::CREATE_AGENT.busy,
            view.create_agent.busy,
            false,
            theme,
            create,
        ));
    let key_control = div()
        .flex()
        .gap(design::space::SM)
        .child(field(
            "settings/agents/key/request",
            words::ADD_KEY.prompt,
            words::KEY_REQUEST,
            &view.agent_key,
            theme,
        ))
        .child(submit(
            "settings/agents/key/submit",
            words::ADD_KEY.verb,
            words::ADD_KEY.busy,
            view.agent_key.busy,
            false,
            theme,
            add,
        ));
    column("settings/agents")
        .gap_0()
        .child(secondary("settings/agents/help", words::AGENTS_HELP, theme).pb(design::space::XS))
        .children(rows)
        .children(problem("agents/standing", &view.agent_standing, "", theme))
        .child(group("settings/agents/add", words::ADD, None, theme))
        .child(design::setting_row(
            "settings/agents/create",
            words::CREATE_AGENT_TITLE,
            words::CONNECT_WHY,
            create_control,
            ladder,
            theme,
        ))
        .children(problem(
            "agents/create",
            &view.create_agent,
            words::CREATE_AGENT.empty,
            theme,
        ))
        .child(design::setting_row(
            "settings/agents/key",
            words::ADD_KEY_TITLE,
            view.key_for.map_or_else(
                || words::ADD_KEY_WHY.to_owned(),
                |(n, made)| words::key_for(n, made),
            ),
            key_control,
            ladder,
            theme,
        ))
        .children(problem(
            "agents/key",
            &view.agent_key,
            words::ADD_KEY.empty,
            theme,
        ))
        .into_any_element()
}

fn agent(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let number = agent.number;
    let standing = agent.standing();
    let revoked = standing == Standing::Revoked;
    let label = words::standing(standing);
    let (foreground, background) = match standing {
        Standing::Active => (theme.success, theme.success_soft),
        Standing::Suspended => (theme.warning, theme.warning_soft),
        Standing::Revoked => (theme.muted, theme.surface_raised),
    };
    let renaming = view.rename_agent.get(&number);
    let editing = renaming.is_some() && !revoked;
    let name: AnyElement = match renaming.filter(|_| !revoked) {
        Some(form) => {
            let submitted = cx
                .listener(move |v: &mut Settings, _: &(), _, cx| v.submit_rename_agent(number, cx));
            let input = field(
                &format!("settings/agents/{number}/name"),
                &words::rename_field(&agent.name),
                words::RENAME.prompt,
                form,
                theme,
            );
            match form.busy {
                true => input.into_any_element(),
                false => input.on_submit(submitted).into_any_element(),
            }
        }
        None => div()
            .id(format!("settings/agents/{number}/label"))
            .font_weight(FontWeight::MEDIUM)
            .child(agent.name.clone())
            .into_any_element(),
    };
    let body = div()
        .flex_1()
        .min_w(super::FIELD_W)
        .flex()
        .flex_col()
        .gap(design::space::HAIR)
        .child(
            div()
                .flex()
                .items_center()
                .gap(design::space::SM)
                .child(name)
                .when(!editing, |line| {
                    line.child(design::badge(
                        format!("settings/agents/{number}/standing"),
                        label,
                        ladder,
                        foreground,
                        background,
                    ))
                    .children(words::runs_badge(&agent.kind).map(|runs| {
                        design::badge(
                            format!("settings/agents/{number}/runs"),
                            runs,
                            ladder,
                            theme.muted,
                            theme.surface_raised,
                        )
                    }))
                }),
        )
        .child(
            design::mono(words::account_keys(number, agent.keys))
                .id(format!("settings/agents/{number}/about"))
                .text_size(design::text::CAPTION)
                .text_color(theme.muted),
        );
    let face = shared::agent_face(
        agent,
        view.picture(agent.avatar, cx),
        design::size::AVATAR,
        theme,
    );
    let row = div()
        .id(format!("settings/agents/{number}"))
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::LG)
        .py(design::space::MD)
        .child(face.font_weight(FontWeight::SEMIBOLD))
        .child(body)
        .when(!revoked, |row| {
            row.child(actions(view, agent, renaming, cx, theme))
        });
    let renamed = renaming.and_then(|form| {
        problem(
            &format!("agents/{number}/rename"),
            form,
            words::RENAME.empty,
            theme,
        )
    });
    let photo_refused = view
        .photo_stopped(number)
        .and_then(|form| problem(&format!("agents/{number}/photo"), form, "", theme));
    let external = agent.harnessed().is_none();
    let answers = (!revoked && external).then(|| answers(view, agent, cx, theme, ladder));
    div()
        .id(format!("settings/agents/{number}/card"))
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(theme.border)
        .when(revoked, |card| card.opacity(0.6))
        .child(row)
        .children(renamed)
        .children(photo_refused)
        .children(answers)
        .into_any_element()
}

fn answers(
    view: &Settings,
    agent: &Agent,
    cx: &mut Context<Settings>,
    theme: &Theme,
    ladder: &Ladder,
) -> Stateful<Div> {
    let number = agent.number;
    let answering = view.answering.get(&number);
    let listing = answering.is_some_and(|answering| answering.listing);
    let busy = answering.is_some_and(Answering::busy);
    let shown = Choice::shown(&agent.ownership, listing);
    let choices = shared::ownership_choices(agent, shown, cx, ladder, theme);
    let open_to_anyone = agent.ownership == Ownership::Anyone;
    let specific = shown == Choice::Specific;
    let picker = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(design::space::LG)
        .child(secondary(
            format!("settings/agents/{number}/answers/label"),
            words::WHO_IT_ANSWERS,
            theme,
        ))
        .child(choices)
        .when(busy, |line| {
            line.child(secondary(
                format!("settings/agents/{number}/answers/saving"),
                words::SAVING,
                theme,
            ))
        });
    let people = specific.then(|| people(agent, answering, busy, cx, theme));
    let refused = answering.and_then(|answering| {
        problem(
            &format!("agents/{number}/answers"),
            answering.stopped(),
            words::ADD_PERSON.empty,
            theme,
        )
    });
    div()
        .id(format!("settings/agents/{number}/answering"))
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .pl(design::size::AVATAR + design::space::LG)
        .pb(design::space::MD)
        .child(picker)
        .when(open_to_anyone, |block| {
            block.child(
                secondary(
                    format!("settings/agents/{number}/answers/warning"),
                    words::ANYONE_WARNING,
                    theme,
                )
                .text_color(theme.warning),
            )
        })
        .children(people)
        .children(refused)
}

fn people(
    agent: &Agent,
    answering: Option<&Answering>,
    busy: bool,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> Stateful<Div> {
    let number = agent.number;
    let listed = agent.ownership.listed();
    let rows: Vec<Stateful<Div>> = listed
        .iter()
        .map(|party| {
            let name = agent.party_name(party);
            let id = words::party_id(party);
            let removed = party.clone();
            let remove = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
                v.remove_listed_person(number, &removed, cx)
            });
            div()
                .id(format!("settings/agents/{number}/answers/{id}"))
                .flex()
                .items_center()
                .gap(design::space::LG)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_baseline()
                        .gap(design::space::SM)
                        .child(div().child(name))
                        .child(
                            design::mono(words::party(party))
                                .text_size(design::text::CAPTION)
                                .text_color(theme.muted),
                        ),
                )
                .child(submit(
                    &format!("settings/agents/{number}/answers/{id}/remove"),
                    words::REMOVE,
                    words::WORKING,
                    busy,
                    false,
                    theme,
                    remove,
                ))
        })
        .collect();
    let empty = listed.is_empty().then(|| {
        secondary(
            format!("settings/agents/{number}/answers/none"),
            words::NOBODY_LISTED,
            theme,
        )
    });
    let add = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.submit_listed_person(number, cx)
    });
    let add_id = format!("settings/agents/{number}/answers/add");
    let adding = match answering {
        Some(answering) => {
            let submitted = cx.listener(move |v: &mut Settings, _: &(), _, cx| {
                v.submit_listed_person(number, cx)
            });
            let input = field(
                &format!("settings/agents/{number}/answers/name"),
                &words::person_field(&agent.name),
                words::ADD_PERSON.prompt,
                &answering.form,
                theme,
            )
            .disabled(busy);
            let input = match busy {
                true => input.into_any_element(),
                false => input.on_submit(submitted).into_any_element(),
            };
            div()
                .flex()
                .gap(design::space::SM)
                .child(input)
                .child(submit(
                    &add_id,
                    words::ADD_PERSON.verb,
                    words::ADD_PERSON.busy,
                    busy,
                    false,
                    theme,
                    add,
                ))
        }
        None => div().flex().child(submit(
            &add_id,
            "Add someone",
            words::ADD_PERSON.busy,
            busy,
            false,
            theme,
            add,
        )),
    };
    div()
        .id(format!("settings/agents/{number}/answers/people"))
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .children(rows)
        .children(empty)
        .child(adding)
}

fn actions(
    view: &Settings,
    agent: &Agent,
    renaming: Option<&Form>,
    cx: &mut Context<Settings>,
    theme: &Theme,
) -> AnyElement {
    let number = agent.number;
    let (toggle, toggle_label, toggle_op) = agent.standing_toggle();
    let toggled = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.set_standing(toggle_op.clone(), cx)
    });
    let revoke = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| {
        v.set_standing(identity::Op::Revoke { account: number }, cx)
    });
    let open = renaming.is_some();
    let renamed = cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| match open {
        true => v.submit_rename_agent(number, cx),
        false => v.begin_rename(number, cx),
    });
    let cancel = cx
        .listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.cancel_rename_agent(number, cx));
    let change = view.reads_files.then(|| {
        let pressed =
            cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.change_photo(number, cx));
        submit(
            &format!("settings/agents/{number}/photo/change"),
            words::CHANGE_PHOTO,
            words::OPENING,
            view.photo_opening(number),
            false,
            theme,
            pressed,
        )
    });
    let remove = agent.avatar.is_some().then(|| {
        let pressed =
            cx.listener(move |v: &mut Settings, _: &ClickEvent, _, cx| v.remove_photo(number, cx));
        submit(
            &format!("settings/agents/{number}/photo/remove"),
            words::REMOVE_PHOTO,
            words::REMOVING,
            view.photo_removing(number),
            false,
            theme,
            pressed,
        )
    });
    let chain = view.session.chain_id.clone();
    let opens = move |id: String, label: String, link: Option<String>| {
        link.map(|link| {
            button(id, label, theme).on_click(
                move |_: &ClickEvent, _: &mut Window, cx: &mut App| cx.host().open_link(&link),
            )
        })
    };
    if let Some(script) = agent.harnessed() {
        let agent_id = number.to_string();
        let link = ducklink::mint(&chain, &script.calls, &[agent_id.as_str()]);
        return div()
            .id(format!("settings/agents/{number}/actions"))
            .flex()
            .flex_none()
            .items_center()
            .gap(design::space::SM)
            .children(change)
            .children(remove)
            .children(opens(
                format!("settings/agents/{number}/open"),
                words::open_in(&script.calls),
                link,
            ))
            .into_any_element();
    }
    let agent_id = number.to_string();
    let harness = opens(
        format!("settings/agents/{number}/harness"),
        words::RUN_ON_HARNESS.to_owned(),
        ducklink::mint(&chain, HARNESS, &["create", agent_id.as_str()])
            .filter(|_| agent.script().is_none()),
    );
    let keyed = cx.listener(move |v: &mut Settings, _: &ClickEvent, window, cx| {
        v.open_key_for(number, false, cx);
        window.focus("settings/agents/key/request");
    });
    let key = button(
        format!("settings/agents/{number}/key/open"),
        words::ADD_A_KEY,
        theme,
    )
    .on_click(keyed);
    let busy = view.agent_standing.busy;
    div()
        .id(format!("settings/agents/{number}/actions"))
        .flex()
        .flex_none()
        .items_center()
        .gap(design::space::SM)
        .child(submit(
            &format!("settings/agents/{number}/rename"),
            if renaming.is_some() {
                words::RENAME.verb
            } else {
                words::RENAME_AGENT
            },
            words::RENAME.busy,
            renaming.is_some_and(|form| form.busy),
            false,
            theme,
            renamed,
        ))
        .when(renaming.is_some_and(|form| !form.busy), |row| {
            row.child(
                button(
                    format!("settings/agents/{number}/rename-cancel"),
                    words::CANCEL,
                    theme,
                )
                .border_color(theme.background)
                .text_color(theme.muted)
                .on_click(cancel),
            )
        })
        .children(change)
        .children(remove)
        .child(key)
        .children(harness)
        .child(submit(
            &format!("settings/agents/{number}/{toggle}"),
            toggle_label,
            words::WORKING,
            busy,
            false,
            theme,
            toggled,
        ))
        .child(
            submit(
                &format!("settings/agents/{number}/revoke"),
                words::REVOKE,
                words::WORKING,
                busy,
                false,
                theme,
                revoke,
            )
            .text_color(theme.danger)
            .border_color(theme.background),
        )
        .into_any_element()
}
