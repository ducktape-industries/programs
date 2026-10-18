//! forge and chat on one [`MockChain`]: chat is the sibling forge queries,
//! and where its emissions land, in the frame that emitted them. The
//! chain's roster is identity's: each key the account it holds, the
//! harness keys ([`HOLDERS`](super::HOLDERS)) from the start, each program
//! its account ([`PROGRAMS`]) and each agent its standing ([`agent`](MemorySandbox::agent)).
//! An agent writes through [`Runner`], the program that runs it.

use abi::role::identity::{Category, Kind, Profile, Script, Standing};
use program::{Env, Error, MockChain, Origin, Principal, Program, Reply};
use program::{ExecCtx, MockHost, QueryCtx};

pub struct MemorySandbox {
    pub chain: MockChain,
    pub forge: MockHost,
    pub chat: MockHost,
    pub access: MockHost,
    pub runner: MockHost,
}

/// The program that runs the sandbox's agents.
pub const RUNNER: &str = "runner";

/// A program that runs agents, as harness does: each op is one forge op it
/// sends for the agent it names, acting as that agent's account.
pub struct Runner;

impl Program for Runner {
    const NAME: &'static str = RUNNER;
    type Op = (u64, forge::Op);
    type Query = ();
    type Reply = ();

    fn execute(ctx: &ExecCtx, (agent, op): (u64, forge::Op)) -> Result<(), Error> {
        ctx.emit_as(agent, forge::PROGRAM, &op, Reply::None);
        Ok(())
    }

    fn query(_: &QueryCtx, (): ()) -> Result<(), Error> {
        Ok(())
    }
}

/// The person every agent of the roster is managed by.
pub const AGENT_MANAGER: u64 = 11;

/// The account identity registered for each program.
pub const PROGRAMS: [(&str, u64); 3] = [("forge", 900), ("chat", 901), ("access", 902)];

/// The program whose account `number` is.
pub fn program_of(number: u64) -> Option<&'static str> {
    PROGRAMS
        .iter()
        .find(|(_, account)| *account == number)
        .map(|(program, _)| *program)
}

impl Default for MemorySandbox {
    fn default() -> Self {
        let mut chain = MockChain::default();
        let forge = chain.seat::<forge::Forge>("forge");
        let chat = chain.seat::<chat::Chat>("chat");
        let access = chain.seat::<access::Access>("access");
        let runner = chain.seat::<Runner>(RUNNER);
        for (key, account) in super::HOLDERS {
            chain.hold(key, account);
        }
        for (program, number) in PROGRAMS {
            chain.register(program, number);
        }
        MemorySandbox {
            chain,
            forge,
            chat,
            access,
            runner,
        }
    }
}

impl MemorySandbox {
    pub fn list_writers(
        &self,
        owner: &[u8],
        repo: &str,
        writers: &[Principal],
    ) -> Result<(), Error> {
        self.list_on(owner, &forge::repo_key(repo), writers)
    }

    pub fn list_on(&self, owner: &[u8], key: &str, writers: &[Principal]) -> Result<(), Error> {
        let parties = writers
            .iter()
            .filter_map(Principal::account)
            .map(program::Party::Account)
            .collect::<Vec<_>>();
        let list = match parties.is_empty() {
            true => None,
            false => Some(program::Ownership::Listed(parties)),
        };
        let set = access::Op::Set {
            program: "forge".into(),
            key: key.to_owned(),
            list,
        };
        self.chain
            .submit(Origin::Signed(owner.to_vec()), "access", &set)
            .map(drop)
    }

    /// Seats `key` in `account`, as identity would.
    pub fn hold(&self, key: &[u8], account: u64) {
        self.chain.hold(key, account);
    }

    /// An agent [`AGENT_MANAGER`] manages and [`Runner`] runs, with its
    /// standing.
    pub fn agent(&self, number: u64, standing: Standing) {
        self.chain.profile(Profile {
            number,
            name: format!("account {number}"),
            kind: Kind::Program {
                script: Some(Script {
                    calls: RUNNER.into(),
                    config: String::new(),
                }),
                manager: AGENT_MANAGER,
                category: Category::Agent,
                standing,
            },
        });
    }

    /// forge's env in a block at `height`, called by the chain itself.
    fn env(&self, height: u64) -> Env {
        Env {
            height,
            time: super::TIME,
            ..MockHost::env("forge")
        }
    }

    /// A write to forge's host at `height`, signed by the system.
    pub fn exec(&self, height: u64) -> ExecCtx {
        self.forge.exec(self.env(height))
    }

    /// A read of forge's host at `height`.
    pub fn reads(&self, height: u64) -> QueryCtx {
        self.forge.query(Env {
            sender: None,
            ..self.env(height)
        })
    }

    /// Who `key` signs as: the account it holds, if any.
    pub fn principal(&self, key: &[u8]) -> Option<Principal> {
        let account = self.chain.roster().keys.get(key).copied();
        account.map(Principal::Account)
    }

    /// Runs one chat message as a frame of its own, signed by `origin`.
    pub fn chat_execute(
        &self,
        origin: Origin,
        height: u64,
        time: u64,
        msg: chat::Op,
    ) -> Result<(), Error> {
        self.chain.at(height, time);
        self.chain.submit(origin, "chat", &msg)?;
        Ok(())
    }

    pub fn chat_query(&self, q: chat::Query) -> Result<chat::Reply, Error> {
        self.chain.query("chat", &q)
    }

    /// `agent`'s forge op in a block at `height`, sent by [`Runner`] as
    /// harness sends an agent's write: its frame is the runner's, acting as
    /// the agent's account.
    pub fn run_for(&self, agent: u64, height: u64, op: forge::Op) -> Result<Vec<u8>, Error> {
        self.chain.at(height, super::TIME);
        let anyone = Origin::Signed(super::STRANGER.to_vec());
        self.chain.submit(anyone, RUNNER, &(agent, op))
    }

    /// forge's op signed by `origin` in a block at `height`, and what it
    /// emitted, in one frame as the kernel runs it: forge's output, or the
    /// refusal that failed the frame (which left every host as it was).
    pub fn frame(
        &self,
        origin: Origin,
        height: u64,
        time: u64,
        op: &forge::Op,
    ) -> Result<Vec<u8>, Error> {
        self.chain.at(height, time);
        self.chain.submit(origin, "forge", op)
    }

    pub fn blob_count(&self) -> usize {
        self.forge.borrow().blobs.len()
    }
}
