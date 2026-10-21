# Chat widgets

A widget is a TOML file in files that a chat channel names in its meta. chat-view reads the file,
asks its one read-only query of any program, and draws the reply in one of six shapes. Nothing is
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
query = { jobs = { page = {} } }      # the query in the JSON shape network_schema gives
rows = "jobs.items"                   # the path in the reply of the list drawn ("" = the reply)
cursor = "jobs.page.after"            # optional: read every page, putting each reply's `next`
                                      # (the field beside `rows`' list) at this path of the query

[view]
kind = "columns"                      # list | columns | table | stat | checklist | text
except = { status = ["done", "merged"] }   # optional: drop rows whose field is one of these
                                      # `where`, the same form, keeps only them
group_by = "status"                   # columns: one column per value
order = ["waiting", "doing"]          # columns: these first, in this order, then as they come
item = "J-{id} {title} · {owner|name}"     # a row's text
on_press = { open = "{channel}" }     # optional: where pressing a row goes
empty = "Nothing waiting."            # optional: what it says with no rows ("Nothing to show.")
```

Per kind:
- `list`: a row per item (`item`), or in parts and sections (below).
- `columns`: a column per `group_by` value, headed by the value and its count, a card per row (`item`).
- `table`: `columns = [{ title = "Job", value = "J-{id}" }, …]`, a header row and a row per item;
  the first cell is the row's link.
- `stat`: one figure, `value` (a template over the reply; `{count}` is the number of rows left
  after `where` and `except`) and an optional `label`.
- `text`: the file `[source.text]` reads (below), drawn as markdown as chat draws a message
  (paragraphs, `**bold**`, `*italic*`, `` `code` ``, links, lists, quotes, fences, dividers,
  `#` headings and `|` tables; a line is its own paragraph). The header slot says its first
  paragraph.
- `checklist`: a row per item with a box; `checked = { status = "done" }` (the form of `where`)
  ticks it. It shows state and sends nothing.

A filter (`where`, `except`, `checked`) maps a field path to a value or a list of values; a row
matches when every field it names holds one of them, compared as a template writes them.

Templates: `{a.b.0.c}` is the value at that path in the row (in the reply for `stat`): a string as
it is, a number or bool as JSON, null or missing as nothing. `{path|name}` writes an account
number as the name chat knows for it. `{path|ago}` writes a time in
milliseconds as how long before the chain's newest block it was (`5m`). `{me}` is the reader's account number, in templates and in
the source query's strings (a string that is `{me}` alone becomes the number).

`on_press.open`: a `duck://` or `https://` result opens through the host; anything else is a chat
channel id, opened in chat. No widget ever sends an op.

## The room, one row, and a file beside the source

`{room}` is the open room's id, in templates and in the source query's strings, so one widget file
named by many rooms asks about each room's own thing (`query = { job_in = { channel = "{room}" } }`).
`rows` may name an object, which is one row, or null, which is none, as well as a list.

```toml
[source.text]                         # optional: a file read beside the source
path = "{folder}/summary.md"          # a template over the first row (and `{room}`, `{me}`);
                                      # an absolute path's doubled `/` count as one
```

A path with nothing at it says so in the widget's place ("Nothing at … yet."). The file is looked
at again on every files block and read again only once its object moved. The job rooms' pane
(`/shared/widgets/job.toml`) is board's `JobIn` and the job folder's `summary.md`.

## A list in parts

A `list` that groups its rows, or names a part beside `item`, draws them in parts (J-088):

```toml
[view]
kind = "list"
group_by = "status"                   # a section per value, under a head the reader folds
order = ["waiting", "doing"]          # these sections first, in this order; one with no rows is left out
fold = ["todo"]                       # the sections that start folded
item = "{title}"                      # the row's title
lead = "J-{id}"                       # muted, before the title
chip = "{owner|name}"                 # a small initial and the name
trail = "{updated.time|ago}"          # muted, at the end
badge = "{mark.doing}"                # a template whose text picks one of `badges`
badges = [
    { when = "Turn", mark = "●", tone = "success", label = "working now" },
    { when = "Build", mark = "◐", tone = "warning", label = "waiting on a build" },
]

[view.icons]                          # before a section's value in its head
doing = "🛠️"

[view.tones]                          # a bar at the left of a section's rows
doing = "accent"
```

A section's head reads `▾ 🛠️ doing   12  ● 3  ◐ 2`: its icon, its value, its rows, and how many
of them wear each badge; pressed, it folds or opens the section. A tone is one of the chat theme's:
accent, success, warning, danger, agent, foreground, muted, faint, link. Under a pointer a row is
one line (badge, lead, title, chip, trail); under a finger, the badge and title over a muted line of
the rest. A row presses as `on_press` says, and assistive tech reads its parts and its badge's
`label`. The header slot says a grouped list as `🔴 waiting 2 · 🛠️ doing 12`.

## How it stays current

- The query runs again on every block that writes to the source program (`program.changes`),
  never on a timer.
- On every files block chat-view looks at the file again and reads it anew when its object moved.
- The program's schema comes from its code: programs `Program(name)` → `entry.code`, `blob.get`,
  the `ducktape.schema` custom section. It is read once per code and kept on the device (view
  `store`, key `widget-schema/<program>`, with the code it came from), so the code is fetched again
  only once the program runs other code.
- A widget whose read failed asks again on the next block it hears (files, or its source program).

## Errors

A file that does not parse, a file missing, a program the network does not run, a query the
program refuses, a reply that does not read: the widget says why in its own place. A read that
failed says which: "Reading board’s code: …" (its entry or its code, for the schema) or "Asking
board: …" (the query). A `cursor` whose reply has no `next` beside its rows says so rather than
stop after one page.

Before it asks anything, a widget is held to its program's schema, and one that doesn't fit says
"The widget doesn’t fit board’s schema: …" with the path and what the schema has there:
- the query fits the program's `Query` (a string holding `{me}` or `{room}` stands for any value);
- `rows` is a path of the program's `Reply`;
- with a `cursor`, the query with the cursor put in is still one query of the program, and `next`
  beside `rows` is a path of the reply. A cursor is a path in the query, so it starts at the query's
  own variant: a query renamed without its cursor fails here, on the first page, not on the second.

## Which programs a widget can read

Any program. Since J-054 a view names no programs: the app host answers a view's
`program.query`, `program.watch` and `program.changes` for whatever program it names, and each program decides
what it answers. An app host from before J-054 still refuses a program the view's manifest
doesn't list, and refuses a chat-view built since J-054 outright (its manifest lists none).

The code: `crates/app/chat-view/src/widget.rs` (the file, templates, filters),
`widget/live.rs` (the data path), `ui/widget.rs` (drawing; `ui/widget/sections.rs`
a list in parts); the codec is `crates/lib/borsh-json`.
