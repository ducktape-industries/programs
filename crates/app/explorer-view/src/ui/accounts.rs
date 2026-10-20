//! The Accounts tab and one account.
use super::*;
use crate::decode::scheme;
use identity::Account;
use view::design::Ladder;

pub(super) fn accounts(view: &Explorer, cx: Cx, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let listed = match &*view.accounts {
        Loadable::Ready(listed) | Loadable::Reloading(listed, _) => listed,
        Loadable::Failed(refusal) => return failed(&design::sentence(refusal), cx, theme, ladder),
        Loadable::Idle | Loadable::Loading(_) => {
            return quiet("explorer-accounts-loading", words::READING_ACCOUNTS, theme);
        }
    };
    let list = rows("explorer-accounts-list", "Accounts", view);
    let routes = listed
        .list
        .iter()
        .map(|account| Route::Account(account.number))
        .collect();
    let list = list.shown(routes, |view, range, cx, theme| {
        let Some(listed) = view.accounts.ready() else {
            return Vec::new();
        };
        range
            .filter_map(|index| listed.list.get(index))
            .map(|account| account_row(view, account, cx, theme))
            .collect()
    });
    let caption_text = words::tx_recorded(listed.list.len() as u64);
    div()
        .id("explorer-accounts")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(
            heading(
                "explorer-accounts-heading",
                "Accounts",
                Some(caption(caption_text, theme)),
                theme,
            )
            .mr(design::size::SCROLLBAR),
        )
        .child(list.build(cx))
        .into_any_element()
}

fn account_row(view: &Explorer, account: &Account, cx: Cx, theme: &Theme) -> Stateful<Div> {
    let name = &account.card.name;
    let sent = view.sent_by(account.number);
    let picture = view.picture(account, cx);
    row(
        SharedString::from(format!("explorer-account-{}", account.number)).into(),
        name.clone(),
        Route::Account(account.number),
        cx,
        theme,
    )
    .child(design::avatar_with(
        name,
        picture,
        design::size::AVATAR,
        theme,
    ))
    .child(div().flex_1().truncate().child(name.clone()))
    .child(div().text_color(theme.muted).child(view.kind_of(account)))
    .child(mono(format!("#{}", account.number)).text_color(theme.faint))
    .child(div().w(DEVICES_W).text_color(theme.muted).child(plural(
        account.keys().len() as u64,
        "device",
        "devices",
    )))
    .child(
        mono(sent.map_or_else(|| words::NO_COUNT.to_owned(), words::tx_count))
            .w(COUNT_W)
            .flex()
            .justify_end()
            .text_color(theme.muted),
    )
}

fn acted(view: &Explorer, number: u64, cx: Cx, theme: &Theme, ladder: &Ladder) -> AnyElement {
    let acted = match landed(view.acted(number)) {
        Ok(acted) => acted,
        Err(Some(refusal)) => return failed(&design::sentence(&refusal), cx, theme, ladder),
        Err(None) => return quiet("explorer-activity-loading", words::READING_ACTIVITY, theme),
    };
    let (none, more) = acted
        .rows
        .read(|acted| (acted.rows().is_empty(), acted.count() > acted.rows().len()));
    if none {
        return quiet("explorer-no-activity", words::NO_ACTIVITY, theme);
    }
    let list = rows("explorer-activity-list", "Activity", view);
    let older = more.then(|| {
        let acted = acted.rows.clone();
        let read = move |_: &mut Explorer, cx: &mut Context<Explorer>| acted.update(cx, read_older);
        div().px_5().py_2().child(press(
            "explorer-activity-older",
            words::SHOW_OLDER,
            cx,
            theme,
            read,
        ))
    });
    div()
        .children(acted.behind.then(|| newer(cx, theme)))
        .child(
            frame_rows(list, &acted.rows, |frames| frames.iter().collect())
                .whole(view, cx, theme)
                .build(cx),
        )
        .children(older)
        .into_any_element()
}

pub(super) fn account(
    view: &Explorer,
    number: u64,
    cx: Cx,
    theme: &Theme,
    ladder: &Ladder,
) -> AnyElement {
    let Some(account) = view.account(number) else {
        return match &*view.accounts {
            Loadable::Ready(_) | Loadable::Reloading(_, _) => empty_state(
                "explorer-no-account",
                "user-x",
                words::no_account(number),
                words::NO_ACCOUNT_HERE,
                ladder,
                theme,
            )
            .into_any_element(),
            Loadable::Failed(refusal) => failed(&design::sentence(refusal), cx, theme, ladder),
            Loadable::Idle | Loadable::Loading(_) => {
                quiet("explorer-account-loading", words::READING_ACCOUNT, theme)
            }
        };
    };
    let recorded = view
        .sent_by(number)
        .map(|total| caption(words::recorded(total), theme));
    let activity = div()
        .id("explorer-activity")
        .flex_1()
        .border_r_1()
        .border_color(theme.border)
        .child(heading(
            "explorer-activity-heading",
            "Activity",
            recorded,
            theme,
        ))
        .child(acted(view, number, cx, theme, ladder));
    div()
        .id("explorer-account")
        .flex()
        .flex_col()
        .flex_1()
        .child(title(view, account, cx, theme))
        .child(
            div()
                .flex()
                .flex_1()
                .border_t_1()
                .border_color(theme.border)
                .child(activity)
                .child(side(view, account, theme)),
        )
        .into_any_element()
}

fn title(view: &Explorer, account: &Account, cx: Cx, theme: &Theme) -> impl IntoElement {
    let name = &account.card.name;
    let about = format!(
        "account {}   {}   {}",
        account.number,
        view.kind_of(account),
        plural(account.keys().len() as u64, "device", "devices")
    );
    let picture = view.picture(account, cx);
    div()
        .flex()
        .items_center()
        .gap_4()
        .px_5()
        .py_4()
        .child(design::avatar_with(name, picture, PAGE_AVATAR, theme))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("explorer-title")
                        .text_size(design::text::TITLE)
                        .role(Role::Heading)
                        .aria_level(1)
                        .child(name.clone()),
                )
                .child(
                    mono(about)
                        .text_size(design::text::CAPTION)
                        .text_color(theme.muted),
                ),
        )
        .child(div().flex_1())
        .children(copy_button(
            view,
            &Route::Account(account.number),
            cx,
            theme,
        ))
}

fn side(view: &Explorer, account: &Account, theme: &Theme) -> impl IntoElement {
    let now = view.chain.now();
    let devices = account.keys().iter().enumerate().map(|(index, key)| {
        let last = view.last_signed(account.number, &key.key);
        let label = key.label.clone().unwrap_or_else(|| words::device(index));
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(ROW_H)
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(div().child(label))
            .child(
                mono(format!("{} {}", scheme(key.scheme), short(&key.key)))
                    .flex_1()
                    .text_size(design::text::CAPTION)
                    .text_color(theme.faint),
            )
            .child(
                mono(words::last_used(now, last))
                    .text_size(design::text::CAPTION)
                    .text_color(theme.muted),
            )
    });
    let used = view.programs_used(account.number).unwrap_or_default();
    let used_heading =
        (!used.is_empty()).then(|| heading("explorer-used-heading", "Programs used", None, theme));
    let programs = used.into_iter().map(|(program, count)| {
        div()
            .flex()
            .items_center()
            .h(ROW_H)
            .px_5()
            .border_b_1()
            .border_color(theme.border)
            .child(mono(program.to_owned()).flex_1())
            .child(mono(words::tx_count(count)).text_color(theme.muted))
    });
    div()
        .id("explorer-account-side")
        .w(SIDE_W)
        .flex_shrink_0()
        .child(heading(
            "explorer-devices-heading",
            "Devices",
            Some(caption(grouped(account.keys().len() as u64), theme)),
            theme,
        ))
        .children(devices)
        .children(used_heading)
        .children(programs)
}
