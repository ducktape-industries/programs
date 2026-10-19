# Tool files

A tool file is a TOML file in files that harness offers an agent as a tool. Nothing is compiled or
deployed per tool: writing the file is the whole of it, and the agent has the tool on its next model
call. A tool file only sends queries and ops its agent could send itself, as that agent, so it adds
no power. What should become a tool at all: `/shared/agents/tool-rules.md`.

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

[input]                               # each field is a required property of the tool's input
repo = { type = "string", description = "repository, e.g. programs" }
number = { type = "integer", description = "the change's number" }

[[step]]                              # the steps run in order, as the calling agent
query = "forge"                       # a query of program forge; `op = "forge"` sends an op
body = { Change = { repo = "$repo", n = "$number", page = {} } }
as = "change"                         # what later steps and `returns` call its answer

[[step]]
op = "forge"
body = { SquashMerge = { repo = "$repo", change = "$number", from = "$change.Change.change.from", into = "$change.Change.change.into", expected_from = "$change.Change.source_head", expected_into = "$change.Change.target_head" } }
as = "merged"

[returns]                             # optional: the call's result
text = "Merged #$number → $merged.Merged.oid"
```

- `type` is a JSON Schema type: `string`, `integer`, `number`, `boolean`, `object` or `array`.
- `body` is the query or op exactly as `network_query` and `network_call` take it, in the shape
  `network_schema` gives. TOML has no null: leave an option field out.
- A step's `as` holds a query's reply as `network_query` reads it, or what an op returns as
  `network_call` reads its `returned` (null when it returns nothing). Nothing is unwrapped: the
  variant is part of the path (`$change.Change.source_head`).
- With no `[returns]`, the result is every step's `as` as JSON.

## `$`, the only thing harness fills in

A string that is one reference alone (`"$number"`, `"$change.Change.source_head"`) becomes that
value with its own JSON type, so an integer input stays an integer. A reference inside other text is
written into it (a string as it is, anything else as JSON). A path walks object keys and array
indexes (`$found.items.0.name`). `$$` is a dollar sign. There are no expressions, conditions or
loops: a tool that needs logic belongs in a program (as an op or a query) or a script in the tools
repo.

## Checked before it is offered

harness reads every tool file before each model call. A file is offered only when it parses with no
unknown keys, its `name` is its file's and none of harness's own tools', every `$` names an input or
an earlier step's `as`, and every step's body fits its program's live schema (read from the
program's code), each `$` string standing for a value of any type. A file that fails is left out,
and the agent's system prompt says why in one line, in the codec's own words:

```
- File tools left out: tool broken: step 2 (op forge): at op.SquashMerge: there is no field number; the fields are repo, into, from, expected_into, expected_from, message, change
```

harness's `Query::AgentTools { agent }` lists every tool an agent gets, each with its source
(harness's own, or the file's path and the snapshot it was read at), the shared files its own take
the place of, and those reasons.

## Running

A call's input is checked against `[input]` before it runs. The file is read again when the call
runs, so a file changed in between runs as it is now. A query step answers in place; an op step is
sent as the agent and the walk goes on from what it returns. A step the program refuses stops the
tool, and the result names it: `step 2 (op forge): wrong_state: …`. What earlier op steps did stays.

The code: `crates/app/harness/src/tools/file.rs`; the schema check is `borsh_json::fits`.
