//! What a program tells an agent about its ops and queries: a line on what it is for, and each
//! variant's description and an example. `#[derive(Described)]` lists an enum's variants (and what
//! each op returns); founding writes them as the `ducktape.tools` section (`borsh_json::tools`).
use borsh::BorshSerialize;

use crate::schema::Return;

pub const SECTION: &str = "ducktape.tools";

// One variant as `#[tool_op(..)]` or `#[tool_query(..)]` tells it: its description (empty where
// left out), an example as the borsh of a value of it, and for a query the `Reply` variant
// `#[ask(..)]` names as its answer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tool {
    pub variant: &'static str,
    pub description: &'static str,
    pub example: Option<Vec<u8>>,
    pub reply: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tools {
    pub about: &'static str,
    pub ops: Vec<Tool>,
    pub queries: Vec<Tool>,
}

impl Tools {
    // A program's ops and queries as their enums tell them, under the `#[tool_about(..)]` of
    // either enum.
    pub fn of<O: Described, Q: Described>() -> Tools {
        let about = match O::ABOUT {
            "" => Q::ABOUT,
            about => about,
        };
        Tools {
            about,
            ops: O::tools(),
            queries: Q::tools(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.about.is_empty() && self.ops.is_empty() && self.queries.is_empty()
    }
}

// What `#[derive(Described)]` writes for an enum. `RETURNS` lists each op variant whose
// `#[tool_op(returns = T)]` says what it returns, for `Program::RETURNS` to name.
pub trait Described {
    const ABOUT: &'static str = "";
    const RETURNS: &'static [Return] = &[];
    fn tools() -> Vec<Tool>;
}

impl Described for () {
    fn tools() -> Vec<Tool> {
        Vec::new()
    }
}

#[doc(hidden)]
pub fn example<T: BorshSerialize>(value: T) -> Vec<u8> {
    borsh::to_vec(&value).expect("borsh encodes an in-memory value")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ops;

    impl Described for Ops {
        fn tools() -> Vec<Tool> {
            vec![Tool {
                variant: "Open",
                description: "Open a poll.",
                example: Some(example(7u64)),
                ..Tool::default()
            }]
        }
    }

    struct Queries;

    impl Described for Queries {
        const ABOUT: &'static str = "Polls.";

        fn tools() -> Vec<Tool> {
            vec![Tool {
                variant: "Poll",
                reply: "Poll",
                ..Tool::default()
            }]
        }
    }

    #[test]
    fn a_programs_tools_are_its_ops_and_queries_under_either_enums_about() {
        let tools = Tools::of::<Ops, Queries>();
        assert_eq!(tools.about, "Polls.");
        assert_eq!(tools.ops[0].example, Some(vec![7, 0, 0, 0, 0, 0, 0, 0]));
        assert_eq!(tools.queries[0].reply, "Poll");
        assert!(Tools::of::<(), ()>().is_empty() && !tools.is_empty());
    }
}
