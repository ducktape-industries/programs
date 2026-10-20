# policy-view

What a view shows of the `ownership` program's policies, written once for every
view that links it (settings-view's Policies pane; chat-view's, forge-view's
and harness-view's "Who can edit" rows). It links `ownership` and `identity`
with `program` off, and `view`; its only writes are the item sheets' `Relink`
and `Transfer`.

- `KINDS`: every kind of owned item (`chat.channel`, `chat.message`,
  `forge.repo`, `forge.change`, `forge.review`, `harness.model`,
  `harness.agent-conversation`, `egress.service`), the program that keeps it
  and what a person calls it; `PROGRAMS`, the programs that keep them, each
  once; `kind(name)`. A test holds `KINDS` to the kinds the programs' owned
  tables claim (each `Map<_, Owned<T>>`'s `kind()`).
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
- `item` (with `Item`, `Standing`, `Editing`, `Sheet`, `Lens`): "Who can edit"
  on one item a view keeps. The view holds an `Editing` slot per item it shows
  a row for and passes a lens to it (`fn(&mut V) -> &mut Editing`).
  `item::hold(view, lens, item, reader, cx)` reads the item's `Record` and
  `Effective` policy (and identity's names for the accounts they name) when it
  is another item or reader than the one held; `item::let_go` empties the slot;
  `item::moved` reads again on an ownership block that touched them.
  `Editing::admits(item)` is whether the policy lets the reader change it (its
  owner, or anyone the policy admits, a program counted as its account), `None`
  before the read lands and for an item with no record, where the view keeps
  the rule it had before policies. `item::row` draws "Who can edit" with who
  may write it, said once ("Only you", "You and Ada"), the row growing with its
  words (a press for the owner), and `item::sheet` the sheet over the view: its
  owner, its policy and who may write it, a line each, then for the owner Change
  policy… (`Relink`, among their live policies) and Transfer… (an account found
  by name or `#number`, then a plain confirm that names it). An item with no
  record reads "Open (from before policies)". `item::find` is that lookup.
  `item::row_titled` is the row under another title (harness-view's "Who can
  start its conversations"), and `item::about(standing, reader)` what a
  `Standing` read with `item::standing` says, for a list that reads one per row.
