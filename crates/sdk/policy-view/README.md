# policy-view

What a view shows of the `ownership` program's policies, written once for every
view that links it (settings-view's Policies pane now; chat-view, forge-view and
harness-view's "who can edit this" rows next). It links `ownership` and
`identity` with `program` off, and `view`; nothing here writes.

- `KINDS`: every kind of owned item (`chat.channel`, `chat.message`,
  `forge.repo`, `forge.change`, `forge.review`, `harness.model`,
  `harness.agent-conversation`, `egress.service`), the program that keeps it
  and what a person calls it; `PROGRAMS`, the programs that keep them, each
  once; `kind(name)`. A test holds `KINDS` to what each program's `TABLES`
  declares owned, so a new owned kind fails it until it is listed here.
- `writers(write, owner, reader, names)`: who may write the items on a policy,
  beside its owner, in words: "Anyone", "Only you", "You, Ada and board" to the
  owner, "Only Maya" to anyone else. `party(party, names)` and `scope(scope)`
  ("Everything", "Everything in chat", "Chat messages") likewise.
- `holdings(host, owner)`: one owner's side read whole (every page of
  `Policies` and `Defaults`, `Open`, and identity's name for each account a
  policy lists), as `Holdings`, whose `default_of(scope)` is the owner's own
  choice there and `inherited(scope)` what the guard falls back to without one
  (a kind's program, then everything, then the open policy).
- `items(host, policy)`: how many items are on a policy. ownership keeps no
  count, so this reads every page of `Linked`; a screen asks it once, when a
  sheet that needs the number opens, never for a list.
