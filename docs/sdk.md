# What the SDK and views already provide

Read this before writing a piece of drawing or parsing in a view: if it is here, link the crate
and call it, and if a view already has something a second one needs, move it into the SDK rather
than writing it twice.

## Documents, code and tables: `crates/sdk/markdown`

- **Markdown.** `markdown::parse(text)` (feature `parse`, pulldown-cmark) reads CommonMark plus GFM
  tables, task lists, strikethrough and footnotes into `Block`s of rich `Text`. Drawn with
  `markdown::desktop::render_blocks` or `markdown::phone::markdown`; a link press goes to the
  caller's `OnLink`. forge-view's READMEs, markdown files and change bodies draw this way, and so do files-view's
  `.md` files. `markdown::links::target(dir, dest)` says where a pressed link goes: the web, or a
  path of the document's own tree resolved against its folder.
- **Highlighting.** `markdown::highlight::tokens(name, lines)` colours source by a file name or a
  fence's language (Rust, JS/TS, shell, TOML/YAML, JSON, markdown source), `style(token, theme)`
  is a token's colour, `language_of(name)` a file's language. Lines draw with
  `desktop::source_line` or `phone::code_line`; a whole fenced block with `desktop::code` or
  `phone::code_box`, a panel that scrolls sideways.
- **Tables and headings.** `Block::Table(rows)` (the header row first) and `Block::Heading(level,
  text)` draw through `desktop::block` / `phone::block`, one block at a time, for a view that keeps
  blocks of its own. chat-view reads its tables (`| … |` lines under a `|---|` row) and `#`
  headings out of chat's paragraphs this way (`chat-view/src/markup.rs`) and links no parser.
- The crate's [README](../crates/sdk/markdown/README.md) is its contract.

## Elsewhere in the SDK

- `view`: the runtime a view is written against (elements, lists, text fields, pictures, the host
  methods), and `view::design`: the palette, the type ladders (`POINTER`, `TOUCH`), spacing,
  `divider`, `eyebrow`, `segmented`, and how keys, bytes, times and accounts read to a person
  (`short_hex`, `fingerprint`, `day`, `plural`, `initial`).
- `view-wire`: every method a view may ask of the host (`view_wire::methods`).
- `policy-view`: "Who can edit" on an item, and a policy, a party and a scope in words.
- `ducklink`: the `duck://` link's one grammar.
- chat-view's widgets ([widgets.md](widgets.md)): any program's query drawn as a list, columns, a
  table, a stat or a checklist from a TOML file, with no view code.
