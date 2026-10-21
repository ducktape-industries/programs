//! What the programs role says was renamed, each old id with its new one, for a program's `Migrate`
//! to rewrite the ids it stores.

use std::collections::BTreeMap;

use abi::role::programs::{Query, Reply};

use crate::{Error, Ownership, Party, ProgramId, QueryCtx, invalid, unexpected_reply};

/// The renames that hold at a height: each old program id with its new one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Renames(BTreeMap<ProgramId, ProgramId>);

impl Renames {
    pub fn moved(&self, program: &ProgramId) -> Option<&ProgramId> {
        self.0.get(program)
    }

    pub fn id(&self, program: &ProgramId) -> ProgramId {
        self.moved(program).unwrap_or(program).clone()
    }

    pub fn party(&self, party: &Party) -> Party {
        match party.program() {
            Some(program) => Party::Program(self.id(program)),
            None => party.clone(),
        }
    }

    pub fn ownership(&self, ownership: &Ownership) -> Ownership {
        match ownership {
            Ownership::Listed(parties) => {
                let followed = parties.iter().map(|party| self.party(party)).collect();
                Ownership::Listed(followed).settled(None)
            }
            Ownership::Owner | Ownership::Anyone => ownership.clone(),
        }
    }
}

impl QueryCtx {
    pub fn renames(&self, height: u64) -> Result<Renames, Error> {
        let now = self.env().height;
        if height > now {
            return Err(invalid(format!(
                "renames are read at a height already reached: {height} is past {now}"
            )));
        }
        let role = self.env().role(abi::role::PROGRAMS)?.clone();
        match self.query::<Query, Reply>(&role, &Query::Renames(height))? {
            Reply::Renames(renames) => Ok(Renames(renames.into_iter().collect())),
            other => Err(unexpected_reply(&role, "Renames", &other)),
        }
    }
}
