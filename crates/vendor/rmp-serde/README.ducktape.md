# Smaller MessagePack guest code

Taken into programs from the gpui-pre fork (`vendor/rmp-serde` at
2622bd8294b97a966f8febbd3b9dcb2cb5850aca) unchanged (J-094: it is not gpui, and
the fork goes). view-wire takes it by path; no `[patch.crates-io]` names it.

Source: https://static.crates.io/crates/rmp-serde/rmp-serde-1.3.1.crate
Archive SHA-256: `72f81bee8c8ef9b577d1681a70ebbc962c232461e397b22c208c43c04b67a155`.
Published sources and MIT license retained. The missing published test dependency
`rmpv` is restored as a crates.io dev-dependency. Upstream tests are unchanged.

Marker/number reads, string buffering and UTF-8 validation, enum headers, and
custom-error formatting are outlined independently of Serde visitor types.
This avoids copying the parser into every wire field and enum visitor. Default
behavior retains the upstream accepted representations and errors.

The opt-in `typed` feature respects Serde's requested shapes: sequences and tuples
accept arrays or binary; structs accept maps or arrays; maps accept maps;
strings/chars accept strings or binary; identifiers accept scalar values,
strings, or binary; byte buffers accept strings, arrays, or binary. Dynamic and
ignored values remain unrestricted. Struct enum variants use the struct reader,
retaining both named-map and positional-array representations. Numeric widths,
binary sequences, UTF-8 byte fallback, depth and leftover checks are preserved.
Custom visitors that request one shape while accepting unrelated shapes must
leave `typed` disabled. The view wire enables it only for wasm guests; it changes
no encoded bytes or view capabilities. Native consumers keep it disabled.

`Deserializer::remaining_slice` (slice reader only) is the leftover check for a
caller that decodes from a slice: the view wire reads a `bin` as one borrowed
slice, and upstream gives that reader no position.

On a bump, reapply outlining and the default-off typed feature, restore the test
dependency if still missing, and run upstream tests both with and without typed.
Also run wire roundtrips, allocation/depth/invalid-input checks, all four view
suites, the app suite and compiled wasm lifecycle probe. Measure optimized sizes
and exact ABI again; do not infer either from dependency versions.
