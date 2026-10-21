//! A room open to every poster whose meta says `listed = involved` seats whoever posts there and
//! whoever a message there mentions, posted or edited in; any other room seats no one so.
use std::collections::BTreeSet;

use super::*;
use crate::state::MEMBERS;

fn involved() -> Chat {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let listed = Op::SetChannelMeta {
        channel_id: "general".into(),
        key: crate::LISTED.into(),
        value: crate::INVOLVED.into(),
    };
    chat.ok(&ADA, listed);
    chat
}

fn seated(chat: &Chat) -> BTreeSet<u64> {
    let rows = MEMBERS.all(&chat.reads()).unwrap();
    let seated = rows.into_iter().filter(|((room, _), _)| room == "general");
    seated
        .filter_map(|((_, principal), _)| principal.account())
        .collect()
}

fn with(before: &BTreeSet<u64>, accounts: &[u64]) -> BTreeSet<u64> {
    before.iter().chain(accounts).copied().collect()
}

#[test]
fn a_post_root_or_reply_seats_its_author_and_whoever_it_mentions() {
    let mut chat = involved();
    let before = seated(&chat);
    chat.post(&BO, "m1", "hi <@3>", None);
    assert_eq!(seated(&chat), with(&before, &[2, 3]));
    chat.post(&Principal::Account(4), "m2", "me too", Some(1));
    assert_eq!(seated(&chat), with(&before, &[2, 3, 4]));
}

#[test]
fn an_edit_that_adds_a_mention_seats_the_one_it_mentions() {
    let mut chat = involved();
    let before = seated(&chat);
    chat.post(&BO, "m1", "first", None);
    chat.ok(&BO, super::channels::edit(1, "now <@5> sees it"));
    assert_eq!(seated(&chat), with(&before, &[2, 5]));
}

#[test]
fn a_room_without_the_meta_seats_no_one_who_posts_or_is_mentioned() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let before = seated(&chat);
    chat.post(&BO, "m1", "hi <@3>", None);
    assert_eq!(seated(&chat), before);
}

#[test]
fn a_members_only_room_seats_no_one_by_a_post_or_a_mention() {
    let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
    let listed = Op::SetChannelMeta {
        channel_id: "general".into(),
        key: crate::LISTED.into(),
        value: crate::INVOLVED.into(),
    };
    chat.ok(&ADA, listed);
    let before = seated(&chat);
    chat.post(&ADA, "m1", "hi <@3>", None);
    assert_eq!(
        seated(&chat),
        before,
        "a mention never lets account 3 post there"
    );
    let refused = chat.refused(&CY, post("general", "m2", "let me in", None));
    assert_eq!(refused, code::UNAUTHORIZED);
}

#[test]
fn one_seated_already_is_not_seated_again() {
    let mut chat = involved();
    chat.post(&BO, "m1", "first", None);
    let at = |chat: &Chat| {
        MEMBERS
            .get(&chat.reads(), &("general".to_owned(), BO))
            .unwrap()
    };
    let first = at(&chat).expect("seated by the first post");
    chat.post(&BO, "m2", "again", None);
    assert_eq!(
        at(&chat),
        Some(first),
        "the seat keeps the block of the first post"
    );
}
