# wake

Wakes an account once on one event, wherever it runs: a harness agent, a bridge (`ducktape bridge`), or a person's own client. The account arms a wake (`WakeOn`), wake hears the event from the program that has it, and the first time it happens wake tells the account through attribution: a `Result` relation on the object `wake/<id>`, actor `Program("wake")`, its detail `Wake <id> is over: <what happened>.`. Something off the network that follows attribution's `to/<account>` rows hears it as it lands, and reads it back after downtime with `ChangesTo`. A harness agent hears it as an input in the conversation whose turn armed the wake (a `WakeOn` its tool call sent), which turns at once, even when the wake ends inside that `WakeOn` (a clear place); a conversation closed since is told it was not delivered, and a wake harness didn't see armed (a booked run's, say) reaches the agent's canonical conversation. harness also arms each of its agents' waits but a chat message's (`wait_for`) as a wake for the agent, keeping the wait's own number, and a wake that backs a wait ends that wait instead (`Wait N is over: …`); so every network founds wake beside harness. Shapes: `src/lib.rs`; checks: `src/rules.rs`.

## Events
| `WaitFor` | Heard from | Told |
|---|---|---|
| `Height { height }` | its own `Due`, scheduled at that height through the scheduler role | `block H has come` |
| `Pod { name }` | refused: wake no longer follows pods, so a pod is waited on with the pods service's own wait stream (a tool file such as pod_wait). A pod wake still open from before ends on its next `PodWait` or `PodEvent`, and each `WakeOn` and `Due` ends one more of those left, oldest first | `wake no longer follows pods, so it stopped waiting on pod <name>: …` |
| `Change { repo, n }` | forge's watchers (`Forged`): its source ref's moves, its close, its merge, forge taking it out of a merge queue (`LeftQueue`), and a review filed on it (`Reviewed`). An already closed or merged change is refused | `change N in <repo>: its source <ref> moved from … to … by …`, `change N in <repo> was closed by …`, `… was merged by … as <oid>`, `… left the merge queue of <branch>: <why>`, `… was reviewed by … (review <id>: approved)` (or `requested changes`, `commented`) |
| `Ref { repo, reference }` | forge's watchers: that ref's moves (a bare branch name is `refs/heads/<name>`). A repository forge doesn't hold is refused | `<ref> in <repo> moved from … to … by …`, `was made at …`, `was deleted by …` |
| `File { path }` | files' watchers (`Filed`): a commit that changes, makes or removes the path or anything below it, or removes or moves a folder above it. wake reads the commit's changed paths (files' `Diff` from its parent, or `Find` for a first commit) and looks up only the wakes on those paths, their folders and, for a folder, the paths below it (`path/`). A path whose node (files' `Stat` at the commit) is the one the wake was armed with, a rewrite of the same bytes, tells nothing | `<path> changed in files snapshot <id> by … ("<message>")`, `was made`, `was removed` |
| `Settled { repo, n, until }` | forge's watchers: its close, its merge and its leaving a merge queue, never a review | as `Change`, with `, so it will not be merged` (or `closed`) when it ends the other way |
| `Program { program }` | the programs role's `Scheduled`: armed only while a `Set`, `Publish` or `Remove` of that program is still to land, else refused. It keeps the program's entry when armed and books its own `Due` at the earliest such height; there it reads `Program` again | `program P's code changed at block H from <old> to <new>`, `was set at block H, code <new>`, `was set again … with new params`, `was removed`; a change that did not land books the next one, or says `did not land, and none is scheduled now` |
| `Clear { channel_id, thread }` | chat's `Holders` for the place (a channel's foot, or a thread by its root's seq), read at arming and again on each `Placed` (chat's `PlaceEvent::Cleared`, watched only while a clear wake is open). The place is clear when every hold and draft there belongs to an account that itself waits for it to clear (its own included), so two accounts waiting on each other both go free while a third one drafting keeps them waiting. Clear at arming ends at once | `#<channel> is clear`, `thread R of #<channel> is clear` |
| `Message { … }` | refused: attribution already tells an account each message for it | |

An event ends its ref's, change's or path's wakes one scheduled frame each, never all in one op: the op that hears it ends the first open wake armed before it, then books `Fan` for the two halves of the ids left for the next block, and each `Fan` frame does the same with its half. One frame's work is one wake on any network, metered or not, and N wakes end within about log2 of the id span in blocks. Only wakes armed before the event are in its range (`fired/` keeps, per key, the last id an event reached), so a wake armed while the fan runs waits for the next event. A review fans over the change's wakes under a key of its own (`Key::Review`), ending only the `Change` ones, so a review never moves where a settled wake's close or merge starts.

A program wake is booked at the earliest change scheduled when it is armed (or when the one it booked did not land): a change scheduled later for an earlier height is heard at the booked block, since the programs role tells no one of a new schedule.

`WaitFor` lists harness's first seven variants in harness's order, then `Program` and `Clear`. harness appends its own `Clear` right after its seven, so it turns each of its events into wake's (`waits::waking`) rather than reusing the bytes.

wake hears forge (`Watch { deliver: [5] }`) only while a forge wake is open, and files (`Watch { deliver: [6] }`) only while a file wake is open: the first such wake turns the watch on, the last one over turns it off with an empty `deliver`.

## Who may call each op
| Op | Who |
|---|---|
| `WakeOn { event }` | any account, for itself (`sender_account`). Returns the wake's id |
| `Cancel { id }` | the wake's account; anyone else is refused `unauthorized`. A cancelled wake is gone, and its scheduler entry is cancelled |
| `Due` | wake's own scheduled entry alone (`Cause::Scheduled`, sent by wake) |
| `PodWait { id }` | anyone: ends wake id if it is a pod wake still open |
| `Fan { firing }` | wake's own scheduled entry alone |
| `PodEvent { id, event }` | the program bound to the egress role alone: ends wake id if it is a pod wake still open |
| `Forged(Watched)` | forge alone (`sent_by`) |
| `Filed(SnapshotRow)` | files alone (`sent_by`) |
| `Placed(PlaceEvent)` | chat alone (`sent_by`) |

The deliver prefixes are borsh's tags of these ops: `PodEvent` is `[4] ++ id u64 LE`, `Forged` `[5]`, `Filed` `[6]`, `Placed` `[8]`.

A told end is a message wake wants answered: if attribution refuses it, the wake's ended text keeps `; not told: <why>`.

## Queries
Answered for anyone: `Wake { id }` (the row, `null` for a cancelled or unknown wake) and `Wakes { account, page }` (an account's wakes, open and ended, oldest first). A row is `{ id, account, made, event, ended: { height, text } }`.

## Rows
| table | key | what |
|---|---|---|
| `wake/` | id | `WakeRow` |
| `account/` | (account, id) | index: an account's wakes |
| `last` | | the last id |
| `entry/`, `scheduled/` | scheduler entry ↔ id | a height or program wake's booked `Due` |
| `pod/` | id | a pod wake still open from when wake followed pods: removed as it ends, never written |
| `ref/`, `wake-ref/` | (repo, ref, id) ↔ id | ref and change wakes by the ref they follow |
| `change/` | (repo, n, id) | change and settled wakes |
| `forge-count`, `file-count` | | how many forge and file wakes are open |
| `file/` | id | a file wake's path and node when armed |
| `path/` | (path, id) | file wakes by path |
| `fired/` | borsh of a ref, change, review, path or place key | the last wake id an event of that key reached |
| `program/` | id | a program wake's entry when armed |
| `clear/`, `clear-by/` | (place, id), (place, account, id) | clear wakes by place, and by place and account |
| `clear-count` | | how many clear wakes are open |
| `telling/` | message seq | an end told and not answered yet |

## Interface
`Op`, `Query`, `Reply` and `WaitFor` grow only at their end (`op_variants_only_append`, `wait_for_variants_only_append`).
