//! What the identity role says about an account a program is asked to name.

use abi::role::identity::{Query, Reply};

use crate::{AccountNumber, Error, QueryCtx, invalid, unexpected_reply};

impl QueryCtx {
    pub fn require_account(&self, number: AccountNumber) -> Result<(), Error> {
        let asked = Query::Profile(number);
        let identity = self.env().role(abi::role::IDENTITY)?.clone();
        let Reply::Profile(profile) = self.query::<Query, Reply>(&identity, &asked)? else {
            return Err(unexpected_reply(&identity, "Profile", &"another reply"));
        };
        if profile.is_none() {
            return Err(invalid(format!("there is no account {number}")));
        }
        Ok(())
    }
}
