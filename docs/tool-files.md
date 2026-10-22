# Tool files

A tool file is a TOML file in files that harness offers an agent as a tool. Nothing is compiled or
deployed per tool: writing the file is the whole of it, and the agent has the tool on its next model
call. A tool file only sends queries, ops and egress service calls its agent could send itself, as
that agent, so it adds no power. What should become a tool at all: `/shared/agents/tool-rules.md`.

## Where a tool file lives

- `/shared/tools/<name>.toml`: every agent's. Who may add one is `/shared/tools`'s write list.
- `/home/<agent>/tools/<name>.toml`: that agent's alone. Where both hold a file of one name, the
  agent's own takes the place of the shared one.

Only `*.toml` files directly in those folders are read. A file removed is a tool gone on the next
call.

## The file

```toml
name = "forge_merge"                  # the file's own name, 1 to 64 letters, digits, _ and -
description = "Squash-merge a forge change into its target branch, refusing if either side moved since it was read."

[input]                               # each field is a property of the tool's input, required unless `optional = true`
repo = { type = "string", description = "repository, e.g. programs" }
number = { type = "integer", description = "the change's number" }

[[step]]                              # the steps run in order, as the calling agent
query = "forge"                       # a query of program forge; `op = "forge"` sends an op
body = { change = { repo = "$repo", n = "$number", page = {} } }
as = "change"                         # what later steps and `returns` call its answer

[[step]]
op = "forge"
body = { squash_merge = { repo = "$repo", change = "$number", from = "$change.change.change.from", into = "$change.change.change.into", expected_from = "$change.change.source_head", expected_into = "$change.change.target_head" } }
as = "merged"

[returns]                             # optional: the call's result
text = "Merged #$number → $merged.merged.oid"
```

- `type` is a JSON Schema type: `string`, `integer`, `number`, `boolean`, `object` or `array`.
- `optional = true` makes an input optional. A value that is exactly `"$x"` for an optional input
  the call leaves out drops its key from the object holding it; inside other text or a list, an
  absent `$x` fails the call, naming it.
- `body` is the query or op exactly as `network_query` and `network_call` take it, in the shape
  `network_schema` gives. TOML has no null: leave an option field out.
- A step's `as` holds a query's reply as `network_query` reads it, or what an op returns as
  `network_call` reads its `returned` (null when it returns nothing). Nothing is unwrapped: the
  variant is part of the path (`$change.change.source_head`).
- With no `[returns]`, the result is every step's `as` as JSON.

## A step that calls an egress service

A step may name an egress service instead of a program: harness sends the call an HTTP request as
the agent, through egress, as it sends every write of a tool.

```toml
[[step]]
service = "local-harness"             # a registered egress service, not http or relay
method = "POST"                       # GET, POST, PUT, PATCH or DELETE; POST when left out
target = "/v1/read"                   # the path on the service; `$` references are filled in
body = { path = "$path", offset = "$offset" }   # any JSON, sent as one application/json part
as = "read"
```

- Answered whole: a 2xx answer's JSON (or its text) is what `as` holds. Any other status fails the
  call in the service's words (`step 1 (service local-harness): local-harness answered 404: no
  such file`), or, when the body is no JSON, by its status and media type (`local-harness answered
  530 with a text/html body, not JSON`).
- An image: a 2xx answer whose `content-type` is png, jpeg, gif or webp is written into files as
  the agent, at `/home/<agent>/seen/<thread>-<call>-<step>.<ext>`, and shown to the model as
  `image_view` shows a file. `as` holds `{path, snapshot, media_type, size, width, height}`. The
  path is the call's own, so a call run again (a relay rerun after a harness update) finds it: the
  same bytes keep the file already there, other bytes are written over it.
- `stream = true`, on the last step only: the call answers at once that it streams, and the
  conversation hears its end once, when the service's `text/event-stream` says `ended` (its data is the
  call's result) or `refused` (its failure). A `note` event before then is told to the
  conversation as it arrives, its data as it is (the pods service notes a pod its wait or exec
  stream sees the scheduler can't place yet), and the stream goes on. harness puts the heartbeat it wants in the request
  header `every` (seconds: half of egress's wait at the block time it measures, the smaller of
  the block the stream was opened over and harness's average up to the block the call was made
  in, so one slow block, a halt included, counts in only one of them; until harness holds a block
  time from before the call, the request waits one more block so it has both), and every `alive` event or comment line moves egress's expiry on, so a long command
  never expires. A stream that ends before `ended` or `refused` wakes the conversation saying it is
  no result and that what it ran may still be running on the service. A streamed step has no
  `as`, and its tool no `[returns]`.

## `$`, the only thing harness fills in

A string that is one reference alone (`"$number"`, `"$change.change.source_head"`) becomes that
value with its own JSON type, so an integer input stays an integer. A reference inside other text is
written into it (a string as it is, anything else as JSON). A path walks object keys and array
indexes (`$found.items.0.name`). `$$` is a dollar sign. There are no expressions, conditions or
loops: a tool that needs logic belongs in a program (as an op or a query) or a script in the tools
repo.

In a service step's `target`, each value a reference fills in is percent-encoded (`%XX`, the
upper-case hex of each UTF-8 byte); the file's own text is sent as written. Before the target's
first `?` (its path), everything but the unreserved characters (`A-Z a-z 0-9 - . _ ~`) and `/` is
encoded, so `"/v1/files$path"` with `$path` = `/home/a b` is `/v1/files/home/a%20b`. After it (its
query), only the unreserved characters pass, so a value may hold `&`, `=`, `/` or `#`.

## Checked before it is offered

harness reads every tool file before each model call. A file is offered only when it parses with no
unknown keys, its `name` is its file's and none of harness's own tools', every `$` names an input or
an earlier step's `as`, every query or op step's body fits its program's live schema (read from
the program's code), and every service step names a service, a method and a target, each `$` string standing for a value of any type. A file that fails is left out,
and the agent's system prompt says why in one line, in the codec's own words:

```
- File tools left out: tool broken: step 2 (op forge): at op.SquashMerge: there is no field number; the fields are repo, into, from, expected_into, expected_from, message, change
```

harness's `Query::AgentTools { agent }` lists every tool an agent gets, each with its source
(harness's own, or the file's path and the snapshot it was read at), the shared files its own take
the place of, and those reasons.

## Running

A call's input is checked against `[input]` before it runs. The file is read again when the call
runs, so a file changed in between runs as it is now. A query step answers in place; an op step or
a service step is sent as the agent and the walk goes on from what it returns. A step the program refuses stops the
tool, and the result names it: `step 2 (op forge): wrong_state: …`. What earlier op steps did stays.

The code: `crates/app/harness/src/tools/file.rs`; the schema check is `borsh_json::fits`.
