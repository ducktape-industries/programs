# chat-view

The Chat view: channels, direct messages, threads, search and widgets over the
`chat` program, on the desktop (`ui/`) and the phone (`ui/phone/`, the Expo
app's screens). `lib.rs` picks the design once per frame by `window.touch()`;
`render` never mutates, and what an event changes lands in `room`, `actions`,
`search`, `compose` and `policies`. The `notifier` feature builds chat's
NOTIFIER (`notifier.rs`) alone, in place of the view.

## Panes and screens
- Rooms: the desktop sidebar (`ui/sidebar.rs`) and the phone's rooms page
  (`ui/phone/rooms.rs`) list the same rooms (`Chat::room_list`): the channels
  the reader is in (opened on this device, listened to, or owned), then their
  direct rooms. Any other room, a change's or a job's too, is found by search,
  which lists the rooms whose names hold the words above the message hits
  (`Chat::found_rooms`; archived and direct rooms never). In both lists, a `/`
  in a channel's name files it under
  folders (`Chat::lines`): `jobs/j-097` is listed as `j-097` under **jobs**,
  each folder where its first channel would be, a channel named as a folder
  is that folder's row, and a closed folder counts the unread rooms inside.
  Only the first `/` makes a folder (`a/b/c` is `b/c` under **a/**). In the
  new-channel form, the text before the first `/` typed becomes one chip at
  the field's start (a token whose id starts `wire::CHIP`); Backspace right
  after it makes it text again, and the whole text is the channel's name.
- The room: its timeline (`ui/timeline.rs`, `ui/phone/timeline.rs`) and
  composer; a message's menu (desktop `ui/menu.rs` at the pointer, phone
  `ui/phone/actions.rs` as a sheet), the reaction picker and the delete
  confirm. A message's blocks draw as chat keeps them, and a run of `| … |`
  lines under a `|---|` row as a table, a `# ` line as a heading (`markup.rs`);
  code blocks, tables and headings draw through the SDK's `markdown` crate,
  coloured by a block's language, with no parser linked.
- Beside the room (desktop) or as a page (phone): the thread, the channel's
  details (`ui/side.rs::details`, `ui/phone/details.rs`) and a widget.
- Rules: a channel's `rules` meta (`rules.rs`, `ui/rules.rs`), the text or the
  content of the file a files path names, read while the details are open and
  again when a files block moves it. The details show them (the desktop's also
  set and clear them), and a faint "This room has rules" over the composer
  opens the details.
- Dialogs: new channel, new direct message (`ui/dialogs.rs`, phone pages).
- Who can edit (`policies.rs`, through `policy-view`'s `item`): a row in the
  channel's details, and "Who can edit" in a message's menu, each opening a
  sheet with Change policy… and Transfer… for the item's owner.

## State
One struct, `Chat` (`state.rs`), with a field per pane: `room` (its messages,
members, thread), `details`, `menu`, `search`, `create`, `dm_pick`, `layout`,
`reads` (unread cursors, kept in the view store), `closed_folders` (the room
list's closed folders, kept in the view store under `folders`), `drafts` (the
composers).
The session keeps each room it opened and left in `visited` (`Visited`): its
newest page of rows (`Chat::screenful`, the tallest pane placed this session
over the shortest row), the thread open beside it with its newest page of
replies, and where each list stood (`Anchor`: a row's id and how far into it).
Pages read further up are never kept, so a room costs one page however far it
was read, and nothing of it outlives the session. Opening it again draws that
at once and reads only what blocks wrote since; a reader who stood above the
kept page lands where they stood. A room opens at its newest row and a thread
at its newest reply. A list reads the page before its first row once layout
puts that row in view (`listing::edged`): a scroll up to it, or a list too
short to fill its pane, until the room's first message. The reader's own posts
never count as unread: a list that shows a newer head of a room they posted in
moves the cursor past their own posts up to the first anyone else wrote.
Reads land in `Loadable` slots. `room_policy` and `message_policy` are
`policy_view::Editing` slots: the room's is read while its details, a
message's menu or its archived strip ask it, the message's while its menu or
sheet is open (`settle_policies`, each frame). What the reader may do follows
them: `may_change_room` (rename, archive, seat and remove members, delete
others' messages) and `may_edit` (edit) answer the item's policy once read,
and the rule from before policies (the channel's owner, the message's author)
until then and for an item with no record. `may_delete` follows chat: the
author's delete is `may_edit`, anyone else's `may_change_room`.

## Follows
The session, routes, visibility, the chain's heads, and the live heads of
chat, identity and ownership (`watch.rs`); a chat block re-reads only
what its `#[reads]` say it wrote.

## Tests
`src/tests/` drives the view against the SDK's fake host. `tests/mod.rs`
`configure` answers chat (`chat_reply`: three rooms, two rows in #general,
a roster of eddy 7, his agent reviewer 8, forge 9 and ada 10), identity (no
pictures, every agent answering anyone), and ownership with no records; `opened()` seats eddy and opens #general.
`owned(cx, key, owner, policy, policies)` puts one key on record, with
identity's names. Every unhandled ask panics, so a new query needs an arm
there. `tests/phone.rs` holds the phone screens and the 360/390/430 sweep.
