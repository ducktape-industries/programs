# conformance

Proof that a program fills a role the kernel calls: `programs`,
`validators`, `identity`, `awaits`, `halo` or `scheduler` (`abi::role`). The suite runs your program's
native `program::Program` over `MockHost` and speaks only the role's bytes:
it encodes `abi::role` values, hands them to your program's own decoding,
and reads the answers back as the role's replies. It checks the contract,
not how you keep your state.

## Filling a role

1. Implement `program::Program` (your `NAME`, `Op`, `Query` and `Reply`) and
   `program::Program`. The role's `Op`, `Query` and `Reply`
   variants are your enums' **first** variants, in the same order and with
   the same fields (borsh encodes the variant index, not its name); your
   own follow. A programs or validators program's `init` takes the role's
   `Genesis`.
2. Add `conformance` as a dev-dependency and implement the role's
   `Fixture`: the few things the role leaves to your program's own ops.
3. Call the role's `run` in a test.

```rust
use program::{AccountNumber, MockHost};

struct Mine;

impl conformance::identity::Fixture for Mine {
    type Program = my_identity::MyIdentity;
    fn host(&self) -> MockHost { MockHost::default() }
    /// `key` comes to hold a new account that acts.
    fn account(&self, host: &MockHost, key: &[u8]) -> AccountNumber { todo!() }
    /// `key` comes to hold an account that does not act.
    fn stopped(&self, host: &MockHost, key: &[u8]) -> AccountNumber { todo!() }
    /// `account` (one `account` made) stops holding `key`.
    fn drop_key(&self, host: &MockHost, account: AccountNumber, key: &[u8]) { todo!() }
    /// `key` comes to hold an active program account.
    fn managed(&self, host: &MockHost, key: &[u8]) -> AccountNumber { todo!() }
    /// `account` (one `managed` made) is revoked.
    fn revoke(&self, host: &MockHost, account: AccountNumber) { todo!() }
    /// `account` (one `managed` made) gets a script that calls `program`.
    fn script(&self, host: &MockHost, account: AccountNumber, program: &str) { todo!() }
}

#[test]
fn fills_the_identity_role() {
    conformance::identity::run(&Mine);
}
```

`conformance::env(program, height, origin, sender)` builds the env your
fixture's ops run in. `tests/reference.rs` has the fixtures of the five
reference programs (`identity`, `valset`, `programs`, `awaits`, `halo`, `scheduler`).

## What each role is held to

Each rule is a public function of its role's program, so a failing one
names itself; `run` calls them all, each on a fresh host.

**identity**: the role's `Op`, `Query` and `Reply` are the program's first
variants, byte for byte; `RegisterProgram` is refused `unauthorized` from a
signed frame or a program and writes nothing, and from the system it is
idempotent (one account, `Kind::System`); a signed or program frame is
refused though the kernel gives it a sender (its key's account, the
program's account): only the system's origin registers; `Account` of an unknown key and
`OfProgram` of an unregistered program are `None`; a key that holds an
acting account resolves to it (a user or an active program account); a
key whose account does not act is refused `unauthorized`, and its profile
is a program account and not active; a key dropped from its account (the fixture's
`drop_key`) resolves to `None`; a revoked program account's keys (`managed`,
`revoke`) resolve to `None` or are refused, and its profile is `Revoked`;
`Profile` past every account is `None`;
`Profiles` ascends, holds at most `limit`, `next` is the last number of a
page that has more, the listing is the same whatever the limit and agrees
with `Profile`; `Profiles { limit: 0 }` is a page, empty with no `next` or
the program's capped first page with `next` as any page has it; every `Kind::System(id)` is the account `OfProgram(id)`
answers; `Runs { program, account }` is refused until the account's
manager gives it a script that calls the program (the fixture's `script`),
then answers it and the profile's script names the program; another
program, a user's account and a revoked one are refused.

**validators**: the role's `Query` and `Reply` read as the program's own,
`Validators` and `Members` at tags 0 and 1 and `QueryPolicy` at tag 6;
init with the role's `Genesis` seats exactly its validators, each a member
at its address, and `QueryPolicy` answers its `queries`; every key is 32 bytes and every validator a member;
seating and unseating (the fixture's `seat`/`unseat`) change the answers.

**programs**: `Query::At` and `Reply::Programs` are the program's at tag
0, and `Query::Hooks` and `Reply::Hooks` at tag 5;
`At` answers the founding programs as founded and no view, which the
program lists apart (the fixture's `views`); `Roles` answers the role table
the founding's `Genesis.roles` names; held code scheduled for a
height (the fixture's `hold`/`set`) runs from that height and not
before, admitting a new program or swapping a running one's code; a
program scheduled to stop (`remove`) runs until its height and not from
it, and stays dropped once the program has run past it; `Hooks` answers the
founding hooks, a hook set (`set_hook`) from its height, a hook removed
(`remove_hook`) from its height, and no hook for a program removed.

**awaits**: the role's `Op`, `Query` and `Reply` are the program's first
variants, byte for byte; `Park` and `Close` are refused `unauthorized` from
a signed frame or a program; `Park` opens one ticket per call, opened at the
parking height, and resumes nothing; only the
called program resolves its ticket (anyone else is refused `unauthorized`),
which closes it and resumes its frame with the answer, a refusal included;
a `Resolve` of a ticket no longer open is no refusal and resumes nothing;
`Close` drops that frame's tickets alone and resumes nothing; `Owed` pages a
target's open tickets by `after` and `limit`. The fixture is only the
founded host: the role leaves nothing to the program's own ops.

**scheduler**: the role's `Op`, `Query` and `Reply` are the program's first
variants, byte for byte; `Schedule` is refused `unauthorized` from a signed
frame or the kernel, and `invalid_input` for a height not after the current
one, and a refused one keeps nothing; a program's `Schedule` answers a new
entry id, and the entry names that program as its scheduler; only that
program cancels the entry (anyone else is refused `unauthorized`), which
removes it from `Due`, and a cancel after the run is refused `wrong_state`;
only the kernel sends `Ran` (anyone else is refused `unauthorized`), the
entry keeps the receipt and leaves `Due`, and a second `Ran` is refused;
`Due` lists one height's entries in the order they were made, every one
with no limit, and pages by `limit` and `after`. The fixture is only the
founded host.

**halo**: the role's `Op`, `Query` and `Reply` are the program's first
variants, byte for byte; init with the role's `Genesis` lists its lanes in
its order from id 0, each served and from block 0; `Lanes` answers in id
order and `Lane` by name (`None` for an unlisted one); an op from a signed
frame, the kernel or any program but the one bound to the validators role
is refused `unauthorized` and changes nothing; `Open` lists a name at the
next id, served, from the block that opened it, and refuses a listed name;
`Set` changes the cap and whether the lane is served under the same id, a
paused lane staying listed with `enabled: false`, and refuses an unlisted
name; `Close` unlists a lane and its id is spent: the name opened again
takes the next id; a lane the genesis pins is never paused or closed (both
refused `invalid_input`), while a `Set` that keeps it served moves its cap;
`Open` lists a lane unpinned. The fixture is only the host: the role leaves
nothing to the program's own ops.
