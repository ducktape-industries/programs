//! The `forge-review-chat` program: it hears forge's change events (forge `Watch`) and posts each
//! as one notice in the chat room the network's rules map the change's source branch to.
mod examples;
mod program;
mod rules;
mod state;
#[cfg(test)]
mod tests;

pub use program::ForgeReviewChat;

use borsh::{BorshDeserialize, BorshSchema, BorshSerialize};

pub const PROGRAM: &str = "forge-review-chat";

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Rule {
    pub branch: String,
    pub room: String,
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub struct Mirror {
    pub repo: String,
    pub n: u64,
    pub room: String,
}

#[derive(
    Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Described,
)]
#[tool_about(
    "Posts forge's change events into chat rooms: a change whose source branch a rule matches as it opens has its opening, reviews, leaving a merge queue, merge and close each told as one notice in the room the rule names, while chat holds that room open to post. The change's conversation stays in its own forge room."
)]
pub enum Op {
    #[tool_op(
        description = "From forge only: one change event it tells its watchers.",
        example = crate::examples::watched
    )]
    Watched(forge::Watched),
    #[tool_op(
        description = "Set the network's rules, tried in order: a change opened from refs/heads/<name> is mirrored into the room of the first rule whose branch matches the start of name, where {} in branch matches one or more characters other than / (the longest run the rest of the pattern follows) and fills {} in room. The first write claims the rules as its writer's, and their ownership policy decides every later write.",
        example = crate::examples::set_rules
    )]
    SetRules { rules: Vec<Rule> },
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    BorshSerialize,
    BorshDeserialize,
    BorshSchema,
    ::program::Ask,
    ::program::Described,
)]
#[ask(ForgeReviewChat)]
pub enum Query {
    #[ask(Reply::Rules(Vec<Rule>))]
    #[tool_query(
        description = "The network's rules, in the order they are tried.",
        example = crate::examples::rules
    )]
    Rules,
    #[ask(Reply::Room(Option<String>))]
    #[tool_query(
        description = "The room change n of repo is mirrored into, or null: not opened under a rule, or merged or closed since.",
        example = crate::examples::room
    )]
    Room { repo: String, n: u64 },
    #[ask(Reply::Mirrored(store::PageResponse<Mirror>))]
    #[tool_query(
        description = "A page of every open change mirrored into a room, by repository then number, each with its room: what a room's changes are read from.",
        example = crate::examples::mirrored
    )]
    Mirrored { page: store::PageRequest },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Rules(Vec<Rule>),
    Room(Option<String>),
    Mirrored(store::PageResponse<Mirror>),
}

pub fn watching() -> Vec<u8> {
    vec![0]
}

pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Watched(_) => ("Hear a change event from forge".to_owned(), Vec::new()),
        Op::SetRules { rules } if rules.is_empty() => {
            ("Mirror no change into chat".to_owned(), Vec::new())
        }
        Op::SetRules { rules } => (
            "Set where changes are mirrored in chat".to_owned(),
            vec![field(
                "rules",
                Value::List(
                    rules
                        .iter()
                        .map(|rule| Value::text(format!("{} to {}", rule.branch, rule.room)))
                        .collect(),
                ),
            )],
        ),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);

#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Watched", "SetRules"]);
}

#[test]
fn query_variants_only_append() {
    assert_eq!(describe::variants::<Query>(), ["Rules", "Room", "Mirrored"]);
}
