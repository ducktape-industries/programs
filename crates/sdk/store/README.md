# store

optional typed storage over `program`'s contexts: the `Map`/`Set`/`Item` descriptors
with `KeyCodec`, and `PageRequest`/`PageResponse`. A read takes `&QueryCtx` (an
`&ExecCtx` serves it), a write `&ExecCtx`. A view links it and calls none of it
