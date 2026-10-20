//! The identity role: `RegisterProgram` from the system, and who a key or a
//! program acts as. [`run`] checks every rule below.

use abi::role::identity::{Category, Kind, Op, Profile, Query, Reply, Standing};
use program::{AccountNumber, Error, MockHost, Origin, Principal, Program, code};

use crate::{ask, execute, root, same_bytes};

/// What the suite needs from the program beyond the role.
pub trait Fixture {
    type Program: Program;

    /// A founded host: the program's init run, and whatever its own ops
    /// need (a verifier).
    fn host(&self) -> MockHost;

    /// `key` comes to hold a new account that acts; its number.
    fn account(&self, host: &MockHost, key: &[u8]) -> AccountNumber;

    /// `key` comes to hold a new account that does not act (a suspended
    /// program account); its number.
    fn stopped(&self, host: &MockHost, key: &[u8]) -> AccountNumber;

    /// `account`, which `key` holds (an account [`Fixture::account`]
    /// made), stops holding `key`. The account may be given another key
    /// first, if yours keeps a last one.
    fn drop_key(&self, host: &MockHost, account: AccountNumber, key: &[u8]);

    /// `key` comes to hold a new program account that acts (an active
    /// one, managed by a user); its number.
    fn managed(&self, host: &MockHost, key: &[u8]) -> AccountNumber;

    /// The program account `account` ([`Fixture::managed`]'s) is revoked,
    /// by its manager.
    fn revoke(&self, host: &MockHost, account: AccountNumber);

    fn script(&self, host: &MockHost, account: AccountNumber, program: &str);
}

/// Every identity rule, each on a fresh host.
pub fn run(fixture: &impl Fixture) {
    the_role_is_the_first_variants(fixture);
    register_program_is_root_only_and_idempotent(fixture);
    register_program_is_refused_to_a_frame_that_acts(fixture);
    an_unknown_key_or_program_holds_nothing(fixture);
    a_held_key_resolves_to_an_acting_account(fixture);
    an_account_that_does_not_act_is_refused(fixture);
    a_dropped_key_holds_nothing(fixture);
    a_revoked_account_s_keys_stop_resolving(fixture);
    an_absent_profile_is_none(fixture);
    profiles_page_ascending(fixture);
    profiles_of_limit_zero_is_a_page(fixture);
    program_profiles_agree_with_of_program(fixture);
    a_program_runs_only_the_program_account_whose_script_calls_it(fixture);
}

fn program() -> String {
    MockHost::role(abi::role::IDENTITY)
}

fn query<F: Fixture>(host: &MockHost, query: Query) -> Result<Reply, Error> {
    ask::<F::Program, Reply>(host, &program(), 1, &query)
}

fn account_of<F: Fixture>(host: &MockHost, query: Query) -> Option<AccountNumber> {
    match self::query::<F>(host, query.clone()) {
        Ok(Reply::Account(number)) => number,
        other => panic!("identity: {query:?} answers Account, not {other:?}"),
    }
}

fn profile<F: Fixture>(host: &MockHost, number: AccountNumber) -> Option<Profile> {
    match query::<F>(host, Query::Profile(number)) {
        Ok(Reply::Profile(profile)) => profile,
        other => panic!("identity: Profile({number}) answers Profile, not {other:?}"),
    }
}

fn every_profile<F: Fixture>(host: &MockHost, limit: u32) -> Vec<Profile> {
    let mut all: Vec<Profile> = Vec::new();
    let mut after = None;
    loop {
        let asked = Query::Profiles { after, limit };
        let (profiles, next) = match query::<F>(host, asked.clone()) {
            Ok(Reply::Profiles { profiles, next }) => (profiles, next),
            other => panic!("identity: {asked:?} answers Profiles, not {other:?}"),
        };
        assert!(
            profiles.len() <= limit as usize,
            "identity: {asked:?} answered {} profiles, past its limit",
            profiles.len()
        );
        assert!(
            after.is_none() || !profiles.is_empty(),
            "identity: {asked:?} answers nothing, yet the page before said more followed: \
             `next` is None at the end"
        );
        for profile in &profiles {
            let last = all.last().map(|p| p.number).or(after);
            assert!(
                last.is_none_or(|last| profile.number > last),
                "identity: {asked:?} is not ascending past {last:?}: {}",
                profile.number
            );
            all.push(profile.clone());
        }
        match next {
            None => return all,
            Some(next) => {
                assert_eq!(
                    Some(next),
                    profiles.last().map(|p| p.number),
                    "identity: {asked:?}: `next` is the last number of a page that has more"
                );
                after = Some(next);
            }
        }
    }
}

fn register<F: Fixture>(
    host: &MockHost,
    origin: Origin,
    sender: Option<Principal>,
    name: &str,
) -> Result<(), Error> {
    let mut env = root(&program(), 1);
    if origin != Origin::Root {
        env.origin = origin;
        env.sender = sender;
    }
    execute::<F::Program>(
        host,
        env,
        &Op::RegisterProgram {
            program: name.into(),
        },
    )
}

/// The role's op, queries and replies are the program's first variants,
/// byte for byte.
pub fn the_role_is_the_first_variants<F: Fixture>(_: &F) {
    type M<F> = <F as Fixture>::Program;
    let m = program();
    let op = Op::RegisterProgram {
        program: "chat".into(),
    };
    same_bytes::<<M<F> as Program>::Op>(&m, "Op", &op);
    for query in [
        Query::Account(b"key".to_vec()),
        Query::OfProgram("chat".into()),
        Query::Profile(4),
        Query::Profiles {
            after: Some(2),
            limit: 5,
        },
        Query::Runs {
            program: "chat".into(),
            account: 4,
        },
    ] {
        same_bytes::<<M<F> as Program>::Query>(&m, "Query", &query);
    }
    let profile = Profile {
        number: 1,
        name: "Scout".into(),
        kind: Kind::Program {
            script: None,
            manager: 2,
            category: Category::Agent,
            standing: Standing::Suspended,
        },
    };
    for reply in [
        Reply::Account(Some(3)),
        Reply::Account(None),
        Reply::Profile(Some(profile.clone())),
        Reply::Profile(None),
        Reply::Profiles {
            profiles: vec![profile],
            next: Some(1),
        },
    ] {
        same_bytes::<<M<F> as Program>::Reply>(&m, "Reply", &reply);
    }
}

/// Only the system registers a program: a signed frame and another program
/// are refused `unauthorized` and write nothing. Registering twice keeps
/// the one account, which is the program's.
pub fn register_program_is_root_only_and_idempotent<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    for origin in [
        Origin::Signed(b"someone".to_vec()),
        Origin::Program("chat".into()),
    ] {
        let answer = host.attempt(|| register::<F>(&host, origin.clone(), None, "probe"));
        assert_eq!(
            answer.map_err(|e| e.code),
            Err(code::UNAUTHORIZED.into()),
            "identity: RegisterProgram from {origin:?} is refused unauthorized"
        );
    }
    register::<F>(&host, Origin::Root, None, "probe")
        .unwrap_or_else(|e| panic!("identity: the system registers a program: {e:?}"));
    let number = account_of::<F>(&host, Query::OfProgram("probe".into()))
        .expect("identity: OfProgram answers the account RegisterProgram gave");
    let before = every_profile::<F>(&host, 50);
    register::<F>(&host, Origin::Root, None, "probe")
        .unwrap_or_else(|e| panic!("identity: registering a program again is no refusal: {e:?}"));
    assert_eq!(
        account_of::<F>(&host, Query::OfProgram("probe".into())),
        Some(number),
        "identity: registering again keeps the program's account"
    );
    assert_eq!(
        every_profile::<F>(&host, 50),
        before,
        "identity: registering again changes no account"
    );
    assert_eq!(
        profile::<F>(&host, number).map(|p| p.kind),
        Some(Kind::System("probe".into())),
        "identity: a registered program's profile is Kind::System"
    );
}

/// The kernel presents who a frame acts as: a signed frame as its key's
/// account, a program's frame as the program's account. Neither is the
/// system, so `RegisterProgram` is refused `unauthorized` and writes nothing
/// though the frame has a sender: the system is the origin, not a sender.
pub fn register_program_is_refused_to_a_frame_that_acts<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let user = fixture.account(&host, b"someone");
    register::<F>(&host, Origin::Root, None, "chat")
        .unwrap_or_else(|e| panic!("identity: the system registers chat: {e:?}"));
    let chat = account_of::<F>(&host, Query::OfProgram("chat".into()))
        .expect("identity: OfProgram answers the account RegisterProgram gave");
    for (origin, sender) in [
        (Origin::Signed(b"someone".to_vec()), user),
        (Origin::Program("chat".into()), chat),
    ] {
        let sender = Some(Principal::Account(sender));
        let answer = host.attempt(|| register::<F>(&host, origin.clone(), sender.clone(), "probe"));
        assert_eq!(
            answer.map_err(|e| e.code),
            Err(code::UNAUTHORIZED.into()),
            "identity: RegisterProgram from {origin:?} acting as {sender:?} is refused \
             unauthorized: only the system's origin registers, whoever the sender"
        );
    }
}

/// A key no account holds, and a program never registered, answer `None`.
pub fn an_unknown_key_or_program_holds_nothing<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    assert_eq!(
        account_of::<F>(&host, Query::Account(b"nobody's key".to_vec())),
        None,
        "identity: Account of an unknown key is None"
    );
    assert_eq!(
        account_of::<F>(&host, Query::OfProgram("never-registered".into())),
        None,
        "identity: OfProgram of an unregistered program is None"
    );
}

/// A key that holds an acting account resolves to it, and its profile is a
/// user or an active program account.
pub fn a_held_key_resolves_to_an_acting_account<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let number = fixture.account(&host, b"held key");
    let other = fixture.account(&host, b"other held key");
    assert_ne!(number, other, "identity: two keys, two accounts");
    for (key, holder) in [(&b"held key"[..], number), (&b"other held key"[..], other)] {
        assert_eq!(
            account_of::<F>(&host, Query::Account(key.to_vec())),
            Some(holder),
            "identity: Account(key) answers the account that key holds"
        );
    }
    let kind = profile::<F>(&host, number).map(|p| p.kind);
    assert!(
        matches!(
            kind,
            Some(Kind::User)
                | Some(Kind::Program {
                    standing: Standing::Active,
                    ..
                })
        ),
        "identity: an account a key acts as is a user or an active program account, not {kind:?}"
    );
}

/// A key whose account does not act is refused `unauthorized`, so the host
/// rejects its frame; its profile says why.
pub fn an_account_that_does_not_act_is_refused<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let number = fixture.stopped(&host, b"stopped key");
    let asked = Query::Account(b"stopped key".to_vec());
    match query::<F>(&host, asked) {
        Err(refused) => assert_eq!(
            refused.code,
            code::UNAUTHORIZED,
            "identity: Account of a key whose account does not act is refused unauthorized"
        ),
        Ok(reply) => panic!(
            "identity: Account of a key whose account does not act is refused, not {reply:?}"
        ),
    }
    let kind = profile::<F>(&host, number).map(|p| p.kind);
    assert!(
        matches!(kind, Some(Kind::Program { standing, .. }) if standing != Standing::Active),
        "identity: an account that does not act is a program account and not active, not {kind:?}"
    );
}

/// A key its account no longer holds resolves to no account.
pub fn a_dropped_key_holds_nothing<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let number = fixture.account(&host, b"dropped key");
    fixture.drop_key(&host, number, b"dropped key");
    assert_eq!(
        account_of::<F>(&host, Query::Account(b"dropped key".to_vec())),
        None,
        "identity: Account of a key removed from account {number} is None"
    );
}

/// A revoked program account's keys never act as it again: `Account` of
/// one is `None` (the keys dropped) or refused `unauthorized` (the account
/// does not act), and its profile is `Revoked`.
pub fn a_revoked_account_s_keys_stop_resolving<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let number = fixture.managed(&host, b"agent key");
    let asked = Query::Account(b"agent key".to_vec());
    assert_eq!(
        account_of::<F>(&host, asked.clone()),
        Some(number),
        "identity: Account of an active program account's key answers it"
    );
    fixture.revoke(&host, number);
    match query::<F>(&host, asked) {
        Ok(Reply::Account(None)) => {}
        Err(refused) if refused.code == code::UNAUTHORIZED => {}
        other => panic!(
            "identity: Account of a key of revoked account {number} is None or refused \
             unauthorized, not {other:?}"
        ),
    }
    let kind = profile::<F>(&host, number).map(|p| p.kind);
    assert!(
        matches!(
            kind,
            Some(Kind::Program {
                standing: Standing::Revoked,
                ..
            })
        ),
        "identity: a revoked account's profile is a program account and Revoked, not {kind:?}"
    );
}

pub fn a_program_runs_only_the_program_account_whose_script_calls_it<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    for name in ["harness", "chat"] {
        register::<F>(&host, Origin::Root, None, name).unwrap();
    }
    let user = fixture.account(&host, b"user key");
    let agent = fixture.managed(&host, b"agent key");
    let runs = |program: &str, account| {
        query::<F>(
            &host,
            Query::Runs {
                program: program.into(),
                account,
            },
        )
    };
    let refused = |program: &str, account| match runs(program, account) {
        Err(_) => {}
        other => panic!("identity: Runs {{ {program}, {account} }} is refused, not {other:?}"),
    };
    refused("harness", agent);
    refused("harness", user);
    fixture.script(&host, agent, "harness");
    match runs("harness", agent) {
        Ok(Reply::Account(Some(number))) if number == agent => {}
        other => panic!(
            "identity: Runs of the program account {agent} whose script calls harness answers it, not {other:?}"
        ),
    }
    let kind = profile::<F>(&host, agent).map(|p| p.kind);
    assert!(
        matches!(&kind, Some(Kind::Program { script: Some(script), .. }) if script.calls == "harness"),
        "identity: the profile of an account whose script calls harness names it, not {kind:?}"
    );
    refused("chat", agent);
    fixture.revoke(&host, agent);
    refused("harness", agent);
}

/// `Profile` of a number no account has is `None`.
pub fn an_absent_profile_is_none<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    fixture.account(&host, b"one");
    let past = every_profile::<F>(&host, 50)
        .last()
        .map_or(1, |p| p.number + 1);
    assert_eq!(
        profile::<F>(&host, past),
        None,
        "identity: Profile({past}), past every account, is None"
    );
}

/// `Profiles` pages ascend by number, hold at most `limit`, and `next`
/// resumes where a page ended; each agrees with `Profile(number)`.
pub fn profiles_page_ascending<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    let made: Vec<AccountNumber> = (0..5u8)
        .map(|n| fixture.account(&host, &[b'k', n]))
        .collect();
    let by_two = every_profile::<F>(&host, 2);
    for number in &made {
        assert!(
            by_two.iter().any(|p| p.number == *number),
            "identity: Profiles lists account {number}"
        );
    }
    assert_eq!(
        every_profile::<F>(&host, 1),
        by_two,
        "identity: Profiles lists the same accounts whatever the limit"
    );
    for listed in &by_two {
        assert_eq!(
            profile::<F>(&host, listed.number).as_ref(),
            Some(listed),
            "identity: Profile({}) agrees with Profiles",
            listed.number
        );
    }
}

/// Every `Kind::System(id)` profile is the account `OfProgram(id)` answers.
pub fn program_profiles_agree_with_of_program<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    for name in ["probe", "other"] {
        register::<F>(&host, Origin::Root, None, name)
            .unwrap_or_else(|e| panic!("identity: the system registers {name}: {e:?}"));
    }
    fixture.account(&host, b"user");
    let listed = every_profile::<F>(&host, 50);
    for name in ["probe", "other"] {
        assert!(
            listed
                .iter()
                .any(|p| matches!(&p.kind, Kind::System(id) if id == name)),
            "identity: Profiles lists the registered program {name}"
        );
    }
    for listed in listed {
        if let Kind::System(id) = &listed.kind {
            assert_eq!(
                account_of::<F>(&host, Query::OfProgram(id.clone())),
                Some(listed.number),
                "identity: OfProgram({id}) is the account whose profile names it"
            );
        }
    }
}

/// `Profiles { limit: 0 }` is a page, neither a refusal nor a panic: empty
/// (`next` then `None`), or the program's capped page, the listing's first
/// profiles with `next` as any page has it.
pub fn profiles_of_limit_zero_is_a_page<F: Fixture>(fixture: &F) {
    let host = fixture.host();
    for n in 0..3u8 {
        fixture.account(&host, &[b'z', n]);
    }
    let all = every_profile::<F>(&host, 50);
    let asked = Query::Profiles {
        after: None,
        limit: 0,
    };
    let answer = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        query::<F>(&host, asked.clone())
    }))
    .unwrap_or_else(|_| panic!("identity: {asked:?} is a page, not a panic"));
    let (profiles, next) = match answer {
        Ok(Reply::Profiles { profiles, next }) => (profiles, next),
        other => panic!("identity: {asked:?} answers Profiles, not {other:?}"),
    };
    let expected = match profiles.last() {
        None => None,
        Some(last) => (profiles.len() < all.len()).then_some(last.number),
    };
    assert!(
        all.starts_with(&profiles) && next == expected,
        "identity: {asked:?} answers empty with no `next`, or the listing's first profiles \
         with `next` the last of them while more remain; not {profiles:?} with next {next:?} \
         of {} profiles",
        all.len()
    );
}
