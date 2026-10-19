# markdown

Markdown as a document, drawn the same way in every view that links it (forge-view's READMEs,
files and change bodies; chat-view's code blocks, tables and headings). It links `view` and, with
its `parse` feature, pulldown-cmark. It reads nothing from a network and writes nothing.

- The model: `Block` (`Heading(level, Text)`, `Paragraph(Text)`, `Item(marker, body)`,
  `Code(lang, text)`, `Quote(body)`, `Rule`, `Table(rows)`, its header row first) and `Text`, a
  run of prose with its marked ranges (`Marks`: bold, italic, strike, code, quiet, link).
  `Text::push` adds words with their marks; `Text::trimmed` drops the whitespace either side and
  cuts each run to what is left.
- `parse(text)` (feature `parse`): CommonMark plus GFM tables, task lists, strikethrough and
  footnotes into blocks. Raw HTML stays text, an image is its alt text (quiet), a bare `https://`,
  `http://` or `duck://` word links, and nothing is fetched. A view that keeps blocks of its own
  (chat-view reads its tables and headings out of chat's paragraphs) leaves the feature off and
  links no parser.
- `highlight`: a small scanner per language family (Rust, JS/TS, shell, TOML/YAML, JSON, and
  markdown source). `tokens(name, lines)` takes a file name or a fence's language (`rust`, `ts`,
  `toml`); `style(token, theme)` is the token's colour; `language_of(name)` names a file's
  language. An unknown language is plain.
- `desktop`: `render_blocks(name, blocks, theme, on_link)` draws a document, `render` parses and
  draws one (feature `parse`), `block(key, block, theme, on_link)` draws one block, `code(key,
  lang, text, theme)` a fenced block (a bordered panel, highlighted, scrolling sideways) and
  `source_line` one highlighted mono line.
- `phone`: the same at the touch ladder (Expo `src/forge/Document.tsx`): `markdown(name, blocks,
  on_link, ladder, theme)`, `block`, `code_box` and `code_line`.
- Links: a pressed link calls the caller's `OnLink` with its raw destination. Where it goes is the
  caller's (forge-view resolves a relative link against the document's folder; chat-view opens a
  link or a mention as it opens one in a paragraph).
- Element ids: a document named `name` keys block `i` as `name-i`; a heading's box is `-h`, a
  table `-table` (cell `-r-c`), a code block `-code` (line `-n`), a list item `-li`.
