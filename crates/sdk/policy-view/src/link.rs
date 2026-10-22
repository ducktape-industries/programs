//! Who writes an item, set to what a person picked: one of its owner's live policies that says
//! exactly that, else a new one named as the item, then a Relink; and access's own list beside it
//! while the roster binds the access role, which gates every write too: who writes is the narrower.
use ::program::{Ownership as List, Party as Listed};
use ownership::{Ownership, Party, PolicyId, Writers};
use view::host::Error;
use view::methods::Submitted;
use view::prelude::*;

use crate::{Holdings, Item, holdings};

pub type Answer = Result<Submitted, Error>;

pub fn same(holdings: &Holdings, write: &Writers) -> Option<PolicyId> {
    holdings
        .live()
        .find(|policy| policy.write == *write && policy.switched_to.is_none())
        .map(|policy| policy.id)
}

pub fn landed(answer: &Submitted) -> bool {
    matches!(answer, Submitted::Applied { .. })
}

pub async fn link(host: &Host, item: Item, owner: Party, write: Writers) -> Answer {
    let held = holdings(host.clone(), owner).await?;
    let policy = match same(&held, &write) {
        Some(policy) => policy,
        None => {
            let create = ownership::Op::Create {
                name: item.label.clone(),
                write,
            };
            let made = host.ask::<Submit<Ownership>>(create).await?;
            let Submitted::Applied { output, .. } = &made else {
                return Ok(made);
            };
            view::methods::decode::<PolicyId>(output).map_err(|error| {
                view::host::malformed(format!("ownership's Create named no policy: {error}"))
            })?
        }
    };
    let relink = ownership::Op::Relink {
        program: item.program,
        key: item.key,
        policy,
    };
    host.ask::<Submit<Ownership>>(relink).await
}

pub async fn access_gates(host: &Host) -> Result<bool, Error> {
    let roles = host.query(programs::ask::Roles).await?;
    Ok(roles.iter().any(|(role, _)| role == abi::role::ACCESS))
}

pub async fn access_list(
    host: &Host,
    program: &str,
    key: &str,
    write: &Writers,
) -> Result<Option<Submitted>, Error> {
    if !access_gates(host).await? {
        return Ok(None);
    }
    let set = access::Op::Set {
        program: program.to_owned(),
        key: key.to_owned(),
        list: Some(list(write)),
    };
    host.ask::<Submit<access::Access>>(set).await.map(Some)
}

pub async fn access_writers(host: &Host, program: &str, key: &str) -> Result<Option<List>, Error> {
    if !access_gates(host).await? {
        return Ok(None);
    }
    let asked = access::ask::Writers {
        program: program.to_owned(),
        key: key.to_owned(),
    };
    Ok(Some(match host.query(asked).await? {
        ::program::Writers::Anyone => List::Anyone,
        ::program::Writers::Listed { list, .. } => list,
    }))
}

pub fn narrower(one: List, other: List) -> List {
    match (one, other) {
        (List::Anyone, list) | (list, List::Anyone) => list,
        (List::Owner, _) | (_, List::Owner) => List::Owner,
        (List::Listed(one), List::Listed(other)) => {
            let both: Vec<Listed> = one
                .into_iter()
                .filter(|party| other.contains(party))
                .collect();
            match both.is_empty() {
                true => List::Owner,
                false => List::Listed(both),
            }
        }
    }
}

pub fn list(write: &Writers) -> List {
    match write {
        Writers::Anyone => List::Anyone,
        Writers::Owner => List::Owner,
        Writers::Listed(parties) => List::Listed(parties.iter().map(listed).collect()),
    }
}

pub fn writers(list: &List) -> Writers {
    match list {
        List::Anyone => Writers::Anyone,
        List::Owner => Writers::Owner,
        List::Listed(parties) => Writers::Listed(parties.iter().map(party).collect()),
    }
}

pub fn listed(party: &Party) -> Listed {
    match party {
        Party::Account(number) => Listed::Account(*number),
        Party::Program(program) => Listed::Program(program.clone()),
        Party::Foreign(foreign) => Listed::Foreign(*foreign),
    }
}

pub fn party(listed: &Listed) -> Party {
    match listed {
        Listed::Account(number) => Party::Account(*number),
        Listed::Program(program) => Party::Program(program.clone()),
        Listed::Foreign(foreign) => Party::Foreign(*foreign),
    }
}
