use borsh::BorshSerialize;
use borsh::schema::Definition;

use super::*;

const LIVE_OPS: [(&str, i64); 22] = [
    ("Close", 0),
    ("Attributed", 1),
    ("Streamed", 4),
    ("NewThread", 5),
    ("ForkThread", 6),
    ("OpenThread", 7),
    ("Book", 8),
    ("Unbook", 9),
    ("RunBooked", 10),
    ("Resume", 11),
    ("Report", 12),
    ("Settle", 13),
    ("Watch", 14),
    ("Heard", 15),
    ("Wait", 18),
    ("Unwait", 19),
    ("ClaimConversations", 24),
    ("AdoptOwners", 25),
    ("SetRoute", 29),
    ("RemoveRoute", 30),
    ("AdoptTallies", 31),
    ("HoldSealed", 33),
];

const LIVE_QUERIES: [(&str, i64); 36] = [
    ("Agent", 0),
    ("Thread", 1),
    ("Threads", 2),
    ("Frames", 3),
    ("Held", 4),
    ("Calls", 5),
    ("Tools", 6),
    ("ThreadAt", 7),
    ("Withheld", 10),
    ("Partial", 11),
    ("Topic", 12),
    ("Bound", 13),
    ("Tally", 14),
    ("FramesFrom", 15),
    ("Call", 16),
    ("Bookings", 17),
    ("Booking", 18),
    ("UsageWindow", 19),
    ("Settling", 20),
    ("Budget", 21),
    ("Waits", 22),
    ("Wait", 23),
    ("Armed", 24),
    ("Card", 25),
    ("Route", 27),
    ("Routes", 28),
    ("Answerers", 29),
    ("Wakes", 30),
    ("AgentTools", 31),
    ("Cards", 32),
    ("ProviderRoute", 33),
    ("ProviderRoutes", 34),
    ("FinalErrors", 35),
    ("Runs", 36),
    ("Conversations", 37),
    ("BudgetFacts", 38),
];

const LIVE_REPLIES: [(&str, i64); 31] = [
    ("Agent", 0),
    ("Thread", 1),
    ("Threads", 2),
    ("Frames", 3),
    ("Held", 4),
    ("Calls", 5),
    ("Tools", 6),
    ("Withheld", 9),
    ("Partial", 10),
    ("Topic", 11),
    ("Tally", 12),
    ("Call", 13),
    ("Bookings", 14),
    ("Booking", 15),
    ("UsageWindow", 16),
    ("Settling", 17),
    ("Budget", 18),
    ("Waits", 19),
    ("Wait", 20),
    ("Route", 22),
    ("Routes", 23),
    ("Answerers", 24),
    ("Wakes", 25),
    ("AgentTools", 26),
    ("Cards", 27),
    ("ProviderRoute", 28),
    ("ProviderRoutes", 29),
    ("FinalErrors", 30),
    ("Runs", 31),
    ("Conversations", 32),
    ("BudgetFacts", 33),
];

const LIVE_CONFIGURED: [(&str, i64); 5] = [
    ("Unreadable", 0),
    ("Malformed", 1),
    ("Routed", 4),
    ("NoRoute", 5),
    ("Unprovided", 6),
];

const SET_MODEL_TAG: u8 = 2;
const REMOVE_MODEL_TAG: u8 = 3;
const SET_COMPACT_AT_TAG: u8 = 16;
const READ_BUDGET_TAG: u8 = 17;
const SET_SERVICES_TAG: u8 = 26;
const READ_BUDGETS_TAG: u8 = 27;
const MODEL_TAG: u8 = 8;
const MODELS_TAG: u8 = 9;
const SERVICES_TAG: u8 = 26;

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
    let schema = ::program::schema::interface::<crate::Harness>();
    let Some(Definition::Enum { variants, .. }) = schema.get_definition(declaration) else {
        panic!("{declaration} is an enum in harness's schema");
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

fn model_id() -> Vec<u8> {
    encoded(&"claude-opus-5-5".to_owned())
}

fn retired_ops() -> [Vec<u8>; 6] {
    let service = || encoded(&"anthropic".to_owned());
    let wire = encoded(&Wire::AnthropicMessages);
    let services = encoded(&vec!["anthropic".to_owned()]);
    [
        tagged(SET_MODEL_TAG, &[model_id(), wire, service(), model_id()]),
        tagged(REMOVE_MODEL_TAG, &[model_id()]),
        tagged(
            SET_COMPACT_AT_TAG,
            &[model_id(), encoded(&Some(650_000u64))],
        ),
        tagged(READ_BUDGET_TAG, &[service()]),
        tagged(SET_SERVICES_TAG, &[model_id(), services]),
        tagged(READ_BUDGETS_TAG, &[]),
    ]
}

#[test]
fn each_live_op_query_reply_and_configured_keeps_its_tag_in_the_schema() {
    holds_each("Op", &LIVE_OPS);
    holds_each("Query", &LIVE_QUERIES);
    holds_each("Reply", &LIVE_REPLIES);
    holds_each("Configured", &LIVE_CONFIGURED);
}

#[test]
fn a_retired_model_or_status_op_tag_is_refused_as_no_op() {
    let world = world();
    for bytes in retired_ops() {
        let refused = world
            .chain
            .submit_raw(signed(ADA), harness(), &bytes)
            .unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
        let undecoded = refused.message.starts_with("harness: Op did not decode: ");
        assert!(undecoded, "{refused}");
    }
}

#[test]
fn a_retired_model_or_services_query_tag_is_no_query() {
    let world = world();
    let queries = [
        tagged(MODEL_TAG, &[model_id()]),
        tagged(MODELS_TAG, &[encoded(&PageRequest::default())]),
        tagged(SERVICES_TAG, &[model_id()]),
    ];
    for bytes in queries {
        let refused = world.chain.query_bytes(PROGRAM, &bytes).unwrap_err();
        assert_eq!(refused.code, code::INVALID_INPUT, "{refused}");
        let undecoded = refused
            .message
            .starts_with("harness: Query did not decode: ");
        assert!(undecoded, "{refused}");
    }
}

const WAIT_DUE_TAG: u8 = 20;
const POD_WAIT_TAG: u8 = 21;
const POD_EVENT_TAG: u8 = 22;
const FORGED_TAG: u8 = 23;
const FILED_TAG: u8 = 28;
const MOVE_WAITS_TAG: u8 = 32;

fn retired_wait_ops(chain: &MockChain) -> [Vec<u8>; 6] {
    commit(chain, ADA, "/shared/a.md", "a");
    let files::Reply::Head(Some(head)) = chain.query(files::PROGRAM, &files::Query::Head).unwrap()
    else {
        panic!("files holds a head")
    };
    let event = egress::Event {
        id: 0,
        seq: 0,
        data: b"event: ended\ndata: {}".to_vec(),
    };
    let closed = forge::Watched::Closed {
        repo: "notes".into(),
        n: 1,
        by: Principal::Account(9),
    };
    [
        tagged(WAIT_DUE_TAG, &[]),
        tagged(POD_WAIT_TAG, &[encoded(&1u64)]),
        tagged(POD_EVENT_TAG, &[encoded(&1u64), encoded(&event)]),
        tagged(FORGED_TAG, &[encoded(&closed)]),
        tagged(FILED_TAG, &[encoded(&head)]),
        tagged(MOVE_WAITS_TAG, &[encoded(&None::<Vec<u8>>), encoded(&1u64)]),
    ]
}

#[test]
fn a_retired_wait_watcher_op_tag_is_refused_as_no_op() {
    let world = world();
    for bytes in retired_wait_ops(&world.chain) {
        let refused = match world.chain.submit_raw(signed(ADA), harness(), &bytes) {
            Ok(_) => panic!("tag {} ran as an op", bytes[0]),
            Err(refused) => refused,
        };
        assert_eq!(
            refused.code,
            code::INVALID_INPUT,
            "tag {}: {refused}",
            bytes[0]
        );
        let undecoded = refused.message.starts_with("harness: Op did not decode: ");
        assert!(undecoded, "tag {}: {refused}", bytes[0]);
    }
}
