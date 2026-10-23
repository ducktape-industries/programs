# harness

agents that consensus runs, nothing outside it driving. An agent is a program account of
the `Agent` category whose script calls harness (identity's `SetScript`); the script's
`config` is the path of its configuration file in `files`: Markdown whose TOML front
matter, between `+++` lines, names the model as its vendor names it and its `provider`
(both required), `max_tokens`, an optional `wait` (the blocks egress waits) and an
optional `thinking` (`off`, `low`, `medium`, `high` or `max`; absent leaves the model's
default, and the Anthropic wire sends `off` as thinking disabled and a level as adaptive
thinking at that effort), and an optional `compact_at` (tokens: once a thread's measured
context reaches it, harness summarizes the thread before its next turn; at least 1, and
absent: a thread summarizes only when the next answer would not fit), an optional
`service` (accepted as metadata but ignored for placement; harness reads `provider`
beside `harness::Config`, as `harness::config::Pin`), and whose body is the system prompt (`harness::Config`). A
model call sends each paragraph of the system prompt as its own system block, in order,
then harness's own block, the ducktape resource manual: the agent's account and its
manager, the conversation, and how writes, tool calls and chat replies work. Harness
reads the file at the head at each model call, and each turn it records names the files
snapshot it read; a conversation keeps the `model`, `max_tokens`, `thinking`,
`compact_at` and `provider` of the first configuration harness could route
for it (read as it opened, or, for a conversation already open when new harness code
first runs, as that run reads its agent's), so an edit to those reaches only
conversations opened after it (that first run also forgets each model window a service
reported none for, so each is asked once more), and `ForkThread` moves a conversation
onto the current front, as the fork is a new conversation; `Query::ThreadCard { thread
}` answers `Card` as that conversation runs it, and `Query::ConversationModels { agent,
page }` what each of an agent's conversations keeps with its model's window, which
harness-view draws for each conversation's model, meter and window wait.
`Query::LastTurn { thread }` answers the block of a conversation's newest turn, its
model's own output (a heard post, a tool result, a notice or a summary never moves it),
or null for one with no turn since harness began keeping it, so board tells an owner's
progress from what the owner was sent. A provider's
route is a record harness keeps (`SetRoute { provider, services }`: every registered
egress service that reaches the provider's models, in the order harness tries them, each
with the wire it speaks; the first account to set a provider's route owns it, an owned
row of the kind `harness.route` whose ownership policy decides each later `SetRoute` and
`RemoveRoute`; `Query::ProviderRoute { provider }` and `Query::ProviderRoutes` read
them), with the context window harness learned from the vendor for each model a provider
serves. A configuration that names no provider (`Configured::Unprovided`, its
configuration kept so its setup can name one) or a provider with no route asks no model.
Before each model call harness chooses an eligible service from the provider route and
sends an unpinned request; ccs chooses an eligible account within that service. No
service from front matter or a kept model, and no previous answering account, pins a
call. A service whose known accounts for the conversation's provider all have unexpired
rejected or full budget windows is skipped; unread accounts remain eligible and each
reported reset reopens its capacity. If every served service is rejected, harness's own
retry waits for the earliest account reset without sending a request; a new input or a
`Resume` asks once anyway, and an answer on any service of the route, for its provider,
pulls a waiting conversation in at the next block. A 429, a 5xx, a broken stream or an
unreachable service moves the call to the next untried eligible service; an exhausted
route waits for the reported reset, or uses the retry policy when no reset was reported.
A call a harness update or a node restart cut off stays where it was. A retry whose
booking the scheduler refused (a frame at the kernel's nesting cap) keeps its row, and
the next answer on its route books it, or `Op::Sweep`, a standing run every
`DEFAULT_WAIT` (64) blocks that anyone may start, does. The conversation keeps its model
and provider. `Query::Route { thread }` and `Query::Routes` record the last answerer, not
a placement preference. Account-bound sealed content and server tools go as plain text on
unpinned calls, while their lanes preserve who wrote them. `Query::Answerers { thread,
page }` records each turn and summary's service, account and move, and usage records the
cache-write cost. `Query::Agent` answers what harness
resolves for an account: its configuration path, the configuration and its provider's
route, or what is wrong, and its current thread. Harness registers with `attribution` at
its init, so a mention of the agent, a direct message to it, a chat notice that concerns
it (which starts a turn at once, as a mention does), or a reply in a chat thread it
started or replied in reaches harness and continues the agent's one current thread,
whoever wrote it (a user of a linked network too, named `name@network`) and wherever
(any channel, direct message or chat thread; the input names its author and its place,
and the model answers each where it came from), when identity says the agent hears its
author (`identity::Query::Hears`: its manager always, anyone else as its `Answers`
allows; a user of a linked network as its `Answers` allows `Party::Foreign` of them).
With no current thread, the first input opens one. A conversation opened bound to a
chat channel or thread (`OpenThread`) hears what reaches the agent there instead. An
input in a forge change's own room (`forge:<repo>:<n>`, `forge::change_of_room`) where
no conversation of the agent is bound goes to the agent's open conversation bound to
the room forge-review-chat mirrors that change into (`forge_review_chat::Query::Room`),
and to the current thread when there is none, the change is mirrored nowhere or
forge-review-chat does not answer. The agent itself, its manager, or
whoever its list at `harness::conversation_key(agent)` in the access program allows,
starts a fresh one with `NewThread { agent }`; its manager or that list starts one that
begins from the current thread's context with `ForkThread { agent }`, which copies the
current thread's frames (its summary included) and leaves out a last turn whose tool
calls are not all back; either becomes current. Its manager may claim who starts its
conversations (`ClaimConversations { agent }`): the ownership role then records the row
as theirs (kind `harness.agent-conversation`), and its policy decides each later
`ForkThread`, and each `NewThread` but the agent's own. A thread that `NewThread` or
`ForkThread` replaced closes once its loop is idle, and stays readable. `Query::Threads`
lists an agent's threads newest first, and `Query::ThreadAt` names the thread an input
from a place last reached. An agent with no current thread on record takes as current
its open thread whose loop is idle and that moved last (its newest frame at the highest
block), so on a network whose harness is swapped for this one the agent carries on in
the thread it was last in; its other threads stay open and readable as they were, and a
thread whose loop was running when the program was swapped lost that loop with the swap
and is left as it is. A configuration that is missing, malformed or names a provider
with no route is told to the thread once as a notice, and the next message tries again.
Each thread runs as one awaiting loop: harness puts the model's request for the thread
as blob parts in the model's wire (one per message, so earlier messages are reused;
`wire::anthropic` speaks the Anthropic Messages API, `/v1/messages`; each part is held
on a line: a thread's own under `<thread>/` (`header`, `summary`, `last`, `sealed/<n>`,
and a tool call's request body on `tool/<call>/` until its answer comes back or its
stream ends), let go together when the thread closes idle, and the close and summarize
parts on `const/close/` and `const/summarize/` lines named by their wire and target;
`HoldSealed` puts parts an open thread sealed before lines on that thread's line, and
lets a closed idle thread go of its sealed parts, one thread a block from harness's own
scheduled run) and calls the program bound to the egress role (`env.roles.egress`) with
an HTTP request to the model's service. A turn's request asks for a streamed reply
(`Delivery::Events`, delivered as harness's `Streamed { thread, event }`): egress hands
harness each event the elected node relays once and in `seq` order, in the block that
carries it or the one that fills the gap before it, and harness folds it, in the model's
wire, into the thread's partial reply (`Query::Partial`), which harness-view shows
growing. A repeated event is dropped, an event back at `seq` 0 starts the reply over,
and a gap breaks it. When the stream ends, the partial becomes the model's turn, the
same turn the whole reply would have given; a broken stream or an `error` event is told
to the thread as a notice. A tool call starts as soon as its block of the streamed reply
is whole (`content_block_stop`), while the rest of the reply still streams; its result
waits until the turn is recorded, and the thread's frames always read the turn, then its
results, then the input held behind the loop. A streamable tool (`chat_react`; every
tool's Anthropic definition sets `eager_input_streaming`) also shows its input while it
arrives: harness reads the longest valid prefix of the partial JSON (`tools::prefix`),
and the tool writes, as the agent, in the event's own block. `chat_post` writes nothing
of its own while it forms: its text joins the agent's draft in that room as it arrives
(below), and its run posts the message `harness-<thread>-<call>` once, whole (a run that
finds it already there edits it to the exact text, or, if the call's place changed,
deletes it and posts `harness-<thread>-<call>-moved`), so a call posts one message and
its notices carry the whole text. `chat_react` adds its reaction once its input is
whole, and its run does not add it again. A mention in a post is heard once the post
lands, with its final text. While a turn answers in a chat place (the place its
conversation is bound to, else that of the chat input that woke it), harness keeps the
agent's draft there (chat's `Drafting`, written as the agent): it opens at each model
call, takes the reply's visible text as it streams, and the text of each `chat_post`
into that room as its input arrives (never its thinking or another call's input), says
`thinking` until the reply's first words, names the tools its calls run (`running
forge_read`; a `chat_post` into the room aside, whose text it shows), `summarizing its
conversation` or `waiting to retry`, and is taken down once the turn is over; the
agent's post in the room takes it down, and the next model call opens it again. A
refused draft leaves the turn as it is. A reply that starts over (`seq` back at 0) takes
back what its calls showed or posted (the message is deleted, the reaction removed), and
says so in a notice; a call that had not run yet never runs, and a call of another tool
that already ran is named in a notice, since what it did stays. A reply that breaks or
fails takes back what calls still forming showed; calls already whole keep their
results, recorded after the part of the turn that was whole. A refused write,
provisional or final, is the call's failed result. A reply the service sends whole (a
refusal such as a 429) is read whole. Only the program bound to the egress role may send
`Streamed`, and only for a thread waiting on its model. While no member node serves any
service the call may go to, a thread tells itself so and does not ask the model. Each
tool call is relayed through egress's `relay` service as it starts (a call of a reply
read whole starts when the reply is recorded); when its relay comes back, in a later
block, harness runs the call: a read of `files`, `chat`, `forge`, the identity role or
`activity` as a query, a search or read of the thread's own transcript, a write to
`files` or `chat` as a call made as the agent, or any program's own op or query through
three tools that read the program's borsh schema (its `Op`, `Query` and `Reply`, and
what each op returns, which founding packs as the `ducktape.schema` section of its code,
found through the programs role): `network_schema` gives the program's shapes as JSON,
`network_query` writes a query from JSON, asks it and reads the reply back to JSON, and
`network_call` writes an op from JSON, sends it as the agent and reads what it returns
back to JSON where the schema names its type. Each write, a named tool's or
`network_call`'s, is a call made as the agent and sent by harness, which the program
allows where it allows the agent's account or harness (in `files`, any path no write
list in the access program covers, and a listed path whose list names the agent or
harness; not its configuration file, which harness-view proposes in its manager's home,
makes, and then gives the list `Owner`, its manager's account number alone).
`files_set_list { path, write }` sets a path's list in the access program
(`access::Op::Set`, which the access program takes only from the path's owner; `write`:
its owner alone, its owner and the accounts and programs listed, or anyone, and none
clears it). A call whose thread closed, or whose agent no longer runs here (its script
was taken back, or it is not active), does not run. Once the turn is recorded and every
result is back, harness asks the model again. Input that arrives while the loop runs is
held and follows the results; behind a turn the model paused (`pause_turn`, mid server
tool), it waits until the continuation is answered, as the continuation goes on from the
paused turn's last block. A paused turn whose continuation is given up keeps no server
tool's use that no result answers (a turn left with nothing else is a notice), so the
input behind it is sent with nothing dangling ahead of it. A thread follows the model's
context window: harness learns the window from the models endpoint, counts the tokens
each answer reports (a reply whose server tools ran it in several passes counts its last
pass, which `usage.iterations` reports under the compaction beta every turn sends, and
which the harness wire's reply carries as `context`), and when that count reaches the
agent's `compact_at`, or the next answer would not fit, or the model says the request
was too long, it has the model summarize the thread so far in its wire (Anthropic's
compaction) and sends that summary in place of the messages it covers; a summary frame
names the wire that wrote it, and another wire is handed only its text. A summary asks for streamed delivery: a provider
that streams compaction forwards upstream progress to keep the request alive, and
harness commits only its completed summary after the stream ends successfully. An
expired, broken or invalid summary leaves the conversation idle with a final error; it
is not treated as a context overflow. A summary refused with HTTP 429/5xx or a
retryable egress failure keeps the model-call retry path. Only an actual overflow
retries a smaller settled prefix, and exhausting those prefixes reports a final error.
A summary ends after an input, never between a tool call and its result, nor between a server tool's
use and a result the API sends in a later turn; if the API finds the summary request
itself too long, the next one covers less. When nothing the model already answered is
left to summarize and the request is still too long, harness leaves the largest new
input out of the model's view, one at a time, and puts a note in its place naming its
frame and size. The transcript keeps every frame whole, and the model reaches it with
`thread_search` (a literal string, over the whole thread or one frame: each matching
line, its frame, line number and offsets) and `thread_read` (a frame, whole or a byte
range), so neither a left-out input nor what a summary covers is lost to it. A failed
model call is told to the thread as a notice and not repeated until new input arrives.
`Query::Wakes { thread }` lists what will wake a conversation again (an unseen input, a
turn, open tool calls, a model call in flight, a retry, a settle window, an armed wait,
a streamed tool call, a booked run); none means nothing will. A streamed tool call whose
listening run a harness update cut off is still heard through its events; once egress no
longer holds its stream (checked on new code's first run and at the stream's deadline,
never by the deadline alone), harness ends it with a notice that its end went unheard
and how to read its result. A wait (`wait_for`, `Op::Wait`) keeps its own number and row
(`Query::Wait`, `Waits` and `Armed` read it, and `Wakes` lists it), and for every event
but a chat message harness arms it as a wake for the agent (wake's `WakeOn`, sent as the
agent, and `Unwait` sends `Cancel`): a height, a pod's end, a forge change (its source
moving, a review filed on it, its close, its merge or its leaving a merge queue; `until`
waits only for its close, merge or leaving a merge queue, never a push or a review) or
ref, a files path, or a chat place clearing of everyone else's hold and draft
(`WaitFor::Clear`). The wake's end, heard through attribution, ends the wait as `Wait N
is over: …` in the conversation that armed it, even when it ends inside its own
`WakeOn`; a wake the agent cancels itself drops the wait it backed. A chat message wait
stays harness's, a filter on what harness already hears from chat. harness watches
nothing else for a wait itself: its own watchers' ops (`WaitDue`, `PodWait`, `PodEvent`,
`Forged`, `Filed`) and the one-time `MoveWaits` that put their waits on wakes are
retired, each tag kept and refused, so a network takes this harness only after the one
before it ran `MoveWaits` until it answered null. A conversation that takes a closed one's
place is its heir: for a canonical one, the next canonical (`NewThread`, `ForkThread`, or the
fresh one an input opens after a `Close`); for a bound one, the next `OpenThread` at its place.
A wait, a wake or a booked run's report for a conversation that has closed reaches its heir,
whose `Armed` lists the waits it took over; with no heir it is a notice on the closed one,
never delivered to another. A promise whose conversation closed reaches its heir, else the
agent's current conversation. A mention, a direct message or a
thread reply that reaches a conversation stays the agent's until the agent posts or
reacts in its place (that thread, or anywhere in that room for a message outside a
thread; a reaction answers where the message it is on sits): when the conversation
closes before that, harness hands what it still holds to the agent's live conversation
for each place (a closed bound one's place goes to the canonical one, a fresh one if
none is open), as one input that opens `notice queue Note: N unanswered` and then quotes
each message as the agent first heard it; an agent not running then gets them with its
next input, and a fork carries what its source held. A chat room's rules (its `rules`
meta: the text, or the content of the file a files path names) reach a conversation as
one input ahead of the first message it hears from the room, and again once they or the
file change, or once the conversation is summarized; a message the agent's choice for
the room drops brings none. Beside its own tools, harness offers an agent each tool file
in `/shared/tools/` and in its `/home/<agent>/tools/` (`docs/tool-files.md`): TOML
naming its input and the queries and ops it sends as the agent in their programs' own
shapes, checked against the live schemas before each model call; a file that fails is
left out and the system prompt says why, and `Query::AgentTools { agent }` lists every
tool an agent gets with its source
