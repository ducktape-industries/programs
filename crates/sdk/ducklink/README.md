# ducklink

The `duck://` link: one grammar for naming a thing on a ducktape network, read in one place.

```text
duck://<label>-<salt>/<program>/<tail…>
duck://dognet-b5b6ea90/chat/general/42
duck://dognet-b5b6ea90/@ducktape-industries/chat/general/42
```

**The authority is the chain id.** The workspace registry keys a network `<label>#<salt>` (the
salt being hex digits of the genesis digest: 8 today, any even count from 8 to 64 reads), and `#`
starts a URL fragment, so a link writes the same pair with `-`. The salt is the match key; the
label is display. Whether a label agrees with the registry is the reader's question, not this
crate's: it reads nothing.

**The program comes next, by its id.** A bare id is one segment (`chat`). An `owner/name` id is
two, the first starting with `@` (`@ducktape-industries/chat`); `@` is in no program id, so a
first segment without it is always a bare id, and every link written before owners existed reads
as it did. This crate doesn't know which programs exist (the `programs` program's roster does), so
it checks only the spelling (`program::is_id`: a name, or an owner and a name, each `1..=64` of
`[a-z0-9_-]`). `Link::program` holds the id itself (`ducktape-industries/chat`), and `Display`
writes it back with its `@`.

**The tail carries any name, in exactly one spelling.** A segment may hold any text but `/`, NUL,
`.` and `..`. `Link::tail` holds it decoded; `Display` writes a byte literally iff it is RFC 3986
unreserved (`A-Z a-z 0-9 - . _ ~`) and every other byte as `%XX` in uppercase hex; `parse` refuses
every other spelling of the same name instead of normalising it. Two spellings of one link would
be two cache keys and two registry lookups. What a tail means is the program's to say, and a view
that names one copies that rule beside itself.

The chain id and the program are lowercase and never encoded, and nothing here case-folds
anything: `A` and `a` in a tail segment are two names.

- `Link::parse` and `Display` round-trip; `Link::new` builds one from its parts by the same rules.
- `mint(chain, program, tail)`: a link from a session's chain id text.
- `path(program, tail)` and `on_chain(chain, path)`: a path on the reader's own chain, for a
  program, which can't name its chain (`@o/forge/programs/213`).
- `tail(path)`, `number(segment)`: a tail read alone, and a segment's one spelling of a number.
- `bare_end(run)`: the length of a link typed in prose, less the sentence's punctuation.

## Repository overview

the `duck://` link: `duck://<chain>/<program>/<tail…>`, one spelling per name, no
program names known here
