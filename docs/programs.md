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
| `src/program.rs` | `pub struct Poll; impl program::Program for Poll` (`NAME`, `Op`, `Query`, `Reply`: what the program is to a view, which names it `Query<poll::Poll>`) and `impl program::Program for Poll`: one match over every op, one over every query, `reply` if it emits with a reply wanted; `program::export!` behind `program` |
| `src/rules.rs` | the tables (`store`), and what each op checks and writes |
| `src/tests.rs` | the program natively over `MockHost` (add a `MockChain` test once it talks to another program) |

`Cargo.toml` declares two features, both off by default:

- `program` adds the two wasm exports (`program::export!`). Only the program's
  own wasm build turns it on (`make wasm-programs` builds each program in a
  target dir of its own with `--features program`). Everything else, the
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
program's ops and queries as JSON through it.

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

**Each query names its reply.** `#[derive(program::Ask)]` with
`#[ask(Poll)]` (the program) and, on each variant, the reply that answers
it writes one type per query into `poll::ask`, built like the variant. A
view asks one alone and gets that reply, typed:
`cx.host().query(poll::ask::Tally { poll_id })` is a `Vec<u64>`, with the
same bytes on the wire as `Query::Tally`, and no arm for the replies that
answer other questions. A reply with named fields is answered as their
tuple (`#[ask(Reply::Thread { root: Option<Row>, replies: PageResponse<Row> })]`).

A field added to an existing variant changes its bytes too; that is a new
variant. `Query` and `Reply` are looser (a view and its program ship
together), but a sibling program that queries you links your types, so
treat them the same.

Every list a client asks for takes a `store::PageRequest` and answers a
`store::PageResponse` (section 3); the row types (`PollRow`) live beside
the enums so a view renders what the program keeps.

## 3. State: tables over `store`

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
`env.signer()` (forge takes repository ops from a signed frame only),
`env.sending_program()`, `env.sent_by("forge")`. Chat's `poll:<name>` ids
belong to the program the prefix names, checked on the origin.

The **identity role** answers who an account is. `ctx.require_user_or_program_account(n)`
asks it (`Profile(n)`) and refuses an absent account, a system account, or a
program account that is suspended or revoked: poll's voters, chat's dm counterpart,
forge's reviewers. Ask a role at the program genesis bound to it,
`ctx.env().role(abi::role::IDENTITY)?`, never by a name you assume. The profiles
themselves (`abi::role::identity::{Profile, Kind, Standing}`) are what a
view shows beside a name.

A rule of who may act on something an account owns is a `program::Ownership`:
`Owner` (the owner alone), `Listed(parties)` (the owner and the parties
listed, each a `Party::Account(number)` or a `Party::Program(id)`) or
`Anyone`. `allows(owner, caller)` decides, where `caller` is every party the
actor is: files passes the sender's account and, for a message, the program
that sent it; harness passes the author's account and, for a program's own
system account, that program. `settled(owner)` keeps each party once and
never the owner's account, an empty list being `Owner`. identity keeps one
per agent for whom it answers; files keeps one per path for who writes under
it. A list holds account numbers and program ids, never names, so renaming
an account changes nothing it allows.

`Env::authority()` admits only a message from the program bound to the
validators role, which sends one when its validators' vote passes a
`Motion::Role` for the programs role; the programs role admits it, or a maintainer's signed frame
by its grant ([roles.md](roles.md), "Updating a program"), before scheduling
or cancelling a change.

A program the programs role lists as a hook ([roles.md](roles.md), "Hooks") is
told of every frame a block carries through `Program::hook(ctx, record)`.
`program::hook` calls it only when `Env::kernel()` admits the origin (the
chain, `Root`); a program that does not override `hook` refuses the call.
`activity` is the one this repo founds.

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
handed to no one.

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
egress role, and checks `seq` itself, because egress does not. The call's
answer, an empty-bodied `http::Response`, says the stream ended.

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
`identity::Identity` where genesis binds the identity role
(`MockHost::roles()`, or the roles given `MockChain::founded(roles)`) and
the chain asks it instead, and the roster's edits do nothing (identity's own ops seat keys
then). `chain.init(program, &params)` runs `init` as the kernel admits a
program: one frame, what it emits run after it, a refusal undoing it all;
`chain.at(height, time)` moves the chain to another block. The two tests that matter
for poll: the announcement lands in the frame that opened the poll, and a
room that refuses the line still gets its poll while a refused closing
line fails the close whole.

`crates/app/forge/tests/common/sandbox.rs` is a larger harness on the same
chain (forge and chat, agents, heights).

**`conformance`: a role.** A program bound to `programs`, `validators`,
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
`ducktape init` founds once its `founding.toml` is filled. To add poll to it:

1. `crates/sdk/founding/src/lib.rs`: `("poll", "poll_view")` in `PROGRAMS`,
   or `"poll"` in `UNVIEWED` for a program with no screen of its own.
2. `crates/sdk/founding/founding.toml`: a `[[programs]]` entry,
   `id = "poll"`, `code = "poll.wasm"` (and `params = …` if `init` takes
   any). The `[roles]` stay bound as they are.
3. qa's `founding.toml`: the same entry with `code = "@PROGRAMS@/poll.wasm"`.
   `kit up NAME` founds a fresh network with the program admitted at
   genesis and its account registered with identity.

A hook is also named in both files' top-level `hooks = [...]`, as
`activity` is.

A program added to a running network goes through the programs role instead
(`ducktape blob put --kind program --wait`, then `Schedule` at a height; [roles.md](roles.md), "Updating a
program"): its `init` runs with the scheduled params when the height comes.
