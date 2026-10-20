# Writing a program

One walk from `make new-program` to a program founded on a local network. The
example is `poll`: a question with options, opened in a chat channel, voted
on by people and agents, closed by its opener. Its snippets are the real
thing; nothing here is generated. The crate is what a reader follows: plain
types, plain functions, one host call per context method.

Read [README.md](../README.md) for the map of the repository and
[roles.md](roles.md) for the roles and the account model first.

## 1. The crate

```sh
make new-program NAME=poll
```

writes `crates/app/poll/` in valset's shape, registers it (`PROGRAMS` and
`VIEW_LINKABLE` in the Makefile, the workspace members and dependencies in
`Cargo.toml`, its schema in `crates/sdk/founding/src/schema.rs`) and prints
what to do next. The files, in reading order:

| File | What |
|---|---|
| `src/lib.rs` | the types on the wire (`Op`, `Query`, `Reply`, the rows they carry), `PROGRAM`, `describe()` and `describe::export!` |
| `src/program.rs` | `pub struct Poll; impl program::Program for Poll` (`NAME`, `Op`, `Query`, `Reply`: what the program is to a view, which names it `Query<poll::Poll>`) and `impl program::Program for Poll`: one match over every op, one over every query, `access` and `resource` (what each op needs of the access program and who owns a key, section 5), `reply` if it emits with a reply wanted; `program::export!` behind `program` |
| `src/rules.rs` | the tables (`store`), and what each op checks and writes |
| `src/tests.rs` | the program natively over `MockHost` (add a `MockChain` test once it talks to another program) |

`Cargo.toml` declares two features, both off by default:

- `program` adds the two wasm exports (`program::export!`). Only the program's
  own wasm build turns it on (`make wasm-programs` builds each program in a
  target dir of its own with `--features program`, one make target each, so
  `make -j` builds them at once). Everything else, the
  types, the rules, the `Poll` type, is always built: a view links the crate
  with the feature off and gets the types with no host import and no export,
  and a native test runs the real `execute` and `query`.
- `describe` makes the crate's wasm build the `ducktape.describe` module: the
  `describe` export alone, no program, no imports (section 7).

A program another program links (forge links chat, poll links chat) is why
the exports sit behind a feature: the exports are global symbols, and a
wasm crate has one `export!`.

## 2. Op, Query, Reply

All three are borsh enums in `lib.rs`; the same types a view and a sibling
program link. A write is an `Op`, a read a `Query` answered by a `Reply`.

```rust
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Op {
    Open { poll_id: String, channel: String, question: String, options: Vec<String> },
    Vote { poll_id: String, option: u32 },
    Close { poll_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema, ::program::Ask)]
#[ask(Poll)]
pub enum Query {
    #[ask(Reply::Poll(Option<PollRow>))]
    Poll { poll_id: String },
    #[ask(Reply::Polls(PageResponse<PollRow>))]
    Polls { page: PageRequest },
    #[ask(Reply::Tally(Vec<u64>))]
    Tally { poll_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize, BorshSchema)]
pub enum Reply {
    Poll(Option<PollRow>),
    Polls(PageResponse<PollRow>),
    /// One count per option, in option order.
    Tally(Vec<u64>),
}
```

**Each type on the wire has a schema.** `Op`, `Query`, `Reply` and every
type they carry derive `BorshSchema`, and a type with a hand-written borsh
encoding writes its schema by hand to match. `crates/sdk/founding/src/schema.rs`
names the program's interface (`program::schema::interface::<poll::Poll>()`),
`make wasm-schemas` writes it as `poll.schema`, and a founding packs it as the
`ducktape.schema` section of the program's code. An agent reads it there:
harness's `network_schema`, `network_query` and `network_call` speak any
program's ops and queries as JSON through it. A byte field (`Vec<u8>`,
`[u8; N]`) reads back as hex; an op or a query may write it as hex or as
`{"utf8": "text"}`, the text's bytes, so a ref name or a path is written as
itself (`{"Ref": {"utf8": "refs/heads/dev"}}`). A refusal names every field
that is unknown, missing or wrong at once.

**Each op that returns something says what.** An op's output
(`ctx.set_return_data`) is bytes no type ties down, so `impl Program` names
the type each op variant's output is the borsh of:
`const RETURNS: &'static [Return] = &[returns::<u64>("Open")];`
(`program::schema::{Return, returns}`). The interface carries them as its
`returns`, a name that is no variant of `Op` fails it, and `network_call`
reads such an op's output back as JSON. An op left out (or one whose output
is no one borsh type) answers its bytes as hex.

**Each op and query says what it does, to an agent.** `#[derive(program::Described)]`
on `Op` and on `Query` lists every variant, in order, with what its one
attribute tells: never a `///` comment.

```rust
#[derive(BorshSerialize, BorshDeserialize, BorshSchema, Debug, program::Described)]
#[tool_about("Polls anyone opens and closes.")]
pub enum Op {
    #[tool_op(
        description = "Open a poll with its choices. Anyone may.",
        returns = u64,
        example = examples::open
    )]
    Open { id: String, choices: Vec<String> },
}
```

`description` says what the variant does, who may send it and what it means,
in plain sentences. `returns` names the type an op's output is the borsh of:
the derive lists it as `Op::RETURNS`, which `impl Program` names
(`const RETURNS: &'static [Return] = Op::RETURNS;`), so the schema's
`returns` and `network_call`'s reading of the output come from it. A query
needs none: the `Reply` variant its `#[ask(..)]` names is what it returns.
`example` is a real value of the variant: a path names a fn that makes one, any
other expression is one, so the compiler checks it. Each is optional; a
variant with no example gets one `borsh_json::example` makes. `impl Program`
gathers both enums with `fn tools() -> Tools { Tools::of::<Op, Query>() }`
(`program::tools::Tools`). `make wasm-schemas` (founding's `schemas`) writes
`<program>.tools` beside `<program>.schema`, and a founding packs it as the
`ducktape.tools` section of the program's code: JSON text,
`{"about": …, "ops": {variant: {"description", "example", "returns"}}, "queries": …}`,
each example as JSON and each `returns` as the shape of what comes back, which
a harness or an adapter reads without the program's schema, and which only
grows by new keys. `borsh_json::tools::check` holds a section to the
interface: every variant listed once, each with a description and an example
that encodes as its own variant. founding's
`every_program_describes_each_op_and_query_with_examples_of_its_own` runs it
for every program here (`UNDESCRIBED` lists the ones not described yet; a
program leaves it as its enums derive `Described`). A retired variant says it
is retired and what replaced it.

harness's `network_schema` reads both sections of the code a program runs.
With no program it lists every program that runs with its `about`; with a
program, its index (each op and query by name with its description); with an
`op` or a `query` too, that variant's description, its example (the
section's, else one `borsh_json::variant_example` makes), its shape, the shape
of what it returns and each type those name; with `whole`, every shape at once.

**The append-only rule.** Borsh encodes a variant by its index, and a host
describes an op that landed at any height with the program's *current*
code. So `Op` only grows at its end: append a variant, never insert,
remove or reorder one. The scaffold's test holds you to it:

```rust
#[test]
fn op_variants_only_append() {
    assert_eq!(describe::variants::<Op>(), ["Open", "Vote", "Close"]);
}
```

**Retire, never delete.** A variant you no longer want stays where it is:
its op is refused as retired, and its reply goes unused, so every variant
after it keeps its tag. Deleting `programs`' `Query::Grants` (#478) moved
`Roster` from tag 7 to 6, and every app built before the move could not
list views on a network that ran it.

The test above lives in the crate it guards, so the change that edits the
enum can edit the list too. The gate a change can't edit is the tools
repo's `interfaces` (`build.sh gates`, and `gates.sh` in CI). It reads each
program's `crates/sdk/founding/interfaces/<program>.schema` at the change's
merge base with dev and at the change, and fails on any difference
`schema-diff` marks `!` (a variant or field removed, moved or retyped, in
`Op`, `Query` or `Reply`), or on a program's file that goes. founding's
`the_committed_interfaces_are_the_built_ones` holds the files to the
crates: a change that grows an interface writes them again with
`INTERFACES_WRITE=1 cargo test -p founding --test schema`.

**Each query names its reply.** `#[derive(program::Ask)]` with
`#[ask(Poll)]` (the program) and, on each variant, the reply that answers
it writes one type per query into `poll::ask`, built like the variant. A
view asks one alone and gets that reply, typed:
`cx.host().query(poll::ask::Tally { poll_id })` is a `Vec<u64>`, with the
same bytes on the wire as `Query::Tally`, and no arm for the replies that
answer other questions. A reply with named fields is answered as their
tuple (`#[ask(Reply::Thread { root: Option<Row>, replies: PageResponse<Row> })]`).

A field added to an existing variant changes its bytes too; that is a new
variant. `Query` and `Reply` follow the same rule: the app, other
programs and views built at other commits read them by tag, and the
`interfaces` gate holds all three.

Every list a client asks for takes a `store::PageRequest` and answers a
`store::PageResponse` (section 3); the row types (`PollRow`) live beside
the enums so a view renders what the program keeps.

## 3. State: tables over `store`

**State holds only what programs need to decide.** Every key a program
writes is consensus state: every node stores it, every proof covers it, and
every node that joins the network downloads it. So a program keeps only what
an op, or another program's ask, reads to decide something. A record of what
happened (who signed which frame, a conversation's transcript, a stream once
it's folded) is an index: whoever wants it rebuilds it from the blocks, and
the blocks are the record. Before adding a table, name the decision that
reads it. And don't rewrite a growing value whole at every step: each
rewrite stays in the log until its floor passes, so a value that grows by a
piece per event costs every node the square of its size. Append the pieces
as rows of their own and fold them when they're read.

A program's state is its own key space of bytes; no other program reads
its keys (section 6 is how you ask). `store` puts typed tables over
it, declared once as `const`s in `rules.rs`:

```rust
use store::{Map, Set};

pub(crate) const POLLS: Map<String, PollRow> = Map::new("poll/");
/// Votes per option: `(poll, option)`.
pub(crate) const TALLY: Map<(String, u32), u64> = Map::new("tally/");
/// Who voted, so each account votes once.
pub(crate) const VOTERS: Set<(String, AccountNumber)> = Set::new("voter/");
```

- **Prefixes.** Every key of a table starts with its prefix, and two
  prefixes must not be prefixes of each other (`"m/"` beside `"m/x/"` would
  read each other's rows). One word and a slash is the convention.
- **`live/` is reserved.** It holds the room grants the node reads
  (`program::live::grant`, `abi::live`): who may publish and subscribe to
  the live tracks of the program's rooms. No table may use it.
- **Keys** encode through `store::KeyCodec` so byte order is value order:
  integers big-endian, strings and byte vectors NUL-terminated (a shorter
  name sorts before its extensions), `Principal` as its borsh, tuples
  concatenated. A tuple key scans by its leading elements: `TALLY.prefix_of(&poll_id)`
  is every `(poll_id, _)` row, which is how every "in this poll" read works.
  A key part that should list newest first is stored as `u64::MAX - n`
  (chat's `newest_first`).
- **Values** are borsh. A row that does not decode is refused `corrupt`,
  never a panic.
- **Reads take `&QueryCtx`**, which an `&ExecCtx` derefs to, so a rule that
  only reads is written once and serves both. Writes take `&ExecCtx`.

The methods: `get`, `has`, `put`, `remove`; `scan(ctx, range)` over a
`Range` the table builds (`prefix_of`, `below`); `all` (unbounded: for a
program's own rules, never a client's query); `range`/`range_of` for a page.
`Item<T>` is one value at one key (`COUNT.update(ctx, |n| *n += 1)`).

**Pages.** A client's listing answers a bounded, resumable page:

```rust
pub(crate) fn polls(ctx: &QueryCtx, page: &PageRequest) -> Result<PageResponse<PollRow>, Error> {
    Ok(POLLS.range(ctx, page)?.map(|(_, row)| row))
}
```

`PageRequest { after, limit }` is what a client sends (`PageRequest::first(n)`,
then `PageRequest::resume(reply.next, n)`); no limit asks every row, and a
bound the network configures applies with `page.bounded(max)`. `PageResponse { height, items, next }` answers with the
height that answered (the ctx's: no program passes it) and an opaque cursor
for the page after. A cursor is
bound to the listing it came from (a cursor from one listing handed to
another is refused `stale`) and cannot escape its prefix.

Reads see the frame's own writes so far. `committed_get`/`committed_scan`
read as of the last committed block, which a program rarely wants.

## 4. Rules, errors and codes

A rule checks, then writes. A refused op's writes are undone by the host,
so nothing needs rolling back; but writing only after every check keeps a
rule readable and the native `attempt` check (section 8) trivially true.

```rust
pub(crate) fn vote(ctx: &ExecCtx, voter: AccountNumber, id: &str, option: u32) -> Result<(), Error> {
    let row = poll(ctx, id)?;
    if row.closed {
        return Err(wrong_state(format!("poll {id} is closed")));
    }
    if option as usize >= row.options.len() {
        return Err(invalid(format!("poll {id} has options 0..{}", row.options.len())));
    }
    ctx.require_user_or_program_account(voter)?;
    let voted = (id.to_owned(), voter);
    if VOTERS.has(ctx, &voted) {
        return Err(already_exists(format!("account {voter} voted in {id}")));
    }
    let key = (id.to_owned(), option);
    let count = TALLY.get(ctx, &key)?.unwrap_or(0);
    TALLY.put(ctx, &key, &(count + 1));
    VOTERS.insert(ctx, &voted);
    Ok(())
}
```

An `Error { code, message }` is a code token and one sentence for a person.
The code says what a caller does about it; the constructors in `program` are
named by it:

| Constructor | Code | The caller |
|---|---|---|
| `invalid(..)` | `invalid_input` | fixes the request; retrying it unchanged never succeeds |
| `not_found(..)` | `not_found` | names a thing that exists |
| `already_exists(..)` | `already_exists` | creates under another id, or treats the create as done |
| `wrong_state(..)` | `wrong_state` | changes the thing's state first (a closed poll takes no vote) |
| `unauthorized(..)` | `unauthorized` | acts as someone else |
| `capacity(..)` | `capacity` | sends less or removes something |
| `stale(..)` | `stale` | re-reads and retries (a cursor from another listing) |
| `corrupt(table, key, what)` | `corrupt` | an operator: stored state failed an invariant |
| `unexpected_reply(program, asked, &reply)` | `unexpected_reply` | an operator: a sibling answered another shape |

`program::code` has the full list; `unknown_program`, `trap`, `protocol` and
`sequence` are the host's, which a program meets only as a sibling's
answer. Word the
sentence for the person who reads it in a refusal: what was asked and why
not, no internals.

Bytes that do not decode as your `Op` or `Query` are refused `invalid_input`
before `execute` runs ("poll: Op did not decode: …"); `init`'s params go
through the same `program::decoded` if you take any.

## 5. Sender, origin, identity

Every context carries the call's `Env`: `chain_id`, `height`, `time`,
`program` (this program's own id), `origin`, `sender`, `roles`, `cause`.

The host resolves **who a frame acts as** once per frame through the
identity role and hands it over as `sender`: a signed frame acts as the
account its key holds, a message as the emitting program's account, genesis
and the kernel as the chain (`Root`). A key that holds no account still
runs, as no one. A write reads it first thing:

```rust
fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
    // every op acts as an account: a user, a program account or a system account
    let account = ctx.sender_account()?;
    match op {
        Op::Open { poll_id, channel, question, options } => open(ctx, account, poll_id, channel, question, options),
        Op::Vote { poll_id, option } => vote(ctx, account, &poll_id, option),
        Op::Close { poll_id } => close(ctx, account, &poll_id),
    }
}
```

`ctx.sender()` is the `Principal` (`Account(n)` or `Root`) and refuses a
frame that acts as no one; `ctx.sender_account()` is the number and refuses
the chain too. No row a program writes ever names a bare key.

The **origin** is the raw caller (`Origin::Signed(key)`, `Program(id)`,
`Root`), for the few rules that need it rather than the account:
`env.signer()` (blobs takes a receipt from a validator's key),
`env.sending_program()`, `env.sent_by("forge")`. Chat's `poll:<name>` ids
belong to the program the prefix names, checked on the origin.

The **identity role** answers who an account is. `ctx.require_account(n)`
asks it (`Profile(n)`) and refuses an account that does not exist: chat's dm
counterpart, forge's reviewers. Ask a role at the program that fills it,
`ctx.env().role(abi::role::IDENTITY)?`, never by a name you assume. The profiles
themselves (`abi::role::identity::{Profile, Kind, Standing}`) are what a
view shows beside a name.

Who may write something an account owns is the access program's to decide
([roles.md](roles.md), "Who may write a key"); a program answers it two
questions. `Program::access(ctx, op)` names what an op needs: a
`Need::write(key)` or a `Need::delete(key)` for each key the op changes,
where a key is a `/`-separated string the program chooses (files uses the
path, chat `chat::message_key(channel, seq)`). `Program::resource(ctx, key)`
answers who owns a key and which list holds when no one has set one,
`Resource { owner, default }`, from what the program already stores. Before
the op runs, the kernel asks `access`; with any need, the access program
checks each against the actor (`env.actor()`: the sender's account and, for
a message, the program that sent it), and its refusal refuses the op. A
program with nothing to gate answers `Ok(Vec::new())` and
`Ok(Resource::default())`, as `crates/sdk/program/examples/counter.rs`
does.

A list is a `program::Ownership`: `Owner` (the owner alone),
`Listed(parties)` (the owner and the parties listed, each a
`Party::Account(number)` or a `Party::Program(id)`) or `Anyone`.
`allows(owner, actor)` decides; `settled(owner)` keeps each party once and
never the owner, an empty list being `Owner`. The lists live in the access
program, never in the program whose keys they cover: a key's owner, or the
program itself, sets one with `access::Op::Set`, and a program never copies
an owner into it. A list holds account numbers and program ids, never
names, so renaming an account changes nothing it allows. A program that
reads a list for a rule of its own asks the access program. Whom an agent
answers is no access list: it is identity's (`SetOwnership`, `Answers`,
`Hears`; [roles.md](roles.md)).

`Env::authority()` admits only a message from the program bound to the
validators role, which sends one when its validators' vote passes a
`Motion::Role` for the programs role; the programs role admits it, or any
account's frame on a program that fills no role ([roles.md](roles.md),
"Updating a program"), before scheduling or cancelling a change.

A program the programs role lists as a hook ([roles.md](roles.md), "Hooks") is
told of every frame a block carries through `Program::hook(ctx, record)`.
`program::hook` calls it only when `Env::kernel()` admits the origin (the
chain, `Root`); a program that does not override `hook` refuses the call.
`activity` is the one this repo founds.

Every key a program writes is **shared** (general data: no owner, no policy,
only the program's own code governs it) unless the write that creates it
**claims** it: `ctx.set_with(key, value, Some(kind))` sends the kernel's
`SetOwned`, and the account the frame acts as becomes the key's owner, an
item of that kind. From then on the kernel hands every run that writes the
key to the ownership role, which refuses another account's change where the
owner's policy says so ([roles.md](roles.md), the ownership row). A claim on
a key that already holds a value is a plain write. A table of owned items is
a `Map<K, Owned<T>>`, whose every `put` claims its row:

```rust
const VOTES: Map<(String, AccountNumber), u8> = Map::new("vote/");
const POLLS: Map<String, Owned<PollRow>> = Map::new("poll/");

impl Kind for PollRow {
    const KIND: &'static str = "poll.poll";
}
```

`Map<K, Owned<T>>` reads and writes `T` itself, the same bytes as
`Map<K, T>`, and claims each row it creates as `T::KIND`, which a user's
defaults and policies know it by (`kind()` says it). A row type another crate
defines names its kind through a marker type of the program's own
(`Map<String, Owned<ServiceRow, Service>>`, `impl Kind for Service`). Every
other table, and a raw `ctx.set`, is shared, and a program that never claims
pays nothing for the guard. This repo's programs own these kinds (forge's reviews,
`forge.review`, egress's services, `egress.service`, harness's models,
`harness.model`, chat's channels and messages, `chat.channel` and
`chat.message`, forge's changes and repositories, `forge.change` and
`forge.repo`, and who starts an agent's conversations,
`harness.agent-conversation`). When others' ops write an item's record (a change's reviews, close
and merge), the owned row is a small row of its own beside the shared record
(forge's `Authorship`), which the item's own ops write.

Four rules hold for every owned table:

- **The guard sees only the keys a run writes.** An op that acts on an owned
  item must write that item's row, even when nothing in it changes (bump a
  `rev` on it), or no policy governs the op.
- **What other accounts write leaves the owned row.** A count, a tally or a
  timestamp that another account's op updates (a reply count, reactions, a
  repository's last activity) lives in a shared table keyed by the item, or
  the owner's narrowed policy refuses those ops.
- **A changed stored type is a new table.** Rows already stored keep
  decoding under the old one until a migration moves them.
- **Never delete an owned row to mean "empty".** A key's owner is whoever
  created it, so a row deleted and written again belongs to its new writer.
  Keep an empty value, or fold the row into the item's own.

Rows a program kept before the ownership role was seated have no record, and
the guard leaves a key with no record alone. Each program that owns kinds has
a one-time open op, `AdoptOwners { kind, after, limit }`, that records them
(J-011 ⑥): anyone sends it, it answers where the next batch starts and sends
the program the same op, and that run sends the ownership role `Adopt` with
each row's owner (from the row's own stored fields) and the policy matching
what the access program enforces for it today. A record already there is
left as it is, so running it again changes nothing. egress's services wait
for access's own removal (its `Op` is the kernel's).

## 6. Talking to other programs

**A query is a read.** `ctx.query::<Q, R>(program, &q)` runs the sibling's
`query` in this frame, borsh both ways, and moves no state. poll asks chat
whether the channel exists before it opens:

```rust
fn room(ctx: &QueryCtx, id: &str) -> Result<Option<chat::ChannelInfo>, Error> {
    let asked = chat::Query::Channel { channel_id: id.to_owned() };
    match ctx.query::<chat::Query, chat::Reply>(chat::PROGRAM, &asked)? {
        chat::Reply::Channel(room) => Ok(room),
        other => Err(program::unexpected_reply(chat::PROGRAM, "Channel", &other)),
    }
}
```

Link the sibling's crate (`chat = { workspace = true }`, its `program`
feature off) for its types. The sibling's refusal is your `Err`.

**An emit is a write** the target runs in this frame, as a message from
this program, once your handler returns Ok; messages run in emit order,
each before the next, depth first, at most 8 deep. `ctx.emit(target, &op,
reply)` names the target's `Op` and answers a `MessageId` (this program and
the message's number, which no other message on the network shares). What
the target's refusal does is `reply`:

- `Reply::None`: the refusal fails this whole frame. Every host is left as
  it was, your writes included. poll's closing line must land, so the
  close is posted this way: a channel that refuses it un-closes the poll.
- `Reply::Wanted`: the target's writes are undone, and its outcome comes
  back to you as `Program::reply(ctx, id, outcome)`, a run of its own whose
  `cause` is `Cause::Reply`: in the same frame, or, when the target is an
  awaiting program that parked, in the submission that finishes it. Return Ok to absorb the
  refusal, Err to fail the frame after all. The default `reply` absorbs an
  applied outcome and returns a refusal, so a program that never wants a
  reply leaves it alone.

`ctx.emit_as(account, target, &op, reply)` (and `emit_raw_as`) sends the
same message as `account` instead of this program's own account. The host
asks the identity role `Runs { program, account }` and runs the message as
`account` only when identity answers with it: an active program account
whose script calls this program (identity's `SetScript`). Any other answer refuses
the message, which `reply` then handles like any refusal. A call acts as an
account the same way (`ctx.call_as`, `call_raw_as`, below), and the refusal
is the call's answer. harness writes as the agents it runs with
`call_raw_as`, so chat and files judge the agent's account, not harness's.

The same script routes `attribution`. A program sends attribution
`Register { deliver }` once (harness does in its `init`, which founding runs
like any frame, so what `init` emits is delivered); `deliver` is the prefix
attribution puts before each change it hands that program, the encoding of
the program's own op variant. A change about an active program account whose
script calls a registered program goes to that program alone, as
`deliver ++ borsh(change)`; a change about a user, a suspended account, or an
account whose script calls a program that never registered is recorded and
handed to no one. Whoever acts for such an account off the network follows
attribution's `to/<recipient>/<seq>` rows (each key is the recipient and the
seq, both u64 big-endian) and reads each change with `Query::Change { seq }`.

A reply names only the message's id. A program that wants to know *what* it
was waiting for keeps that itself:

```rust
/// The poll each announcement was posted for, by the message's number; the
/// reply removes it.
const ANNOUNCED: Map<u64, String> = Map::new("announced/");

// in open(): the room may refuse the line (members only); the poll opens anyway
let posted = post(ctx, &channel, &format!("{id}:open"), &line, Reply::Wanted);
ANNOUNCED.put(ctx, &posted.seq, &id);

/// What chat did with an announcement: the poll records whether its line
/// landed; a refusal is absorbed, not raised, so the open stands.
pub(crate) fn announced(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    let Some(poll_id) = ANNOUNCED.get(ctx, &id.seq)? else {
        return Err(wrong_state(format!("no announcement is message {}", id.seq)));
    };
    ANNOUNCED.remove(ctx, &id.seq);
    if let Outcome::Applied { .. } = outcome {
        let mut row = poll(ctx, &poll_id)?;
        row.announced = true;
        POLLS.put(ctx, &poll_id, &row);
    }
    Ok(())
}
```

and `program.rs` points `reply` at it:

```rust
fn reply(ctx: &ExecCtx, id: &MessageId, outcome: &Outcome) -> Result<(), Error> {
    announced(ctx, id, outcome)
}
```

The message acts as *your* account (identity's `OfProgram`), so the target
sees `sender: Account(poll's number)` and `origin: Program("poll")`. Chat
lets a program post under its own `<program>:<name>` ids
(`chat::namespace::id(PROGRAM, name)`) and nowhere else.

`ctx.event(bytes)` adds to the call's receipt, for readers of the chain;
`ctx.set_return_data(bytes)` is the execute's output, which a `Reply::Wanted`
emitter gets back as `Outcome::Applied { output }`.

**A call waits for the answer.** A program whose execute is `async`
implements `Awaiting` beside `Program` (and leaves `Program::execute` out),
and exports with `program::export_awaiting!`. Its handler takes an owned
`Ctx`, which derefs to `ExecCtx`, and the op already decoded:

```rust
impl Awaiting for Feed {
    async fn execute(ctx: Ctx, Refresh { pair }: Refresh) -> Result<(), Error> {
        let price: u64 = ctx.call(ORACLE, &Ask { pair: pair.clone() }).await?;
        QUOTES.put(&ctx, &pair, &Quote { price, at: ctx.env().height });
        Ok(())
    }
}
```

`ctx.call(target, &op).await` (and `call_raw`) runs `target` with `op` and
`cause: Cause::Call { ticket }`, and answers its output, or its refusal as
`Err`. A target that answers in that run resumes you in the same
submission. A target that cannot answer yet calls `ctx.defer()`, keeps the
ticket, and later sends `ctx.resolve(ticket, answer)`; until then your frame
is parked, and it resumes in the submission that carries the resolve, with
`ctx.env()` reading that block. A call waits until its target answers; the
awaits role puts no deadline on it.
Each run between two awaits is its own transaction: a refusal undoes only
the run it ends, never what an earlier block kept. `program::all(futures)`
awaits several calls together and answers their results in order; each
resumes your frame as its own answer comes. On a `MockChain`, seat
the awaits program (`chain.seat::<awaits::Awaits>("awaits")`) and the
awaiting program with `chain.seat_awaiting`.

The program bound to the egress role is called this way:

```rust
use abi::role::egress::{Delivery, Op};
let egress = ctx.env().role(abi::role::EGRESS)?.clone();
let asked = Op::Request { request, delivery: Delivery::Whole, wait: None };
let answer = ctx.call_raw(egress, abi::encode(&asked)).await;
```

It answers the elected node's output, in a later block (an `Http` request's
is an encoded `http::Response`), or a refusal when the node reports a failure
or the request expires, `wait` blocks after it was made (`DEFAULT_WAIT`, 64,
for `None`). `Delivery::Events { deliver }` hears a `text/event-stream` reply
event by event instead ([roles.md](roles.md), the egress row). harness asks
a model's turn this way: `deliver` is `harness::streamed_as(thread)`, the
encoding of its `Op::Streamed { thread, event }` up to the event, so each
event arrives as that op. It checks the sender is the program bound to the
egress role. Egress hands it each `seq` once and in order, and a `seq` back
at 0 means the node called the model again. The call's answer, an
empty-bodied `http::Response`, comes after the last event and says the
stream ended.

**A run for a later block.** `ctx.schedule(height, target, &op).await`
asks the program bound to the scheduler role to run `target` with `op` at
the end of block `height`, which must be after the current one, and answers
the entry id; `ctx.cancel(entry).await` withdraws it before it runs. The
run comes from your program (`Origin::Program`, your account) with
`cause: Cause::Scheduled { entry }`, so a target that accepts it only from
you checks both, as egress does for its own `Expire`. The scheduler keeps
the run's receipt (`Query::Entry`). On a `MockChain`, seat the scheduler
(`chain.seat::<scheduler::Scheduler>("scheduler")`): moving past a block
with `chain.at` runs what was due in it.

## 7. Describe

A program ships, beside its code, a pure wasm module that turns an op's
bytes into what a person reads: a title and typed fields. Explorer shows
it on a transaction's page; no list of transactions shows it.
`lib.rs` has the function and the one export line; `make wasm-describes`
builds the program (`--features describe`) and qa packs it into the
program.

```rust
pub fn describe(op: &Op) -> describe::Description {
    use describe::{Value, field};
    let (title, fields) = match op {
        Op::Open { poll_id, channel, question, options } => (
            format!("Open poll {poll_id} in #{channel}"),
            vec![
                field("question", Value::text(question)),
                field("options", Value::List(options.iter().map(Value::text).collect())),
            ],
        ),
        Op::Vote { poll_id, option } => (
            format!("Vote in {poll_id}"),
            vec![field("option", Value::count(u64::from(*option)))],
        ),
        Op::Close { poll_id } => (format!("Close poll {poll_id}"), vec![]),
    };
    describe::Description { title, fields }
}

describe::export!(Op, describe);
```

`Value` is a small vocabulary a reader renders each its own way: `Text`,
`Account(n)` (shown as a name), `Key`, `Program(name)` (a link), `Hash`,
`Amount { value, decimals }` (`Value::count(n)` for a plain integer),
`Time` (ms since the epoch), `Bytes` (`Value::bytes(..)` keeps a preview),
`List`. Title first, as a sentence a person would say ("Post in #design").

## 8. Native tests

A program's tests run its real `execute` and `query` natively. There are two
harnesses in `program`, and a third in `conformance` for a program that fills a
role.

**`MockHost`: the program alone.** State, blobs and what the program sent
(output, emissions, events) in maps a test reads. The contexts a test makes
over it are the same types the host makes:

```rust
let host = MockHost::default();
let ada = MockHost::env(PROGRAM).signed(key(ADA), Some(ADA));
Poll::execute(&host.exec(ada), op)?;
let reply = Poll::query(&host.query(MockHost::env(PROGRAM)), query)?;
```

`MockHost::env(program)` is a person's frame, as the chain sends every one
outside genesis: signed by key `[1; 32]`, which holds account 1. A test
moves what it cares about with `Env::signed(key, account)`,
`Env::from_program(program, account)`, `Env::root()` (the chain itself, as
genesis calls `init`) or struct update
(`Env { height: 7, ..MockHost::env(PROGRAM) }`). Siblings answer through
`host.sibling::<chat::Chat>("chat", &chat_host)` (a second host holding
chat's state, asked through chat's own decoding) and the identity role
through `host.identity(profiles)` over a fixed roster. An emitted message
is kept, not run: `host.take_emissions()` shows what left and whether a
reply was wanted. The attack check every program shares:

```rust
let refusal = host.refused(|| Poll::execute(&host.exec(bo), vote("lunch", 0)));
assert_eq!(refusal.code, code::ALREADY_EXISTS);
```

`refused` (and `attempt` for a write that may succeed) asserts a refusal
left the host exactly as it found it: no state, blob, emission, event or
output.

**`MockChain`: several programs, a submission as one frame.** Each program
is seated by name over a host of its own; every seat queries every other;
a submission is run as the kernel runs it: the signer resolved through
identity, the program, then what it emitted, depth first, replies included,
undone whole on a rejection nobody absorbs.

```rust
let mut chain = MockChain::default();
chain.seat::<Poll>(PROGRAM);
chain.seat::<chat::Chat>("chat");
chain.register(PROGRAM, POLL);        // poll's account, as identity registers it
chain.hold(key(ADA), ADA);           // Ada's key holds account 1, a user
chain.submit(Origin::Signed(key(ADA)), "chat", &channel("general", PostPolicy::Open))?;
chain.submit(Origin::Signed(key(ADA)), PROGRAM, &open("lunch", "general"))?;
let reply: Reply = chain.query(PROGRAM, &Query::Poll { poll_id: "lunch".into() })?;
```

With no identity program seated, the chain's roster answers as the role
would (`hold`, `register`, `profile` for a program account and its standing); seat
`identity::Identity` under the id that fills the identity role
(`MockHost::roles()`, or the roles given `MockChain::founded(roles)`) and
the chain asks it instead, and the roster's edits do nothing (identity's own ops seat keys
then). `chain.init(program, &params)` runs `init` as the kernel admits a
program: one frame, what it emits run after it, a refusal undoing it all.
Before each op the chain asks the program's `access` as the kernel does, and
when it answers needs, runs the access role's `Check` on the program seated
there (seat `access::Access` under `"access"`); and once a run that claimed a
new key, or changed a key the ownership program holds a record for, returns,
the chain asks the program seated at the ownership role, as the kernel does,
before anything the run emitted runs (seat `ownership::Ownership` under
`"ownership"`; with none seated, nothing is guarded);
`chain.at(height, time)` moves the chain to another block. The two tests that matter
for poll: the announcement lands in the frame that opened the poll, and a
room that refuses the line still gets its poll while a refused closing
line fails the close whole.

`crates/app/forge/tests/common/sandbox.rs` is a larger harness on the same
chain (forge and chat, agents, heights).

**`conformance`: a role.** A program that fills `programs`, `validators`,
`identity`, `awaits` or `halo` at genesis speaks that role's bytes: the role's `Op`, `Query`
and `Reply` variants are your enums' first, in order, with the same
fields. Take `conformance` as a dev-dependency, implement the role's
`Fixture` (the few things the role leaves to your own ops) and call
`conformance::identity::run(&fixture)` in a test; its
[README](../crates/sdk/conformance/README.md) lists what each role is held
to.

`cargo test -p poll` runs one program's tests; `make test` everything (the
founding suite under `crates/system/programs/tests` builds the
boot set itself and founds ducktape's host over it).

## 9. The edit loop

```sh
make dev P=poll            # the program's wasm, its native tests
make dev P=poll V=poll-view   # and the view's wasm, gated
```

`make dev` rebuilds what cargo finds stale with the fast profile (the
README's Building), one line per artifact
(`poll  165,724 B` or `unchanged`), gates a rebuilt view (ABI) and runs the
native tests of the crates whose test binaries cargo rebuilt. A program
that fails to build for wasm32 fails here; the usual cause is a dependency
that reaches the host or a signing library (`make view-wasm-check` names
them for the crates a view may link).

`make new-view NAME=poll-view` scaffolds the screen over the program
(`Query<poll::Poll>`); replace its `count()` with what your `Query` answers before its
first `make dev`. Where a view's bytes go: `make wasm-why V=poll-view`
(`twiggy top` over a build that keeps its names; `cargo install twiggy`).

`make wasm-modules` builds every program, its describe module and schema and every
view under `$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/`, unpacked;
nothing built is committed.

## 10. Founding

`crates/sdk/founding` packs each program with its view, describe module and schema,
and `make wasm-publish` releases the result as `founding.tar.gz`, which
`ducktape init` founds once its `founding.toml` is filled. Each time `dev` or
`main` moves, it is also published with every module unpacked at
`https://dl.ducktape.industries/programs/<channel>/<version>/`, where
`dev` is the fast profile's build and `main` the release profile's, and
`programs/<channel>/latest.json` names the newest version, its commit and each
file's sha256 (the README's Building):

```sh
curl -fsS https://dl.ducktape.industries/programs/main/latest.json | jq '{display, commit, wire_id}'
```

To add poll to it:

1. `crates/sdk/founding/src/lib.rs`: `("poll", "poll_view")` in `PROGRAMS`,
   or `"poll"` in `UNVIEWED` for a program with no screen of its own.
2. `crates/sdk/founding/founding.toml`: a `[[programs]]` entry,
   `id = "poll"`, `code = "poll.wasm"` (and `params = …` if `init` takes
   any). poll fills no role, so its entry names no `roles`; a program that
   fills one names it there (`roles = ["scheduler"]`).
3. qa's `founding.toml`: the same entry with `code = "@PROGRAMS@/poll.wasm"`.
   `kit up NAME` founds a fresh network with the program admitted at
   genesis and its account registered with identity.

A hook is also named in both files' top-level `hooks = [...]`, as
`activity` is.

Each program a founding packs carries the commit it was built from as its
`ducktape.source` section, and the programs role's `Sources` answers it for
every program that runs, so a network says which commit each program's code
came from. Before scheduling new code over a running program, `schema-diff`
(`crates/sdk/founding`) compares its interface with the running code's: an op
or query that only grew at its end is safe, anything marked `!` breaks the
bytes clients and stored ops already hold.

A program added to a running network goes through the programs role instead
(`ducktape blob put --kind program --wait`, then `Schedule` at a height; [roles.md](roles.md), "Updating a
program"): its `init` runs with the scheduled params when the height comes.
A program that fills a role is scheduled as `Change::Publish` with its
roles, which only the validators' vote may do.
