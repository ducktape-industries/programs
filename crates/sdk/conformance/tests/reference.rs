// The suite run on the reference programs.

use abi::role::programs::{Entry, View};
use abi::role::validators::Member;
use conformance::env;
use program::{AccountNumber, BlobId, Error, MockHost, Origin, Principal, Program, Scheme};

const VALIDATOR: u8 = 5;

struct Identity;

impl Identity {
    fn run(host: &MockHost, key: &[u8], as_: Option<AccountNumber>, op: identity::Op) -> u64 {
        let env = env(
            "identity",
            1,
            Origin::Signed(key.to_vec()),
            as_.map(Principal::Account),
        );
        identity::Identity::execute(&host.exec(env), op).unwrap();
        let output = host.take_output();
        if output.is_empty() {
            0
        } else {
            abi::decode(&output).unwrap()
        }
    }

    fn add_key(key: &[u8], account: AccountNumber) -> identity::Op {
        identity::Op::AddKey {
            scheme: Scheme::Ed25519,
            label: None,
            consent: identity::Consent {
                key: key.to_vec(),
                account,
                expires_at: u64::MAX,
                proof: vec![],
            },
        }
    }

    fn get(host: &MockHost, number: AccountNumber) -> identity::Account {
        let env = env("identity", 1, Origin::Root, None);
        match identity::Identity::query(&host.query(env), identity::Query::Get { number }) {
            Ok(identity::Reply::Account(Some(account))) => account,
            other => panic!("Get({number}) answers the account, not {other:?}"),
        }
    }

    fn as_manager(host: &MockHost, agent: AccountNumber, op: identity::Op) {
        let identity::Control::Program { manager, .. } = Self::get(host, agent).control else {
            panic!("{agent} is a program account");
        };
        let key = Self::get(host, manager).keys()[0].key.clone();
        Self::run(host, &key, Some(manager), op);
    }

    fn user(host: &MockHost, key: &[u8]) -> AccountNumber {
        let op = identity::Op::Create {
            name: "user".into(),
            scheme: Scheme::Ed25519,
        };
        Self::run(host, key, None, op)
    }
}

impl conformance::identity::Fixture for Identity {
    type Program = identity::Identity;

    fn host(&self) -> MockHost {
        let host = MockHost::default();
        host.borrow_mut().verifier = Some(Box::new(|_, _, _, _, _| true));
        host
    }

    fn account(&self, host: &MockHost, key: &[u8]) -> AccountNumber {
        Self::user(host, key)
    }

    fn stopped(&self, host: &MockHost, key: &[u8]) -> AccountNumber {
        let agent = self.managed(host, key);
        Self::as_manager(host, agent, identity::Op::Suspend { account: agent });
        agent
    }

    fn drop_key(&self, host: &MockHost, account: AccountNumber, key: &[u8]) {
        let spare = [key, b"'s spare"].concat();
        Self::run(host, &spare, None, Self::add_key(key, account));
        let remove = identity::Op::RemoveKey {
            account,
            key: key.to_vec(),
        };
        Self::run(host, key, Some(account), remove);
    }

    fn managed(&self, host: &MockHost, key: &[u8]) -> AccountNumber {
        let manager_key = [key, b"'s manager"].concat();
        let manager = Self::user(host, &manager_key);
        let create = identity::Op::CreateAgent {
            name: "agent".into(),
        };
        let agent = Self::run(host, &manager_key, Some(manager), create);
        Self::as_manager(host, agent, Self::add_key(key, agent));
        agent
    }

    fn revoke(&self, host: &MockHost, account: AccountNumber) {
        Self::as_manager(host, account, identity::Op::Revoke { account });
    }

    fn script(&self, host: &MockHost, account: AccountNumber, program: &str) {
        let op = identity::Op::SetScript {
            account,
            script: Some(identity::Script {
                calls: program.into(),
                config: String::new(),
            }),
        };
        Self::as_manager(host, account, op);
    }
}

#[test]
fn identity_fills_the_identity_role() {
    conformance::identity::run(&Identity);
}

struct Valset;

impl Valset {
    fn run(host: &MockHost, signer: &[u8], op: valset::Op) -> Result<Vec<u8>, Error> {
        let env = env("valset", 1, Origin::Signed(signer.to_vec()), None);
        valset::Valset::execute(&host.exec(env), op)?;
        Ok(host.take_output())
    }

    fn validators(host: &MockHost) -> Vec<Vec<u8>> {
        let env = env("valset", 1, Origin::Root, None);
        match valset::Valset::query(&host.query(env), valset::Query::Validators) {
            Ok(valset::Reply::Validators(validators)) => validators,
            other => panic!("Validators answers Validators, not {other:?}"),
        }
    }

    fn pass(host: &MockHost, motion: valset::Motion) -> Result<(), Error> {
        for voter in Self::validators(host) {
            let output = Self::run(host, &voter, valset::Op::Vote(motion.clone()))?;
            let voted: valset::Voted = abi::decode(&output).unwrap();
            if voted == valset::Voted::Enacted {
                return Ok(());
            }
        }
        Err(program::wrong_state(
            "every validator voted and the motion did not pass",
        ))
    }

    fn invite(host: &MockHost, invitee: &[u8]) -> valset::Invite {
        host.borrow_mut().verifier = Some(Box::new(|_, key, _, message, signature| {
            signature == [key, message].concat()
        }));
        let issuer = Self::validators(host).remove(0);
        let grant = valset::Grant {
            network: env("valset", 1, Origin::Root, None).chain_id,
            nonce: invitee[..16].try_into().unwrap(),
            expires: u64::MAX,
        };
        let signature = [issuer.as_slice(), &grant.preimage()].concat();
        valset::Invite {
            issuer,
            grant,
            signature,
        }
    }
}

impl conformance::validators::Fixture for Valset {
    type Program = valset::Valset;

    fn seat(&self, host: &MockHost, member: Member) -> Result<(), Error> {
        let enroll = valset::Op::Enroll {
            address: member.address,
            halo: member.halo,
            invite: Some(Self::invite(host, &member.key)),
        };
        Self::run(host, &member.key, enroll)?;
        Self::pass(host, valset::Motion::Promote { key: member.key })
    }

    fn unseat(&self, host: &MockHost, key: &[u8]) -> Result<(), Error> {
        Self::pass(host, valset::Motion::Demote { key: key.to_vec() })
    }
}

#[test]
fn valset_fills_the_validators_role() {
    conformance::validators::run(&Valset);
}

struct Programs {
    blobs: MockHost,
}

impl Programs {
    fn new() -> Programs {
        let valset = MockHost::default();
        let genesis = valset::Genesis {
            validators: vec![Member {
                key: vec![VALIDATOR; 32],
                address: None,
                halo: "validator.example.org:9100".into(),
            }],
            member_cap: 4,
        };
        valset::Valset::init(
            &valset.exec(conformance::root("valset", 0)),
            &abi::encode(&genesis),
        )
        .unwrap();
        let blobs = MockHost::default();
        blobs.sibling::<valset::Valset>(MockHost::role(abi::role::VALIDATORS), &valset);
        Programs { blobs }
    }

    fn blobs(&self, op: blobs::Op, signer: Vec<u8>) -> Result<Vec<u8>, Error> {
        let env = env(
            "blobs",
            1,
            Origin::Signed(signer),
            Some(Principal::Account(1)),
        );
        blobs::Blobs::execute(&self.blobs.exec(env), op)?;
        Ok(self.blobs.take_output())
    }

    fn run(host: &MockHost, height: u64, op: programs::Op) -> Result<(), Error> {
        let validators = Origin::Program(MockHost::role(abi::role::VALIDATORS));
        let env = env("programs", height, validators, None);
        programs::Programs::execute(&host.exec(env), op)
    }

    fn schedule(
        host: &MockHost,
        height: u64,
        at: u64,
        change: programs::Change,
    ) -> Result<(), Error> {
        let scheduled = programs::Scheduled { height: at, change };
        Self::run(host, height, programs::Op::Schedule(scheduled))
    }
}

impl conformance::programs::Fixture for Programs {
    type Program = programs::Programs;

    fn hold(&self, host: &MockHost, body: &[u8]) -> Result<BlobId, Error> {
        host.sibling::<blobs::Blobs>(MockHost::role(abi::role::BLOBS), &self.blobs);
        let framed = blob::frame(programs::CODE_KIND, body).unwrap();
        let entry = blobs::Entry::of(&framed).unwrap();
        let code = entry.id;
        let put = blobs::Op::Put { blobs: vec![entry] };
        let bundle: u64 = abi::decode(&self.blobs(put, vec![1; 32])?).unwrap();
        self.blobs(blobs::Op::Receipt { bundle }, vec![VALIDATOR; 32])?;
        Ok(code)
    }

    fn set(&self, host: &MockHost, height: u64, at: u64, entry: Entry) -> Result<(), Error> {
        Self::schedule(host, height, at, programs::Change::Set(entry))
    }

    fn remove(&self, host: &MockHost, height: u64, at: u64, program: &str) -> Result<(), Error> {
        let change = programs::Change::Remove(program.into());
        Self::schedule(host, height, at, change)
    }

    fn set_hook(&self, host: &MockHost, height: u64, at: u64, program: &str) -> Result<(), Error> {
        let change = programs::Change::SetHook(program.into());
        Self::schedule(host, height, at, change)
    }

    fn remove_hook(
        &self,
        host: &MockHost,
        height: u64,
        at: u64,
        program: &str,
    ) -> Result<(), Error> {
        let change = programs::Change::RemoveHook(program.into());
        Self::schedule(host, height, at, change)
    }

    fn views(&self, host: &MockHost, height: u64) -> Vec<View> {
        let env = env("programs", height, Origin::Root, None);
        let query = programs::Query::Views(height);
        match programs::Programs::query(&host.query(env), query).unwrap() {
            programs::Reply::Views(views) => views,
            other => panic!("Views answers Views, not {other:?}"),
        }
    }
}

#[test]
fn program_programs_fills_the_programs_role() {
    conformance::programs::run(&Programs::new());
}

struct Awaits;

impl conformance::awaits::Fixture for Awaits {
    type Program = awaits::Awaits;

    fn host(&self) -> MockHost {
        MockHost::default()
    }
}

#[test]
fn awaits_fills_the_awaits_role() {
    conformance::awaits::run(&Awaits);
}

struct Halo;

impl conformance::halo::Fixture for Halo {
    type Program = halo::Halo;

    fn host(&self) -> MockHost {
        MockHost::default()
    }
}

#[test]
fn halo_fills_the_halo_role() {
    conformance::halo::run(&Halo);
}

struct Scheduler;

impl conformance::scheduler::Fixture for Scheduler {
    type Program = scheduler::Scheduler;

    fn host(&self) -> MockHost {
        MockHost::default()
    }
}

#[test]
fn scheduler_fills_the_scheduler_role() {
    conformance::scheduler::run(&Scheduler);
}
