# forge-review-chat

Posts forge's change events into chat rooms a network chooses. forge gives every change its own room (`forge:<repo>:<n>`), where its conversation lives. This program is a forge watcher: it maps a change's source branch to another room by the network's rules and tells that room what happens to the change, one notice per event, so a team that works in a room per task sees its changes there. It only posts: it never creates, renames or archives a room, and nothing flows back from the room into forge.

## Rules
`SetRules { rules }` writes the network's rules, tried in order. A change opened from `refs/heads/<name>` is mirrored into the room of the first rule whose `branch` matches the start of `<name>`. In `branch`, `{}` matches one or more characters other than `/` (the longest run the rest of the pattern follows), and it fills `{}` in `room`. `{ branch: "issue-{}/", room: "issue-{}" }` mirrors a change from `issue-42/fix-login` into room `issue-42`. A branch holds `{}` at most once, and a room only where its branch does. A change from a commit rather than a branch, or from a branch no rule matches, is not mirrored.

The room is decided once, as the change opens: the change is mirrored only while chat holds the room unarchived and open to post (not members-only), and its later events go to the same room. A room archived after that refuses the notice; the refusal is absorbed and never retried.

## Notices
Each notice is a chat `Notify` in the room, at top level, with the kind `change` (`forge::NOTICE`), the subject `<repo> #<n>: <title>`, the link `forge/<repo>/<n>` (`forge::notice_link`, a path on the reader's chain), and no account told (`concerns` empty): forge's own notice in the change's room already tells the accounts the event concerns.

| forge event (`Watched`) | group | status | detail |
|---|---|---|---|
| `Opened` | `opened <repo> <n>` | Running | `opened by account <a> from <branch>; its conversation is in <the change's room>` |
| `Reviewed` | `review <id> <repo> <n>` | Passed (approve), Failed (request changes), Note (comment) | `review <id>: approved`, `changes requested` or `commented`, then `by account <a>` |
| `LeftQueue` | `left queue <repo> <n>` | Failed | `left the merge queue of <branch>: <why>` |
| `Merged` | `merged <repo> <n>` | Passed | `merged into <branch> by account <a>` |
| `Closed` | `closed <repo> <n>` | Note | `closed by account <a>` |

`Moved` and `Previewed` post nothing. A review's body stays in forge.

## Rows
| key | what |
|---|---|
| `rules` | the rules, an owned row (kind `forge-review-chat.rules`): its first writer's, so the ownership policy it is linked to decides every later `SetRules` |
| `room/` (repo, n) | the room an open change is mirrored into; removed when it merges or closes |
| `posted` | how many notices it posted, which names each one's message id (`forge-review-chat:<n as 16 hex digits>`) |

## Ops
- `SetRules { rules }`: any account. The first write claims the rules row for its writer, and the ownership role links it to the writer's policy for the kind `forge-review-chat.rules` (their default, which lets anyone write until they set one), which decides every later write. A rule with an empty branch or room, a second `{}` in its branch, or a `{}` in its room that its branch lacks is refused `invalid_input`.
- `Watched(event)`: forge only (its watch deliveries, `sent_by`). The program sends forge `Watch { deliver: [0] }` from `init`, so it hears every event from its admission on.

## Queries
`Rules`: the rules, in the order they are tried. `Room { repo, n }`: the room change `n` is mirrored into, or null. `Mirrored { page }`: a page of every open mirrored change (`Mirror { repo, n, room }`), by repository then number: the `room/` rows, so it costs what is open, never what has ended. A reader that wants a room's changes (a board telling two rooms their changes touch one path) reads them here.

## Interface
`Op`, `Query` and `Reply` only grow at their end. `Op::Watched` stays tag 0: it is the prefix forge delivers under.
