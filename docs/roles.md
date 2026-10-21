# Roles, accounts and updates

The kernel knows the programs it calls by the role each fills, never by
their ids. Each founding program names the roles it fills, and the programs
role keeps the table from then on; every call the kernel makes goes to the
program that fills the role, speaking the role's interface in ducktape's `abi`
(`abi::role::{programs, validators, identity, awaits, halo, scheduler,
access, ownership}`) or, for blobs, ducktape's `blob::contract`. The programs in
`crates/system/` are one implementation of each. The scheduler role
(`abi::role::scheduler`) is optional: on a network that fills none,
nothing scheduled runs. The egress role (`abi::role::egress`) is optional and
the kernel never calls it: a node reads it, and `crates/app/egress` fills it.
The ownership role (`abi::role::ownership`) is optional: on a network that
fills none, no write is guarded.

## The roles

| Role | Filled here by | What the kernel does with it |
|---|---|---|
| `programs` | `programs` | at founding, writes its params (`role::programs::Genesis`: every founding program and view, the founding hooks and the role table the founding programs declare, `roles`); at each block, before any frame, asks `At(height)` and admits, swaps or drops programs to match the answer, then asks `Hooks(height)` for the programs it tells of each frame the block carries ("Hooks", below). Before it admits, it asks `Renames(height)` and renames in place each `(from, to)` while `from` runs, `to` does not, and `At` lists `to` and not `from` ("Renaming a program", below). Whenever the roster changes it asks `Roles` and keeps the answer as the network's role table, which every `Env.roles` carries; it refuses an answer that leaves a required role unfilled or names a program that is not running, and keeps the table it had |
| `validators` | `valset` | at founding, writes its params (`role::validators::Genesis`: the founding validators and the member cap, the most members it may ever hold, 1 to `MAX_MEMBERS` = 128); at founding and on the last block of each epoch, asks `Validators` and `Members`: it seats the Validators for the next epoch and tracks the Members (a Resident follows the chain but never votes or proposes); a Members answer past the cap halts the network there. `Genesis.queries` is whether the network answers unsigned queries (`QueryPolicy::Signed` or `Open`); each node asks `QueryPolicy` before it answers one, runs it as the empty key (`abi::Origin::unsigned()`), and refuses it unless the answer is `Open`. A network founded before the policy holds no row and answers `Signed`. The node (`ducktape join`, `invite`, `vote`, `halo`, `leave`) signs the role's `Op` with its identity key. Each member registers the halo address other members dial it at (`Member.halo`, host:port): `Enroll` and `SetHalo` refuse an unspecified or loopback host as `invalid_input` |
| `identity` | `identity` | as it admits a program (at founding, and later off the programs role), executes `RegisterProgram { program }` with the `System` origin, and as it renames a program in place ("Renaming a program") first executes `RenameProgram { from, to }`, which moves the program's account to the new id: a refusal leaves the program under its old id for that block; for each signed frame asks `Account(key)`; for each message and reply asks `OfProgram(sender)`, and for a message a program sends as another account asks `Runs { program, account }` |
| `awaits` | `awaits` | when a frame parks waiting on calls, sends `Park { frame, program, calls }` with the `System` origin, and `Close { frame }` when the frame finishes or the programs role changes its code; the role opens a ticket per call, and a `Resolve { ticket, answer }` from the called program makes it hand the frame its answer through the host's `Resume`; a ticket has no deadline. A refusal of `Park` fails the frame. Its `Migrate { height, first, last }`, anyone's, rewrites each open ticket's caller and target by the programs role's `Renames(height)` ("Renaming a program") |
| `scheduler` | `scheduler` | at the end of each block, after its frames and hooks and before the epoch step, asks `Due { height }` page by page (no limit: every entry) and, for each entry still due (`Entry { id }` asked again first, so a run that cancels a later entry of the same block holds), runs `payload` on `target` as the program that scheduled the entry would send a message: `Origin::Program(scheduler)`, that program's account, `Cause::Scheduled { entry }`, the network's per-frame limits; a refused run undoes only its own writes. It then sends `Ran { entry, receipt }` with the `System` origin, and the role keeps the receipt on the entry; the block does not carry it. A program sends `Schedule { height, target, payload }` (the SDK's `ctx.schedule`), answered with the entry id: the role takes the scheduler from the origin, so a program schedules only as itself, and refuses a height not after the current one, so a scheduled run never lands in its own block. Only the scheduling program sends `Cancel { entry }`, before the run |
| `egress` | `egress` | nothing: the node reads it. After each block a member node asks `Assigned { key }` with its own key and `Node { key }`; it calls each request elected to it, through the services its `egress.toml` names, and signs `Answer { id, answer }` (for a request that asks `Delivery::Events`, a `text/event-stream` response's events instead, as `Event { id, seq, data }` in frames sent as the events arrive, without waiting for a block to carry the frame before, then `EndStream { id, events, end }`), and `Serve { services }` while the role's record of it differs from its config. A program asks `Serving { service }` for the member nodes that would be elected, and calls the role with `Request { request, delivery, wait }`. An events request's requester hears each event as its own op, `deliver` followed by the encoded `Event`, and its call answers the stream's end: `Complete` as the encoded `http::Response` with the stream's status and headers and an empty body (what a whole `Http` reply answers too, so the call decodes one type either way), a cut as a refusal; each event moves the request's expiry `wait` blocks past it. `seq` counts from 0 per call the node makes, and the requester hears each `seq` once and in order whatever order the frames land in: egress keeps an event that lands before the one due until the gap fills, drops a repeated `seq`, and takes a `seq` back at 0 as the node calling the service again after a restart, which drops what waited and voids what came before. It holds an `EndStream` until the requester has heard every event the end counts. An event the requester refuses is passed over, and the next one is heard. A relay is answered whole, and asking one with `Delivery::Events` is refused. The egress program expires a request itself: it schedules its own `Expire { id }` through the scheduler role `wait` blocks on (`DEFAULT_WAIT`, 64, when `wait` is `None`) and cancels it when the answer comes; no node signs an expiry |
| `blobs` | `blobs` | when a program reads a blob (`blob_get`) that no program put, and when the programs role admits code, asks `Held { blob }`: the blob is served only once a bundle naming it is held, which it is from the receipt that brings two thirds of the current validators. The node watches the role's writes: it gathers every bundle put, and a validator's node receipts a bundle once it holds every blob |
| `access` | `access` | while a program fills the role, before every op it executes (`ProgramCall::Execute`; not a reply to a program's message and not a run by the chain itself), asks the target program `ProgramCall::Access(payload)`, which answers what the op needs: `Vec<Need>`, each `Need { key, action }` with `Action::Write` or `Action::Delete`. With any need, it runs the role's `Op::Check { program, actor, needs }` as the chain, in the same unit as the op: the check may send no message, and its refusal refuses the op. `actor` is the sender's account and, for a message from a program, that program too (`Party::Account(n)`, `Party::Program(id)`). "Who may write a key", below, has the rules |
| `ownership` | `ownership` | once a run (an op, a resumed op, a reply) that wrote keys returns, and before anything it emitted runs, executes `Op::Guard { program, origin, sender, writes }` with the `System` origin: the frame's origin and sender, and each key the run created with a claim or changed while the ownership program held a record for it (`Write { key, claimed, existed, kept }`, once each, in key order). A key is shared unless the write that creates it claims it (`HostOp::SetOwned { key, value, kind }`, the SDK's `set_with` and `Map<K, Owned<T>>`); the host knows a key is recorded by reading the ownership program's state at `role::ownership::record_key(program, key)`. A run that claimed no new key and changed no recorded key makes no call; the kernel's own runs, the validators' messages and the ownership program's own runs are never guarded. The guard may send no message, and its refusal undoes the run. `crates/system/ownership` records each claimed key's owner (the account the frame acts as, or the program when it acts as none) and the policy its owner's defaults choose (kind, then program, then everything, then the owner's "Only me", which admits only them; `Query::OnlyMe` names it once made, and the open policy is only what an adopted row that anyone wrote keeps; neither is ever edited, switched or retired), and admits a change or delete only where that policy admits the actor. A program moves a record between two of its own keys with `Op::Moved { from_key, to_key }`, sent in its own message as its own account (files, when a guarded path moves) |
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
`ACCESS`, `OWNERSHIP`). A new role is a new row. The programs role keeps the table (its
`ROLES` map) and answers it with `Query::Roles`. The roles in
`abi::role::REQUIRED` (programs, validators, identity, blobs, awaits and
halo) are filled for the network's life: the kernel calls programs,
validators, identity, blobs and awaits, and every node reads halo's lane
table. Any other role, access, egress, scheduler and ownership here, is
optional: the kernel checks ops with access, and guards writes with
ownership, only while a program fills that role. Removing a program unbinds
every role it fills in the same block. Founding
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
program itself (blobs takes a receipt from a validator's key; chat's `:`
namespace belongs to the program its prefix names).

## Accounts

Everything that acts is an account in `identity`:
`Account { number, card, control }`. The `Card` is what it shows (name,
avatar, bio); the `Control` is who acts as it:

- `User { keys }`: a person and the keys they hold.
- `Program { manager, category, life, script }`: a program account. Its
  manager, the account that created it (a user, another program account or
  a system account), declares its `category` (`Agent`), gives it keys, sets
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
| `CreateAgent { name }` | any account that acts; it becomes the manager of a new program account of the `Agent` category, with no key and no script |
| `AddKey` | the account itself, a user's or an active program account's: the new key signs the frame and a key already on the account consents. A program account's manager too: the manager signs and the new key consents to itself. Never a system account's, a suspended program account's by its own key, or a revoked program account's |
| `RemoveKey` | the account's own key removes itself or a junior one, never the last, on a user's or an active program account. A program account's manager removes any of its keys |
| `SetName`, `SetProfile` | whoever may write the account's key in the access program, `identity::account_key(n)`: its owner (a user or a system account itself, a program account's manager) and the parties the owner lists there; with no list set, a program account itself too. Not on a revoked program account |
| `Suspend`, `Resume`, `Revoke` | a program account's, by whoever may write its key: its manager and the parties the manager lists; with no list set, the program account itself too |
| `SetScript { account, script }` | a program account's, by whoever may write its key, not once revoked. `Some(Script { calls, config })` names in `calls` a program that has an account; that program then sends messages as the account (`Runs`) and reads `config`. `None` takes it back |
| `SetOwnership { account, ownership }` | a program account's, by whoever may write its key, not once revoked: whom it answers. `Anyone` (the default, kept as no row), `Owner` (its manager only) or `Listed(parties)` (its manager and these accounts or programs), kept settled: sorted, each once, the manager left out |

The role's queries, which views and other programs ask too, are
`Account(key)`, `OfProgram(program)`, `Profile(number)`,
`Profiles { after, limit }` and `Runs { program, account }`. A `Profile` is
`{ number, name, kind }`, where `Kind` is `User`,
`Program { manager, category, standing, script }` or `System(id)`. `design::badge`, `design::runner_badge` and `design::note` turn it into what a view
shows beside a name ("Agent · managed by Dev" in chat; "Agent · harness · managed by Dev"
in members and explorer, where `design::runs` names the program an agent's script calls,
or `external` for an agent with no script; "Program · chat"; "suspended"),
the one place that happens.

Beyond the role's, identity answers `Answers { account }`: whom a program
account answers, as `SetOwnership` left it (`None` for an account that is no
program account). And `Hears { account, from }`: whether that account takes
a message from `from`, its manager always, anyone else as `Answers` says,
and a program's own account also as that program (`Party::Program`). It is
the one rule every runtime that delivers messages to an agent asks, harness
and agents run off the chain alike. `Narrowed { page }` answers every program
account that narrowed whom it answers, with its `Answers`, a page at a time,
so a view reads them all in one paged query; an account it doesn't list
answers `Anyone`. chat-view (mention hints, a DM's header, an agent's card)
and settings-view (a manager's agents) read it so; harness-view's page of one
agent asks `Answers`.

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
| `Migrate { height, first, last }` | anyone: rewrites the program ids it stores by the programs role's `Renames(height)` ("Renaming a program"), over the lists `first..=last` (each a `(program, key)`, either end open; a `height` past the current one is refused): a renamed program's lists move to its new id, a list already set there is kept, and each renamed `Party::Program` a list names follows. Its renames come from the role, never from the sender, so a second run changes nothing |

Its queries: `List { program, key }` (the list set on the key, if any),
`Resource { program, key }` (the program's answer for the key),
`Writers { program, key }` (who writes it: `Writers::Anyone`, or
`Writers::Listed { at, owner, list }`, the list that decides and where) and
`Lists { program, page }` (every list a program's keys carry). Its state is
one map, `LISTS` (prefix `l/`), from `(program, key)` to `Ownership`.

What each program asks:

| Program | Key | Ops that need it | Owner, default |
|---|---|---|---|
| `files` | the path; `files::pin_key(name)` for a pin | `Put`, `Mkdir`, `Symlink` write the path, `Remove` deletes it, `Move` writes both ends (and moves the lists with `access::Op::Move`); `Unpin` deletes the pin's key | the path's creator in the head tree, and for a namespace root's folders the owner files' params name (`files::paths::Root`: a `Named` root's folder one level down, the one account named so; a `Program` root's folders down to its depth, that program); no default. A pin's owner is the account that made it, default `Owner` |
| `forge` | `forge::repo_key(repo)`, `forge::settings_key(repo)`, `forge::change_key(repo, n)` | `Push`, `Merge`, `SquashMerge`, `Stage`, `DeleteBranch`, `Enqueue`, `Dequeue`, and `ChangeClose` by anyone but the change's author, write the repository's key; `Configure`, `Protect`, `RequireChecks` and `Archive` write its settings key; `Preview` and `Status` need none; `ChangeEdit` and `ChangeRepoint` write the change's key; `Rebase` writes the change's key and the repository's | the repository's owner, the change's author; a repository's default `Owner` for its key and its settings key (a repository created before that default, with no `d/` row in forge: `Anyone`, its settings key following the repository's list), a change's default `Owner` |
| `identity` | `identity::account_key(n)` | `SetName`, `SetProfile`, `Suspend`, `Resume`, `Revoke`, `SetScript` write it | a user or a system account itself, a program account's manager; default `Owner`, for a program account `Listed` with the account itself |
| `chat` | the channel id; `chat::message_key(channel, seq)` | `RenameChannel`, `SetChannelArchived`, `SetMembership` write the channel; `EditMessage` writes the message, `DeleteMessage` deletes it | the channel's owner, the message's author; default `Owner` |
| `egress` | `egress::service_key(name)`, `services/<name>` | `Assign` and `Unregister` write it | the service's owner, default `Owner` |
| `harness` | `harness::agent_key(agent)`, `harness::conversation_key(agent)`, `harness::thread_key(thread)` | `NewThread` and `ForkThread` write the agent's conversation key; `Close` of a canonical conversation writes the thread, and `Close` of one bound to a place (`OpenThread`) needs nothing, so anyone closes it; `SetRoute` and `RemoveRoute` need nothing of access, since a provider's route is an owned row (`harness.route`) whose ownership policy decides them. No op needs the agent's key itself; whom the agent answers is identity's (`SetOwnership`, asked with `Hears`) | the agent's manager (agent, conversation, thread); default `Owner` |

Keys, governance and signatures stay in their own programs: identity's
keys and valset's votes.

## Updating a program

`programs` holds the roster and the changes scheduled to it:

1. `ducktape blob put FILE --kind program --wait` uploads the code to the
   node, which submits the blobs role's `Put` naming it. Every member gathers
   it; each validator's node receipts it once it holds it; the command
   returns once two thirds of the current validators have receipted it, and
   the blobs role answers `Held` for it. The programs role refuses to schedule code
   the blobs role does not hold, and a held blob of any kind but
   `"program"`.
2. `ScheduleOver { scheduled: Scheduled { height, change: Change::Set(Entry { program, code, params }) }, expected }`
   lands it at a later height, over the code it expects: it is refused `wrong_state`,
   naming what is there, unless `expected` is what is under the name both just before
   `height` and once every change already waiting for that name has landed (`None`:
   nothing is). A deploy sends its rollback code as `expected`, so two deploys of one
   program never both land. Plain `Schedule(Scheduled)` takes the same change with no
   `expected`, and two deploys from one base can then both land.
3. At that height the kernel's `At(height)` sees the new code. Under an id
   already running, the code is swapped: the state, the account and the id
   stay, and `init` does not run again. Under a new id, the program is
   admitted (its `init` gets `params`) and registered with identity.

A change lands at its height whether or not an op runs on the programs role
after it: the kernel asks `At(height)` for the block it applies, and a
client (the app's rail, Explorer) asks `Roster`, the programs and views at
the query's own height, with that height.

What each running program was built from is `Sources`: per program its code
and the `Provenance` that code carries, `Commit(Source { repo, commit })` from
its `ducktape.source` section, `Unstamped` for code with none, or `Unread`
with why (a blob not held here, a section that does not decode). A founding
stamps the section as it packs a program (`crates/sdk/founding`), with the
commit of the checkout it ran in while git sees no change there and nothing
otherwise, so code built from uncommitted work never names a commit. The
stamp is part of the code, so it cannot be mislabelled, and one commit packs
to one set of bytes.

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

## Renaming a program

A program id is a name, or an owner and a name (`owner/name`), each name 1
to 64 of `[a-z0-9_-]` (`program::is_id`). A new id, one with no holder and
nothing under it, is `owner/name`; an id that runs, is listed or is held
keeps its bare name. No two ids a change brings in or the roster lists
write the same with `/` as `-` (`program::slug`, how a channel id names its
program), so a `Set`, `SetView`, `Publish` or `Rename` that would is refused
`already_exists`.

`Change::Rename { from, to }` moves a program or a view-only entry to a new
id at its height. It is scheduled under `from`, by whoever may `Set` it
(`from`'s holder, or the vote for a program that fills a role), and `to`
must be free. The rename claims `to` for `from`'s holder as it is
scheduled, and `from` stays held, so nobody takes the old name. From its
height `At` lists `from`'s entry under `to`, its roles and its hook follow,
and a `Set` of `to` at the same height lands its code under `to` whichever
of the two the fold applies first. The kernel keeps the program's state,
its account and its storage place (it tells identity `RenameProgram`
first), and swaps the code if `to`'s differs, with no `init`. Nothing
answers to `from` after it.

`Renames(height)` (tag 12, answered at tag 9) lists each landed rename that
still holds at `height`: its `to` listed and its `from` not. The kernel asks
it at every block, so a rename identity refused is tried again at the next
one, and each program's `Migrate` rewrites the ids it stores by it. A
programs role that refuses it renames nothing.

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
repo founds: it keeps the frames each account acted in, newest first, and
how many each account sent each program. Every frame, and every frame by
signing key, is the node's to list (`/v1/blocks`, `/v1/activity`).

Who may schedule or cancel (putting a blob is open to any account):

- The validators' vote, any change: `Env::authority()` in `program` admits
  only a message from the program that fills the validators role, and
  `valset` sends one only when the vote passes a `Motion::Role { role:
  "programs", op }`: the enacting vote emits `op` to the programs role in the
  same frame; a refusal there undoes the programs' writes, the vote stands,
  and the motion is gone.
- The account that holds an id: a frame that acts as an account, through
  any origin. An id (a program's, or a view's) has at most one holder
  (`Query::Owner`, table `o/`): the vote holds every id genesis founds,
  and every id that ran, was listed or had a change waiting before
  holders were recorded; the first account to `Set`, `Publish` or
  `SetView` a free id holds it from the block it schedules that change,
  so no second account claims it during the notice; a removal keeps the
  holder. `Own { program, holder }` hands an id to an account, to a list
  of accounts (`Holder::Accounts`, any of which acts as the holder; never
  an empty list) or to the vote, sent by an account that holds it or by
  the vote. Another
  account's change is refused `unauthorized`, naming the holder. The
  holder sets, publishes with an empty role list and removes its id, but
  never the id of a program that fills a role, and its change lands at
  least `NOTICE` (60) blocks after the block it is made in, so the vote
  can cancel it first. It may not change an id the vote has a change
  waiting on, nor claim an id an account's waiting removal frees (a
  program under a view being removed, or the reverse) before that
  removal lands. It cancels any change an account made to an id it holds
  that fills no role, never one the vote made. `Scheduled` answers who
  made each change (`Pending.by`, `None` for the vote), and lists only
  the changes still to land: one at or below the answering height has
  landed, whether or not an op on the programs role has folded it since.
- Hooks: the account that holds a hook's id may replace its code (`Set`
  of a hooked id).
  The hook keeps its status: it still cannot emit and runs on the kernel's
  capped hook fuel. Only the vote sets, unsets or removes a hook
  (`SetHook`, `RemoveHook`, `Remove` of a hooked id).

The vote wins: its change to an id takes every change an account made to
that id with it. The vote's removal of an id that holds nothing at its height
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
  state), `Queries { policy }` (the other query policy) or `Role { role, op }` (enacted by emitting `op` to the program
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
`Reply` read the role's variants at the role's tags, byte for byte, can fill it
(most roles' are the first variants; validators' `QueryPolicy` is tag 6), named in
its founding entry's `roles` or published with the role by the vote.
`crates/sdk/conformance` checks one (its
README has the steps). What the kernel does with the answers: identity's
`RegisterProgram` is idempotent and only the system sends it, and refusing
it fails a founding or undoes a later admission; a refusal of
`Account(key)` or `OfProgram(program)` rejects the frame or the message.
The programs role's `At(height)` answers every program that runs at that height; one
it leaves out is dropped. A client names the height it asks about:
`ProgramsAt`, `ViewsAt` and `HooksAt { at_height }` answer the lists at block
`at_height` (0 is genesis, not now), and `Roster` answers at the query's own
height. `At` and `Hooks` are the kernel's role queries and answer the same;
`Views(height)` is retired: its tag answers no query, and `ViewsAt` lists the
view-only entries. `Renames(height)` is the kernel's too, and a filler may
refuse it: that reads as no renames. A refusal of validators' `Members` stops the node.
