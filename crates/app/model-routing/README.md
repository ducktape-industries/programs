# model-routing

Which gateway account each conversation calls a model provider on, kept on chain so that every conversation of a caller learns at once what one of them met. A model client (harness) records what each gateway reports: its pool, the accounts an egress service reaches for a provider in the gateway's order, and each account that refused for its quota (a 429) with the reset the provider gave. Each conversation keeps one pinned account per provider, so its prompt cache stays warm; the pin moves only when its account goes full or leaves its pool, never because the pool was reordered or another account recovered. Nothing is written unless it changes: a call that succeeds writes nothing here.

Every row is kept in its writer's namespace, the account the op acts as (`sender_account`): harness's ops write under harness's account, for all its conversations together, and another caller's never meet them. There is no owner, list or admin: anyone routes in their own namespace, and queries name the namespace they read.

The gateway's priority and wire stay with the caller: harness's provider route lists the services in the order it tries them, and `Pin` and `Resolve` take that list. model-routing keeps no credential and no service registration, and egress still says whether a gateway is served.

## Choosing

`Pin { conversation, provider, services }` walks the candidates: each listed service's pool in order, each pool's accounts in the gateway's order. A pin stands (`Pinned`) while its account is in a listed service's pool and not full. Otherwise the walk starts just after a full pinned account (at the first candidate when there is no pin, or its account or service is gone from the route) and wraps, and takes the first account with room (`Chosen`), which it pins. A service whose pool was never recorded is answered `Discover(service)` when the walk reaches it before an account with room: the caller reads that gateway's pool (its `/ccs/status`) and records it, and never spends a model call to learn it; an empty recorded pool offers no account. With no account with room the answer is `Wait(reset)`: the earliest reset on record among the full candidates, or null when none is known, so the caller asks no model until then or until a `Pool`, `Full` or `Available` changes what the walk finds.

An account is full while its episode is in force: `Reset(t)` until chain time in seconds (`env().time`, which is milliseconds, over 1000) reaches `t`, `Unknown` until `Available` ends it. A passed reset writes nothing: the walk reads it as room. Each episode keeps the chain time in seconds it began (`since`), so a gateway status read before a 429 that lands after it never ends the episode that 429 began.

## Tables

| prefix | key | row |
| --- | --- | --- |
| `p/` | consumer, provider, service | the pool: account labels in the gateway's order |
| `f/` | consumer, provider, service, account | the full episode: `Reset(seconds since the epoch)` or `Unknown`, and the chain time in seconds it began |
| `c/` | consumer, conversation, provider | the pinned service and account |

## Who may call each op

Every op: any account (`sender_account`), written in that account's namespace.

- `Pool { provider, service, accounts }`: the whole pool, empty when the gateway holds none. A label named twice or a blank one is refused, since a label names one account. Written only when it differs from the kept pool; then the episodes of accounts it leaves out whose reset has passed are dropped. An account that comes back keeps an episode still in force.
- `Full { provider, service, account, until }`: written, with the chain time as the episode's start, only when it makes the account full for longer: a first report (or one after the last episode passed) whose reset is still ahead, a reset where the kept one was `Unknown`, or a later reset than the kept one (a spent week reported after a session window). The same or an earlier reset, or `Unknown` while a reset is in force, writes nothing, so every call in flight that meets the same 429 writes once.
- `Available { provider, service, account, read_at }`: a gateway status read at `read_at` (seconds since the epoch) says the account has room; it ends the account's episode, whatever its reset, when the episode began before `read_at`, and writes nothing when none is kept or it began at `read_at` or later.
- `Pin { conversation, provider, services }`: answers the `Choice` (above) and writes only a `Chosen` pin. A service listed twice is refused. It reads the record in the op itself, so a choice made on a stale read is never installed.
- `Forget { conversation }`: drops every pin of that conversation.

## Queries

- `Resolve { consumer, conversation, provider, services }`: what `Pin` would answer now, without pinning.
- `Pools { consumer, page }`, `Fulls { consumer, page }`, `Pins { consumer, page }`: the consumer's rows, in key order. A passed episode is listed until a later report replaces it.

## Cost

A `Pin` or `Resolve` reads one pool row per listed service, the pin, and one episode row per candidate it passes. `Pool` scans the episodes of that one gateway when the pool changed; `Forget` scans that conversation's pins.
