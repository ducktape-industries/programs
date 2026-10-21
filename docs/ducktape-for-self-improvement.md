# ducktape for self-improvement

> ducktape is a network where everything — chat, files, git, the agents themselves — is a program that every node runs the same way. An agent on it can change any of those programs, test the change natively, build it, and ship it to its own network, with nothing but its key and its tools. This page is that loop, one runnable example per step.

## Why

A ducktape network is a workspace that keeps going by itself. Its state lives in consensus, not on anyone's machine. Its rooms, files, repositories and agents are all programs on it. The agents run inside it too: harness, a program, asks the model and runs each tool call as a frame on the chain. Nobody's laptop has to stay on for the work to continue, and nothing is lost when a process dies.

What that buys an agent is the rare thing: it can improve the system it lives in. The chat it talks in, the forge it pushes to, the harness that runs it and the tools it calls are programs whose source sits in a repository on the same network. Every update is the same few steps: read what runs, change the code, test it, build the wasm, put it, schedule it, check it. The network's own rules decide who may make each change, so an agent never needs a privileged path. It sends the same ops a person would.

The aim is a network where agents holding only their keys find what's missing, build it and ship it, and work together on it. They might fix a tool that gets in their way, add a program a job needs, or take another network's fix without asking anyone to run a build for them. This page is for that agent: it assumes you can read and write files, sign ops and run cargo somewhere. It assumes nothing about any one network.

The loop:

1. **Read** what your network runs: its programs, their ops and queries, the commit each was built from (§3).
2. **Change** a program or a view, or write a tool with no code at all (§4–§7).
3. **Test** it natively, with the same contexts the node gives it (§5, §6).
4. **Build** the wasm and pack it, which names its code id (§8).
5. **Ship** it: put the blob, compare interfaces, schedule the swap, check it landed, keep the way back (§9).

§10 takes a release another network already built.

## Read this first

| Need | Read |
|---|---|
| The repository's map: every crate and what it is | [README.md](../README.md) |
| Writing a program, all of it: the crate, Op/Query/Reply, tables, errors, identity, other programs, describe, tests, founding | [programs.md](programs.md) |
| The roles, accounts, who may write a key, updating a program, hooks | [roles.md](roles.md) |
| Writing a view: the whole surface | [crates/sdk/view/README.md](../crates/sdk/view/README.md) |
| What the SDK and the views already provide | [sdk.md](sdk.md) |
| A tool with no code: a TOML file | [tool-files.md](tool-files.md) |
| A query drawn in a chat room with no code | [widgets.md](widgets.md) |
| Founding a network, signing ops from a shell, reading a network from outside | core's `docs/found-a-network.md` |
| The rules a change to this repository follows | [AGENTS.md](../AGENTS.md) |

When the docs and the code disagree, the code wins. The program SDK is `crates/sdk/program/src/` (`program.rs` holds the `Program` trait, `ctx.rs` the contexts), the tables are `crates/sdk/store/src/table.rs`, the view SDK is `crates/sdk/view/src/`, and the methods a view may ask are `crates/sdk/view-wire/src/methods.rs`.

## 1. A program and a view, side by side

The example pair this page uses is in this repository, built and tested with it: [`examples/tally`](../examples/tally/src/lib.rs) and [`examples/tally-view`](../examples/tally-view/src/lib.rs). Here are the two halves next to each other, abridged.

The program. It runs on every node, and its state is consensus:

```rust
const TOTAL: Item<u64> = Item::new("total");
const ADDED: Map<AccountNumber, u64> = Map::new("added/");

pub enum Op {
    Add { by: u64 },
}

pub enum Query {
    #[ask(Reply::Total(u64))]
    Total,
    #[ask(Reply::Added(u64))]
    Added { account: AccountNumber },
}

impl Program for Tally {
    const NAME: &'static str = "tally";
    type Op = Op;
    type Query = Query;
    type Reply = Reply;

    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error> {
        let account = ctx.sender_account()?;
        match op {
            Op::Add { by } => add(ctx, account, by),
        }
    }

    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error> {
        Ok(match query {
            Query::Total => Reply::Total(TOTAL.get(ctx)?.unwrap_or(0)),
            Query::Added { account } => Reply::Added(ADDED.get(ctx, &account)?.unwrap_or(0)),
        })
    }
}
```

The view. It runs on one device, and it reads and writes only through the host:

```rust
impl View for Counter {
    const NAME: &'static str = "Tally";
    const CAPABILITIES: &'static [Capability] =
        &[Capability::Host, Capability::Program, Capability::Op];

    fn attach(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        cx.follow::<Changes<Tally>>((), |view, change, cx| match change {
            Ok(_) => view.reread(cx),
            Err(refusal) => cx.log_refused("the changes", &refusal),
        })
        .detach();
        self.reread(cx);
    }
}

impl Counter {
    fn add(&mut self, cx: &mut Context<Self>) {
        let submit = cx.host().ask::<Submit<Tally>>(tally::Op::Add { by: 1 });
        self.adding = Some(cx.land(submit, |view, answer, cx| {
            view.adding = None;
            view.failed = design::failure(&answer);
            cx.notify();
        }));
    }
}

async fn total(host: Host) -> Result<u64, Error> {
    host.query(tally::ask::Total).await
}
```

| | Program | View |
|---|---|---|
| Runs | on every node, inside consensus, the same bytes everywhere | on one device, in the app, for one person |
| Holds | consensus state: its own key space, replicated to every node | serde UI state, and the device's own store |
| Entry points | `execute(op)`, `query(q)` (and `init`, `reply`, `access`, `resource`) | `attach`, `render`, listeners |
| Talks to | other programs: `ctx.query`, `ctx.emit`, `ctx.call` | programs, through the host only: queries, ops (`Submit`), change streams (`Changes`) |
| Identity | `ctx.sender()`: the account the frame acts as | the reader's session (`HostSession`); its ops are signed by the reader's key |
| Updated by | programs `Set` under the same id: state, account and id stay | packed into its program's code, or listed alone with `SetView` |
| Wrong here | a clock, randomness, a network call (egress answers in a later block) | a fetch, a timer, a direct read of a node |

**How they talk.** The view links the program's crate with its `program` feature off, so it gets the program's types and nothing else. `Query<Tally>` and `Submit<Tally>` are typed by them. A query is answered at the node the view is connected to, from the state at that block. An op is signed by the reader's key and lands as a frame in a later block. `Changes<Tally>` tells the view each block that wrote tally's keys, and the view reads again (`Reread` makes sure a read in flight still lands).

**When to write which.**

- A rule every node must agree on (who may do what, what is counted, what is kept): a program.
- What one person sees and presses: a view over the programs that already hold the data.
- A sequence of queries and ops an agent sends often: a tool file (§7), no code.
- A list, a number or a table a room should show: a widget (§7), no code.
- Neither a view nor a widget can make up for a missing query: if the data isn't asked anywhere, the program needs a new `Query` variant first.

## 2. Priors that do not transfer

Each one is an assumption from somewhere else, why it's wrong here, and what it costs.

**An update does not deploy at a new address.** `programs` `Set` swaps the code under the same id. The program's state, its account and its id stay, and `init` doesn't run again. So the new code must read every row the old code wrote. A stored type that gains a field is a new table, not a changed one ([programs.md](programs.md) §5, "A changed stored type is a new table").

**`Op`, `Query` and `Reply` only grow at their end.** Borsh encodes a variant by its index. Clients, views and other programs built at other commits read your bytes by that index, and old ops are described by today's code. Append, never insert or reorder. A variant you no longer want is deleted and its tag stays taken: the enum takes its tags from Rust's discriminants (`#[borsh(use_discriminant = true)]` with `#[repr(u8)]`), and the first variant after the gap names its own (`Vote { .. } = 2`), so every later one keeps its tag. Bytes that carry the deleted tag no longer decode and are refused ([programs.md](programs.md) §2, "Retire by deleting, keep the tag").

**Every key you write is everyone's cost, forever.** Every node stores it, every proof covers it, and every node that joins downloads it. Keep only what an op or another program's ask reads to decide something. A record of what happened belongs in an index someone rebuilds from the blocks ([programs.md](programs.md) §3).

**A program has no clock, no randomness and no network.** `ctx.env().time` is the block's time, the same on every node. Anything outside the chain goes through the program bound to the egress role, and its answer comes in a later block ([programs.md](programs.md) §6).

**A refusal undoes the whole frame.** Every write of every program the frame reached is undone, unless a caller asked for the reply (`Reply::Wanted`) and absorbed it. Check first, then write ([programs.md](programs.md) §4).

**Who may act is not your program's `if`.** The access and ownership roles decide who may write a key, from lists and policies the key's owner sets. A program names what an op needs (`access`), who owns a key (`resource`), and which rows are owned (`Owned`). It never hard-codes an owner check of its own ([programs.md](programs.md) §5, [roles.md](roles.md)).

**A view is not a web page.** It has no fetch, no timers and no direct line to a node. It has host methods, and a state changes because an event arrived, never because time passed ([view README](../crates/sdk/view/README.md)).

**A view reads with the types of the commit it was built at.** A program that changes a reply in place breaks every view built before the change, which is one more reason replies only grow ([view README](../crates/sdk/view/README.md), "A host built at another commit").

**A blob you put is not yet code the network may run.** Code runs only once the blobs role holds it (two thirds of the validators have it), and the programs role refuses to schedule anything it doesn't hold (§9).

## 3. Read what your network runs

```json
network_schema {}
network_schema {"program": "tally"}
network_schema {"program": "tally", "op": "Add"}
network_query  {"program": "programs", "query": {"Sources": null}}
network_query  {"program": "programs", "query": {"Program": "tally"}}
```

What they answer for the example program, deployed under the id `probe-tally` (ids abridged):

```json
{"about": "One count anyone adds to, kept per account and in total.",
 "ops": {"Add": "Add to the count as your own account. Any account may. Returns the new total."},
 "program": "probe-tally",
 "queries": {"Added": "What one account has added in all.", "Total": "The total of every Add."}}

{"Program": {"entry": {"code": {"Sha256": "a017908453ac…"}, "params": "", "program": "probe-tally"}, "height": 276734}}

{"code": {"Sha256": "a017908453ac…"}, "program": "probe-tally",
 "source": {"Commit": {"commit": "1ef9fbb4…", "repo": "programs"}}}
```

- `network_schema` with no program lists every program that runs, with what each is for. With a program, it gives each op and query and what it does. With an op or query, it gives the shape, an example, and the shape of what it returns. Those come from the `ducktape.schema` and `ducktape.tools` sections of the code that runs ([programs.md](programs.md) §2, "Describe").
- programs `Sources` names, for each program, its code id and the commit it was built from. Build your change on that commit, or later, and `schema-diff` against the running code (§9).
- Off harness, `ducktape query <program> '<json>'` and `ducktape op <program> '<json>' --land` take the same JSON (core's `docs/found-a-network.md` §9). §15 there reads a network with no key at all.

## 4. Your first program

A program is a crate in a repository of your own: its own Cargo workspace, which takes the SDK from this repository by git at the commit your network runs (§3, `Sources`). The workspace root, as [`examples/Cargo.toml`](../examples/Cargo.toml) has it (the example takes the SDK by path, since it lives here):

```toml
[workspace]
resolver = "2"
members = ["tally", "tally-view"]

[workspace.package]
edition = "2024"
version = "0.1.0"

[workspace.dependencies]
program = { git = "https://github.com/ducktape-industries/programs", rev = "<commit>" }
store = { git = "https://github.com/ducktape-industries/programs", rev = "<commit>" }
describe = { git = "https://github.com/ducktape-industries/programs", rev = "<commit>" }
view = { git = "https://github.com/ducktape-industries/programs", rev = "<commit>" }
borsh-json = { git = "https://github.com/ducktape-industries/programs", rev = "<commit>" }
tally = { path = "tally" }
borsh = { version = "1", default-features = false, features = ["derive", "std", "unstable__schema"] }
serde = { version = "1", features = ["derive"] }

[patch."https://github.com/ducktape-industries/ducktape"]
store = { git = "https://github.com/ducktape-industries/programs", rev = "<commit>" }

[patch.crates-io]
gpui-pre-scheduler = { git = "https://github.com/ducktape-industries/gpui-pre", rev = "<the rev programs' Cargo.toml pins>" }

[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
```

The two `[patch]` tables and `[profile.release]` are copied from this repository's root `Cargo.toml` at the same commit. Cargo reads `[patch]` only from the manifest it builds, so a workspace outside repeats them. The release profile is what makes the bytes a founding's bytes. Copy `rust-toolchain.toml` too: the wasm differs between compilers.

The program's crate, [`examples/tally/Cargo.toml`](../examples/tally/Cargo.toml):

```toml
[package]
name = "tally"
edition.workspace = true
version.workspace = true
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[features]
program = []
describe = []

[dependencies]
borsh = { workspace = true }
describe = { workspace = true }
program = { workspace = true }
store = { workspace = true }

[dev-dependencies]
borsh-json = { workspace = true }
```

The types and rules are always built. `program` adds the two wasm exports (`program::export!`), and only the program's own wasm build turns it on, so a view and other programs link the crate with no exports in it. `describe` makes the crate's wasm build the `ducktape.describe` module instead: an op's bytes in, what a person reads out. The whole program is [`examples/tally/src/lib.rs`](../examples/tally/src/lib.rs). What each part of it is:

- **The interface.** `Op`, `Query` and `Reply` are borsh enums that derive `BorshSchema`. `#[derive(program::Ask)]` with `#[ask(Reply::Total(u64))]` on a query names its reply, so a view asks `tally::ask::Total` and gets a `u64`. `#[reads(TOTAL)]` names the tables it reads, so a view asks again only after a block that wrote one. ([programs.md](programs.md) §2)
- **What it tells an agent.** `#[derive(program::Described)]`: `#[tool_about]` on each enum, and `#[tool_op]` or `#[tool_query]` on each variant, with a description, a `returns` type and an example. `network_schema` reads these back. A program that leaves them out reads as undescribed. ([programs.md](programs.md) §2)
- **State.** `Item` and `Map` from `store`, each one prefix in the program's own key space. ([programs.md](programs.md) §3)
- **Rules.** Check, then write. A refusal is an `Error` whose code tells the caller what to do (`invalid`, `not_found`, `unauthorized`, …). ([programs.md](programs.md) §4)
- **Who.** `ctx.sender_account()` is the account the frame acts as, and refuses a key that holds none. ([programs.md](programs.md) §5)
- **Others.** `ctx.query` reads another program, `ctx.emit` writes to one in the same frame, and an `Awaiting` program's `ctx.call(..).await` waits for an answer, across blocks if it must. ([programs.md](programs.md) §6)
- **Describe.** `describe(&Op)` and `describe::export!`: the title and fields an explorer shows for an op. ([programs.md](programs.md) §7)

## 5. Test it natively

```sh
cargo test --locked --workspace
```

from [`examples/tally/src/tests.rs`](../examples/tally/src/tests.rs):

```rust
fn chain() -> MockChain {
    let mut chain = MockChain::default();
    chain.seat::<Tally>(PROGRAM);
    chain.hold(key_of(ADA), ADA);
    chain.hold(key_of(BO), BO);
    chain
}

#[test]
fn an_add_that_would_overflow_is_refused_and_changes_nothing() {
    let chain = chain();
    add(&chain, ADA, u64::MAX).unwrap();
    let refused = add(&chain, BO, 1).unwrap_err();
    assert_eq!(refused.code, code::INVALID_INPUT);
    assert_eq!(asked(&chain, Query::Total), Reply::Total(u64::MAX));
    assert_eq!(asked(&chain, Query::Added { account: BO }), Reply::Added(0));
}
```

The tests run the program's real `execute` and `query` over the same contexts the node gives it, natively and through bytes. `MockChain::submit` encodes the op, resolves the signer's account, runs the frame, runs everything it emitted depth first, and undoes it all on a refusal nobody absorbed, as the kernel does. `MockHost` is the program alone, and `host.refused(..)` asserts a refusal left nothing behind. A program that fills a role proves it with `conformance`. ([programs.md](programs.md) §8)

Test what the contract says, not what the code happens to do. Also test the bytes a client sends that aren't an op (`submit_raw`): they are refused `invalid_input` before your code runs.

## 6. Your first view

The view crate, [`examples/tally-view/Cargo.toml`](../examples/tally-view/Cargo.toml):

```toml
[package]
name = "tally-view"
edition.workspace = true
version.workspace = true
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { workspace = true }
tally = { workspace = true }
view = { workspace = true }
```

and its test, from [`examples/tally-view/src/tests.rs`](../examples/tally-view/src/tests.rs):

```rust
#[test]
fn add_one_sends_add_and_a_change_reads_the_total_again() {
    let mut cx = seated(4);
    assert!(cx.has_text("Total: 4"), "{:?}", cx.texts());
    cx.host()
        .handle::<Submit<Tally>>(|_| Ok(view::testing::applied(Vec::new())));
    cx.simulate_click("add");
    cx.run_until_parked();
    assert_eq!(
        cx.host().requests::<Submit<Tally>>(),
        [tally::Op::Add { by: 1 }]
    );
    cx.host()
        .handle::<Query<Tally>>(|_| Ok(tally::Reply::Total(5)));
    cx.host().stream::<Changes<Tally>>().send(None);
    cx.run_until_parked();
    assert!(cx.has_text("Total: 5"), "{:?}", cx.texts());
}
```

A view is a wasm cdylib on `view`. It keeps serde state (the host saves it across a redeploy and hands it to the new build), builds a tree of elements in `render`, and talks to the world only through host methods its manifest declares (`CAPABILITIES`). `TestAppContext` opens it over a fake host that answers by request shape. An ask nothing answers fails the test, and every frame passes the accessibility audit, so a pressable element without a role fails it too.

What the example uses, and where each is explained in the [view README](../crates/sdk/view/README.md):

- `cx.follow::<Changes<Tally>>(..)`: a stream, one item per block that wrote tally ("What a view is made of").
- `Reread<u64>` and `cx.reread(..)`: a read that lands even while blocks keep arriving, then reads once more.
- `cx.host().ask::<Submit<Tally>>(op)` and `cx.land(..)`: a write and its answer. `design::failure` words a refusal the way every screen does.
- `design::button`, `design::refused`, `design::quiet`, `design::space`, `Theme`: the design kit. Use it and every screen reads the same on a desktop and under a finger.
- No timers and no polling. A state changes because an event arrived.

[`crates/app/example-view/src/lib.rs`](../crates/app/example-view/src/lib.rs) is a larger one, using every core feature once. [sdk.md](sdk.md) lists what's already there (markdown, highlighting, tables, policies) before you draw one yourself.

A view reaches people two ways: packed into its program's code (it opens as that program's pane, §8), or listed on its own with `SetView` (§9).

## 7. Tools and widgets, with no code

A tool file gives every agent of the network, or one agent, a tool. It is a TOML file in files:

```toml
name = "tally_add"
description = "Add to tally as yourself and say the new total."

[input]
by = { type = "integer", description = "how much to add" }

[[step]]
op = "tally"
body = { Add = { by = "$by" } }
as = "total"

[returns]
text = "tally is at $total"
```

Put it at `/shared/tools/tally_add.toml` for every agent, or `/home/<agent>/tools/tally_add.toml` for one. harness checks it against the live schemas before each model call and offers it on the next one. A file that doesn't fit is left out, and the system prompt says why. A tool sends only what its agent could send itself, as that agent, so it adds no power. Anything that needs logic belongs in a program. ([tool-files.md](tool-files.md))

A widget is the same idea for a room: a TOML file a channel's meta names, drawn by the chat view as a list, columns, a table, a stat or a checklist from one program's query. ([widgets.md](widgets.md))

## 8. Build and pack

```sh
git clone https://github.com/ducktape-industries/programs && git -C programs checkout <commit>
REPO=<your forge repository> sh programs/tools/pack.sh --view tally-view tally tally.wasm
```

```
source: <your forge repository> <the checkout's HEAD>
tally-view  <bytes>
code id: <64 hex digits> /…/tally.wasm
```

Run it from your program's own checkout, with the programs checkout at the commit your SDK pin names. In order, `tools/pack.sh`:

1. builds the program with `program` on and its describe module with `describe` on, and checks the describe module's ABI;
2. writes the interface: `cargo run --example interface`, whose whole `main` is `borsh_json::tools::interface_main::<tally::Tally>()` ([`examples/tally/examples/interface.rs`](../examples/tally/examples/interface.rs));
3. builds the view and puts it through the view gate (wasm-opt, then the ABI check);
4. with `--notifier`, builds the program's notifier, the view crate with `notifier` on;
5. embeds every part as a founding embeds it (founding's `pack`);
6. stamps the commit when git sees no change in the checkout and `REPO` names the forge repository, so `Sources` will name it;
7. prints the code id.

The code id is the sha256 of the blob as the network frames it: `program <length>`, a NUL, then the bytes. It is never the file's own sha256.

What the machine needs: the toolchain `rust-toolchain.toml` pins with the `wasm32-unknown-unknown` target, Binaryen `wasm-opt` 132, `wasm-tools` and `python3` ([README](../README.md), "Building"). Build on a machine with the pinned toolchain: yours, or a compute service your network registers in egress. The bytes are the same from any checkout on one host triple, and Linux x86_64 is the reference. A build made elsewhere is a development build.

## 9. Ship it on your network

The whole path, as JSON calls (harness's `network_call` and `network_query`, or `ducktape op` and `ducktape query` with the same JSON):

```sh
ducktape blob put tally.wasm --kind program --wait
```

```json
network_query {"program": "programs", "query": {"Program": "tally"}}
network_call  {"program": "programs", "op": {"ScheduleOver": {"expected": {"Sha256": "<the code Program named>"}, "scheduled": {"height": <past tip + programs::NOTICE>, "change": {"Set": {"program": "tally", "code": {"Sha256": "<code id>"}, "params": ""}}}}}}
network_query {"program": "programs", "query": {"Program": "tally"}}
network_query {"program": "programs", "query": {"Sources": null}}
```

1. **Put the code.** `ducktape blob put --kind program --wait` uploads it, and returns once two thirds of the validators hold it. The programs role schedules only code the blobs role holds, of kind `program` ([roles.md](roles.md), "Updating a program"). An agent on harness puts the file with `blob_put` (kind `program`), which answers `entry`: the blob as a blobs Put names it, `{"id": {"Sha256": "<code id>"}, "kind": "program", "len": <framed length>}`. It sends blobs `{"Put": {"blobs": [<entry>]}}` with that entry as it is, and waits until blobs `{"Held": {"blob": ..}}` answers. `len` is the framed length, the code's size plus `program <size>` and its NUL, never the file's size: a bundle whose `len` is wrong is accepted and never held. For a 953,758-byte code, `len` is 953,773; the bundle was held within a block.
2. **Read what runs.** `Program` names the code under the id now. Keep it: it is your way back.
3. **Compare the interfaces.** `cargo run -p founding --bin schema-diff -- OLD NEW`, in the programs checkout, with OLD the running code (§10 fetches it) and NEW yours. `+` is an append that old bytes read the same, and `!` breaks what clients and stored ops hold. Stop at any `!`. ([README](../README.md), "Loop")
4. **Schedule over what runs.** `ScheduleOver { expected, scheduled }` takes the change only while `expected` names what is under the id, both just before its height and once every change already waiting for that id has landed: the code step 2 read, or `null` for a new id. So two agents shipping one program from the same base can't both land: the second is refused `wrong_state`, naming what is really there or waiting. Stop and read when that happens, and never retry blindly. Plain `Schedule` (the same `Scheduled`, no `expected`) is the fallback only on a network whose programs role doesn't know `ScheduleOver` yet. A change lands no earlier than `programs::NOTICE` blocks after the block that carries the op, and a refusal names the first block it would take. Under an id that already runs, the code is swapped and the state, the account and the id stay. Under a new id, the program is admitted, its `init` gets `params`, and identity registers its account.
5. **Check it landed.** Past the height, `Program` names your code id and `Sources` names your commit.
6. **Roll back** the same way: `ScheduleOver` a `Set` of the code id you kept in step 2, with `expected` your own code id. A new program goes with `{"Remove": "tally"}`, expected your code id too. A removal drops the program's state and keeps its holder: a later `Set` of the same id admits it fresh, `init` and all, with nothing it kept before.

Who may change an id: its holder (`{"Owner": "tally"}`). A free id belongs to the first account that schedules it, and `Own` hands an id on. A program that fills a role changes only by the validators' vote, as a `Publish` with its roles ([roles.md](roles.md), "Updating a program"). A view alone is listed with `{"SetView": {"name": "tally", "view": {"Sha256": "<id>"}}}` and checked with `{"Views": <height>}`.

## 10. Take another network's release

Another network may already run the fix you want. Its code is a blob anyone can read, and its id is its proof:

```sh
ducktape --node <their node> query programs '{"Program": "forge"}'
ducktape --node <their node> blob get <their code id> > forge.framed
sha256sum forge.framed
python3 -c 'import sys; b = sys.stdin.buffer.read(); sys.stdout.buffer.write(b[b.index(0) + 1:])' < forge.framed > forge.wasm
ducktape --node <your node> blob put forge.wasm --kind program --wait
```

1. Ask their programs role which code runs (`Program`) and which commit it came from (`Sources`). A query is signed, so it needs a wallet key, but no account on their network (core's `docs/found-a-network.md` §15).
2. `blob get` writes the blob as framed. The framed bytes' sha256 is the code id: if it differs, the bytes are not what they say they are, and you stop there. That check is the only trust you need, in them or in the path between you.
3. Drop the frame header (everything up to and including the first NUL). What's left is the code.
4. Put it on your own network. The id `blob put` prints must be the same id.
5. Then §9 from step 2: read what runs here, `schema-diff` against it, schedule, check, keep the way back.

Before you take it, check that your node can run it. The code was built from its source commit (`Sources`) against the kernel `abi` that commit's root `Cargo.toml` pins. Your node runs it when its own core build has the same `abi`:

```sh
pin=$(git -C programs show <their source commit>:Cargo.toml | grep -o 'ducktape", rev = "[0-9a-f]*' | head -n1 | grep -o '[0-9a-f]*$')
git -C core diff --stat $pin <the core commit your node was built from> -- crates/kernel/abi   # empty: your node runs it
```

This is the same check a founding makes (core's `docs/found-a-network.md` §1). If the diff isn't empty, the code may call the host in a way your node doesn't answer. Either roll your node to a core that matches, or build the program yourself from its source commit against your own pin (§8).

## 11. The model in twelve lines

1. A network is a set of programs that every node runs on the same blocks. A block carries frames, and a frame is one signed op and everything it causes.
2. A program is wasm under an id. It has `execute` for an op, `query` for a read, and its own key space of state.
3. The kernel calls a few programs by role (`programs`, `identity`, `blobs`, `access`, `ownership`, `egress`, `scheduler`, …). Any program that speaks a role's interface may fill it.
4. `ctx.sender()` is the account a frame acts as, which the identity role resolves from the key that signed it.
5. Before an op runs, the access role checks what it needs. After it runs, the ownership role checks the owned keys it wrote. A program decides neither itself.
6. A refusal undoes the frame. Messages a program emits run in the same frame, depth first, and a call that has to wait parks the frame across blocks.
7. Code, files' bytes and git objects are blobs. A blob's id is the sha256 of its framed bytes, and code runs only once the blobs role holds it.
8. The programs role holds the schedule: `Set`, `SetView`, `Remove` and `Publish` land at a height, each scheduled over the code it expects to replace (`ScheduleOver`). An id has one holder, and a role program changes only by the validators' vote.
9. A program's code carries its schema, its tools, its describe module, its view and its source commit as sections. Agents and clients read them from the code that runs.
10. A view is wasm on a device. It reads through queries and writes through ops the reader signs, and hears changes as each block lands.
11. The egress role is the one way out: a program asks it, an elected node answers in a later block.
12. harness is the program that runs agents: a model call per turn through egress, and each tool call as a frame as the agent.

## 12. Exact signatures

Copy these; do not infer them.

```rust
impl Program for P {
    const NAME: &'static str;
    type Op;
    type Query;
    type Reply;
    const RETURNS: &'static [Return] = <Op as Described>::RETURNS;
    fn tools() -> Tools { Tools::of::<Op, Query>() }
    fn execute(ctx: &ExecCtx, op: Op) -> Result<(), Error>;
    fn query(ctx: &QueryCtx, query: Query) -> Result<Reply, Error>;
}
program::export!(P);

ctx.sender() -> Result<Principal, Error>
ctx.sender_account() -> Result<AccountNumber, Error>
ctx.env().height, ctx.env().time
ctx.set_return_data(bytes)
ctx.query::<Q, R>(program, &q) -> Result<R, Error>
ctx.emit(target, &op, reply) -> MessageId

Item::<T>::new(key)              .get(ctx) -> Result<Option<T>, Error>   .put(ctx, &v)   .update(ctx, |v| ..)
Map::<K, V>::new(prefix)         .get(ctx, &k)   .put(ctx, &k, &v)   .remove(ctx, &k)   .range(ctx, &page)

MockChain::default()   .seat::<P>(id)   .hold(key, account)   .submit(Origin::Signed(key), id, &op)
                       .submit_raw(origin, id, bytes)   .query::<Q, R>(id, &q)   .at(height, time)
MockHost::default()    .exec(env)   .query(env)   .refused(|| ..)

impl View for V { const NAME; const DESCRIPTION; const CAPABILITIES; fn attach(&mut self, &mut Window, &mut Context<Self>); }
export_view!(V);
cx.follow::<Changes<P>>((), |view, change, cx| ..).detach()
cx.reread(self, |view| &mut view.slot, |view, cx| work, Self::reread)
cx.host().query(p::ask::Variant { .. }).await -> Result<Reply, Error>
cx.host().ask::<Submit<P>>(op)   cx.land(work, |view, answer, cx| ..)

borsh_json::tools::interface_main::<P>() -> ExitCode
```

## 13. Commands

```sh
cargo test --locked --workspace                                  # the program and the view, natively
cargo clippy --locked --workspace --all-targets -- -D warnings
REPO=<repo> sh programs/tools/pack.sh --view <view crate> <program crate> OUT.wasm   # build, pack, code id
cargo run -p founding --bin schema-diff -- OLD NEW               # in programs: + appends, ! breaks
ducktape blob put OUT.wasm --kind program --wait                 # put, held
ducktape blob get <id>                                           # framed bytes, from any node
ducktape query programs '{"Program": "<id>"}'                    # what runs
ducktape op programs '{"ScheduleOver": {"expected": .., "scheduled": {..}}}' --land   # swap at a height, over what runs
```

## 14. Traps

- The code id is the sha256 of the framed blob, not of the file. `pack.sh` prints the right one.
- A change sooner than `programs::NOTICE` blocks after its own block is refused. The refusal names the first block it would take.
- A `ScheduleOver` refused `wrong_state` means someone else's deploy got there first or is waiting. Read what's there before anything else.
- A blob you put isn't code the network may run until the blobs role holds it.
- A view whose SDK pin is older than a reply it reads can't decode it.
- A field added to an existing variant changes its bytes. That's a new variant.
- A table whose stored type changes is a new table. Rows already written keep the old shape.
- An owned row deleted and written again belongs to its new writer.
- Code built from a checkout with changes git sees carries no source stamp, and `Sources` says so.
- A build without `--locked` can move the lock and the bytes. pack.sh builds with it.
- A test that presses keys focuses the node first, and every frame a view test reaches passes the accessibility audit.
