# programs

The ducktape contract line and the programs written against it, one
repository. Everything here compiles to wasm:

```
crates/sdk/     abi error program store conformance describe ducklink view-wire view view-derive design
crates/system/  programs valset identity blobs awaits halo scheduler access
crates/app/     chat chat-view forge forge-view activity files egress attribution harness members-view node-view explorer-view settings-view
crates/lib/     gitcore borsh-json
```

| Path | What |
|---|---|
| `crates/sdk/abi` | the borsh bytes ABI a program and the host share: `ProgramCall`, `HostOp`/`HostReply`, `Env`, `Refusal`, `Principal`, `Roles`, and the `role::programs`, `role::validators`, `role::identity`, `role::awaits`, `role::halo`, `role::scheduler`, `role::access` and `role::ownership` interfaces the kernel calls (`role::access` also holds `Party`, `Ownership`, `Need`, `Resource`, `Writers` and the `writers` rule over a key's `/`-separated ancestors), the `role::egress` interface the node reads (the blobs role's contract is ducktape's `blob::contract`, which programs link), under the kernel's names. A copy of ducktape's `crates/kernel/abi`, plus what only a view needs (`unhex`, `preview`, and `Kind::badge`/`note`, an account's badge) |
| `crates/sdk/error` | `Error { code, message }` and its `code` tokens, the one error type a program, the host and a view share (borsh; serde behind a feature). Depends on no ducktape crate; `program` re-exports it and `view-wire` carries it |
| `crates/sdk/program` | the program SDK, enough alone: the one `Program` trait (a program's `NAME`, the borsh `Op`, `Query` and `Reply` it speaks, what it does with each, and what each op needs of the access program and who owns a key, `access` and `resource`; a view names the program by that type, `Query<chat::Chat>`, and `view-wire` re-exports it), `Ask` and `Reads` (one question to a program as its own type, and the tables it reads), the `ExecCtx` and `QueryCtx` contexts its entry points receive (env, raw state, blobs, events, `set_return_data`, `verify`, and `emit` (a write another program runs in this frame) and `query` (a read of one)), `ExecCtx::sender` (the `Principal` the host resolved: an account, a program's too, or `Root`), `Env::actor` (the parties the access program judges: the sender's account and, for a message, the sending program), `ctx.resource(program, key)` (another program's owner of a key), `Env.roles` (the program that fills each role, as the programs role answers), the `Env` origin checks (`signer`, `sending_program`, `sent_by`) and `authority` (a message from the program bound to the validators role), `export!`, `Awaiting` and `export_awaiting!` (an `async fn execute(ctx: Ctx, op)` whose `ctx.call(target, &op).await` waits, parked between blocks when it must, for the target's answer), `ctx.schedule(height, target, &op)` and `ctx.cancel(entry)` (a run of `target` at a later height, through the scheduler role, and its cancel), `defer` and `resolve` (how a called program answers later), `Error` and its constructors and `decoded` and `MockHost`, the native host the same contexts run over in a test, and `MockChain`, which seats several programs over their own `MockHost`s and runs a submission as one frame, its messages and replies as the kernel runs them, and the scheduler role's due entries at the end of each block `at` moves past. `src/kernel.rs` is the one place the kernel's names (`Refusal`, `ProgramId`, `ItemRef`, `Scan`, …) become the SDK's (`Error`, `ProgramId`, `MessageId`, `Range`, …), byte for byte; `kernel::error_from`/`refusal_from` convert an error. `examples/counter.rs` is a program written with it alone |
| `crates/sdk/store` | optional typed storage over `program`'s contexts: the `Map`/`Set`/`Item` descriptors with `KeyCodec`, and `PageRequest`/`PageResponse`. A read takes `&QueryCtx` (an `&ExecCtx` serves it), a write `&ExecCtx`. A view links it and calls none of it |
| `crates/sdk/conformance` | proof that a program fills a role the kernel calls (`programs`, `validators`, `identity`, `awaits`, `halo`, `scheduler`), native over `MockHost`: a program takes it as a dev-dependency, implements the role's `Fixture` and calls its `run` in a test; see its [README](crates/sdk/conformance/README.md) |
| `crates/sdk/describe` | what an op means to a person: the pure wasm module a program ships in its `ducktape.describe` section, and the sandbox that runs it |
| `crates/sdk/ducklink` | the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no program names known here |
| `crates/sdk/view-wire`, `view`, `view-derive`, `design` | the host<->view wire (`view_wire::methods`: every method a view may ask, and in `methods::refusal` every code the host refuses one with), the runtime a wasm view is written against, the palette |
| `crates/system/programs` | the boot set's root: `programs`, the program that fills the programs role (its `Op`, `Query`, `Reply`). It keeps the network's role table (`Query::Roles`): genesis fills it from the roles each founding program names, and the validators' vote changes it by publishing a program with the roles it fills (`Change::Publish`). Its `tests/system.rs` founds ducktape's host over the bytes `make wasm-programs` built and drives every system program |
| `crates/system/valset`, `identity`, `blobs`, `awaits`, `halo`, `scheduler` | the other boot programs, the same shape: types always built, the wasm exports behind `program`. valset holds the members and runs their rules: enroll (by invite or an open door, with the halo address other members dial), a member moving its halo address, leave, and the validators' vote on members, the door, and an op of the program that fills a role (`Motion::Role`: programs changes, which include publishing a program with the roles it fills, and halo's lanes). identity holds every account that acts, `Account { number, card, control }`: a user's (a person holding their own keys), a program account's (managed by the account that created it, which suspends, resumes and revokes it and sets the script that runs its actions; whom it answers is its list in the access program, which harness reads) and each program's own system account (registered by the kernel as it admits the program). blobs records each bundle of blobs an account puts, by id, length and kind; a current validator receipts a bundle once its node holds every blob, and two thirds of the current validators make it held, which is when the host serves its blobs to a program. awaits keeps a ticket for each call a parked frame waits on, and hands the frame its answer when the called program resolves the ticket; a wait has no deadline. halo holds the lanes halo carries between members, each name with its wire id, the class its streams are served at most, whether it is served and whether the founding pinned it; only the validators' vote opens, sets or closes one, no vote pauses or closes a pinned lane (the founding pins blobs'), and an id is never handed out twice. scheduler keeps the runs programs schedule for later heights (`Schedule { height, target, payload }`, answered with the entry id; only the program that scheduled an entry cancels it, and only before it runs), lists them by height (`Due`), and keeps each run's receipt, which the kernel hands it (`Ran`) after it runs the entry at the end of its block as the program that scheduled it (`Cause::Scheduled`) |
| `crates/system/access` | the program that fills the access role: who may write each key another program names. Before every op it executes, the kernel asks the target program what the op needs (`Program::access`: a `Need::write(key)` or `Need::delete(key)` per key) and, with any need, runs this program's `Op::Check { program, actor, needs }` (the kernel's alone). A program keeps its owners itself and answers a key's owner and default list from what it stores (`Program::resource`: `Resource { owner, default }`); this program keeps only the lists set on keys (`LISTS`): `Op::Set { program, key, list }`, by the key's owner or the program itself, and `Op::Move { program, from, to }`, by the program alone, when a path is renamed. A write is allowed by the nearest list at or above the key (its `/`-separated ancestors, the key first), else that key's default, else anyone; a delete also by the owner of any key above it, and drops the lists at and below it. Queries: `List`, `Resource`, `Writers`, `Lists { program, page }`. [roles.md](docs/roles.md), "Who may write a key", has each program's keys |
| `crates/system/ownership` | the ownership role: who may write each owned key a program holds. The kernel asks it once per run that wrote owned keys (`role::ownership::Op::Guard`); it records each new key's owner and the policy its owner's defaults choose, and admits a change or delete only where the key's policy admits the actor. Its owners' ops: `Create`, `Edit`, `Retire`, `Switch`, `Relink`, `Transfer`, `SetDefault` |
| `crates/app/chat`, `chat-view` | the reference app program: `chat` is one crate whose types, rules and `Chat` program are always built (native, tested over `MockHost`), and whose wasm exports sit behind its `program` feature. `chat-view` links `chat` with the feature off: the types, no host import, no program export. A mention is a span marked `Mark::Mention`: picking an account in the composer writes `<@n>`, and chat itself, as a message is posted or edited, turns a typed `@name` into the same span when exactly one account holds that name (the identity role's profiles, compared after NFC and lowercasing). A typed name is an `@` at the start of a word (not after a letter, a digit, `-`, `_`, `.` or `@`) followed by letters, digits, `-`, `_` and `.`, less any trailing `.`, so `@chief,` and `@chief.` name `chief`; it is read in paragraph and quote text, never in code or a link. A name that no account or several hold stays text, and a name with any other character (a space) is picked, not typed. Chat tells attribution (`chat::attributed`) each message's mentions, a direct message's counterpart (`DIRECT`), and for a thread reply each account that took part in the thread before it (`THREAD`): the root's author and every account whose first reply there came earlier. chat-view also draws widgets: TOML files in files that a channel's meta names (`widget.header`, `widget.pinned`, `widget.side`), each one program's query read by that program's schema and drawn as a list, columns, a table, a stat or a checklist ([widgets.md](docs/widgets.md)) |
| `crates/app/forge`, `forge-view` | the git server as a program, the same shape as `chat`: a push is one op carrying git's ref commands and naming its pack, a held blob, a merge is an op that lands the commit the client built, fetch and the ref advertisement are queries; a git object's blob id is its oid. it links `gitcore` for the git; merging is the client's. The program runs natively over `MemorySandbox` (forge and chat seated on a `MockChain`), which is where `fixtures/` comes from; `forge-view` links `forge` with `program` off. A program that sends `Watch { deliver }` hears every ref forge moves (push, merge, squash), every change it opens and every Preview it makes, each as `deliver ++ borsh(Watched)`; its refusal never fails forge's write. A branch with required checks (`RequireChecks`) moves only by a merge of a change whose latest Preview (`Preview`, the squash the merge would land on the target's head) its reporter's `Status`es mark Passed |
| `crates/app/actions`, `actions-pods` | CI on forge. `actions` watches forge: a push, or a change's Preview (forge `Preview`, asked for when it opens and whenever its source or target moves; one forge refuses, a conflict, makes an errored run that says why), reads `.forge/workflows/*.toml` at that commit (`on.push` branches, `on.change` opened/updated, `[notify] channel`, `[[jobs]]` with `id`, `runs-on`, `image`, `steps`, `env`, `inputs`) and queues one check per job (queued, running, passed, failed or errored; each is its commit's forge `Status` in `<workflow>#<job>`), offered to the program runners whose labels cover `runs-on`; a runner claims and reports it, anyone reruns it, and a push's red check is posted to the workflow's channel. `actions-pods` is a runner: it runs a claimed check as a pod of the pods service through egress, hears it end on the pod's `wait` stream, copies its `/out` into files at `/shared/actions/<repo>/<commit>/<workflow>/<job>-<attempt>/`, reports and deletes the pod |
| `crates/app/activity` | the hook the kernel tells of every frame a block carries ([roles.md](docs/roles.md), "Hooks"): it keeps each frame (its place, time, signer, target and whether it applied) newest first network-wide (`Frames`), by the key that signed it (`Signed`) and by the account it acted as (`Acted`), and counts each account's frames by program (`Used`). It takes no op. explorer-view's transaction lists and both views' lists of what an account did are its frames: a row is the program, whether it applied, its place and its age, and no transaction is read to draw it |
| `crates/app/files`, `files-view` | a replicated filesystem as a program: a commit edits one tree of directories, files and symlinks and names the result by a snapshot; each tree records, beside every entry, the account number that first made it with `Put`, `Mkdir` or `Symlink` (and the directories that write made); a move carries it with the entry and a remove drops it. Who writes a path is the access program's to decide, with the path as the key: files answers a path's owner, its creator in the head tree, and for a home folder `/home/<name>` the one account the identity role names `<name>` (`Profiles`). `Put`, `Mkdir` and `Symlink` write their path, `Remove` deletes it, `Move` writes both ends and moves the lists at and below the old path to the new one (`access::Op::Move`), and `Unpin` deletes the pin's key (`files::pin_key(name)`, owned by the account that made the pin). A write is allowed by the nearest list at or above the path, which its owner sets with `access::Op::Set` (`program::Ownership`: `Owner`, its creator alone; `Listed`, its creator and the parties listed, each a `Party::Account` by number or a `Party::Program` by id; `Anyone`), and with none by anyone; the owner of a folder above a path may also remove it. An account matches a list by its number; a program matches when the write is its message (`Origin::Program`), whatever account it acts as, so a list naming harness allows every agent's write through harness. A commit by an account at the namespace roots themselves, `/home`, `/shared` and each `/home/<name>`, is refused; the chain's own writes are never checked. A list only controls writing: every member holds every file. `Change::SetOwnership` is always refused. A file's bytes are `chunk` blobs of at most `CHUNK_SIZE` (1 MiB), put through the node's blob upload (a view's `op.put`) and held by the network before one commit names them: no transaction carries a file's bytes; trees, files and snapshots are blobs too, and its state keeps only the head, the history, a snapshot's place in it and the named pins. Reads (stat, list, read, find, grep, diff) take the head or any snapshot it minted. `files-view` browses it: a folder at the head, a file's text, the history and its pins, who writes a path (the access program's `Writers`), and the writes the reader may make there (a new folder or a new text file, an edit, a rename or move, a removal), each one commit with a message; a new folder or file may carry a write list of its own, which files-view sets in the access program once the commit lands |
| `crates/app/attribution` | which accounts each program's objects relate to, and every change to that. A program reports an object of its own (`Attribute`: its relations as of a revision that only grows; a mention, an author, an owner, an assignee, or a reason it names) and attribution records what was added, withdrawn or transferred, indexed by recipient and by object. Each change is handed to one program at most: when its recipient is an active program account whose script names a program (identity's `SetScript`), and that program registered (`Register`, with the prefix of its own `Op` attribution puts before the change), attribution hands it the change as it is recorded; its refusal changes nothing. A change about any other account is recorded and handed to no one. chat reports each message's mentions, in a direct message the counterpart (`chat::attributed::DIRECT`), and in a thread reply everyone who took part in the thread before it (`chat::attributed::THREAD`) |
| `crates/app/egress` | a program's call to something outside the chain, bound to the egress role (`abi::role::egress`, whose `Op` and leading queries it speaks). A `Request` names a service: `Relay` carries a note and is answered with nothing, a later block's call back; `Http` names a service, an `http::Head` (a method, a target, headers) and the body as blob parts. For the reserved `http` service the target is a full `http://` or `https://` URL on the public internet; any other service is registered (`Register`) by an account, which owns the name and assigns the node keys that may serve it (`Assign`), and its target is a path under the base the serving node's operator gives it. A member node's key says which services it serves (`Serve`). A program calls egress with a request and awaits it; egress checks the head (the framing headers and every `ducktape-` header are the node's to set) and that its parts are on the network, then elects in turn among the member nodes serving that service, and for a registered service only those its owner assigned; that node alone answers (`Answer`), in a later block than the request, and its answer, or its reported failure as a refusal, resolves the program's call. An `Http` request's call answers an encoded `http::Response`. One that asks for its reply as events (`Delivery::Events { deliver }`) hears each event of a `text/event-stream` reply (`Event`) as its own op, `deliver` followed by the encoded event, and its call answers the stream's end (`EndStream`): `Complete` as the `http::Response` with its status and headers and an empty body, a cut as a refusal; each event moves its expiry `wait` blocks on, and any other reply comes whole. An event's `seq` counts from 0 per call the node makes: a repeated `seq` is a repeat, and a `seq` back at 0 is the node calling the service again (it restarted), which voids what came before; egress passes `seq` through unchecked, for the requester to act on. A relay is answered whole, and asking one with `Delivery::Events` is refused. A request waits `wait` blocks, `DEFAULT_WAIT` (64) when the caller names none: egress schedules its own `Expire` for that height through the scheduler role as it records the request (a network with no scheduler role refuses every request), cancels it when the answer comes, and an `Expire` from that scheduled run alone drops the request and answers the call with a refusal that says it expired; a later answer is refused. The node itself does the answering: ducktape's `noded` reads this program after each block, calls what it was elected for with the services its `egress.toml` names, and signs `Answer` and `Serve` with its key |
| `crates/app/harness` | agents that consensus runs, nothing outside it driving. An agent is a program account of the `Agent` category whose script calls harness (identity's `SetScript`); the script's `config` is the path of its configuration file in `files`: Markdown whose TOML front matter, between `+++` lines, names a model id, `max_tokens`, an optional `wait` (the blocks egress waits) and an optional `thinking` (`off`, `low`, `medium`, `high` or `max`; absent leaves the model's default, and the Anthropic wire sends `off` as thinking disabled and a level as adaptive thinking at that effort), and an optional `compact_at` (tokens: once a thread's measured context reaches it, harness summarizes the thread before its next turn; at least 1, and absent leaves the model's default), and whose body is the system prompt (`harness::Config`). A model call sends each paragraph of the system prompt as its own system block, in order, then harness's own block, the ducktape resource manual: the agent's account and its manager, the conversation, and how writes, tool calls and chat replies work. Harness reads the file at the head at each model call, and each turn it records names the files snapshot it read. A model id is a record harness keeps (`SetModel { id, wire, service, name }`: the wire the model speaks, the registered egress service that reaches it, the vendor's name for it; the first account to set an id owns it, and only it, or a party its list at `harness::model_key(id)` in the access program names, changes or removes it with `RemoveModel`), with the context window harness learned from the vendor, and an optional `compact_at` default the same parties set or clear with `SetCompactAt { id, compact_at }` (`null` clears it; `SetModel` keeps it, and `RemoveModel` removes it). `Query::Agent` answers what harness resolves for an account: its configuration path, the configuration and its model, or what is wrong, and its current thread. Harness registers with `attribution` at its init, so a mention of the agent, a direct message to it, or a reply in a chat thread it started or replied in reaches harness and continues the agent's one current thread, whoever wrote it and wherever (any channel, direct message or chat thread; the input names its author and its place, and the model answers each where it came from), when the agent's list in the access program allows its author (`access::Query::List` at `harness::agent_key(agent)`, a key its manager owns, `Owner` when none is set: its manager always; `Listed` adds the accounts it lists and every program whose own system account wrote it, `Anyone` everyone). With no current thread, the first input opens one. The agent's manager, or whoever its list at `harness::conversation_key(agent)` in the access program allows, starts a fresh one with `NewThread { agent }`, or one that begins from the current thread's context with `ForkThread { agent }`, which copies the current thread's frames (its summary included) and leaves out a last turn whose tool calls are not all back; either becomes current. A thread that `NewThread` or `ForkThread` replaced closes once its loop is idle, and stays readable. `Query::Threads` lists an agent's threads newest first, and `Query::ThreadAt` names the thread an input from a place last reached. An agent with no current thread on record takes as current its open thread whose loop is idle and that moved last (its newest frame at the highest block), so on a network whose harness is swapped for this one the agent carries on in the thread it was last in; its other threads stay open and readable as they were, and a thread whose loop was running when the program was swapped lost that loop with the swap and is left as it is. A configuration that is missing, malformed or names no model record is told to the thread once as a notice, and the next message tries again. Each thread runs as one awaiting loop: harness puts the model's request for the thread as blob parts in the model's wire (one per message, so earlier messages are reused; `wire::anthropic` speaks the Anthropic Messages API, `/v1/messages`) and calls the program bound to the egress role (`env.roles.egress`) with an HTTP request to the model's service. A turn's request asks for a streamed reply (`Delivery::Events`, delivered as harness's `Streamed { thread, event }`): egress hands harness each event the elected node relays once and in `seq` order, in the block that carries it or the one that fills the gap before it, and harness folds it, in the model's wire, into the thread's partial reply (`Query::Partial`), which harness-view shows growing. A repeated event is dropped, an event back at `seq` 0 starts the reply over, and a gap breaks it. When the stream ends, the partial becomes the model's turn, the same turn the whole reply would have given; a broken stream or an `error` event is told to the thread as a notice. A tool call starts as soon as its block of the streamed reply is whole (`content_block_stop`), while the rest of the reply still streams; its result waits until the turn is recorded, and the thread's frames always read the turn, then its results, then the input held behind the loop. A streamable tool (`chat_post`, `chat_react`; its Anthropic definition sets `eager_input_streaming`) also shows its input while it arrives: harness reads the longest valid prefix of the partial JSON (`tools::prefix`), and the tool writes, as the agent, in the event's own block. `chat_post` posts once `channel` (and `reply_to`, when it comes first) is whole and the text has begun, as the message `harness-<thread>-<call>`, and edits that one message as the text grows; its run edits it to the exact final text (or, if the call's place changed, deletes it and posts `harness-<thread>-<call>-moved`), so a call posts one message. `chat_react` adds its reaction once its input is whole, and its run does not add it again. A mention in a post still showing is heard once the post is whole, with its final text. A reply that starts over (`seq` back at 0) takes back what its calls showed or posted (the message is deleted, the reaction removed), and says so in a notice; a call that had not run yet never runs, and a call of another tool that already ran is named in a notice, since what it did stays. A reply that breaks or fails takes back what calls still forming showed; calls already whole keep their results, recorded after the part of the turn that was whole. A refused write, provisional or final, is the call's failed result. A reply the service sends whole (a refusal such as a 429) is read whole. Only the program bound to the egress role may send `Streamed`, and only for a thread waiting on its model. While no member node serves that service, a thread tells itself so and does not ask the model. Each tool call is relayed through egress's `relay` service as it starts (a call of a reply read whole starts when the reply is recorded); when its relay comes back, in a later block, harness runs the call: a read of `files`, `chat`, `forge`, the identity role or `activity` as a query, a search or read of the thread's own transcript, a write to `files` or `chat` as a call made as the agent, or any program's own op or query through three tools that read the program's borsh schema (its `Op`, `Query` and `Reply`, and what each op returns, which founding packs as the `ducktape.schema` section of its code, found through the programs role): `network_schema` gives the program's shapes as JSON, `network_query` writes a query from JSON, asks it and reads the reply back to JSON, and `network_call` writes an op from JSON, sends it as the agent and reads what it returns back to JSON where the schema names its type. Each write, a named tool's or `network_call`'s, is a call made as the agent and sent by harness, which the program allows where it allows the agent's account or harness (in `files`, any path no write list in the access program covers, and a listed path whose list names the agent or harness; not its configuration file, which harness-view proposes in its manager's home, makes, and then gives the list `Owner`, its manager's account number alone). `files_set_list { path, write }` sets a path's list in the access program (`access::Op::Set`, which the access program takes only from the path's owner; `write`: its owner alone, its owner and the accounts and programs listed, or anyone, and none clears it). A call whose thread closed, or whose agent no longer runs here (its script was taken back, or it is not active), does not run. Once the turn is recorded and every result is back, harness asks the model again. Input that arrives while the loop runs is held and follows the results. A thread follows the model's context window: harness learns the window from the models endpoint, counts the tokens each answer reports, and when that count reaches the agent's `compact_at` (else its model's), or the next answer would not fit, or the model says the request was too long, it has the model summarize the thread so far in its wire (Anthropic's compaction) and sends that summary in place of the messages it covers; a summary frame names the wire that wrote it, and another wire is handed only its text. A summary ends after an input, never between a tool call and its result; if the API finds the summary request itself too long, the next one covers less. When nothing the model already answered is left to summarize and the request is still too long, harness leaves the largest new input out of the model's view, one at a time, and puts a note in its place naming its frame and size. The transcript keeps every frame whole, and the model reaches it with `thread_search` (a literal string, over the whole thread or one frame: each matching line, its frame, line number and offsets) and `thread_read` (a frame, whole or a byte range), so neither a left-out input nor what a summary covers is lost to it. A failed model call is told to the thread as a notice and not repeated until new input arrives |
| `crates/app/members-view`, `node-view`, `explorer-view`, `settings-view` | the system views, which link the system crates with `program` off |
| `crates/lib/gitcore` | git as a library over one `Objects` trait: objects, packs (read, delta, write), walks, diffs, and the server side of the wire (receive-pack verification, upload-pack); no merging, that is the git client's. `forge` links it into its wasm |
| `crates/lib/borsh-json` | any program's values as JSON, by the borsh schema its code carries (`ducktape.schema`): `encode` (JSON to the bytes the type's own `BorshSerialize` writes), `decode` (bytes to the one JSON form `encode` reads back), `shape` (each type as JSON a model follows) and `variant` (an enum's variant by its tag). harness's `network_*` tools and chat-view's widgets use it |

A view links its program by path and reads its types. A program is a cdylib
for wasm32 the host loads by blob id; a view is a cdylib for wasm32 the
desktop loads from a file. The host (runtime, state, blobs, node, consensus,
the daemon and the CLI) lives in ducktape; the founding suite links it at
the revision `Cargo.toml` pins, patched to compile against `crates/sdk/abi`,
so a copy that drifts from the kernel fails to build.
What is not wasm lives elsewhere, except `crates/sdk/founding`, which packs a
founding out of what this repo builds. The forge smoke (real git against
`forge.wasm` on ducktape's runtime) is in the qa repo.

The kernel calls the boot programs by role, never by id. Each founding
program names the roles it fills (`roles = [...]` on its `[[programs]]`
entry in `founding.toml`): `programs`, `validators`, `identity`, `blobs`,
`awaits`, `halo` and `access` always (`abi::role::REQUIRED`), and optionally
`egress`, `scheduler` and `ownership` (on a network that fills none, no write
is guarded). The programs role keeps that table from genesis
on, and only the validators' vote changes it, by publishing a program with
the roles it fills. Every `Env` carries the table (`env.roles`, a map from
role name to program; the names are constants in `abi::role`, and a new
role is a new row). The host asks the identity role once per frame which
account the frame acts as, and hands it to the program as `ctx.sender()`.
Before an op runs (not a reply and not the chain's own call), the host
asks the target program what the op needs and has the access role check
it. A role is an interface in `abi`,
so another program that speaks it can fill the role.
[`docs/roles.md`](docs/roles.md) has the roles, the account model,
the update path, and how to write a program for a role.

Membership changes only by a vote of the current validators, in `valset`;
a stranger enrolls there as a resident, by invite or while the door is
open. The same vote decides a program swap and the roles a program fills:
`programs` schedules or cancels a change on a message from the program that
fills the validators role (`program::Env::authority`), or from any account,
which never changes a program that fills a role. An
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

[`docs/programs.md`](docs/programs.md) is the walk from `make new-program` to
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
   reads old bytes otherwise, and exits 1 on any `!`.
6. A new program: `make new-program NAME=x`, then `make new-view NAME=x-view`;
   each prints what to do next, down to its place in `crates/sdk/founding`
   ([`docs/programs.md`](docs/programs.md) walks it).
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
loop and for garage, the network that runs dev; a release, and every
program a hosted network (duckpond) runs, is always the release profile's
bytes. A hosted network never runs a release: its pods service, or
eddy-claude (account 24), builds a commit on the network's forge `dev`, and
the builder puts it as a blob. A release founds a local network (qa's kit).
`WASM_PROFILE=fast` on a wasm target's command line asks for it
(`make dev WASM_PROFILE=release` for the bytes that ship), and in the
environment of an `--example live` it picks the rig's profile.
`make wasm-reproducible` proves the bytes do not depend on the checkout. They do depend on the host triple (the
Makefile says why at `WASM_RUSTFLAGS`), and the reference host is Linux x86_64:
a founding or a release is packed from a build made there, and a Mac's build
is a development build.

`make wasm-publish` builds every artifact at the checked-out commit and
publishes them as the GitHub release `wasm-<commit>`, in two archives, each
with its `SHA256SUMS`. `programs-wasm.tar.gz` holds every artifact unpacked.
`founding.tar.gz` (`make wasm-founding`) holds a founding: each program with its describe module,
its schema and any view embedded (`crates/sdk/founding`), stamped with the commit it was built
from (`ducktape.source`, which the programs role's `Sources` reads back; a checkout with changes
git sees stamps none), `forge.params`, and a
`founding.toml` that `ducktape init` reads once its network, time, key and
address are filled. It runs only on the reference host, from a clean checkout
whose commit is on origin, and never replaces a published commit. A founder
downloads these bytes instead of building them, so only the machine that
publishes needs the wasm tools below and `gh`. Each
time `main` moves, the self-hosted runner fabrics-1 publishes it
(`.github/workflows/release.yml`).

Each time `dev` or `main` moves, its build is also published to
https://dl.ducktape.industries/programs/. A branch is a channel: `dev`
publishes the fast profile's bytes, `main` the release profile's (the same
build as its GitHub release). A version is the commit's first-parent count
(`git rev-list --count --first-parent HEAD`), shown as `dev-1234` or
`main-87`. `programs/<channel>/<version>/` holds every artifact `make
wasm-modules` builds, unpacked, `founding.tar.gz` and `manifest.json`, and
nothing in it is ever replaced. `programs/<channel>/latest.json` is a copy of
the newest version's manifest, cached for a minute and written after every
file it names. `built_at` is the commit's committer time and `wire_id` the
`WIRE_ID` the views speak; `artifacts` has one entry per file:

```json
{
  "repo": "programs",
  "channel": "dev",
  "version": 660,
  "display": "dev-660",
  "commit": "41affe2c50df0a4fb280934f7f3e1e7d861345d9",
  "profile": "fast",
  "built_at": "2026-10-18T01:26:17Z",
  "wire_id": "967e0ea397677413",
  "artifacts": {
    "chat_view.wasm": {
      "sha256": "f2b278b1aed0587d1771f2bcf705d792f840df1ff9d5586f2acbd2d405d7e9e9",
      "bytes": 3312535
    }
  }
}
```

```sh
dl=https://dl.ducktape.industries/programs/dev
curl -fsS $dl/latest.json > latest.json
curl -fsSO "$dl/$(jq -r .version latest.json)/founding.tar.gz"
jq -r '.artifacts["founding.tar.gz"].sha256' latest.json   # sha256sum founding.tar.gz prints the same
```

A file is checked by the manifest's sha256, fetched over HTTPS; nothing is
signed yet, since nothing verifies a signature. fabrics-1 builds and stages
each version (`make dl-stage`), and a job on GitHub's Ubuntu, the only one
that holds the bucket's keys, uploads the stage (`tools/publish-dl.py`).
`make dl-stage CHANNEL=dev WASM_PROFILE=fast` is the dry run: it builds,
writes the stage and its manifest under `$CARGO_TARGET_DIR/dl/dev/`, and
prints the upload order, without credentials. `make dl-publish-check` checks
the script.

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
