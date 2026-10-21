use crate::{Telegram, login::Screen, words};
use view::prelude::*;

pub(crate) fn render(view: &Telegram, window: &Window, cx: &mut Context<Telegram>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = design::ladder(window.touch());
    let mut page = div()
        .size_full()
        .flex()
        .flex_col()
        .p(design::space::LG)
        .gap(design::space::MD)
        .bg(theme.background)
        .text_color(theme.foreground)
        .text_size(design::text::BODY)
        .child(words::TITLE)
        .child(design::quiet(words::DEVICE, &theme))
        .child(view.status.clone());
    if view
        .live
        .scope
        .as_ref()
        .is_none_or(|(_, signer)| signer.is_empty())
    {
        return page.into_any_element();
    }
    page = page
        .children(
            view.notice
                .clone()
                .map(|notice| design::quiet(notice, &theme)),
        )
        .children(
            view.failed
                .clone()
                .map(|why| div().text_color(theme.danger).child(why)),
        );
    if matches!(view.screen, Screen::Chats) {
        if view.history.is_some() {
            page = page.child(conversation(view, window, cx));
        } else {
            let mut list = div()
                .id("dialogs")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(design::space::SM);
            for dialog in &view.chats.dialogs {
                let peer = dialog.peer;
                list = list.child(
                    div()
                        .id(format!("chat-{}", dialog.peer.key()))
                        .py(design::space::SM)
                        .role(Role::Button)
                        .focusable()
                        .on_click(cx.listener(move |view: &mut Telegram, _, _, cx| {
                            view.open_chat(peer, cx)
                        }))
                        .child(view.peer_name(dialog.peer))
                        .child(design::quiet(dialog.text.clone(), &theme)),
                );
            }
            if !view.chats.loaded {
                list = list.child(design::quiet(words::READING_CHATS, &theme));
            }
            let loaded_chats_are_empty = view.chats.loaded && view.chats.dialogs.is_empty();
            if loaded_chats_are_empty {
                list = list.child(design::empty_state(
                    "empty-chats",
                    "message-circle",
                    words::NO_CHATS,
                    words::NO_CHATS_DETAIL,
                    &ladder,
                    &theme,
                ));
            }
            page = page
                .child(design::button(
                    "refresh",
                    words::REFRESH_CHATS,
                    &ladder,
                    &theme,
                    cx.listener(|view: &mut Telegram, _, _, cx| {
                        view.start_updates(cx);
                        view.read_dialogs(false, cx);
                    }),
                ))
                .child(list);
            if view.chats.more {
                page = page.child(design::button(
                    "more-chats",
                    words::MORE_CHATS,
                    &ladder,
                    &theme,
                    cx.listener(|view: &mut Telegram, _, _, cx| view.read_dialogs(true, cx)),
                ));
            }
        }
        page = page.child(
            design::button(
                "logout",
                words::LOGOUT,
                &ladder,
                &theme,
                cx.listener(|view: &mut Telegram, _, _, cx| view.logout(cx)),
            )
            .enabled(!view.live.ready || !view.login_busy()),
        );
    } else {
        let (id, field, label, secure) = match view.screen {
            Screen::Phone => ("phone", &view.phone, words::PHONE, false),
            Screen::Code => ("code", &view.code, words::CODE, false),
            Screen::Password => ("password", &view.password, words::PASSWORD, true),
            Screen::Chats => unreachable!(),
        };
        let enabled = view.live.ready && !view.login_busy();
        page = page
            .child(
                Input::new(id, field, label)
                    .secure(secure)
                    .disabled(!enabled)
                    .h(design::size::CONTROL)
                    .px(design::space::SM)
                    .border_1()
                    .border_color(theme.border_strong)
                    .on_submit(cx.listener(|view: &mut Telegram, _, _, cx| view.submit_login(cx))),
            )
            .child(
                design::button(
                    "login",
                    words::CONTINUE,
                    &ladder,
                    &theme,
                    cx.listener(|view: &mut Telegram, _, _, cx| view.submit_login(cx)),
                )
                .enabled(enabled),
            )
            .child(design::quiet(words::SIGNUP, &theme));
        let refused_keys_removal_on_phone = matches!(view.screen, Screen::Phone)
            && view
                .live
                .unconfirmed_keys_removal
                .as_ref()
                .is_some_and(|removal| removal.refused);
        if refused_keys_removal_on_phone {
            page = page.child(design::button(
                "remove-keys-again",
                words::REMOVE_KEYS_AGAIN,
                &ladder,
                &theme,
                cx.listener(|view: &mut Telegram, _, _, cx| view.remove_keys_from_key_store(cx)),
            ));
        }
    }
    let connection_can_be_started = !view.live.ready && view.live.bootstrap.is_none();
    if connection_can_be_started {
        page = page.child(design::button(
            "connect",
            words::CONNECT,
            &ladder,
            &theme,
            cx.listener(|view: &mut Telegram, _, _, cx| view.connect(cx)),
        ));
    }
    page.into_any_element()
}

fn conversation(view: &Telegram, window: &Window, cx: &mut Context<Telegram>) -> AnyElement {
    let theme = *cx.global::<Theme>();
    let ladder = design::ladder(window.touch());
    let history = view.history.as_ref().unwrap();
    let mut messages = div()
        .id("history")
        .track_scroll(&history.scroll)
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .gap(design::space::MD)
        .on_scroll_wheel(
            cx.listener(|view: &mut Telegram, event: &ScrollWheelEvent, _, cx| {
                if event.delta.pixel_delta(px(1.)).y > px(0.) {
                    view.read_history(true, cx);
                }
            }),
        );
    if !history.loaded {
        messages = messages.child(design::quiet(words::READING_MESSAGES, &theme));
    }
    let loaded_history_is_empty = history.loaded && history.messages.is_empty();
    if loaded_history_is_empty {
        messages = messages.child(design::empty_state(
            "empty-messages",
            "message-circle",
            words::NO_MESSAGES,
            words::NO_MESSAGES_DETAIL,
            &ladder,
            &theme,
        ));
    }
    for message in history.messages.values() {
        let sender = message
            .author
            .clone()
            .unwrap_or_else(|| view.peer_name(message.sender));
        messages = messages.child(
            div()
                .id(format!("message-{}", message.id))
                .flex()
                .flex_col()
                .gap(design::space::XS)
                .child(design::quiet(
                    words::message_author(
                        &sender,
                        &design::date(message.date.max(0) as u64 * 1000),
                    ),
                    &theme,
                ))
                .child(message.text.clone()),
        );
    }
    let mut page = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap(design::space::SM)
        .child(design::button(
            "back",
            words::CHATS,
            &ladder,
            &theme,
            cx.listener(|view: &mut Telegram, _, _, cx| {
                view.close_chat(cx);
            }),
        ))
        .child(view.peer_name(history.peer));
    if history.more {
        page = page.child(design::button(
            "older",
            words::OLDER_MESSAGES,
            &ladder,
            &theme,
            cx.listener(|view: &mut Telegram, _, _, cx| view.read_history(true, cx)),
        ));
    }
    page.child(messages)
        .child(
            Input::new("compose", &view.compose, words::MESSAGE)
                .placeholder(words::MESSAGE)
                .disabled(!view.live.ready)
                .h(design::size::CONTROL)
                .px(design::space::SM)
                .border_1()
                .border_color(theme.border_strong)
                .autocorrect(true)
                .suggestions(true)
                .on_submit(cx.listener(|view: &mut Telegram, _, _, cx| view.send_text(cx))),
        )
        .child(
            design::button(
                "send",
                words::SEND,
                &ladder,
                &theme,
                cx.listener(|view: &mut Telegram, _, _, cx| view.send_text(cx)),
            )
            .enabled(view.live.ready),
        )
        .into_any_element()
}
