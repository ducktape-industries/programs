//! Each account's own order of its rooms and folders (`SetRoomOrder`), and the rooms it wrote in
//! (`PostedIn`).
use super::*;
use crate::Entry;

fn order_of(chat: &Chat, account: u64) -> Vec<Entry> {
    match chat.ask(Query::RoomOrder { account }) {
        Reply::RoomOrder(order) => order,
        other => panic!("{other:?}"),
    }
}

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

fn set(order: &[Entry]) -> Op {
    Op::SetRoomOrder {
        order: order.to_vec(),
    }
}

fn room(id: &str) -> Entry {
    Entry::Room(id.into())
}

fn folder(name: &str) -> Entry {
    Entry::Folder(name.into())
}

#[test]
fn an_account_sets_reads_replaces_and_clears_its_order() {
    let mut chat = Chat::default();
    assert!(order_of(&chat, 1).is_empty());
    let first = [folder("jobs"), room("general")];
    chat.ok(&ADA, set(&first));
    assert_eq!(order_of(&chat, 1), first);
    let second = [room("general"), folder("jobs"), room("random")];
    chat.ok(&ADA, set(&second));
    assert_eq!(order_of(&chat, 1), second);
    chat.ok(&ADA, set(&[]));
    assert!(order_of(&chat, 1).is_empty());
}

#[test]
fn each_account_writes_only_its_own_order() {
    let mut chat = Chat::default();
    chat.ok(&ADA, set(&[room("general")]));
    chat.ok(&BO, set(&[folder("jobs")]));
    assert_eq!(order_of(&chat, 1), [room("general")]);
    assert_eq!(order_of(&chat, 2), [folder("jobs")]);
    assert!(order_of(&chat, 3).is_empty());
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

#[test]
fn an_order_reads_in_the_explorer_as_its_rooms_and_folders() {
    use describe::Value;
    let described = crate::describe(&set(&[folder("jobs"), room("general")]));
    assert_eq!(described.title, "Order your rooms");
    let order = Value::List(vec![Value::text("jobs/"), Value::text("#general")]);
    assert_eq!(described.fields, [describe::field("order", order)]);
}
