//! The programs role: which code runs at a height. [`run`] checks every
//! rule below.

use abi::Roles;
use abi::role::programs::{Entry, Genesis, Query, Reply, View};
use program::{BlobId, Error, MockHost, Program, ProgramId};

use crate::{ask, init, same_bytes};

/// What the suite needs from the program beyond the role: code it may
/// schedule and scheduling changes, which the role leaves to it. Each op is
/// sent at `height`; a change lands at `at`.
pub trait Fixture {
    type Program: Program;

    /// Makes `body` code the program may schedule; its blob id.
    fn hold(&self, host: &MockHost, body: &[u8]) -> Result<BlobId, Error>;

    /// Schedules `entry` to run from `at`.
    fn set(&self, host: &MockHost, height: u64, at: u64, entry: Entry) -> Result<(), Error>;

    /// Schedules `program` to stop at `at`.
    fn remove(&self, host: &MockHost, height: u64, at: u64, program: &str) -> Result<(), Error>;

    /// The views the program lists at `height`, apart from its programs.
    fn views(&self, host: &MockHost, height: u64) -> Vec<View>;

    fn set_hook(&self, host: &MockHost, height: u64, at: u64, program: &str) -> Result<(), Error>;

    fn remove_hook(
        &self,
        host: &MockHost,
        height: u64,
        at: u64,
        program: &str,
    ) -> Result<(), Error>;
}

/// Every programs rule, each on a fresh host.
pub fn run(fixture: &impl Fixture) {
    the_role_is_the_programs_bytes(fixture);
    genesis_programs_run_and_views_are_apart(fixture);
    genesis_roles_are_answered(fixture);
    a_set_lands_at_its_height_and_not_before(fixture);
    a_remove_drops_at_its_height(fixture);
    a_hook_lands_at_its_height_and_goes_with_its_program(fixture);
}

fn program() -> String {
    MockHost::role(abi::role::PROGRAMS)
}

fn entry(program: &str, code: BlobId) -> Entry {
    Entry {
        program: program.into(),
        code,
        params: vec![],
    }
}

fn lens() -> View {
    View {
        name: "lens".into(),
        view: BlobId::Sha256([9; 32]),
    }
}

fn roles() -> Roles {
    abi::role::REQUIRED
        .iter()
        .map(|role| (*role, "a"))
        .collect()
}

fn founded<F: Fixture>() -> MockHost {
    let host = MockHost::default();
    let genesis = Genesis {
        programs: vec![
            entry("b", BlobId::Sha256([2; 32])),
            entry("a", BlobId::Sha256([1; 32])),
        ],
        views: vec![lens()],
        hooks: vec!["a".into()],
        roles: roles(),
    };
    init::<F::Program>(&host, &program(), &genesis);
    host
}

fn at<F: Fixture>(host: &MockHost, height: u64) -> Vec<Entry> {
    let asked = Query::At(height);
    let from = height.saturating_sub(1).max(1);
    let mut entries = match ask::<F::Program, Reply>(host, &program(), from, &asked) {
        Ok(Reply::Programs(entries)) => entries,
        other => panic!("programs: At({height}) answers Programs, not {other:?}"),
    };
    entries.sort_by(|a, b| a.program.cmp(&b.program));
    entries
}

fn programs<F: Fixture>(host: &MockHost, height: u64) -> Vec<ProgramId> {
    at::<F>(host, height)
        .into_iter()
        .map(|e| e.program)
        .collect()
}

pub fn the_role_is_the_programs_bytes<F: Fixture>(_: &F) {
    type M<F> = <F as Fixture>::Program;
    let m = program();
    same_bytes::<<M<F> as Program>::Query>(&m, "Query", &Query::At(5));
    same_bytes::<<M<F> as Program>::Query>(&m, "Query", &Query::Hooks(5));
    let reply = Reply::Programs(vec![entry("a", BlobId::Sha256([1; 32]))]);
    same_bytes::<<M<F> as Program>::Reply>(&m, "Reply", &reply);
    let hooks = Reply::Hooks(vec!["a".into()]);
    same_bytes::<<M<F> as Program>::Reply>(&m, "Reply", &hooks);
}

fn hooks<F: Fixture>(host: &MockHost, height: u64) -> Vec<ProgramId> {
    let asked = Query::Hooks(height);
    let from = height.saturating_sub(1).max(1);
    let mut hooks = match ask::<F::Program, Reply>(host, &program(), from, &asked) {
        Ok(Reply::Hooks(hooks)) => hooks,
        other => panic!("programs: Hooks({height}) answers Hooks, not {other:?}"),
    };
    hooks.sort();
    hooks
}

pub fn a_hook_lands_at_its_height_and_goes_with_its_program<F: Fixture>(fixture: &F) {
    let host = founded::<F>();
    assert_eq!(hooks::<F>(&host, 1), ["a"], "programs: genesis hooks a");
    fixture
        .set_hook(&host, 1, 5, "b")
        .unwrap_or_else(|e| panic!("programs: scheduling b's hook at 5: {e:?}"));
    fixture
        .remove_hook(&host, 1, 6, "a")
        .unwrap_or_else(|e| panic!("programs: scheduling a's unhook at 6: {e:?}"));
    fixture
        .remove(&host, 1, 7, "b")
        .unwrap_or_else(|e| panic!("programs: scheduling b's removal at 7: {e:?}"));
    assert_eq!(
        hooks::<F>(&host, 4),
        ["a"],
        "programs: b is no hook before 5"
    );
    assert_eq!(hooks::<F>(&host, 5), ["a", "b"], "programs: b hooks from 5");
    assert_eq!(hooks::<F>(&host, 6), ["b"], "programs: a unhooks at 6");
    assert_eq!(
        hooks::<F>(&host, 7),
        Vec::<ProgramId>::new(),
        "programs: a program dropped is no hook"
    );
}

/// `At` answers every founding program as founded, and no view: views are
/// listed apart.
pub fn genesis_programs_run_and_views_are_apart<F: Fixture>(fixture: &F) {
    let host = founded::<F>();
    assert_eq!(
        at::<F>(&host, 1),
        [
            entry("a", BlobId::Sha256([1; 32])),
            entry("b", BlobId::Sha256([2; 32]))
        ],
        "programs: At answers the founding programs, and no view"
    );
    assert_eq!(
        fixture.views(&host, 1),
        [lens()],
        "programs: the founding view is listed apart"
    );
}

pub fn genesis_roles_are_answered<F: Fixture>(_: &F) {
    let host = founded::<F>();
    match ask::<F::Program, Reply>(&host, &program(), 1, &Query::Roles) {
        Ok(Reply::Roles(answered)) => assert_eq!(
            answered,
            roles(),
            "programs: Roles answers the founding's table"
        ),
        other => panic!("programs: Roles answers Roles, not {other:?}"),
    }
}

/// Held code scheduled for a height runs from that height, not
/// before: a new program is admitted, a running one's code swapped.
pub fn a_set_lands_at_its_height_and_not_before<F: Fixture>(fixture: &F) {
    let host = founded::<F>();
    let code = fixture
        .hold(&host, b"new code")
        .unwrap_or_else(|e| panic!("programs: holding code: {e:?}"));
    for program in ["o/new", "a"] {
        fixture
            .set(&host, 1, 5, entry(program, code))
            .unwrap_or_else(|e| panic!("programs: scheduling {program} at 5: {e:?}"));
    }
    assert_eq!(
        at::<F>(&host, 4),
        [
            entry("a", BlobId::Sha256([1; 32])),
            entry("b", BlobId::Sha256([2; 32]))
        ],
        "programs: nothing scheduled at 5 runs at 4"
    );
    for height in [5, 6] {
        assert_eq!(
            at::<F>(&host, height),
            [
                entry("a", code),
                entry("b", BlobId::Sha256([2; 32])),
                entry("o/new", code)
            ],
            "programs: at {height}, the set scheduled at 5 runs"
        );
    }
}

/// A program scheduled to stop at a height runs until it, and not from it,
/// even once the program has run past it.
pub fn a_remove_drops_at_its_height<F: Fixture>(fixture: &F) {
    let host = founded::<F>();
    fixture
        .remove(&host, 1, 6, "b")
        .unwrap_or_else(|e| panic!("programs: scheduling b's removal at 6: {e:?}"));
    assert_eq!(
        programs::<F>(&host, 5),
        ["a", "b"],
        "programs: b runs until 6"
    );
    assert_eq!(
        programs::<F>(&host, 6),
        ["a"],
        "programs: b is dropped at 6"
    );
    let later = fixture
        .hold(&host, b"later")
        .unwrap_or_else(|e| panic!("programs: holding code: {e:?}"));
    fixture
        .set(&host, 7, 9, entry("o/c", later))
        .unwrap_or_else(|e| panic!("programs: scheduling c at 9 from 7: {e:?}"));
    assert_eq!(
        programs::<F>(&host, 7),
        ["a"],
        "programs: b stays dropped once the program has run past 6"
    );
    assert_eq!(
        fixture.views(&host, 7),
        [lens()],
        "programs: removing a program leaves the views"
    );
}
