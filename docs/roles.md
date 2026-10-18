# Roles, accounts and updates

The kernel knows the programs it calls by the role each fills, never by
their ids. Each founding program names the roles it fills, and the programs
role keeps the table from then on; every call the kernel makes goes to the
program that fills the role, speaking the role's interface in `crates/sdk/abi`
(`abi::role::{programs, validators, identity, awaits, halo, scheduler,
access}`) or, for blobs, ducktape's `blob::contract`. The programs in
`crates/system/` are one implementation of each. The scheduler role
(`abi::role::scheduler`) is optional: on a network that fills none,
nothing scheduled runs. The egress role (`abi::role::egress`) is optional and
the kernel never calls it: a node reads it, and `crates/app/egress` fills it.

## The roles

| Role | Filled here by | What the kernel does with it |
|---|---|---|
| `programs` | `programs` | at founding, writes its params (`role::programs::Genesis`: every founding program and view, the founding hooks and the role table the founding programs declare, `roles`); at each block, before any frame, asks `At(height)` and admits, swaps or drops programs to match the answer, then asks `Hooks(height)` for the programs it tells of each frame the block carries ("Hooks", below). Whenever the roster changes it asks `Roles` and keeps the answer as the network's role table, which every `Env.roles` carries; it refuses an answer that leaves a required role unfilled or names a program that is not running, and keeps the table it had |
| `validators` | `valset` | at founding, writes its params (`role::validators::Genesis`: the founding validators and the member cap, the most members it may ever hold, 1 to `MAX_MEMBERS` = 128); at founding and on the last block of each epoch, asks `Validators` and `Members`: it seats the Validators for the next epoch and tracks the Members (a Resident follows the chain but never votes or proposes); a Members answer past the cap halts the network there. The node (`ducktape join`, `invite`, `vote`, `halo`, `leave`) signs the role's `Op` with its identity key. Each member registers the halo address other members dial it at (`Member.halo`, host:port): `Enroll` and `SetHalo` refuse an unspecified or loopback host as `invalid_input` |
| `identity` | `identity` | as it admits a program (at founding, and later off the programs role), executes `RegisterProgram { program }` with the `System` origin; for each signed frame asks `Account(key)`; for each message and reply asks `OfProgram(sender)`, and for a message a program sends as another account asks `Runs { program, account }` |
| `awaits` | `awaits` | when a frame parks waiting on calls, sends `Park { frame, program, calls }` with the `System` origin, and `Close { frame }` when the frame finishes or the programs role changes its code; the role opens a ticket per call, and a `Resolve { ticket, answer }` from the called program makes it hand the frame its answer through the host's `Resume`; a ticket has no deadline. A refusal of `Park` fails the frame |
| `scheduler` | `scheduler` | at the end of each block, after its frames and hooks and before the epoch step, asks `Due { height }` page by page (no limit: every entry) and, for each entry still due (`Entry { id }` asked again first, so a run that cancels a later entry of the same block holds), runs `payload` on `target` as the program that scheduled the entry would send a message: `Origin::Program(scheduler)`, that program's account, `Cause::Scheduled { entry }`, the network's per-frame limits; a refused run undoes only its own writes. It then sends `Ran { entry, receipt }` with the `System` origin, and the role keeps the receipt on the entry; the block does not carry it. A program sends `Schedule { height, target, payload }` (the SDK's `ctx.schedule`), answered with the entry id: the role takes the scheduler from the origin, so a program schedules only as itself, and refuses a height not after the current one, so a scheduled run never lands in its own block. Only the scheduling program sends `Cancel { entry }`, before the run |
| `egress` | `egress` | nothing: the node reads it. After each block a member node asks `Assigned { key }` with its own key and `Node { key }`; it calls each request elected to it, through the services its `egress.toml` names, and signs `Answer { id, answer }` (for a request that asks `Delivery::Events`, a `text/event-stream` response's events instead, each as `Event { id, seq, data }` once a block carried the one before, then `EndStream { id, events, end }`), and `Serve { services }` while the role's record of it differs from its config. A program asks `Serving { service }` for the member nodes that would be elected, and calls the role with `Request { request, delivery, wait }`. An events request's requester hears each event as its own op, `deliver` followed by the encoded `Event`, and its call answers the stream's end: `Complete` as the encoded `http::Response` with the stream's status and headers and an empty body (what a whole `Http` reply answers too, so the call decodes one type either way), a cut as a refusal; each event moves the request's expiry `wait` blocks past it. `seq` counts from 0 per call the node makes: a repeated `seq` is a repeat, a `seq` back at 0 is the node calling the service again after a restart and voids what came before; egress passes it through unchecked. A relay is answered whole, and asking one with `Delivery::Events` is refused. The egress program expires a request itself: it schedules its own `Expire { id }` through the scheduler role `wait` blocks on (`DEFAULT_WAIT`, 64, when `wait` is `None`) and cancels it when the answer comes; no node signs an expiry |
| `blobs` | `blobs` | when a program reads a blob (`blob_get`) that no program put, and when the programs role admits code, asks `Held { blob }`: the blob is served only once a bundle naming it is held, which it is from the receipt that brings two thirds of the current validators. The node watches the role's writes: it gathers every bundle put, and a validator's node receipts a bundle once it holds every blob |
| `access` | `access` | before every op it executes (`ProgramCall::Execute`; not a reply to a program's message and not a run by the chain itself), asks the target program `ProgramCall::Access(payload)`, which answers what the op needs: `Vec<Need>`, each `Need { key, action }` with `Action::Write` or `Action::Delete`. With any need, it runs the role's `Op::Check { program, actor, needs }` as the chain, in the same unit as the op: the check may send no message, and its refusal refuses the op. `actor` is the sender's account and, for a message from a program, that program too (`Party::Account(n)`, `Party::Program(id)`). "Who may write a key", below, has the rules |
| `halo` | `halo` | at founding, writes its params (`role::halo::Genesis`: the founding lanes, given ids in order from 0, each `Founded { name, cap, pinned }`); at boot and after any block that writes the role's state, the node asks `Lanes` and serves halo's lanes by that table: a lane's name to its wire id, the class its streams are served at most, whether it is served, and whether it is pinned. Only the validators' vote (`Motion::Role { role: "halo", op }`) changes it: `Open` lists a name at the next id, unpinned, `Set` changes its cap or pauses it, `Close` unlists it, and no id is handed out twice. A pinned lane is never paused or closed (`invalid_input`, refused as the vote enacts it): this repo's founding pins `ducktape/blobs`, over which a node fetches the blobs it needs to apply a block, so no vote can cut a node off from the block that would undo it |

Each `[[programs]]` entry of the founding file names the roles its program
fills in `roles`; a program that fills none leaves the key out
(`crates/sdk/founding/founding.toml` is the template):

```toml
[[programs]]
id = "valset"
code = "valset.wasm"
roles = ["validators"]

[[programs]]
id = "chat"
code = "chat.wasm"
```

The roles are a table, not a fixed set of fields: `abi::Roles` maps a role's
name to its program, and the names are constants in `abi::role` (`PROGRAMS`,
`VALIDATORS`, `IDENTITY`, `BLOBS`, `AWAITS`, `HALO`, `EGRESS`, `SCHEDULER`,
`ACCESS`). A new role is a new row. The programs role keeps the table (its
`ROLES` map) and answers it with `Query::Roles`. The roles in
`abi::role::REQUIRED` (programs, validators, identity, blobs, awaits, halo
and access) are filled for the network's life: the kernel calls programs,
validators, identity, blobs, awaits and access, and every node reads halo's
lane table. Any other role, egress and scheduler here, is optional. Founding
refuses a role two programs name and a founding that leaves a required role
unfilled, naming it; a node refuses a network that records no roles. Only
the validators' vote changes the table, by publishing a program with the
roles it fills (`Change::Publish`, "Updating a program", below). Every
frame's `Env` carries the table (`env.roles`), so a program asks a role by
its name, `ctx.env().role(abi::role::IDENTITY)?`, never by a program id it
assumes; chat's roster does this (`crates/app/chat/src/origin.rs`).
`Env::role` refuses a role the network does not fill as `unsupported`;
`env.roles.get(name)` answers `None` for one.

## Who a frame acts as

The host resolves the sender once per frame, through the identity role, and
hands it to the program as `Env.sender`:

- a signed frame acts as the account its key holds (`Account(key)`); a key
  that holds none still runs, as no one (`sender: None`), which is how
  identity's own `Create` works; a key whose account does not act (a
  suspended program account) is refused, and the host rejects the frame;
- a message or a reply from another program acts as that program's account
  (`OfProgram(program)`);
- a message a program sends as another account (`ctx.emit_as(account, ..)`,
  `Message.acting`) acts as that account when identity answers
  `Runs { program, account }` with it: a program account whose script calls
  that program, while the account is active. Any other answer, or a refusal,
  rejects the message, so no key is held anywhere for an account a program
  runs;
- founding and the kernel's own calls act as the chain (`Root`).

A program reads it as `ctx.sender()`, which is `Principal::Account(number)`
or `Principal::Root` and refuses a frame that acts as no one, so no row a
program writes names a bare key. `Env::signer`, `sending_program` and
`sent_by` check the raw origin instead, where a program needs the key or the
program itself (forge requires a signed frame; chat's `:` namespace belongs to
the program its prefix names).

## Accounts

Everything that acts is an account in `identity`:
`Account { number, card, control }`. The `Card` is what it shows (name,
avatar, bio); the `Control` is who acts as it:

- `User { keys }`: a person and the keys they hold.
- `Program { manager, category, life, script }`: a program account. Its
  manager, a user, declares its `category` (`Agent`), gives it keys, sets
  its `script`, and decides its `Life`: `Active { keys }`,
  `Suspended { keys }` (keys kept, frames refused) or `Revoked` (keys
  dropped, final). A `Script { calls, config }` names the program that runs
  the account's actions and hands it `config`, text only that program gives
  a meaning: harness reads it as the path of the agent's configuration file
  in `files`. While the account is active, `attribution` hands every change
  about it to the program `calls` names, once that program registered with
  it (`Register { deliver }`); a change about any other account reaches no
  program. Not every program account is an agent; `Agent` is the one
  category so far.
- `System { program }`: a program's own account, registered by the kernel
  as it admits the program. It holds no keys; the program alone acts as it.

Who may do what (the acting account is `ctx.sender()`):

| Op | Who |
|---|---|
| `RegisterProgram { program }` | the system (`Root` origin) alone; a program that has an account keeps it |
| `Create { name, scheme }` | a key that holds no account; it becomes a user's first key |
| `CreateAgent { name }` | a user; they become the manager of a new program account of the `Agent` category, with no key and no script |
| `AddKey` | a user's account: the new key signs the frame and a key already on the account consents. A program account's: its manager signs and the new key consents to itself. Never a system account's or a revoked program account's |
| `RemoveKey` | a user removes their own key or a junior one, never their last. A program account's manager removes any of its keys |
| `SetName`, `SetProfile` | whoever may write the account's key in the access program, `identity::account_key(n)`: its owner (a user or a system account itself, a program account's manager) and the parties the owner lists there. Not on a revoked program account |
| `Suspend`, `Resume`, `Revoke` | a program account's, by whoever may write its key: its manager and the parties the manager lists |
| `SetScript { account, script }` | a program account's, by whoever may write its key, not once revoked. `Some(Script { calls, config })` names in `calls` a program that has an account; that program then sends messages as the account (`Runs`) and reads `config`. `None` takes it back |
| `SetOwnership { account, ownership }` | no one: always refused. Whom an agent answers is its list in the access program at `harness::agent_key(agent)` |

The role's queries, which views and other programs ask too, are
`Account(key)`, `OfProgram(program)`, `Profile(number)`,
`Profiles { after, limit }` and `Runs { program, account }`. A `Profile` is
`{ number, name, kind }`, where `Kind` is `User`,
`Program { manager, category, standing, script }` or `System(id)`. `Kind::badge` and `Kind::note` in `abi` turn it into what a view
shows beside a name ("Agent · managed by Dev", "Program · chat", "suspended"),
the one place that happens.

## Who may write a key

The `access` program (`crates/system/access`) fills the access role. It
keeps who may write each key another program names, and the kernel asks it
before an op runs ("The roles", above). A program keeps its owners itself:
the access program asks the program `ProgramCall::Resource(key)` (through
the host op `HostOp::Resource { program, key }`, `ctx.resource(program,
key)` in `program`), and the program answers `Resource { owner, default }`
(`owner: Option<Party>`, `default: Option<Ownership>`) from facts it
already stores. A program writes these in `Program::access` and
`Program::resource`.

A list is an `Ownership`: `Owner` (the key's owner alone), `Listed(parties)`
(the owner and the parties listed, each a `Party::Account(number)` or a
`Party::Program(id)`) or `Anyone`. A list holds account numbers and program
ids, never names, so renaming an account changes nothing it allows.

- A write need on a key is allowed by the nearest list at or above it. The
  key and its `/`-separated ancestors are read in turn, the key itself
  first; at each, the list set on it in the access program counts, else
  the `default` its program answers for it. With neither anywhere, anyone
  writes it: a network allows every key nobody lists and whose program
  gives no default.
- A delete need is allowed to whoever the write rule allows, and to the
  owner of any ancestor. A delete drops every list at and below the key.

| Op | Who |
|---|---|
| `Check { program, actor, needs }` | the kernel alone (`Root`); judges each need and refuses the first it does not allow |
| `Set { program, key, list }` | the key's owner, or `program` itself for its own keys. `Some(list)` sets the key's list (each listed account and program must exist in identity), `None` clears it |
| `Move { program, from, to }` | `program` alone: moves every list at and below `from` to the same place under `to`, when a path is renamed |

Its queries: `List { program, key }` (the list set on the key, if any),
`Resource { program, key }` (the program's answer for the key),
`Writers { program, key }` (who writes it: `Writers::Anyone`, or
`Writers::Listed { at, owner, list }`, the list that decides and where) and
`Lists { program, page }` (every list a program's keys carry). Its state is
one map, `LISTS` (prefix `l/`), from `(program, key)` to `Ownership`.

What each program asks:

| Program | Key | Ops that need it | Owner, default |
|---|---|---|---|
| `files` | the path; `files::pin_key(name)` for a pin | `Put`, `Mkdir`, `Symlink` write the path, `Remove` deletes it, `Move` writes both ends (and moves the lists with `access::Op::Move`); `Unpin` deletes the pin's key | the path's creator in the head tree, and for `/home/<name>` the one account named `<name>`; no default. A pin's owner is the account that made it, default `Owner` |
| `forge` | `forge::repo_key(repo)`, `forge::settings_key(repo)`, `forge::change_key(repo, n)` | `Push`, `Merge`, `Stage`, and `ChangeClose` by anyone but the change's author, write the repository's key; `Configure` writes its settings key; `ChangeEdit` writes the change's key | the repository's owner, the change's author; default `Owner` |
| `identity` | `identity::account_key(n)` | `SetName`, `SetProfile`, `Suspend`, `Resume`, `Revoke`, `SetScript` write it | a user or a system account itself, a program account's manager; default `Owner` |
| `chat` | the channel id; `chat::message_key(channel, seq)` | `RenameChannel`, `SetChannelArchived`, `SetMembership` write the channel; `EditMessage` writes the message, `DeleteMessage` deletes it | the channel's owner, the message's author; default `Owner` |
| `egress` | `egress::service_key(name)`, `services/<name>` | `Assign` writes it | the service's owner, default `Owner` |
| `harness` | `harness::agent_key(agent)`, `harness::conversation_key(agent)`, `harness::thread_key(thread)`, `harness::model_key(id)` | `NewThread` and `ForkThread` write the agent's conversation key; `Close` writes the thread; `SetModel` of a model that exists writes it, `RemoveModel` deletes it. No op needs the agent's key: its list says whom the agent answers | the agent's manager (agent, conversation, thread), the model's owner; default `Owner` |

Keys, governance and signatures stay in their own programs: identity's
keys, valset's votes and the programs role's grants.

## Updating a program

`programs` holds the roster and the changes scheduled to it:

1. `ducktape blob put FILE --kind program --wait` uploads the code to the
   node, which submits the blobs role's `Put` naming it. Every member gathers
   it; each validator's node receipts it once it holds it; the command
   returns once two thirds of the current validators have receipted it, and
   the blobs role answers `Held` for it. The programs role refuses to schedule code
   the blobs role does not hold, and a held blob of any kind but
   `"program"`.
2. `Schedule(Scheduled { height, change: Change::Set(Entry { program, code, params }) })`
   lands it at a later height.
3. At that height the kernel's `At(height)` sees the new code. Under an id
   already running, the code is swapped: the state, the account and the id
   stay, and `init` does not run again. Under a new id, the program is
   admitted (its `init` gets `params`) and registered with identity.

A change lands at its height whether or not an op runs on the programs role
after it: the kernel asks `At(height)` for the block it applies, and a
client (the app's rail, Explorer) asks `Roster`, the programs and views at
the query's own height, with that height.

`Change::Remove` drops a program and the roles it fills; `Cancel`
withdraws a scheduled change. A role's program updates the same way: a
`Set` swaps its code and leaves its roles alone.

`Change::Publish { entry, roles }` is scheduled like a `Set`: from its height
the program runs `entry` and fills exactly `roles`. It gives up any role it
filled that the list leaves out, and a role the list names moves to it from
the program that filled it. A role name is 1 to 64 of `[a-z0-9_-]`, named
once per change. No scheduled change may leave a role in
`abi::role::REQUIRED` unfilled, or leave a role naming a program that no
entry runs at that height. Only the validators' vote publishes with roles,
and a program that fills any role is changed only by the vote.

## Hooks

A hook is a program the kernel tells of every frame a block carries. The
programs role lists the hooks: the founding file's top-level `hooks = [...]`
seeds them, `Change::SetHook(id)` makes a running program one from its
height, and `Change::RemoveHook(id)` stops it there. `Change::Remove` of a
program drops its hook with it. At each block the kernel asks
`Hooks(height)`; a programs role that refuses or answers something else names
no hooks.

After each frame runs, the kernel calls each hook with a `hook::Record`:
the block height, the frame's place in the block, its signer, the account
it acted as, its target and whether it applied. A refused frame is told
too. The call's origin is the kernel itself (`Origin::Root`, from
`Env::kernel()` in `program`), and `program::hook` refuses any other. A hook
never changes a frame: its receipt stays as it ran, a hook that refuses,
traps, runs out of fuel or emits a message keeps none of its writes, and
the block goes on. `activity` (`crates/app/activity`) is the hook this
repo founds: it lists every frame newest first, and indexes who signed
what, by key and by account.

Who may schedule or cancel (putting a blob is open to any account):

- The validators' vote, any change: `Env::authority()` in `program` admits
  only a message from the program that fills the validators role, and
  `valset` sends one only when the vote passes a `Motion::Role { role:
  "programs", op }`: the enacting vote emits `op` to the programs role in the
  same frame; a refusal there undoes the programs' writes, the vote stands,
  and the motion is gone.
- A maintainer: a signed frame whose account holds a `Grant` of the id
  (`Scope::Program(id)`, which may run nothing yet) or of every program
  (`Scope::All`). It sets, publishes with an empty role list, removes and
  cancels changes of the ids it covers, never the id of a program that
  fills a role, and its change lands at least `NOTICE` (60)
  blocks after the block it is made in, so the vote can cancel it first.
  It may not change an id the vote has a change waiting on, nor claim an
  id a maintainer's waiting removal frees (a program under a view being
  removed, or the reverse) before that removal lands, and it cancels only a
  grant's change.
  `Scheduled` answers who made each change (`Pending.by`, `None` for the
  vote).
- Hooks: a maintainer may replace a hook's code (`Set` of a hooked id).
  The hook keeps its status: it still cannot emit and runs on the kernel's
  capped hook fuel. Only the vote sets, unsets or removes a hook
  (`SetHook`, `RemoveHook`, `Remove` of a hooked id).

Only the vote grants (`Op::Grant`, to a user or a program account identity
profiles, never a system account); `Op::Revoke` is the vote's or the
holder's own, and drops every change the account made that it no longer
covers. The vote wins: its change to an id takes every change a grant
made to that id with it, and its removal of an id the id's `Program`
grants too. The vote's removal of an id that holds nothing at its height
takes those along and waits as no entry, and its cancel of a change
already gone is no error, so a motion never sits one vote short of a
done thing.

## Membership

`valset` takes `abi::role::validators::Op`, signed by a node's identity
key (`env.signer()`); every other origin is refused.

- `Enroll { address, halo, invite }`: `address` is where peers dial the
  node, or none for a node that only dials out; `halo` is the host:port
  other members dial it at over QUIC, refused as `invalid_input` when its
  host is unspecified (`0.0.0.0`, `::`) or loopback. A member enrolling
  again only moves its addresses. A new key joins as a Resident while the door is open, or with an
  `Invite`: a `Grant { network, nonce, expires }` that names this network,
  has not expired, and is signed under `INVITE_NAMESPACE` over
  `grant.preimage()` by a current validator. Each (issuer, nonce) admits
  once. The door starts closed, and the member cap holds.
- `SetHalo { halo }`: a member moves its halo address, under the same rule.
- `Leave`: the signer leaves; the last validator cannot.
- `Vote(motion)`: a validator votes for `Promote` (a resident),
  `Demote` (a validator), `Remove` (a member), `Door { open }` (the other
  state) or `Role { role, op }` (enacted by emitting `op` to the program
  that fills `role`, which must be filled). valset reads no role's op:
  the role's program judges it when the vote passes, and a refusal there
  undoes that program's writes while the vote stands and the motion is
  gone, so a vote never enacts it again. A motion passes at `n - (n-1)/3`
  of the current validators; a voter no longer a validator is dropped at
  the tally. The output is `Voted::Counted { votes, needed }` or
  `Voted::Enacted`. Removing a member clears the open motions that name it,
  and the last validator is neither demoted nor removed.

## Filling a role with another program

A role is an interface, not a program: any program whose `Op`, `Query` and
`Reply` start with the role's variants, byte for byte, can fill it, named in
its founding entry's `roles` or published with the role by the vote.
`crates/sdk/conformance` checks one (its
README has the steps). What the kernel does with the answers: identity's
`RegisterProgram` is idempotent and only the system sends it, and refusing
it fails a founding or undoes a later admission; a refusal of
`Account(key)` or `OfProgram(program)` rejects the frame or the message.
The programs role's `At(height)` answers every program that runs at that height; one
it leaves out is dropped. A refusal of validators' `Members` stops the
node.
