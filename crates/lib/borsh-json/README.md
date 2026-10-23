# borsh-json

any program's values as JSON, by the borsh schema its code carries (`ducktape.schema`):
`encode` (JSON to the bytes the type's own `BorshSerialize` writes), `decode` (bytes to
the one JSON form `encode` reads back), `shape` (each type as JSON a model follows) and
`variant` (an enum's variant by its tag). harness's `network_*` tools and chat-view's
widgets use it
