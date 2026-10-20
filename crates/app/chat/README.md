# chat

The `chat` program: channels, direct messages, threads, reactions and members. Its contract is
its row in the repository's README; `src/lib.rs` says how its files are laid out.

## Posting text

`Post { channel_id, message_id, text, thread }` posts as `PostMessage` does, with its blocks
parsed in chat from `text` by `parse_message`, the composer's own markdown (each line its own
paragraph, `<@N>` a mention). A writer with text in hand, an agent or a script, sends it as it is;
the posted message is the one `PostMessage` with `parse_message(text)` makes.

## Channel meta

A channel's meta is a map of text keys to text values. Whoever may rename the channel sets a key
with `SetChannelMeta` and removes it with `RemoveChannelMeta`; a direct room takes none. chat
keeps what it is given and answers it in `Query::Channel` and `Query::Channels`
(`ChannelInfo.meta`). It gives no key a meaning of its own: the keys below are conventions its
readers share.

| key | value | read by |
|---|---|---|
| `category` | a section name | nobody now: chat-view files rooms by the `/` in their names, and leaves the value as it is |
| `widget.header`, `widget.pinned`, `widget.side` | a widget file's path | chat-view ([widgets.md](../../../docs/widgets.md)) |
| `rules` (`chat::RULES`) | how to talk in the room | everyone who talks there |

### `rules`

A room's rules say how to talk in it: style, language, what belongs there. A value that starts
with `/` and names a file files holds is a path, and that file's content is the rules. Any other
value, a path that names no file included, is the text itself.

Rules are something people and agents respect. Nothing enforces them, and they are not
harness's: every runtime and every person reads the same meta through `Query::Channel`.
- harness hands them to the agents it runs, as one input ahead of the first message a
  conversation hears from the room. It hands them again when the value or the file's content
  changes, and after the conversation is summarized.
- An agent that runs outside harness reads them itself: `Query::Channel { channel_id }`, then
  `meta["rules"]`, and the file through files when the value is a path.
- chat-view shows them in the room's details, where they are set and cleared, and says "This room
  has rules" over the composer.

## Names

`Query::Names { ids }` answers the name of each room `ids` names, as `(id, name)` in the order
asked, and nothing else of the room (no meta, no head): a reader that holds room ids (harness's
conversations name their place as `chat/<channel>`) names them in one read. An id no room has is
left out.

## Search

`Query::TextSearch { text, viewer, channel_id, page }` answers one page of the messages holding
every whitespace-separated term of `text` as a substring (NFC, case folded), newest first, in one
channel or all; `next` resumes it. `Query::TagSearch` pages the messages whose `#tags` hold a tag
the same way. Both walk the messages: chat keeps no search index. `Query::Search` answers the
first page alone, its `capped` saying the walk stopped at `page.limit`; harness's `chat_search`
tool reads it.

## Drafts

Someone answering in a room before they post keeps a draft there (`Op::Drafting`): its text, a
piece at a time, what they say they are doing (`doing`, such as `running forge_read`), and the
thread root it answers under, if any. `anew` starts the text over and keeps the block the draft
opened at. Drafting is allowed where the author may post, and a draft under a thread names that
thread's root. The author's post in the room takes their draft there down, and so does
`Op::Undraft`. A draft tells nobody: it never reaches attribution. `Query::Drafts { channel_id }`
answers who is answering in the room, each draft with its text folded from its pieces (a growing
draft adds a row per piece and never rewrites one). An author has one draft per room: drafting
under another thread moves it and starts its text over.

harness keeps one for each turn of an agent's conversation that answers in a chat place, and
chat-view draws each at the foot of the list it answers in ("tech is answering…").

## Each reader's own lists

An account puts its rooms and folders in its own order with `Op::SetRoomOrder { order }`: a list
of `Entry::Room(channel_id)` and `Entry::Folder(name)`, a folder being the text before a channel
name's first `/`. The op writes the sender's own order whole, and an empty list takes it off.
`Query::RoomOrder { account }` answers it, empty for an account that set none. chat gives the
order no meaning of its own: chat-view lists, at each level, what the order leaves out first in
chat's order, then what it names in its order, so the reader's lists follow it across devices.

`Query::PostedIn { account, page }` answers the channels an account wrote in the timeline of, kept
as each post is noted (beside `Followed`'s room seq). It starts from the code that keeps it: posts
made before are not listed.

## What is meant for a reader

`notice::why` says whether a message is meant for an account, and why: it is in a direct room of
theirs, mentions them, replies in a thread they are in (they wrote its root, replied, or were
mentioned there before it), or is in the timeline of a room they are in (`Query::Followed`). What
they chose for the room goes over it: listening is every message, `SetNotices` mentions only (and
direct rooms) or nothing. chat-view's notifier tells by it.

`Query::Unread { account, rooms }` counts by it: for each `(channel_id, since)`, the messages after
`since` meant for `account`, answered as `UnreadCount { channel_id, to, meant }` in the order asked,
`to` being the room's head it counted to (a room no channel has counts nothing). The cursors are the
reader's own (chat-view keeps them on the device), so it names them. It answers a count, not the
messages, so a reader's badge costs one small read however much it left unread.

## Who hears a room

Each account chooses what a room tells it: every message (`Listen`), or what `SetNotices` cuts it
to (`Mentions`, `Off`, or chat's default with none). The two are one choice, and each takes the
other off. An account's manager in identity chooses for it with `ListenFor` and `SetNoticesFor`,
and for several accounts at once with `ChooseForMany { accounts, channel_id, listening, notices }`:
one choice for every account named, each the sender or one it manages, all written or none; a
choice both listening and cut is refused. `Query::Chosen` reads one account's choice for a room,
and `Query::ChosenBy { channel_id, accounts }` several in one read, in the order asked. chat-view's
room details offer the reader's own choice, then a row for each active agent the reader manages
(and one for all of them), read with `ChosenBy` and written with `ChooseForMany`.

## Notices

A module's status line (a pod ended, a check failed, a change merged) is a notice: `Notify {
channel_id, message_id, thread, notice }` posts a message whose one block is `Block::Notice(Notice
{ kind, group, subject, link, status, detail, concerns })`, where the room lets the sender post, as
`PostMessage` does. Only `Notify` writes one: `PostMessage`, `Post` and `EditMessage` refuse a
`Notice` block, and a notice is never edited. A later state of the same thing (a run's start, then
its end) is the next notice under the same `group`.

chat tells attribution a notice only for the accounts in `concerns` (`attributed::NOTICE`) and for
a wait watching the room (`WATCH`), never for the room's listeners, a mention or a thread's
participants, so harness wakes the agents it concerns and nobody else. `notice::why`
(`Why::Notice`) and `Query::Unread` count it only for the accounts it concerns, unless they turned
the room `Off`; a cut to `Mentions` keeps it. Its `text` (`notice_text`) is one line: the subject,
the status word, the detail and the link. chat-view draws it as one quiet line: a mark in the
status's tone, the subject (a link when it has one), the status, the detail, and whom it is for.
