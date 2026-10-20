//! The validators role: the consensus set the kernel seats each epoch.
//! [`run`] checks every rule below.

use abi::role::validators::{Genesis, Member, Query, Reply};
use program::{Error, MockHost, Program};

use crate::{ask, init, same_bytes};

/// What the suite needs from the program beyond the role: its own update
/// path, which the role leaves to it.
pub trait Fixture {
    type Program: Program;

    /// Seats `member` as a validator, by the program's own op.
    fn seat(&self, host: &MockHost, member: Member) -> Result<(), Error>;

    /// Unseats the validator `key`, by the program's own op.
    fn unseat(&self, host: &MockHost, key: &[u8]) -> Result<(), Error>;
}

/// Every validators rule, each on a fresh host.
pub fn run(fixture: &impl Fixture) {
    the_role_is_the_first_variants(fixture);
    genesis_seats_the_given_set(fixture);
    an_update_changes_the_answer(fixture);
    the_cap_bounds_the_members(fixture);
}

fn program() -> String {
    MockHost::role(abi::role::VALIDATORS)
}

fn member(n: u8) -> Member {
    Member {
        key: vec![n; 32],
        address: Some(format!("node-{n}")),
        halo: format!("node-{n}.example.org:9100"),
    }
}

fn founded_with_two_validators<F: Fixture>() -> MockHost {
    let host = MockHost::default();
    let genesis = Genesis {
        validators: vec![member(2), member(1)],
        member_cap: 3,
    };
    init::<F::Program>(&host, &program(), &genesis);
    host
}

fn answers<F: Fixture>(host: &MockHost) -> (Vec<Vec<u8>>, Vec<Member>) {
    let m = program();
    let mut validators = match ask::<F::Program, Reply>(host, &m, 1, &Query::Validators) {
        Ok(Reply::Validators(keys)) => keys,
        other => panic!("validators: Validators answers Validators, not {other:?}"),
    };
    let members = match ask::<F::Program, Reply>(host, &m, 1, &Query::Members) {
        Ok(Reply::Members(members)) => members,
        other => panic!("validators: Members answers Members, not {other:?}"),
    };
    for key in validators.iter().chain(members.iter().map(|m| &m.key)) {
        assert_eq!(key.len(), 32, "validators: a key is 32 bytes: {key:?}");
    }
    for key in &validators {
        assert!(
            members.iter().any(|m| &m.key == key),
            "validators: validator {key:?} is among the Members"
        );
    }
    validators.sort();
    (validators, members)
}

/// The role's queries and replies are the program's first variants, byte
/// for byte.
pub fn the_role_is_the_first_variants<F: Fixture>(_: &F) {
    type M<F> = <F as Fixture>::Program;
    let m = program();
    for query in [Query::Validators, Query::Members] {
        same_bytes::<<M<F> as Program>::Query>(&m, "Query", &query);
    }
    for reply in [
        Reply::Validators(vec![vec![1; 32]]),
        Reply::Members(vec![member(1)]),
    ] {
        same_bytes::<<M<F> as Program>::Reply>(&m, "Reply", &reply);
    }
}

/// Init with the role's genesis seats exactly its validators, each a
/// member at its address.
pub fn genesis_seats_the_given_set<F: Fixture>(_: &F) {
    let host = founded_with_two_validators::<F>();
    let (validators, members) = answers::<F>(&host);
    assert_eq!(
        validators,
        [member(1).key, member(2).key],
        "validators: genesis seats exactly its validators"
    );
    for seated in [member(1), member(2)] {
        assert!(
            members.contains(&seated),
            "validators: genesis member {seated:?} is among the Members"
        );
    }
}

/// Seating and unseating through the program's update path change what
/// Validators and Members answer.
pub fn an_update_changes_the_answer<F: Fixture>(fixture: &F) {
    let host = founded_with_two_validators::<F>();
    fixture
        .seat(&host, member(3))
        .unwrap_or_else(|e| panic!("validators: seating a validator: {e:?}"));
    let (validators, members) = answers::<F>(&host);
    assert_eq!(
        validators,
        [member(1).key, member(2).key, member(3).key],
        "validators: a seated validator is answered"
    );
    assert!(
        members.contains(&member(3)),
        "validators: a seated validator is a member"
    );
    fixture
        .unseat(&host, &member(1).key)
        .unwrap_or_else(|e| panic!("validators: unseating a validator: {e:?}"));
    let (validators, _) = answers::<F>(&host);
    assert_eq!(
        validators,
        [member(2).key, member(3).key],
        "validators: an unseated validator is no longer answered"
    );
}

/// Past the cap genesis names, a newcomer is not seated: the third member
/// fills the cap of three, the fourth errs and Members stays at three.
pub fn the_cap_bounds_the_members<F: Fixture>(fixture: &F) {
    let host = founded_with_two_validators::<F>();
    fixture
        .seat(&host, member(3))
        .unwrap_or_else(|e| panic!("validators: seating the third of three: {e:?}"));
    assert!(
        fixture.seat(&host, member(4)).is_err(),
        "validators: a member past the cap is refused"
    );
    let (_, members) = answers::<F>(&host);
    assert_eq!(members.len(), 3, "validators: the cap bounds the Members");
}
