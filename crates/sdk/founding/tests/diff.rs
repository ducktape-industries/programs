// The types here are never built: each is here for its schema alone.
#![allow(dead_code)]

use borsh::BorshSchema;
use borsh::schema::BorshSchemaContainer;
use founding::{Change, Difference};

#[derive(BorshSchema)]
struct Interface<O, Q, R> {
    op: O,
    query: Q,
    reply: R,
}

fn of<O: BorshSchema, Q: BorshSchema, R: BorshSchema>() -> BorshSchemaContainer {
    BorshSchemaContainer::for_type::<Interface<O, Q, R>>()
}

mod before {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
        Delete(u64),
    }

    #[derive(BorshSchema)]
    pub enum Query {
        Thread(Box<Node>),
    }

    #[derive(BorshSchema)]
    pub struct Node {
        pub replies: Vec<Node>,
    }
}

mod appended {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
        Delete(u64),
        React { seq: u64, emoji: String },
    }
}

mod retired {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    #[borsh(use_discriminant = true)]
    #[repr(u8)]
    pub enum Op {
        Post { channel: String, text: String },
        React { seq: u64, emoji: String } = 2,
    }
}

mod retired_then_appended {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    #[borsh(use_discriminant = true)]
    #[repr(u8)]
    pub enum Op {
        Post { channel: String, text: String },
        React { seq: u64, emoji: String } = 2,
        Pin(u64),
    }
}

mod reused {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
        Pin(u64),
        React { seq: u64, emoji: String },
    }
}

mod first_only {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post { channel: String, text: String },
    }
}

mod broken {
    use borsh::BorshSchema;

    #[derive(BorshSchema)]
    pub enum Op {
        Post {
            channel: String,
            body: String,
            at: u64,
        },
        Remove(u32),
    }
}

fn diff(old: &BorshSchemaContainer, new: &BorshSchemaContainer) -> Vec<String> {
    let found = founding::differences(old, new).unwrap();
    found.iter().map(Difference::to_string).collect()
}

#[test]
fn an_interface_read_against_itself_differs_nowhere() {
    let schema = of::<before::Op, before::Query, u8>();
    assert!(diff(&schema, &schema).is_empty());
    let chat = founding::schema("chat").unwrap();
    assert!(founding::differences(&chat, &chat).unwrap().is_empty());
}

#[test]
fn a_variant_added_at_the_end_reads_old_bytes_the_same() {
    let old = of::<before::Op, before::Query, u8>();
    let new = of::<appended::Op, before::Query, u8>();
    assert_eq!(diff(&old, &new), ["+ op: variant 2 React added at the end"]);
    let found = founding::differences(&new, &old).unwrap();
    assert_eq!(found[0].change, Change::Breaking);
    assert_eq!(
        found[0].to_string(),
        "! op: variant 2 React is gone from the end, where the next variant appended would take its tag"
    );
}

#[test]
fn a_variant_deleted_with_its_tag_kept_is_retired_and_the_next_append_counts_on_past_it() {
    let old = of::<appended::Op, before::Query, u8>();
    let gapped = of::<retired::Op, before::Query, u8>();
    assert_eq!(
        diff(&old, &gapped),
        ["- op: variant 1 Delete retired, its tag kept"]
    );
    let found = founding::differences(&old, &gapped).unwrap();
    assert_eq!(found[0].change, Change::Retired);
    let grown = of::<retired_then_appended::Op, before::Query, u8>();
    assert_eq!(
        diff(&gapped, &grown),
        ["+ op: variant 3 Pin added at the end"]
    );
}

#[test]
fn a_retired_tag_taken_again_or_the_last_variant_dropped_breaks_old_bytes() {
    let gapped = of::<retired::Op, before::Query, u8>();
    let reused = of::<reused::Op, before::Query, u8>();
    assert_eq!(
        diff(&gapped, &reused),
        [
            "! op: variant 1 Pin takes a tag at or below 2, the old last: a tag never means two things"
        ]
    );
    let shrunk = of::<first_only::Op, before::Query, u8>();
    let found = founding::differences(&gapped, &shrunk).unwrap();
    assert_eq!(found[0].change, Change::Breaking);
    assert_eq!(
        found[0].to_string(),
        "! op: variant 2 React is gone from the end, where the next variant appended would take its tag"
    );
}

#[test]
fn a_renamed_retyped_or_regrown_part_breaks_old_bytes() {
    let old = of::<before::Op, before::Query, u8>();
    let new = of::<broken::Op, before::Query, u16>();
    assert_eq!(
        diff(&old, &new),
        [
            "! op::Post: field 1 was text, is body",
            "! op::Post: field at added",
            "! op: variant 1 was Delete, is Remove",
            "! op::Remove.0: was u64 (a fixed-size value), is u32 (a fixed-size value)",
            "! reply: was u8 (a fixed-size value), is u16 (a fixed-size value)",
        ]
    );
}

#[test]
fn two_programs_differ_and_a_bare_type_is_no_interface() {
    let harness = founding::schema("harness").unwrap();
    let chat = founding::schema("chat").unwrap();
    let found = founding::differences(&chat, &harness).unwrap();
    assert!(
        found.iter().any(|found| found.change == Change::Breaking),
        "two programs' interfaces differ"
    );
    let unnamed = BorshSchemaContainer::for_type::<u64>();
    assert!(founding::differences(&unnamed, &chat).is_err());
}
