# forge-view

The forge program's view: repositories, their code, commits and refs, and the
changes a reviewer lives in. It links `forge` and `chat` with `program` off (the
wire and the rules), and `ownership` and `policy-view` for who can edit.

## Panes
- Desktop (`ui/`): the repositories rail, one content column (the list, a
  repository's tabs README · Code · Commits · Changes · Refs · Settings, or a
  change), and a docked panel (a repository's About; a change carries its own
  details sidebar, `ui/dock.rs`). Narrow windows fold the rail and the dock into
  toggles.
- Phone (`ui/phone/`): the Expo app's forge stack, one page at a time from
  `Nav::pages`, with one overlay over it (`Forge::overlay`: a ref picker, a
  sheet, New repository, Finish your review, or a policy sheet), which Back
  closes first.
- Who can edit (`policies.rs`): a repository's Settings tab (both designs) and a
  change's details (the desktop sidebar, the phone's Details tab) carry
  policy-view's "Who can edit" row for its `CUSTODY` and `AUTHORSHIP` keys, and
  its owner relinks or transfers it from the row's sheet.

## State
- `state.rs`: `Forge`, what the screen shows and what the reader typed; a
  snapshot keeps navigation and drafts, never wire records. `select.rs` reads
  the screens' facts out of it.
- `sync.rs` and `queries.rs`: reads are a cache keyed by the query itself; `sync`
  asks what the screen needs and drops what it left.
- `navigate.rs`, `actions.rs`, `review.rs`, `tree.rs`: what an event changes.
- What the reader may do: `may_write` (connected, with an account); for an item
  ownership has on record, its policy's answer (`may_change_repo` for merge and
  closing another's change, `may_configure` for the settings, `may_edit_change`);
  for one with none, the rule from before policies (the owner and access's
  writers, the owner, the author). An author closes their own change, as forge
  lets them. The access list itself (Grant, Revoke) stays its owner's to set.

## Follows
`HostSession`, `HostRoute`, `HostVisible`, and the live heads of forge (the
reads on screen), chat (change conversations), identity (names), access (the
writers list) and ownership (who can edit what is held). Out of sight
(`HostVisible` false) it reads nothing, and shows again by reading what is on
screen once. A refused read stays on screen, saying why, until a block writes a
table it reads or the reader retries. The Refs tab reads every branch's
standing in one `Compares`.

## Tests
`tests.rs` replays the forge program's own reply bytes
(`crates/app/forge/fixtures/replies.bin`); its `configure` answers chat, access
and ownership (no record for any key), and an unhandled ask panics. Accounts: 1
Ada owns `project` and wrote change 1, 2 Rae reviews and holds no write, 9 Wren
is access's granted writer. `change_tests.rs`, `screen_tests.rs`,
`log_tests.rs` and `phone_tests.rs` (the phone at 390 wide) share its helpers;
`policies_tests.rs` puts keys on record (`owned`). `notifier/` tests forge's
notifier on its own host.
