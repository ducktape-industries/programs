//! The rooms each account wrote in (`PostedIn`).
use super::*;

fn posted_in(chat: &Chat, account: u64) -> Vec<String> {
    let asked = Query::PostedIn {
        account,
        page: PageRequest::default(),
    };
    match chat.ask(asked) {
        Reply::PostedIn(page) => page.items,
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_post_in_the_timeline_lists_its_room_for_its_author() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.ok(&ADA, create("random", PostPolicy::Open));
    chat.post(&BO, "m1", "hi", None);
    chat.post(&BO, "m2", "again", None);
    chat.ok(&BO, post("random", "m3", "there", None));
    assert_eq!(posted_in(&chat, 2), ["general", "random"]);
    assert!(posted_in(&chat, 1).is_empty());
    assert!(posted_in(&chat, 3).is_empty());
}
