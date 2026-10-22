//! The words Account shows, once for both designs, as the Expo app keeps
//! them in `src/account/words.ts`: what an account is, a key's name, whom
//! an agent answers, an agent's standing, why a form did not go, the
//! network's name, and every sentence both designs show. Pure: nothing here
//! reads the state or writes.
use identity::{Kind, Ownership, Party, Standing};
use view::methods::Session;

use crate::state::Problem;

pub(crate) struct FormWords {
    pub(crate) prompt: &'static str,
    pub(crate) empty: &'static str,
    pub(crate) verb: &'static str,
    pub(crate) busy: &'static str,
}

pub(crate) const ACCOUNT_NAME: &str = "Account name";
pub(crate) const CREATE_ACCOUNT: FormWords = FormWords {
    prompt: "Name the new account",
    empty: "Enter an account name.",
    verb: "Create account",
    busy: "Creating…",
};

pub(crate) const AGENT_NAME: &str = "Agent name";
pub(crate) const CREATE_AGENT: FormWords = FormWords {
    prompt: "Name the new agent",
    empty: "Enter an agent name.",
    verb: "Create agent",
    busy: "Creating…",
};

pub(crate) const KEY_REQUEST: &str = "Agent key request";
pub(crate) const ADD_KEY: FormWords = FormWords {
    prompt: "Paste an agent's key request",
    empty: "",
    verb: "Add key",
    busy: "Adding…",
};

pub(crate) fn rename_field(agent: &str) -> String {
    format!("Rename {agent}")
}
pub(crate) const RENAME: FormWords = FormWords {
    prompt: "New name",
    empty: "Enter the agent's new name.",
    verb: "Save",
    busy: "Renaming…",
};

pub(crate) fn person_field(agent: &str) -> String {
    format!("Add someone {agent} answers")
}
pub(crate) const ADD_PERSON: FormWords = FormWords {
    prompt: "Name, account number or program",
    empty: "Enter a name, an account number or a program.",
    verb: "Add",
    busy: "Adding…",
};

pub(crate) const READING: &str = "Reading your account…";
pub(crate) const NO_ACCOUNT: &str = "No account";
pub(crate) const NO_KEY: &str = "No host key is selected. Sign in with a key to create an account.";
pub(crate) const UNREGISTERED: &str = "Unregistered key";
pub(crate) const NO_ACCOUNT_YET: &str = "no account yet";
pub(crate) const AGENT_KEY: &str = "a key of an agent";
pub(crate) const KEYS: &str = "Keys";
pub(crate) const VALIDATOR: &str = "Validator";
pub(crate) const CREATE_ACCOUNT_TITLE: &str = "Create an account";
pub(crate) const CREATE_ACCOUNT_WHY: &str =
    "Your key isn't linked to an account yet. An account gives you a name others see.";
pub(crate) const AGENTS_HELP: &str =
    "Agents act as accounts you manage. You answer for what they do.";
pub(crate) const CREATE_AGENT_TITLE: &str = "Connect an external agent";
pub(crate) const CONNECT_WHY: &str = "An agent another machine runs with its own key. To have this network run one, create it in Agents.";
pub(crate) const ADD_A_KEY: &str = "Add a key…";
pub(crate) const RUN_ON_HARNESS: &str = "Run it on harness…";
pub(crate) const ADD_KEY_TITLE: &str = "Add a key to an agent";
pub(crate) const ADD_KEY_WHY: &str = "Paste the key request the agent's device shows.";
pub(crate) const WHO_IT_ANSWERS: &str = "Who it answers";
pub(crate) const ANYONE_WARNING: &str = "Anyone on the network can tell this agent what to do.";
pub(crate) const NOBODY_LISTED: &str =
    "Add people by name or account number, or a program by its name.";
pub(crate) const WORKING: &str = "Working…";
pub(crate) const SAVING: &str = "Saving…";
pub(crate) const ADD: &str = "Add";
pub(crate) const CANCEL: &str = "Cancel";
pub(crate) const RENAME_AGENT: &str = "Rename";
pub(crate) const REVOKE: &str = "Revoke";
pub(crate) const REMOVE: &str = "Remove";
pub(crate) const CHANGE_PHOTO: &str = "Change photo…";
pub(crate) const OPENING: &str = "Opening…";
pub(crate) const REMOVE_PHOTO: &str = "Remove photo";
pub(crate) const REMOVING: &str = "Removing…";
pub(crate) const PHOTO_TITLE: &str = "Change photo";
pub(crate) const PHOTO_HELP: &str = "Drag the picture to place it in the circle.";
pub(crate) const PHOTO_FRAME: &str = "Move the picture in the circle";
pub(crate) const ZOOM: &str = "Zoom";
pub(crate) const SAVE: &str = "Save";

pub(crate) fn open_in(program: &str) -> String {
    format!("Open in {program}")
}

pub(crate) fn runs_badge(kind: &Kind) -> Option<String> {
    view::design::runs(kind)
}

pub(crate) fn key_for(number: u64, created: bool) -> String {
    let asks = format!(
        "On the agent's machine, sign a key request for account {number}, then paste it here."
    );
    match created {
        true => format!("Created account {number}. {asks}"),
        false => asks,
    }
}

pub(crate) fn who_answers(agent: &str) -> String {
    format!("Who {agent} answers")
}

pub(crate) fn account(number: u64) -> String {
    format!("account {number}")
}

pub(crate) fn foreign(foreign: &abi::Foreign) -> String {
    format!(
        "account {} of network {}",
        foreign.account,
        abi::hex(&foreign.network)
    )
}

pub(crate) fn party(party: &Party) -> String {
    match party {
        Party::Account(number) => account(*number),
        Party::Program(_) => "program".into(),
        Party::Foreign(foreign) => self::foreign(foreign),
    }
}

pub(crate) fn foreign_id(foreign: &abi::Foreign) -> String {
    format!("foreign-{}-{}", abi::hex(&foreign.network), foreign.account)
}

pub(crate) fn party_id(party: &Party) -> String {
    match party {
        Party::Account(number) => number.to_string(),
        Party::Program(program) => format!("program-{program}"),
        Party::Foreign(foreign) => foreign_id(foreign),
    }
}

pub(crate) fn account_keys(number: u64, keys: usize) -> String {
    format!("{} · {}", account(number), keys_count(keys))
}

pub(crate) fn keys_count(keys: usize) -> String {
    view::design::plural(keys as u64, "key", "keys")
}

pub(crate) fn what(kind: &Kind) -> String {
    match kind {
        Kind::User => "Person".into(),
        Kind::Program { .. } => "Agent".into(),
        Kind::System(program) => format!("Program · {program}"),
    }
}

pub(crate) fn key_label(label: Option<&str>, index: usize) -> String {
    match label {
        Some(label) => label.to_owned(),
        None => format!("Device {}", index + 1),
    }
}

pub(crate) fn answers(ownership: &Ownership) -> String {
    match ownership {
        Ownership::Owner => "only me".into(),
        Ownership::Anyone => "anyone".into(),
        Ownership::Listed(_) => {
            let people = ownership.accounts().count() as u64;
            let programs = ownership.programs().count() as u64;
            let people_words = view::design::plural(people, "person", "people");
            let program_words = view::design::plural(programs, "program", "programs");
            match (people, programs) {
                (_, 0) => people_words,
                (0, _) => program_words,
                _ => format!("{people_words}, {program_words}"),
            }
        }
    }
}

pub(crate) fn standing(standing: Standing) -> &'static str {
    match standing {
        Standing::Active => "active",
        Standing::Suspended => "suspended",
        Standing::Revoked => "revoked",
    }
}

pub(crate) fn problem_text(problem: &Problem, empty: &str) -> String {
    match problem {
        Problem::Empty => empty.to_owned(),
        Problem::NotAKeyRequest => "That isn’t a key request for one of your agents.".into(),
        Problem::NoSuchAccount => "No account or program has that name or number.".into(),
        Problem::SeveralAccounts => {
            "More than one account has that name. Add it by its number.".into()
        }
        Problem::NotAPicture => "That file isn’t a picture this app can open.".into(),
        Problem::Refused(said) => said.clone(),
    }
}

pub(crate) fn network(session: &Session) -> Option<&str> {
    session
        .chain_id
        .split('#')
        .next()
        .filter(|name| !name.is_empty())
}
