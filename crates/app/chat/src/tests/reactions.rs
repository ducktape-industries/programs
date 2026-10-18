//! Reactions: one per principal and emoji, counted on the message.
use super::channels::archive;
use super::*;

#[test]
fn a_reaction_counts_once_per_principal_and_knows_its_reader() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "ship it", None);
    chat.ok(&ADA, react(1, "👍", true));
    chat.ok(&BO, react(1, "👍", true));
    let twice = chat.store.borrow().state.clone();
    chat.ok(&BO, react(1, "👍", true));
    assert_eq!(
        chat.store.borrow().state,
        twice,
        "choosing it again changes nothing"
    );
    let seen_by = |viewer: Vec<Principal>| {
        let Reply::Roots(page) = chat.ask(Query::Roots {
            channel_id: "general".into(),
            viewer,
            page: PageRequest::default(),
        }) else {
            panic!("roots answer roots");
        };
        let reaction = page.items[0].reactions[0].clone();
        (reaction.count, reaction.reacted_by_me)
    };
    assert_eq!(seen_by(vec![ADA]), (2, true));
    assert_eq!(seen_by(vec![CY]), (2, false));
}

#[test]
fn removing_a_reaction_uncounts_it_and_the_last_one_leaves() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "ship it", None);
    chat.ok(&ADA, react(1, "👍", true));
    chat.ok(&BO, react(1, "👍", true));
    chat.ok(&ADA, react(1, "👍", false));
    assert_eq!(chat.message(1).reactions[0].count, 1);
    let unchosen = chat.store.borrow().state.clone();
    chat.ok(&CY, react(1, "👍", false));
    assert_eq!(
        chat.store.borrow().state,
        unchosen,
        "dropping what was never chosen"
    );
    chat.ok(&BO, react(1, "👍", false));
    assert!(chat.message(1).reactions.is_empty());
}

/// Adding and removing are one op with one set of guards.
#[test]
fn a_reaction_needs_an_emoji_a_standing_message_and_a_seat() {
    for on in [true, false] {
        let mut chat = Chat::with_channel(PostPolicy::MembersOnly);
        chat.post(&ADA, "m1", "members only", None);
        chat.ok(&ADA, react(1, "👍", true));
        for bad in ["", "a/b"] {
            assert_eq!(chat.refused(&ADA, react(1, bad, on)), code::INVALID_INPUT);
        }
        assert_eq!(chat.refused(&BO, react(1, "👍", on)), code::UNAUTHORIZED);
        assert_eq!(chat.refused(&ADA, react(9, "👍", on)), code::NOT_FOUND);
        chat.ok(&ADA, archive(true));
        assert_eq!(chat.refused(&ADA, react(1, "👍", on)), code::WRONG_STATE);
        chat.ok(&ADA, archive(false));
        chat.ok(&ADA, delete(1));
        assert_eq!(chat.refused(&ADA, react(1, "👍", on)), code::WRONG_STATE);
    }
}

// A chip's tooltip names who chose it: each message's emoji in order,
// each with its principals, and one who drops it leaves the list.
#[test]
fn reactors_name_who_chose_each_emoji() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&BO, "m1", "ship it", None);
    chat.post(&BO, "m2", "and this", None);
    chat.post(&BO, "m3", "nobody", None);
    chat.ok(&BO, react(1, "👍", true));
    chat.ok(&ADA, react(1, "👍", true));
    chat.ok(&CY, react(1, "🎉", true));
    chat.ok(&CY, react(2, "👀", true));
    chat.ok(&CY, react(2, "👀", false));
    chat.ok(&ADA, react(2, "👀", true));
    let Reply::Reactors(reactors) = chat.ask(Query::Reactors {
        channel_id: "general".into(),
        // a seq named twice, out of order, answers once and in order
        seqs: vec![2, 1, 3, 2],
    }) else {
        panic!("reactors answer reactors");
    };
    let by = |seq, emoji: &str, by: Vec<Principal>| ReactedBy {
        seq,
        emoji: emoji.into(),
        by,
    };
    assert_eq!(
        reactors,
        vec![
            by(1, "🎉", vec![CY]),
            by(1, "👍", vec![ADA, BO]),
            by(2, "👀", vec![ADA]),
        ]
    );
}
