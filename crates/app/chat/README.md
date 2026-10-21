# chat

The `chat` program: channels, direct messages, threads, reactions and members. Its contract is
its row in the repository's README; `src/lib.rs` says how its files are laid out.

## Posting text

`Post { channel_id, message_id, text, thread }` posts as `PostMessage` does, with its blocks
parsed in chat from `text` by `parse_message`, the composer's own markdown (each line its own
paragraph, `<@N>` a mention). A writer with text in hand, an agent or a script, sends it as it is;
the posted message is the one `PostMessage` with `parse_message(text)` makes.

## Threads

A reply's `thread` is its thread's root. Every op that names a thread (`PostMessage`, `Post`,
`Notify`, `NotifyWith`, `Drafting`, `Take`, `Release`, `Break`) may name the root or any reply under it: a
reply's seq stands for its root, so a reply to a reply lands under the root, and the root counts
it once. The queries (`Thread`, `Busy`, `Holders`) take the root's seq.

## Clients

A post keeps the client its poster's frame names (`ctx.client()`: the signer's own word for the
software it signed with, such as `ducktape-app/0.1.0+437f749 (linux; cef)`), in `client/` by
channel and seq, and keeps none when the frame names none. `Clients { channel_id, seqs }` answers
each seq with its client or null; messages posted before clients were kept have none. It is for
showing, and for choosing what to show, never for deciding who may act.

## Members-only rooms

A room's post policy says who posts there: `Open` lets anyone, `MembersOnly` its owner and the
principals seated with `SetMembership`. `SetPostPolicy { channel_id, post_policy }` changes it after
`CreateChannel`, for whoever may rename the room, never a direct room. Going members-only ends the
holds and drafts of everyone it shuts out, in the same op, as an unseat does.
`Query::Seated { principal, channel_ids }` answers which of `channel_ids` the principal holds a
seat in, in the order asked, so a reader that lists its rooms learns its seats in one read. A
room's owner posts there seated or not, so whether someone may post is `ChannelRow::admits` with
what `Seated` says.

## Channel meta

A channel's meta is a map of text keys to text values. Whoever may rename the channel sets a key
with `SetChannelMeta` and removes it with `RemoveChannelMeta`; a direct room takes none. chat
keeps what it is given and answers it in `Query::Channel` and `Query::Channels`
(`ChannelInfo.meta`). It gives one key a meaning of its own, `listed`; the others below are
conventions its readers share.

| key | value | read by |
|---|---|---|
| `category` | a section name | nobody now: chat-view files rooms by the `/` in their names, and leaves the value as it is |
| `widget.header`, `widget.pinned`, `widget.side` | a widget file's path | chat-view ([widgets.md](../../../docs/widgets.md)) |
| `rules` (`chat::RULES`) | how to talk in the room | everyone who talks there |
| `filters` | tags, separated by commas (`node,app,mobile`) | chat-view: "All" and a chip per tag over the timeline, each showing the room's posts carrying that tag (`TagSearch`) |
| `listed` (`chat::LISTED`) | `involved` (`chat::INVOLVED`) | chat, which seats whoever posts there (root or reply) and every account a message posted or edited there mentions, while the room is open to every poster; chat-view, which lists the room only for someone seated there or its owner |

### `listed = involved`

The program that makes a room that many accounts could follow but few take part in (board's job
rooms, forge's change rooms) sets `listed = involved`. Posting stays as the room's post policy says.
While the room is open to every poster, chat seats each poster and each account a message mentions
there, once, in the op that posts or edits it, so a room someone was pulled into is one they sit
in, and `Seated` answers it. A members-only room seats no one so, since a seat there lets its holder
post; seats taken while the room was open stay if it goes members-only, and so let those accounts
post there. A reader's
sidebar lists such a room once they sit in it or own it; anyone else reaches it by a link or search.

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
the same way. Both walk the messages: chat keeps no search index. Harness's `chat_search` tool
reads `TextSearch` and shows each hit through `search_terms`, `sentences_holding` (the sentences of
its text that hold a term, a sentence ending at `.`, `?` or `!` before whitespace) and
`around_matches` (the bytes on each side of each match, overlapping windows joined), so a search
answers what it found rather than whole messages.

## Drafts

Someone answering in a room before they post keeps a draft there (`Op::Drafting`): its text, a
piece at a time, what they say they are doing (`doing`, such as `writing an answer`), and the
thread root it answers under, if any. `anew` starts the text over and keeps the block the draft
opened at. Drafting is allowed where the author may post, and a draft under a reply answers under
that reply's root. The author's post in the room takes their draft there down, and so does
`Op::Undraft`. A draft tells nobody: it never reaches attribution. `Query::Drafts { channel_id }`
answers who is answering in the room, each draft with its text folded from its pieces (a growing
draft adds a row per piece and never rewrites one). An author has one draft per room: drafting
under another thread moves it and starts its text over.

harness keeps one for each turn of an agent's conversation that answers in a chat place, and
chat-view draws each at the foot of the list it answers in ("tech is answering…").

## Holding a place

An account about to answer in a place, a channel's foot or one thread (`thread` its root's seq, or
a reply's, which stands for its root),
takes it first with `Op::Take { channel_id, thread }`. chat grants it while no one else holds that
place or has a draft open there (a place is exact: a thread is not its channel's foot, and a draft
counts only under the thread it answers in). Taking one you already hold is ok, whoever drafts there
meanwhile; one anyone else holds is refused `wrong_state`, `busy: <who> holds <place>` or `busy:
<who> is drafting in <place>`. Take is allowed where the sender may post, the rule `Drafting`
follows. A hold is advisory: it never refuses a post, so people and clients that never take one
post as before. chat keeps a row per held place (`lease/`, keyed by channel and `Option` root), and
nothing draws it.

A hold ends on an event, never on a timer, and every end is told to watchers:
- its holder's post in that place, or its holder's `Op::Release` (letting go of one you do not hold
  does nothing);
- `Op::Break { channel_id, thread }`, which ends the place's hold, whoever took it, and takes down
  every draft open there; whoever may rename the channel may send it (the channel's write policy);
- an op that takes away the holder's right to post there, in the same op: `SetChannelArchived`
  archiving the room ends every hold and draft in it, and `SetMembership` unseating someone from a
  members-only room ends their holds and their draft there, and `SetPostPolicy` making a room
  members-only ends those of everyone it shuts out. Bringing the room back or seating them again
  brings nothing back.

`Query::Busy { channel_id, thread, for_account }` answers who else is in the place: its holder
(`Holding::Lease`), else an open draft there (`Holding::Draft`), by anyone but `for_account`, as a
`Holder { principal, kind, height }` with the block it began; null when the place is free for
`for_account`. The height lets a reader see a place held long, and `Break` it.
`Query::Holders { channel_id, thread, page }` pages everyone there, as `Holder`s: the hold first,
then every open draft in that exact place; empty when no one is. `Busy` is the first of them
that isn't `for_account`. A reader that must discount
some of them (a wake whose waiters wait on each other) reads it rather than `Busy`.

A program that sends `Op::WatchPlaces { deliver }` hears each time a hold or a draft ends, as
`deliver ++ borsh(PlaceEvent::Cleared { channel_id, thread })`, once per place: a `Release`, a
`Break`, an archive or an unseat that ends them, an `Undraft`, a post that ends its author's hold or
takes down its draft there, and a draft moving to another thread (told for the place it left).
`PlaceEvent` only grows at its end. An empty `deliver` stops it, and its refusal never fails the op
that cleared. With no program watching, chat sends nothing. A place told cleared may still be held
or drafted in by someone else: the watcher reads `Busy` again.

## Each reader's own lists

chat keeps no reader's order of their lists: chat-view keeps it on the device. `SetRoomOrder`
(op tag 23) and `RoomOrder` (query and reply tag 20) are retired, their tags refused.

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
other off. An account's manager in identity chooses for it, or for several accounts at once, with
`ChooseForMany { accounts, channel_id, listening, notices }`: one choice for every account named, each the sender or one it manages, all written or none; a
choice both listening and cut is refused. `Query::Chosen` reads one account's choice for a room,
and `Query::ChosenBy { channel_id, accounts }` several in one read, in the order asked. chat-view's
room details offer the reader's own choice, then a row for each active agent the reader manages
(and one for all of them), read with `ChosenBy` and written with `ChooseForMany`.

## Asks

An ask is a question put to accounts, with two to four options and one of them the default its
asker goes on with (`chat::asking`). It is an ordinary message: chat keeps no ask state and has no ask
op. `asking::blocks` writes it in blocks every reader already shows: a first line that mentions whom
it asks, the question in bold and `#ask`; one numbered line per option (`N. label`, then
` — detail` when it has one), the default's label alone in bold; and an italic line saying what goes
ahead unless someone picks. A reader that draws no buttons shows it as text, and an answer is a
reply in its thread, typed or pressed. `asking::read` finds an ask in any message of that form, one
typed by hand included, and `asking::answer` reads a reply's words as an option: its label, case
folded, or its number. `asking::check` refuses what `read` could not read back. chat-view draws
an ask put to its reader as buttons, one per option and Other, and its answer once given.

`Query::Repliers { channel_id, roots }` answers, for each root asked and in the order asked, the
first reply each account wrote in its thread that is no notice, in reply order (a deleted one included, marked
deleted), so a reader learns who has answered which asks, and in what words, in one read.

## Notices

A module's status line (a pod ended, a check failed, a change merged) is a notice: `Notify {
channel_id, message_id, thread, notice }` posts a message whose one block is `Block::Notice(Notice
{ kind, group, subject, link, status, detail, concerns })`, where the room lets the sender post, as
`PostMessage` does. Only `Notify` and `NotifyWith` write one: `PostMessage`, `Post` and
`EditMessage` refuse a `Notice` block, and a notice is never edited. A later state of the same thing
(a run's start, then its end) is the next notice under the same `group`.

`NotifyWith { …, by, text }` is `Notify` with what a person wrote under the line and who acted.
`text`, when it holds anything, is markdown as `Post` takes it (`parse_message`), written as the
blocks after the notice's own, so a line whose event has words of its own (a review's body and line
comments) shows them under it. A notice is a message whose first block is the `Notice`
(`MsgRow::notice`), and a mention in its text tells no one, since a notice is told only to its
`concerns`. `by`, when given, names who did what the line reports (a review, a merge), a
`Principal`, an account that exists, never `Root`: `by` follows the line's thread as if it had
replied there, so the replies under the line reach it by the thread rule (`Why::Thread`,
attribution's `THREAD`), while the line itself is told only to `concerns`, which leaves out whoever
acted. It reaches no one a mention in a reply could not, so it is allowed wherever `Notify` is.
`NotifyBy` (tag 32) is retired: `NotifyWith` with `text: null` does what it did.

chat tells attribution a notice only for the accounts in `concerns` (`attributed::NOTICE`) and for
a wait watching the room (`WATCH`), never for the room's listeners, a mention or a thread's
participants, so harness wakes the agents it concerns and nobody else. `notice::why`
(`Why::Notice`) and `Query::Unread` count it only for the accounts it concerns, unless they turned
the room `Off`; a cut to `Mentions` keeps it. The notice's own text (`notice_text`) is one line:
the subject, the status word, the detail and the link, and a row's `text` follows it with its
body's. chat-view draws the notice as one quiet line: a mark in the status's tone, the subject (a
link when it has one), the status, the detail, and whom it is for, with its body under it as any
message's blocks are drawn.

A notice's `link` is either a whole link (`duck://…`, `https://…`) or a path on the reader's own
chain, `<program>/<tail…>` (`ducklink::path`), which is how a program links, since it can't name
its network's chain. A reader puts its own session's chain in front (`ducklink::on_chain`) and
never takes a chain from the path, so a path can't point at another network.
