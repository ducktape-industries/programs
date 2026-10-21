//! What halo tells an agent of its ops and queries. Both enums are the halo role's, defined in
//! abi, so markers carry each variant's words and a working value of it.
use ::program::tools::{Described, Tool, example};

use crate::{Class, Op, Query};

pub(crate) struct Ops;

pub(crate) struct Queries;

fn op(variant: &'static str, description: &'static str, value: Op) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: "",
    }
}

fn query(variant: &'static str, description: &'static str, value: Query) -> Tool {
    Tool {
        variant,
        description,
        example: Some(example(value)),
        reply: variant,
    }
}

fn lane() -> String {
    "files".into()
}

impl Described for Ops {
    const ABOUT: &'static str = "The lanes halo carries between members: each a name with a wire id, the class its streams are served at most (Critical outranks Member outranks Client), whether it is served, and whether the founding pinned it. Every op is the validators' vote (a Role motion of valset); a signed frame is refused.";

    fn tools() -> Vec<Tool> {
        vec![
            op(
                "Open",
                "List name, 1 to 64 printable ASCII bytes not listed yet, at the next id, served and not pinned. An id is handed out once, in order from 0, and the 256 a lane byte holds are all there are.",
                Op::Open {
                    name: lane(),
                    cap: Class::Member,
                },
            ),
            op(
                "Set",
                "Change a listed lane's cap and whether it is served; its id stays. A pinned lane is never paused.",
                Op::Set {
                    name: lane(),
                    cap: Class::Client,
                    enabled: true,
                },
            ),
            op(
                "Close",
                "Unlist a lane; its id is never handed out again. A pinned lane is never closed.",
                Op::Close { name: lane() },
            ),
        ]
    }
}

impl Described for Queries {
    fn tools() -> Vec<Tool> {
        vec![
            query(
                "Lanes",
                "Every listed lane, in id order: its name, id, cap, whether it is served and pinned, and the block that opened it.",
                Query::Lanes,
            ),
            query(
                "Lane",
                "One lane by name, or null when none is listed under it.",
                Query::Lane { name: lane() },
            ),
        ]
    }
}
