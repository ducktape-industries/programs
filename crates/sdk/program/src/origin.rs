//! Who may call, read off the env's [`Origin`]: the checks a program makes
//! before it trusts a frame's raw caller rather than its sender.

use crate::{Env, Error, Origin, Party, Principal, ProgramId, unauthorized};

impl Env {
    pub fn actor(&self) -> Vec<Party> {
        let account = self
            .sender
            .as_ref()
            .and_then(Principal::account)
            .map(Party::Account);
        let program = match &self.origin {
            Origin::Program(program) => Some(Party::Program(program.clone())),
            Origin::Signed(_) | Origin::Root => None,
        };
        account.into_iter().chain(program).collect()
    }

    /// The signing key of a signed frame; any other origin is refused.
    pub fn signer(&self) -> Result<Vec<u8>, Error> {
        match &self.origin {
            Origin::Signed(key) if key.is_empty() => Err(unauthorized(
                "an unsigned query holds no key: only a signed frame may do this",
            )),
            Origin::Signed(key) => Ok(key.clone()),
            other => Err(unauthorized(format!(
                "only a signed frame may do this, not {other:?}"
            ))),
        }
    }

    /// The program that sent this message; any other origin is refused.
    pub fn sending_program(&self) -> Result<ProgramId, Error> {
        match &self.origin {
            Origin::Program(program) => Ok(program.clone()),
            other => Err(unauthorized(format!(
                "only a program may do this, not {other:?}"
            ))),
        }
    }

    /// Refused unless the program bound to the validators role sent this:
    /// the validators' vote, enacted.
    pub fn authority(&self) -> Result<(), Error> {
        let validators = self.role(abi::role::VALIDATORS)?;
        let by_validators = matches!(&self.origin, Origin::Program(sender) if sender == validators);
        if by_validators {
            return Ok(());
        }
        Err(unauthorized(format!(
            "only the validators' vote ({validators}) may do this, not {:?}",
            self.origin
        )))
    }

    pub fn kernel(&self) -> Result<(), Error> {
        if self.origin == Origin::Root {
            return Ok(());
        }
        Err(unauthorized(format!(
            "only the kernel may do this, not {:?}",
            self.origin
        )))
    }

    /// Refused unless `program` or the chain itself sent this.
    pub fn sent_by(&self, program: &str) -> Result<(), Error> {
        let by_program = matches!(&self.origin, Origin::Program(sender) if sender == program);
        if by_program || self.origin == Origin::Root {
            return Ok(());
        }
        Err(unauthorized(format!(
            "only {program} may do this, not {:?}",
            self.origin
        )))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Env, MockHost, Origin, code};

    fn env(origin: Origin) -> Env {
        Env {
            origin,
            ..MockHost::env("m")
        }
    }

    #[test]
    fn each_check_admits_only_its_origin() {
        let signed = env(Origin::Signed(vec![1]));
        let program = env(Origin::Program("gov".into()));
        let root = env(Origin::Root);
        assert_eq!(signed.signer().unwrap(), [1]);
        assert_eq!(program.signer().unwrap_err().code, code::UNAUTHORIZED);
        assert_eq!(program.sending_program().unwrap(), "gov");
        assert_eq!(root.sending_program().unwrap_err().code, code::UNAUTHORIZED);
        assert!(program.sent_by("gov").is_ok() && root.sent_by("gov").is_ok());
        assert!(program.sent_by("other").is_err() && signed.sent_by("gov").is_err());
    }

    #[test]
    fn an_unsigned_query_holds_no_signer() {
        let unsigned = env(Origin::Signed(Vec::new()));
        assert_eq!(unsigned.signer().unwrap_err().code, code::UNAUTHORIZED);
    }

    #[test]
    fn authority_admits_only_the_program_bound_to_the_validators_role() {
        let validators = MockHost::role(abi::role::VALIDATORS);
        assert!(env(Origin::Program(validators)).authority().is_ok());
        for origin in [
            Origin::Signed(vec![1; 32]),
            Origin::Program("gov".into()),
            Origin::Program(MockHost::role(abi::role::PROGRAMS)),
            Origin::Root,
        ] {
            let refused = env(origin).authority().unwrap_err();
            assert_eq!(refused.code, code::UNAUTHORIZED);
        }
        let mut rebound = env(Origin::Program("gov".into()));
        rebound.roles = MockHost::roles_with(abi::role::VALIDATORS, "gov");
        assert!(rebound.authority().is_ok());
        let mut unbound = env(Origin::Program("gov".into()));
        unbound.roles = MockHost::roles()
            .iter()
            .filter(|(role, _)| *role != abi::role::VALIDATORS)
            .collect();
        assert_eq!(unbound.authority().unwrap_err().code, code::UNSUPPORTED);
    }
}
