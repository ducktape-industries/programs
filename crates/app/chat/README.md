# chat

The `chat` program: channels, direct messages, threads, reactions and members. Its contract is
its row in the repository's README; `src/lib.rs` says how its files are laid out.

## Channel meta

A channel's meta is a map of text keys to text values. Whoever may rename the channel sets a key
with `SetChannelMeta` and removes it with `RemoveChannelMeta`; a direct room takes none. chat
keeps what it is given and answers it in `Query::Channel` and `Query::Channels`
(`ChannelInfo.meta`). It gives no key a meaning of its own: the keys below are conventions its
readers share.

| key | value | read by |
|---|---|---|
| `category` | a section name | chat-view, which files the room under that section of its sidebar |
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
