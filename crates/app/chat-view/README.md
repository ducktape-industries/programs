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
  the reader is in (opened on this device, listened to, owned, or written in:
  chat's `PostedIn`), then their direct rooms. The channels they are not in,
  a change's or a job's too, wait under the channels' last row, "N more
  channels", which a press unfolds, by name, into the folders their names give
  (`jobs/`, `pr/`, …), each closed until opened there and apart from the
  reader's own folder of that name (archived rooms never). A members-only
  room is listed only where the reader may post (seated, or its owner), and a
  room whose meta says `listed = involved` (a job's, a change's) only once the
  reader sits in it or owns it: chat seats whoever posts or is mentioned
  there (chat's `Seated`, one read for every such room). Search
  finds any room too: it lists the rooms whose names hold the words above the message hits
  (`Chat::found_rooms`; archived and direct rooms never). In both lists, a `/`
  in a channel's name files it under
  folders (`Chat::lines`): `jobs/j-097` is listed as `j-097` under **jobs**,
  each folder where its first channel would be, a channel named as a folder
  is that folder's row, and a closed folder counts the unread rooms inside.
  Only the first `/` makes a folder (`a/b/c` is `b/c` under **a/**). Each
  level (the top, and inside a folder) lists what the reader's own order
  (kept on this device under `reads/<signer>/order`, never on chain) leaves
  out first, as chat lists it, then what it names, in its order. A row moves only within its level: Alt+↑/↓ on the active row,
  Move up/down in a room's menu, a drag on the desktop, a long press then the
  finger on the phone (the host relays the held finger's moves and lift), or a
  phone row's Move up/down actions for a screen reader; each move writes the
  whole order there, the level as drawn set among the places the order gives
  rooms not listed yet, so a move made before every room landed drops none.
  Rows move once the kept order has been read. In the
  new-channel form, the text before the first `/` typed becomes one chip at
  the field's start (a token whose id starts `wire::CHIP`); Backspace right
  after it makes it text again, and the whole text is the channel's name.
- The room: its timeline (`ui/timeline.rs`, `ui/phone/timeline.rs`) and
  composer; a message's menu (desktop `ui/menu.rs` at the pointer, phone
  `ui/phone/actions.rs` as a sheet), the reaction picker and the delete
  confirm. A message's blocks draw as chat keeps them, and a run of `| … |`
  lines under a `|---|` row as a table, a `# ` line as a heading (`markup.rs`),
  and a line that is one `duck://` link on the reader's chain to a page (a
  files path ending `.html` or `.htm`, or a blob `duck://<chain>/blobs/<hex>`)
  as the artifact it names (the SDK's `artifact`, so the manifest declares
  `artifact`): the app draws the page as tall as it is and reloads it when the
  file changes, or the link alone where it has no web view; a link inside a
  sentence stays a link;
  code blocks, tables and headings draw through the SDK's `markdown` crate,
  coloured by a block's language, with no parser linked.
- Answering (`ui/answering.rs`): at the foot of the room and of an open
  thread, each draft chat holds there (`Drafts`) but the reader's own. Who
  holds the place by chat's `Busy` is said with the block they began at, and
  Break for whoever may change the room: on the holder's draft row, or on a
  line of its own for a hold (`Take`). The reader's own typing (`typing.rs`) is
  chat's empty draft, `doing` "typing": a keystroke that takes a composer on
  screen from empty to words sends it, and that composer emptying or leaving
  the screen takes it down (a post does itself). Words already in a composer
  send nothing. The rooms it was sent in are kept under
  `reads/<signer>/typing`, each until its `Undraft` lands, and each start takes
  it down in every room kept there but one typed in this session. A switch of
  account leaves the old account's typing up until it next starts here.
- Asks (`asks.rs`): a top-level message `chat::asking::read` reads as an ask
  put to the reader, in a room they may write in, draws a button per option
  (the default's says so) and **Other…** under its blocks, on the desktop and
  the phone. A button posts its option's label as a reply in the ask's thread
  (`Chat::pick`, through the reply composer's send, with no files), and Other
  opens the thread with its reply field taking the keys. Whether an addressee
  has answered is chat's `Repliers`, read for the asks put to the reader among
  the rows each time rows land, beside who reacted (`Room::read_marks`), and a reply of the
  reader's own still in flight counts too: the buttons then give way to
  "Answered: <option>", or "Answered in the thread" for words of their own.
  Anyone else reads the ask as its text.
- Beside the room (desktop) or as a page (phone): the thread, the channel's
  details (`ui/side.rs::details`, `ui/phone/details.rs`) and a widget.
- Rules: a channel's `rules` meta (`rules.rs`, `ui/rules.rs`), the text or the
  content of the file a files path names, read while the details are open and
  again when a files block moves it. The details show them (the desktop's also
  set and clear them), and a faint "This room has rules" over the composer
  opens the details.
- Filters: a channel's `filters` meta (`filters.rs`, `ui/filters.rs`), tags
  separated by commas (a leading `#` and repeats dropped). The room draws "All"
  and a chip per tag over its timeline (the SDK's `segmented`), `#tag` under a
  pointer and the bare tag on a phone, whose chips share the row. A chip shows
  only the room's posts carrying its tag, newest at the foot, read through
  chat's `TagSearch` a screenful a page (the SDK's `Paged`); the first row in
  view reads the page after, and a block that writes the room's messages or
  reactions reads it again. All is the room's live timeline. A room without
  the meta draws no chips. The pick is the session's: another room, or a swap,
  opens on All, and so does a meta that no longer names the picked tag.
- Dialogs: new channel, new direct message (`ui/dialogs.rs`, phone pages).
- Who can edit (`policies.rs`, through `policy-view`'s `item`): a row in the
  channel's details, and "Who can edit" in a message's menu, each opening a
  sheet with Change policy… and Transfer… for the item's owner.

## State
One struct, `Chat` (`state.rs`), with a field per pane: `room` (its messages,
members, thread), `details`, `menu`, `search`, `create`, `dm_pick`, `layout`,
`reads` (unread cursors, kept in the view store), `closed_folders` (the room
list's closed folders, kept in the view store under `folders`), `order` (the
reader's order of the lists, kept under `reads/<signer>/order`), `drafts` (the
composers).
The session keeps each room it opened and left in `visited` (`Visited`): its
newest page of rows (`Chat::screenful`: the tallest pane placed this session,
or before any the window's viewport, over the shortest row), the thread open beside it with its newest page of
replies, and where each list stood (`Anchor`: a row's id and how far into it).
Pages read further up are never kept, so a room costs one page however far it
was read, and nothing of it but where the timeline stood outlives the session. Opening it again draws that
at once and reads only what blocks wrote since; a reader who stood above the
kept page lands where they stood. Where the timeline stood is also kept in the
view store under `scroll/<room id>` (`view::scroll`, written by `standing` as the
list settles, one write out at a time, removed while it follows its newest
row), so a room opened with no `Visited` (a new run, or after a swap, whose
snapshot keeps no `visited`) reads it beside its first page and opens on that
row, landing on its seq when the page lacks it. A thread's place lasts the
session only. Otherwise a room opens at its newest row and a thread
at its newest reply. Every read of rows is a screenful, and a link lands on a
screenful each side of its message; an open room keeps every page read in it
(the reader's scroll bounds it), and its list draws only the rows in view. Rooms,
members, and the rooms listened to and written in are read whole, one request
each (`limit: None`). A list reads the page before its first row once layout
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
author's delete is `may_edit`, anyone else's `may_change_room`. Both shells
offer a message's actions through one `offered`, as chat checks them: react
and edit only where the reader may write (`may_write`, chat's `writable`),
delete by `may_delete` alone, archived or not; edit only where `editable` (no
notice), and none of the three on a deleted message.

A file is drawn from files alone, never from the device: the composer reads a
picked file only to put its chunks, its chip shows progress with no picture, and
it lands once a block holds it in files, on the snapshot files minted there (the
commit's own answer is the node's prediction, and files mints its id in the
block). Every message, the reader's own as anyone else's, draws its files by the
path and snapshot it names, read at that snapshot. A read there that answers
`not_found` (files minted no such snapshot, as for a post that recorded the node's
prediction, or this node pruned it) is read at files' newest instead, once. A
picture's bytes are read by the chunk blobs its file object names, each a blob of
its own, so no commit landing mid-read prunes them under it. A file files holds nothing at
yet is read again once a files block writes its path (one `Head` and one `Diff`
a block under the folder the waiting files share, however many wait); a read
that failed is read again on the next files block and, failing again, once the
node's link reopens or the view comes back on screen; and files' blocks stay
followed while any such file is out being read. Until a file is read, its row
says so where its size goes: "Reading…" while a read is out, "Couldn’t read the
file" once one failed, and "Missing from files" while files holds nothing at its
path; its Save stays faint until then.

## Follows
The session, routes, visibility, the chain's heads, and the live heads of
chat, identity and ownership (`watch.rs`); a chat block re-reads only
what its `#[reads]` say it wrote.

While a room is open it stands its questions on the node (`program.watch`,
`questions.rs`): its rows (the newest roots, as many as it holds and at least a
screenful, or the span a landing holds), the open thread, who is answering, its
members, who holds it busy, who reacted, and the rooms the reader listens to
and wrote in. Each answer lands where its read would, so the
answers a block moves land in one frame: a draft that becomes its post hands its
node to the post's row. A block then reads again only what no answered question
covers (a held row older than the newest roots the room asks), and the channel
list still reads each room the block wrote (`Channel`). When the node can't
answer a question this block, the room reads it itself; a host or node that
stands none ends them, and the room reads each block as before.

An agent's draft (chat's `Drafting`) is a row at the foot of the list it
answers in, keyed by its author and opening block. When the block that posts
takes it down, the row stays as it last was until the post's row lands, and that
row is drawn inside the draft's node: one node carries from the draft to the
message, whichever read lands first.

## Tests
`src/tests/` drives the view against the SDK's fake host. `tests/mod.rs`
`configure` answers chat (`chat_reply`: three rooms, two rows in #general,
a roster of eddy 7, his agent reviewer 8, forge 9 and ada 10), identity (no
pictures, every agent answering anyone), and ownership with no records; `opened()` seats eddy and opens #general.
`owned(cx, key, owner, policy, policies)` puts one key on record, with
identity's names. Every unhandled ask panics, so a new query needs an arm
there. `tests/phone.rs` holds the phone screens and the 360/390/430 sweep.

## The notifier

`notifier.rs` is chat's NOTIFIER: the host runs it once per network, whether or not a chat pane is
open, and the panes post nothing. It hands the host's notification centre a message from someone
else, landing in a room the reader is not looking at, that is meant for them, as chat's
`notice::why` decides: in a direct room of theirs; a mention of them, in any room; a reply in a
thread they are in (they wrote its root, replied in it, or were mentioned in it, before this reply);
or in the timeline of a room they are in (they wrote in it before, opened it, or hold a seat in it,
for a members-only room). Where the reader is in a room chat keeps on chain, so every device and
every server that runs the notifier tells the same: each read of a room's new messages asks it
(`Query::Followed`) for the room and the threads those messages reply in. What the reader chose
for a room in its details goes over all of that: every message there, mentions only, or nothing.

A notice's title is the sender and why (`<sender> mentioned you`), its body the message, its tag
the room (`@peer`, `#channel`), its link the message.

The tab badge counts such messages in rooms the reader has not read, as chat counts them
(`Query::Unread`, one read for every room behind): no message is read to count them, so a launch
costs the same however much the reader left unread. The read cursors are the panes'
(`reads/<signer>`, kept by `kept.rs`); a pane that reads a room moves its cursor, the host tells
the notifier (`store.changes`), and the pane takes the room's notices down itself (`notify.seen`).
The room a pane shows on screen (`host.screen`) gets no notice and leaves the badge.

What it announced it keeps under `notified/<signer>`: the seq announced up to in each room. That
seq moves only once the read of those messages, their rows and where the reader is
(`Query::Followed`), has landed whole in the run that asked it: a read that fails, either half,
leaves them to the room's next head, and a run that ends while it reads (a disconnect, a
switch of key) leaves them to the next run. A run that starts (a launch, a reconnect, a new
deployment) posts, once, what came for the reader past both that and their cursor while nothing
ran, reading only those messages; a room it never saw is new, read from its start. The very first
run on a device has nothing kept: what is there is history, not news. A room with no read cursor
on this device (never opened here) is counted for the badge from where this device first saw it,
kept under `unread-since/<signer>`, not from what was announced, so its count outlives a launch.
