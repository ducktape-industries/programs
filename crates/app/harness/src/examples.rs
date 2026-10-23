//! A working value of each harness op and query an agent or a person sends, as `#[tool_op]` and
//! `#[tool_query]` name them; the ops other programs deliver take the example the schema makes.
use crate::{Op, PageRequest, Place, Query, Wire};

fn page() -> PageRequest {
    PageRequest {
        after: None,
        limit: Some(20),
    }
}

pub(crate) fn close() -> Op {
    Op::Close { thread: 144 }
}

pub(crate) fn new_thread() -> Op {
    Op::NewThread { agent: 21 }
}

pub(crate) fn fork_thread() -> Op {
    Op::ForkThread { agent: 21 }
}

pub(crate) fn open_thread() -> Op {
    Op::OpenThread {
        agent: 21,
        place: Place::Channel {
            channel: "j-111".into(),
        },
    }
}

pub(crate) fn book() -> Op {
    Op::Book {
        height: 170000,
        target: "chat".into(),
        payload: Vec::new(),
        thread: None,
    }
}

pub(crate) fn unbook() -> Op {
    Op::Unbook { entry: 7 }
}

pub(crate) fn resume() -> Op {
    Op::Resume { thread: 144 }
}

pub(crate) fn unwait() -> Op {
    Op::Unwait { wait: 42 }
}

pub(crate) fn claim() -> Op {
    Op::ClaimConversations { agent: 21 }
}

pub(crate) fn adopt() -> Op {
    Op::AdoptOwners {
        kind: "harness.agent-conversation".into(),
        after: None,
        limit: 100,
    }
}

pub(crate) fn agent() -> Query {
    Query::Agent { agent: 21 }
}

pub(crate) fn thread() -> Query {
    Query::Thread { thread: 144 }
}

pub(crate) fn threads() -> Query {
    Query::Threads {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn frames() -> Query {
    Query::Frames {
        thread: 144,
        page: page(),
    }
}

pub(crate) fn held() -> Query {
    Query::Held { thread: 144 }
}

pub(crate) fn calls() -> Query {
    Query::Calls {
        thread: 144,
        page: page(),
    }
}

pub(crate) fn tools() -> Query {
    Query::Tools
}

pub(crate) fn thread_at() -> Query {
    Query::ThreadAt {
        agent: 21,
        place: "chat/j-111".into(),
    }
}

pub(crate) fn withheld() -> Query {
    Query::Withheld { thread: 144 }
}

pub(crate) fn partial() -> Query {
    Query::Partial { thread: 144 }
}

pub(crate) fn topic() -> Query {
    Query::Topic { thread: 144 }
}

pub(crate) fn bound() -> Query {
    Query::Bound {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn tally() -> Query {
    Query::Tally { thread: 144 }
}

pub(crate) fn frames_from() -> Query {
    Query::FramesFrom {
        thread: 144,
        from: 100,
        page: page(),
    }
}

pub(crate) fn call() -> Query {
    Query::Call {
        thread: 144,
        call: "toolu_01".into(),
    }
}

pub(crate) fn bookings() -> Query {
    Query::Bookings {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn booking() -> Query {
    Query::Booking { entry: 7 }
}

pub(crate) fn settling() -> Query {
    Query::Settling { thread: 144 }
}

pub(crate) fn waits() -> Query {
    Query::Waits {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn wait() -> Query {
    Query::Wait { wait: 42 }
}

pub(crate) fn armed() -> Query {
    Query::Armed {
        thread: 144,
        page: page(),
    }
}

pub(crate) fn card() -> Query {
    Query::Card { agent: 21 }
}

pub(crate) fn route() -> Query {
    Query::Route { thread: 144 }
}

pub(crate) fn routes() -> Query {
    Query::Routes { page: page() }
}

pub(crate) fn answerers() -> Query {
    Query::Answerers {
        thread: 144,
        page: page(),
    }
}

pub(crate) fn wakes() -> Query {
    Query::Wakes { thread: 144 }
}

pub(crate) fn agent_tools() -> Query {
    Query::AgentTools { agent: 21 }
}

pub(crate) fn cards() -> Query {
    Query::Cards { page: page() }
}

pub(crate) fn set_route() -> Op {
    Op::SetRoute {
        provider: "anthropic".into(),
        services: vec![crate::RouteService {
            service: "models".into(),
            wire: Wire::Harness,
        }],
    }
}

pub(crate) fn remove_route() -> Op {
    Op::RemoveRoute {
        provider: "openai".into(),
    }
}

pub(crate) fn provider_route() -> Query {
    Query::ProviderRoute {
        provider: "anthropic".into(),
    }
}

pub(crate) fn provider_routes() -> Query {
    Query::ProviderRoutes { page: page() }
}

pub(crate) fn final_errors() -> Query {
    Query::FinalErrors {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn runs() -> Query {
    Query::Runs { thread: 144 }
}

pub(crate) fn conversations() -> Query {
    Query::Conversations {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn budget_facts() -> Query {
    Query::BudgetFacts
}

pub(crate) fn adopt_tallies() -> Op {
    Op::AdoptTallies {
        after: None,
        limit: 100,
    }
}

pub(crate) fn hold_sealed() -> Op {
    Op::HoldSealed
}

pub(crate) fn sweep() -> Op {
    Op::Sweep
}

pub(crate) fn promise() -> Op {
    Op::Promise {
        thread: 504,
        owed_by: 21,
        channel: "j-287".into(),
        seq: 1,
        text: "post the design in j-287".into(),
        due: 350000,
    }
}

pub(crate) fn keep() -> Op {
    Op::Keep {
        promise: 1,
        kept: true,
        how: "posted it as message 2".into(),
    }
}

pub(crate) fn promises() -> Query {
    Query::Promises {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn promise_row() -> Query {
    Query::Promise { promise: 1 }
}

pub(crate) fn asked() -> Query {
    Query::Asked {
        agent: 17,
        page: page(),
    }
}

pub(crate) fn thread_card() -> Query {
    Query::ThreadCard { thread: 144 }
}

pub(crate) fn conversation_models() -> Query {
    Query::ConversationModels {
        agent: 21,
        page: page(),
    }
}

pub(crate) fn last_turn() -> Query {
    Query::LastTurn { thread: 144 }
}
