# Chat widgets

A widget is a TOML file in files that a chat channel names in its meta. chat-view reads the file,
asks its one read-only query of any program, and draws the reply in one of five shapes. Nothing is
compiled or deployed per widget: writing the file and setting the meta is the whole of it.

## Where a widget shows

A channel's meta (chat `SetChannelMeta`) names one file per slot:

| key | desktop | phone |
|---|---|---|
| `widget.header` | a line under the room's title | a line over the timeline |
| `widget.pinned` | a card over the messages the reader folds | the same card |
| `widget.side` | a pane beside the room, opened by a header button named by the widget's title | a section of the room's details page |

The value is the file's absolute path, e.g. `/shared/widgets/board.toml`.

## The file

```toml
title = "Board"                       # the card's or pane's heading, the side pane's button

[source]                              # one read-only query, asked as the reader
program = "board"                     # a program's name
query = { Jobs = { page = {} } }      # the query in the JSON shape network_schema gives
rows = "Jobs.items"                   # the path in the reply of the list drawn ("" = the reply)
cursor = "Jobs.page.after"            # optional: read every page, putting each reply's `next`
                                      # (the field beside `rows`' list) at this path of the query

[view]
kind = "columns"                      # list | columns | table | stat | checklist
except = { status = ["done", "merged"] }   # optional: drop rows whose field is one of these
                                      # `where`, the same form, keeps only them
group_by = "status"                   # columns: one column per value
order = ["waiting", "doing"]          # columns: these first, in this order, then as they come
item = "J-{id} {title} · {owner|name}"     # a row's text
on_press = { open = "{channel}" }     # optional: where pressing a row goes
```

Per kind:
- `list`: a row per item (`item`).
- `columns`: a column per `group_by` value, headed by the value and its count, a card per row (`item`).
- `table`: `columns = [{ title = "Job", value = "J-{id}" }, …]`, a header row and a row per item;
  the first cell is the row's link.
- `stat`: one figure, `value` (a template over the reply; `{count}` is the number of rows left
  after `where` and `except`) and an optional `label`.
- `checklist`: a row per item with a box; `checked = { status = "done" }` (the form of `where`)
  ticks it. It shows state and sends nothing.

A filter (`where`, `except`, `checked`) maps a field path to a value or a list of values; a row
matches when every field it names holds one of them, compared as a template writes them.

Templates: `{a.b.0.c}` is the value at that path in the row (in the reply for `stat`): a string as
it is, a number or bool as JSON, null or missing as nothing. `{path|name}` writes an account
number as the name chat knows for it. `{me}` is the reader's account number, in templates and in
the source query's strings (a string that is `{me}` alone becomes the number).

`on_press.open`: a `duck://` or `https://` result opens through the host; anything else is a chat
channel id, opened in chat. No widget ever sends an op.

## How it stays current

- The query runs again on every block that writes to the source program (`program.changes`),
  never on a timer.
- On every files block chat-view looks at the file again and reads it anew when its object moved.
- The program's schema comes from its code: programs `Program(name)` → `entry.code`, `blob.get`,
  the `ducktape.schema` custom section. It is read once per code, so an upgraded program is read
  again.

## Errors

A file that does not parse, a file missing, a program the network does not run, a query the
program refuses, a reply that does not read: the widget says why in its own place.

## Which programs a widget can read

Until J-054 lands, the app host answers a view only for the programs its manifest lists as
targets. chat-view lists chat, identity, access, files, board and programs, so a widget over
another program shows the host's refusal in its place.

The code: `crates/app/chat-view/src/widget.rs` (the file, templates, filters),
`widget/live.rs` (the data path), `ui/widget.rs` (drawing); the codec is `crates/lib/borsh-json`.
