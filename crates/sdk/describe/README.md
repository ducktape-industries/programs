# describe

What an op means to a person, as the program that runs it says.

A program ships a standalone wasm module in the custom section
`ducktape.describe` (`SECTION`) of its code blob, beside `ducktape.view`;
the kernel ignores both. The module has no imports and exports one pure
function:

```text
export memory
export alloc(len: u32) -> u32
export describe(ptr: u32, len: u32) -> u64   (ptr << 32) | len
```

`describe` reads the borsh op bytes the host wrote into an `alloc`ed buffer
and answers a borsh `Description`; `0` (an empty answer) when the bytes are
no op of its. A program writes `describe(&Op) -> Description` and one
`export!` line; its `describe` feature builds the module
(`make wasm-describes`).

`Value` is a small vocabulary, so a reader (Explorer) can show an account
as a name, a program as a link, a hash short.

## Versions

A host describes with the program's current code, whatever height the op
landed at. So old op bytes read the same under new code: an `Op` enum only
grows at its end, and a retired variant is deleted with its tag kept
(`docs/programs.md`, "Retire by deleting, keep the tag"). Each program's
test holds it over `variants`: the variant names in tag order, read from the
enum's borsh schema (the tags and names schema-diff reads), a tag no variant
holds written `-`, up to the last tag. An op the module cannot read, a retired
tag among them, is `None`, and the reader falls back to its bytes.

## Repository overview

what an op means to a person: the pure wasm module a program ships in its
`ducktape.describe` section, and the sandbox that runs it
