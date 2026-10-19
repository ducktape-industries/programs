//! A working value of each identity op and query, as `#[tool_op]` and `#[tool_query]` name them:
//! what an agent edits rather than build one from the schema.
use ::program::Scheme;

use crate::{Consent, Op, Ownership, PageRequest, Query, Reference, Script};

fn key() -> Vec<u8> {
    vec![0; 32]
}

pub(crate) fn register() -> Op {
    Op::RegisterProgram {
        program: "chat".into(),
    }
}

pub(crate) fn create() -> Op {
    Op::Create {
        name: "Ada".into(),
        scheme: Scheme::Ed25519,
    }
}

pub(crate) fn create_agent() -> Op {
    Op::CreateAgent {
        name: "scout".into(),
    }
}

pub(crate) fn add_key() -> Op {
    let consent = Consent {
        key: key(),
        account: 21,
        expires_at: u64::MAX,
        proof: vec![0; 64],
    };
    Op::AddKey {
        scheme: Scheme::Ed25519,
        label: Some("laptop".into()),
        consent,
    }
}

pub(crate) fn remove_key() -> Op {
    Op::RemoveKey {
        account: 21,
        key: key(),
    }
}

pub(crate) fn set_name() -> Op {
    Op::SetName {
        account: 21,
        name: "tech".into(),
    }
}

pub(crate) fn set_profile() -> Op {
    Op::SetProfile {
        account: 21,
        avatar: None,
        bio: Some("The resident engineer.".into()),
    }
}

pub(crate) fn suspend() -> Op {
    Op::Suspend { account: 40 }
}

pub(crate) fn resume() -> Op {
    Op::Resume { account: 40 }
}

pub(crate) fn revoke() -> Op {
    Op::Revoke { account: 40 }
}

pub(crate) fn set_ownership() -> Op {
    Op::SetOwnership {
        account: 40,
        ownership: Ownership::Owner,
    }
}

pub(crate) fn set_script() -> Op {
    let script = Script {
        calls: "harness".into(),
        config: "/home/scout/md".into(),
    };
    Op::SetScript {
        account: 40,
        script: Some(script),
    }
}

pub(crate) fn of_key() -> Query {
    Query::OfKey { key: key() }
}

pub(crate) fn of_program() -> Query {
    Query::OfProgram {
        program: "chat".into(),
    }
}

pub(crate) fn profile() -> Query {
    Query::Profile { number: 21 }
}

pub(crate) fn profiles() -> Query {
    Query::Profiles {
        after: None,
        limit: u32::MAX,
    }
}

pub(crate) fn runs() -> Query {
    Query::Runs {
        program: "harness".into(),
        account: 21,
    }
}

pub(crate) fn get() -> Query {
    Query::Get { number: 21 }
}

pub(crate) fn generation() -> Query {
    Query::Generation { key: key() }
}

pub(crate) fn resolve() -> Query {
    Query::Resolve {
        references: vec![Reference::Account(21), Reference::Key(key())],
    }
}

pub(crate) fn list() -> Query {
    Query::List {
        page: PageRequest {
            after: None,
            limit: None,
        },
    }
}

pub(crate) fn managed() -> Query {
    Query::Managed {
        by: 15,
        page: PageRequest {
            after: None,
            limit: None,
        },
    }
}
