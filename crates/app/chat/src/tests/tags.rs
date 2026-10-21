use borsh::BorshSerialize;
use borsh::schema::Definition;

use super::*;

const LIVE_OPS: [(&str, i64); 27] = [
    ("CreateChannel", 0),
    ("CreateDmChannel", 1),
    ("RenameChannel", 2),
    ("SetChannelArchived", 3),
    ("PostMessage", 4),
    ("EditMessage", 5),
    ("DeleteMessage", 6),
    ("AddReaction", 7),
    ("RemoveReaction", 8),
    ("SetMembership", 9),
    ("CreateChannelWithMeta", 10),
    ("SetChannelMeta", 11),
    ("RemoveChannelMeta", 12),
    ("Listen", 14),
    ("SetNotices", 16),
    ("AdoptOwners", 17),
    ("Watch", 20),
    ("Drafting", 21),
    ("Undraft", 22),
    ("SetRoomOrder", 23),
    ("Post", 24),
    ("ChooseForMany", 25),
    ("Notify", 26),
    ("Take", 27),
    ("Release", 28),
    ("WatchPlaces", 29),
    ("Break", 30),
];

const LIVE_QUERIES: [(&str, i64); 25] = [
    ("Channels", 0),
    ("Channel", 1),
    ("MessageById", 2),
    ("ThreadAttention", 3),
    ("Roots", 4),
    ("MessagesAround", 5),
    ("Thread", 6),
    ("Members", 7),
    ("TagSearch", 9),
    ("Accounts", 10),
    ("Reactors", 11),
    ("Listening", 13),
    ("Followed", 14),
    ("Notices", 15),
    ("Watching", 16),
    ("Chosen", 17),
    ("Drafts", 18),
    ("Names", 19),
    ("RoomOrder", 20),
    ("PostedIn", 21),
    ("ChosenBy", 22),
    ("Unread", 23),
    ("TextSearch", 24),
    ("Busy", 25),
    ("Holders", 26),
];

const LIVE_REPLIES: [(&str, i64); 25] = [
    ("Channels", 0),
    ("Channel", 1),
    ("Message", 2),
    ("Attention", 3),
    ("Roots", 4),
    ("Messages", 5),
    ("Thread", 6),
    ("Members", 7),
    ("TagHits", 9),
    ("Accounts", 10),
    ("Reactors", 11),
    ("Listening", 13),
    ("Notices", 14),
    ("Followed", 15),
    ("Watching", 16),
    ("Chosen", 17),
    ("Drafts", 18),
    ("Names", 19),
    ("RoomOrder", 20),
    ("PostedIn", 21),
    ("ChosenBy", 22),
    ("Unread", 23),
    ("TextHits", 24),
    ("Busy", 25),
    ("Holders", 26),
];

const MIGRATE_TAG: u8 = 13;
const LISTEN_TAG: u8 = 14;
const MIGRATE_SOME_TAG: u8 = 15;
const SET_NOTICES_TAG: u8 = 16;
const UNMIGRATED_TAG: u8 = 12;
const LISTEN_FOR_TAG: u8 = 18;
const SET_NOTICES_FOR_TAG: u8 = 19;
const SEARCH_TAG: u8 = 8;

fn encoded<T: BorshSerialize>(value: &T) -> Vec<u8> {
    borsh::to_vec(value).unwrap()
}

fn tagged(tag: u8, fields: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = vec![tag];
    for field in fields {
        bytes.extend_from_slice(field);
    }
    bytes
}

fn tags_of(declaration: &str) -> Vec<(String, i64)> {
    let schema = ::program::schema::interface::<crate::Chat>();
    let Some(Definition::Enum { variants, .. }) = schema.get_definition(declaration) else {
        panic!("{declaration} is an enum in chat's schema");
    };
    variants
        .iter()
        .map(|(tag, name, _)| (name.clone(), *tag))
        .collect()
}

fn holds_each(declaration: &str, live: &[(&str, i64)]) {
    let tags = tags_of(declaration);
    for (name, tag) in live {
        let kept = tags.contains(&((*name).to_owned(), *tag));
        assert!(kept, "{declaration} {name} {tag} in {tags:?}");
    }
}

fn sent(chat: &mut Chat, who: &Principal, bytes: &[u8]) -> Result<(), ::program::Error> {
    let ctx = chat.next(who);
    ::program::execute::<crate::Chat>(&ctx, bytes)
}

fn chosen_by_bo(chat: &Chat) -> Reply {
    chat.ask(Query::Chosen {
        account: 2,
        channel_id: "general".into(),
    })
}

#[test]
fn each_live_op_query_and_reply_keeps_its_tag_in_the_schema() {
    holds_each("Op", &LIVE_OPS);
    holds_each("Query", &LIVE_QUERIES);
    holds_each("Reply", &LIVE_REPLIES);
}

#[test]
fn listen_and_set_notices_written_as_bytes_land_and_read_back() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let listen = tagged(
        LISTEN_TAG,
        &[encoded(&"general".to_owned()), encoded(&true)],
    );
    sent(&mut chat, &BO, &listen).unwrap();
    let listened = Reply::Chosen {
        listening: true,
        notices: None,
    };
    assert_eq!(chosen_by_bo(&chat), listened);
    let no_notices: Option<crate::Notices> = None;
    let cut = tagged(
        SET_NOTICES_TAG,
        &[encoded(&"general".to_owned()), encoded(&no_notices)],
    );
    sent(&mut chat, &BO, &cut).unwrap();
    let cleared = Reply::Chosen {
        listening: false,
        notices: None,
    };
    assert_eq!(chosen_by_bo(&chat), cleared);
}

#[test]
fn a_migrate_or_migrate_some_tag_is_refused_and_writes_nothing_and_an_unmigrated_tag_is_no_query() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "g1", "alpha", None);
    let before = chat.store.borrow().state.clone();
    let migrate = tagged(MIGRATE_TAG, &[encoded(&"general".to_owned())]);
    let migrate_some = tagged(MIGRATE_SOME_TAG, &[encoded(&5u64)]);
    for bytes in [migrate, migrate_some] {
        let ctx = chat.next(&BO);
        let outcome = chat
            .store
            .attempt(|| ::program::execute::<crate::Chat>(&ctx, &bytes));
        assert_eq!(chat.store.borrow().state, before, "{outcome:?}");
        let refused = outcome.unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
        let undecoded = refused.message.starts_with("chat: Op did not decode: ");
        assert!(undecoded, "{refused}");
    }
    let unmigrated = tagged(UNMIGRATED_TAG, &[encoded(&PageRequest::default())]);
    let asked = ::program::query::<crate::Chat>(&chat.reads(), &unmigrated);
    let refused = asked.unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    let undecoded = refused.message.starts_with("chat: Query did not decode: ");
    assert!(undecoded, "{refused}");
}

#[test]
fn a_listen_for_or_set_notices_for_tag_is_refused_and_writes_nothing() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    let before = chat.store.borrow().state.clone();
    let no_notices: Option<crate::Notices> = None;
    let listen_for = tagged(
        LISTEN_FOR_TAG,
        &[
            encoded(&2u64),
            encoded(&"general".to_owned()),
            encoded(&true),
        ],
    );
    let set_notices_for = tagged(
        SET_NOTICES_FOR_TAG,
        &[
            encoded(&2u64),
            encoded(&"general".to_owned()),
            encoded(&no_notices),
        ],
    );
    for bytes in [listen_for, set_notices_for] {
        let ctx = chat.next(&BO);
        let refused = chat
            .store
            .refused(|| ::program::execute::<crate::Chat>(&ctx, &bytes));
        assert_eq!(chat.store.borrow().state, before);
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
        let undecoded = refused.message.starts_with("chat: Op did not decode: ");
        assert!(undecoded, "{refused}");
    }
}

#[test]
fn a_search_tag_is_no_query() {
    let mut chat = Chat::with_channel(PostPolicy::Open);
    chat.post(&ADA, "g1", "alpha", None);
    let no_channel: Option<String> = None;
    let search = tagged(
        SEARCH_TAG,
        &[
            encoded(&"alpha".to_owned()),
            encoded(&Vec::<Principal>::new()),
            encoded(&no_channel),
            encoded(&PageRequest::default()),
        ],
    );
    let asked = ::program::query::<crate::Chat>(&chat.reads(), &search);
    let refused = asked.unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
    let undecoded = refused.message.starts_with("chat: Query did not decode: ");
    assert!(undecoded, "{refused}");
}
