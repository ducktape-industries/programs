# Development

## Programs and roles

A view links its program by path and reads its types. A program is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape; the founding suite links it at
the revision `Cargo.toml` pins, and `abi` comes from the same revision.
What is not wasm lives elsewhere, except `crates/sdk/founding`, which packs a
founding out of what this repo builds. The forge smoke (real git against
`forge.wasm` on ducktape's runtime) is in the qa repo.

The kernel calls the boot programs by role, never by id. Each founding
program names the roles it fills (`roles = [...]` on its `[[programs]]`
entry in `founding.toml`): `programs`, `validators`, `identity`, `blobs`,
`awaits` and `halo` always (`abi::role::REQUIRED`), and optionally
`access`, `egress`, `scheduler` and `ownership` (on a network that fills
neither access nor ownership, no op is checked and no write is guarded). The programs role keeps that table from genesis
on, and only the validators' vote changes it, by publishing a program with
the roles it fills. Every `Env` carries the table (`env.roles`, a map from
role name to program; the names are constants in `abi::role`, and a new
role is a new row). The host asks the identity role once per frame which
account the frame acts as, and hands it to the program as `ctx.sender()`.
Before an op runs (not a reply and not the chain's own call), the host
asks the target program what the op needs and has the access role check
it. A role is an interface in `abi`,
so another program that speaks it can fill the role.
[`docs/roles.md`](roles.md) has the roles, the account model,
the update path, and how to write a program for a role.

Membership changes only by a vote of the current validators, in `valset`;
a stranger enrolls there as a resident, by invite or while the door is
open. The same vote decides a program swap and the roles a program fills:
`programs` schedules or cancels a change on a message from the program that
fills the validators role (`program::Env::authority`), or from the account
that holds the id (the vote holds every founded one, and `Own` hands one
on), which never changes a program that fills a role. An
update replaces a program's code under the same id. The system programs beyond the boot set are archived at
`ducktape-industries/ducktape-system-modules-archive`.

## A program

```rust
use program::{Error, ExecCtx, Principal, Program, QueryCtx};

/// A count per account.
pub struct Counter;

impl Program for Counter {
    const NAME: &'static str = "counter";
    type Op = u64;
    type Query = Principal;
    type Reply = u64;

    fn execute(ctx: &ExecCtx, by: u64) -> Result<(), Error> {
        let who = program::abi::encode(&ctx.sender()?);
        let n: u64 = ctx.record(&who)?.unwrap_or(0);
        ctx.put(who, &(n + by));
        Ok(())
    }

    fn query(ctx: &QueryCtx, who: Principal) -> Result<u64, Error> {
        Ok(ctx.record(program::abi::encode(&who))?.unwrap_or(0))
    }
}

program::export!(Counter);
```

An entry point receives its context (`ExecCtx` reads, writes, sends and
sets return data; `QueryCtx` reads) and nothing else: `ctx.env()` is the call's
`Env`, and nothing reaches the host by any other path. `ctx.sender()` is who
the write acts as, the account the host resolved or the chain. `access`
answers what an op needs of the access program and `resource` who owns a
key; both answer nothing unless a program says otherwise, so a counter
anyone may add to leaves them out. `export!`
only emits the `alloc`/`call` exports. The same contexts run over a
`MockHost` natively, so a program's test runs its real code
(`crates/sdk/program/examples/counter.rs`); programs that emit to each other run
together on a `MockChain`, a submission as one frame as the kernel runs it.

[`docs/ducktape-for-self-improvement.md`](ducktape-for-self-improvement.md)
is where an agent improving its own network starts: why ducktape, a program
and a view side by side, then read, write, test, build, pack, ship, roll back
and take another network's release, each step with a runnable example from
`examples/`.

[`docs/programs.md`](programs.md) is the walk from `make new-program` to
a program founded in qa: the crate and its features, `Op`/`Query`/`Reply`
and the append-only rule, tables and pages over `store`, errors and codes,
sender and identity, queries, emits and replies, `describe`, the native
tests, the edit loop and founding.

## A view

A view implements `Render::render(window, cx)` on serializable, `Default` state,
and `View::attach(window, cx)` to follow the host. `Context<V>` dereferences to
`App`; `listener` turns a method into a typed event callback, whose route id
the tree takes when it lowers to `wire::Node`. Call `cx.notify()` after state changes: the view decides when it
renders (`view/README.md`, "When a view renders").
Unnotified frames retain their tree and event routes; native debug builds catch
serialized state changes without notification.

A view composes from child entities as a gpui view does: `cx.new` builds
one from any state, `entity.update(cx, ..)` changes it from a listener or a
task, and `cx.emit`/`cx.subscribe`/`cx.observe` connect it to its parent. The
snapshot carries the root alone, so `attach` builds the children
(`view/README.md`, "Child entities").

`cx.host()` is the typed host method. Keep the `Task` returned by `cx.spawn`, or
call `.detach()`; dropping it cancels the future and any owned subscription.
Consume host streams with `while let Some(item) = stream.next().await` and update
through `WeakEntity`. `TestAppContext` supplies typed fake handlers and feeds,
input simulation, and tree assertions. See `examples/exported_view.rs` in
`view` for a whole view, and each app view's `src/tests.rs` (chat and
explorer: `src/tests/`) for its tests.

Snapshot/restore transfers the root view's serde state, not entity identities.
Snapshots wait for ordinary work to settle; host streams start in
`View::attach`, which runs on a first mount and on a restore alike. Keep independent writes in separate tasks: an opaque joined
future sharing a stream waiter cannot expose whether its other work is pending.
The tree vocabulary, manifests, and five-function Wasm ABI are unchanged.

## Loop

1. Edit a program or a view.
2. `make dev`: rebuilds what cargo finds stale with the fast profile (one
   line per artifact: `name  1,181,498 B` or `unchanged`), gates the rebuilt
   views (ABI) and runs the native tests of the crates cargo rebuilt.
   `P=forge` / `V=forge-view` narrow it to one.
3. Push the branch and `make wasm-publish` (below); then in qa,
   `kit new NAME --modules <branch> && kit up NAME` downloads the
   founding and founds it, and `kit app NAME` opens the app on them.
4. Where a view's bytes go: `make wasm-why V=members-view` (`twiggy top`, `cargo install twiggy`).
5. Before an update lands, compare its interface with what runs:
   `cargo run -p founding --bin schema-diff OLD NEW`, each a program's code or
   a `.schema` `make wasm-schemas` wrote, prints each difference, `+` for an
   enum that grew at its end (old bytes read the same) and `!` for one that
   reads old bytes otherwise, and exits 1 on any `!`. Every change is held
   to it already: the tools repo's `interfaces` gate compares
   `crates/sdk/founding/interfaces/*.schema` at the change's merge base
   with dev against the change's own (`docs/programs.md`, "Retire, never
   delete").
6. A new program: `make new-program NAME=x`, then `make new-view NAME=x-view`;
   each prints what to do next, down to its place in `crates/sdk/founding`
   ([`docs/programs.md`](programs.md) walks it).
   `make scaffold-check` proves both still build, gate and test. `make test`
   runs everything; the founding suite builds the boot set itself.

## Building

`make test` (`cargo test --workspace`; the founding suite runs `make
wasm-programs` itself), clippy, `make program-wasm-check`,
`make view-wasm-check`, `make wasm-views`, `make scaffold-check`,
`make wasm-reproducible` and `make dl-publish-check` are what CI runs. The toolchain is pinned in `rust-toolchain.toml`.

Every wasm artifact is a build output: `make wasm-modules` builds every
program and every view, and writes every program's schema, under `$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/`;
nothing built is committed. `make dev` and a view's `--example live` build
with the `fast` profile instead (`Cargo.toml`): no fat LTO, sixteen codegen
units and no wasm-opt (a notifier is built for size, through wasm-opt,
whatever the profile), into `wasm32-unknown-unknown/fast/`, for the edit
loop and for garage, the network that runs dev; a founding, and every
program a hosted network (duckpond) runs, is always the release profile's
bytes. A hosted network's pods service, or eddy-claude (account 24), builds
a commit on the network's forge `dev`, and the builder puts it as a blob.
`WASM_PROFILE=fast` on a wasm target's command line asks for it
(`make dev WASM_PROFILE=release` for the bytes that ship), and in the
environment of an `--example live` it picks the rig's profile.
`make wasm-reproducible` proves the bytes do not depend on the checkout. They do depend on the host triple (the
Makefile says why at `WASM_RUSTFLAGS`), and the reference host is Linux x86_64:
a founding is packed from a build made there, and a Mac's build
is a development build.

`make wasm-founding` packs a founding, `founding.tar.gz`: each program with its describe module,
its schema and any view embedded (`crates/sdk/founding`), stamped with the commit it was built
from (`ducktape.source`, which the programs role's `Sources` reads back; a checkout with changes
git sees stamps none), `forge.params`, and a
`founding.toml` that `ducktape init` reads once its network, time, key and
address are filled. A network is founded from one built at a commit on forge
`dev`; core's `docs/found-a-network.md` walks a founding through every step.
A local network (qa's kit) builds its founding with `make wasm-founding`
too: `make wasm-publish` and `.github/workflows/release.yml`, which published
the GitHub release `wasm-<commit>`, are still in the tree, but the workflow
is disabled.

A program built outside this repository packs to code the same way:
`tools/pack.sh [--view VIEW_CRATE [--notifier]] PROGRAM_CRATE OUT`, run from a
checkout of the program's own repository, builds the program (`program`), its
describe module (`describe`), its interface and any view and notifier, embeds
each as a founding does (`founding::pack_parts`, through founding's `pack`
bin), stamps the checkout's HEAD as built from the forge repository `REPO`
names when git sees no change there, and prints its code id
(`founding::code_id`: the id the blobs contract gives the code framed as a
`program` blob, never the file's own sha256). The crate writes its interface with an `examples/interface.rs` whose whole
`main` is `borsh_json::tools::interface_main::<poll::Poll>()`, `borsh-json` a
dev-dependency, and its workspace's `[profile.release]` (and `[profile.notify]`
for a notifier) are this one's.

`make dev`'s view gate requires `wasm-tools` and Python 3; view releases,
and every notifier build (`make wasm-modules`, `fast` too), also need [Binaryen wasm-opt 132](https://github.com/WebAssembly/binaryen/releases/tag/version_132).
`make wasm-views` builds, optimizes, and checks the exact view ABI, printing
`name  bytes`. The bytes before wasm-opt stay beside each
artifact as `<name>.wasm.unoptimized`. `WASM_OPT=/path/to/wasm-opt` can
select the pinned optimizer. It preserves the view manifest and never
supplies imports or removes capabilities. `make wasm-why V=<view>` builds the
view with `--profile why` (release plus names, its own output dir) and runs
`twiggy top -n 25` over it; the release profile strips names, which is why
twiggy cannot read the shipped bytes.

GPUI uses the fork's default-off consumer configuration: `web` is disabled for
views and remains default-on in the native app. The root patches select the
fork's scheduler, logging, and MessagePack crates as well. MessagePack's `typed`
feature is enabled only for wasm: it follows the wire's Serde shapes and retains
the existing named encoding. Dynamic/ignored values remain supported. Shared
writers and table-driven keyboard enums avoid duplicate generated codec code.
Native and view roundtrip, limit, and malformed-input tests cover that boundary.
