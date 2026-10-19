//! Who can edit a room and a message (`policies.rs`): the row and sheets say what ownership
//! records, the owner relinks and transfers from them, and what the reader may do follows the
//! item's policy once read, the rule from before policies until then and for an item with none.
use super::*;
use ownership::{Op as Owned, Ownership, Party, Writers};

fn details(cx: &mut TestAppContext, view: &Entity<Chat>) {
    cx.update(view, |chat, _, cx| chat.toggle_details(cx));
    cx.run_until_parked();
}

fn more(cx: &mut TestAppContext, view: &Entity<Chat>, seq: u64) {
    cx.update(view, |chat, window, cx| {
        cx.notify();
        chat.open_menu(Pane::Timeline, seq, 0, Mode::More, window, cx);
    });
    cx.run_until_parked();
}

fn general() -> Vec<u8> {
    chat::tables::CHANNELS.key(&"general".to_owned())
}

#[test]
fn the_details_say_who_can_edit_the_room_and_its_owner_relinks_it() {
    let (mut cx, view) = opened();
    let team = a_policy(3, 7, "Team", Writers::Listed(vec![Party::Account(10)]));
    let mine = vec![
        a_policy(1, 7, "Open", Writers::Anyone),
        team.clone(),
        a_policy(4, 7, "Close", Writers::Owner),
    ];
    owned(&cx, general(), 7, team, mine);
    details(&mut cx, &view);
    assert!(cx.has_text("You · Team · You and ada"), "{:?}", cx.texts());
    cx.simulate_click("chat-room-policy");
    cx.run_until_parked();
    assert!(cx.find("chat-room-policy/sheet").is_some());
    cx.simulate_click("chat-room-policy/relink");
    cx.run_until_parked();
    assert!(cx.has_text("In use · You and ada"), "{:?}", cx.texts());
    cx.simulate_click("chat-room-policy/relink/4");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<Ownership>>();
    assert!(
        matches!(sent.as_slice(), [Owned::Relink { program, key, policy: 4 }] if program == "chat" && *key == general()),
        "{sent:?}"
    );
    assert!(
        cx.find("chat-room-policy/sheet").is_none(),
        "the landed relink closes it"
    );
}

#[test]
fn the_owner_transfers_a_room_after_a_confirm_that_names_the_new_owner() {
    let (mut cx, view) = opened();
    let only = a_policy(4, 7, "Close", Writers::Owner);
    owned(&cx, general(), 7, only.clone(), vec![only]);
    details(&mut cx, &view);
    cx.simulate_click("chat-room-policy");
    cx.run_until_parked();
    cx.simulate_click("chat-room-policy/transfer");
    cx.run_until_parked();
    cx.simulate_input("chat-room-policy/transfer/person", "#10");
    cx.simulate_click("chat-room-policy/find/submit");
    cx.run_until_parked();
    let confirm = "ada becomes the owner of #General. You can't take it back: only ada can give it \
                   back to you.";
    assert!(cx.has_text(confirm), "{:?}", cx.texts());
    assert!(
        cx.host().requests::<Submit<Ownership>>().is_empty(),
        "nothing sent before the confirm"
    );
    cx.simulate_click("chat-room-policy/transfer/submit");
    cx.run_until_parked();
    let sent = cx.host().requests::<Submit<Ownership>>();
    assert!(
        matches!(
            sent.as_slice(),
            [Owned::Transfer {
                to: Party::Account(10),
                ..
            }]
        ),
        "{sent:?}"
    );
}

#[test]
fn a_message_its_policy_opens_to_anyone_offers_its_edit_and_says_whose_it_is() {
    let (mut cx, view) = opened();
    let open = a_policy(1, 8, "Open", Writers::Anyone);
    let key = chat::tables::MESSAGE_IDS.key(&"m2".to_owned());
    owned(&cx, key, 8, open.clone(), vec![open]);
    more(&mut cx, &view, 2);
    assert!(
        cx.find("chat-menu-edit").is_some(),
        "the policy admits the reader"
    );
    cx.simulate_click("chat-menu-who-can-edit");
    cx.run_until_parked();
    assert!(cx.find("chat-message-policy/sheet").is_some());
    assert!(cx.has_text("reviewer · Open · Anyone"), "{:?}", cx.texts());
    assert!(
        cx.find("chat-message-policy/relink").is_none(),
        "only its owner relinks it"
    );
}

#[test]
fn an_item_with_no_record_keeps_the_rule_from_before_policies() {
    let (mut cx, view) = opened();
    more(&mut cx, &view, 2);
    assert!(
        cx.find("chat-menu-edit").is_none(),
        "reviewer's message, as before"
    );
    cx.simulate_click("chat-menu-who-can-edit");
    cx.run_until_parked();
    assert!(
        cx.has_text("Open (from before policies)"),
        "{:?}",
        cx.texts()
    );
    // another message's menu: the slot holds that one, and the sheet is gone with the last
    more(&mut cx, &view, 1);
    assert!(cx.find("chat-message-policy/sheet").is_none());
    assert!(
        cx.find("chat-menu-edit").is_some(),
        "the reader's own message, as before"
    );
}
