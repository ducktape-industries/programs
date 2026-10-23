# model-wire

The harness wire: the one vocabulary harness speaks to a model, and an adapter per provider that
translates it to that provider's own wire and back. harness (on chain) writes requests and reads
events in it; the adapter service (`harness-model-adapter/` in forge's `services`) runs the
adapters. A provider's field names appear only in its adapter's module: `anthropic` (Messages,
`/v1/`) and `codex` (OpenAI's Responses on the Codex surface a ChatGPT plan opens,
`/backend-api/`: the system prompt rides as the developer's message, no `max_output_tokens`,
`store: false`).

## Endpoints the adapter service answers

| | request | answer |
|---|---|---|
| `POST /turn` | a `Request` | server-sent events, each `data:` one `Event`; the stream ends with exactly one `reply` or `failed`. A comment line (`: <name>`) carries no `Event`: the adapter sends one for each provider event that makes none, so egress hears a model that thinks in silence |
| `POST /summary` | a `Request` | a `Summarized` (200), else a `Failure` |
| `GET /models/{name}` | | a `Model` (200), else a `Failure` |

An answer that is no reply carries a `Failure` as its body, under the provider's own status, with
the provider's own headers (its rate-limit windows) passed back as they came.
Every answer also carries the budget in harness's own words (`budget`): for each usage window
`x-harness-window-<name>-used` (the share used, a fraction), `-reset` (seconds since the epoch) and
`-status`, then `x-harness-status` and `x-harness-account`: the account that answered, by the name
a `pinned` target gives it where the service names its accounts, else the provider's own id for it,
and `x-harness-identity` where the gate names the login that answers under that account (a provider
account id or a short label the gate's owner picks, never an email), so a swap behind a name shows.
A gate that knows which accounts it serves names them all on every answer in `x-harness-pool`:
the names `x-harness-account` gives them, one per login, comma-separated, empty when it serves none.
harness then keeps budget rows for those accounts only; an answer without it changes no rows.
A window is named by its length (`5h`, `7d`), so every provider's windows read alike.
There is no version anywhere: the vocabulary changes with harness, and this crate's tests hold both
sides to it.

## The vocabulary

- `Request`: model, max_tokens, thinking, system, tools (name, description, JSON schema), server
  tools, messages, the `target` the call goes to (`pinned` to a service and account, `unpinned`, or
  `unnamed` from a harness that names none; harness always names `unpinned`, leaving account
  eligibility to the service), and a `cache_key` the conversation's calls share
  (harness sends its conversation's number; the Codex surface sends it as the body's
  `prompt_cache_key` and as the `session-id`, `thread-id` and `x-client-request-id` headers, as
  the Codex CLI does). A user message holds text, images and tool results;
  an assistant message holds `Block`s and the
  `Lane` that wrote it: provider, model, and the egress service and account that answered.
- `Model`: the provider, the context window, and the picture limits where the provider states them.
- `Block`: text (with the provider's citations), reasoning (with its seal), redacted reasoning, a
  tool use, a server-tool use and result, and a summary. `sealed` and `citations` hold what a
  provider issued and takes back verbatim; nothing on harness's side reads them.
- `Event`: a block opens at its place in the reply (`block`), grows by text (`delta`: its text, its
  reasoning or its input's JSON) and ends (`end`); then `reply` (the authority on the turn) or
  `failed`. A signature or other seal never streams: it arrives in the reply.
- `Usage`: input (the prompt's uncached share), output (reasoning included), cache read and write.
  `Reply.prompt` is the prompt's own size where the provider reports it before answering, and
  `Reply.context` the tokens a reply's last pass left in context where it ran in several (server
  tools), while `usage` stays the sum, what the reply cost.
- `Failure`: a `Class` (dropped, provider, overloaded, wire, overflow, pictures, limited), the
  words, and for a limited or busy account the reset it waits for (seconds since the epoch).

## The lane rule (`fit`)

Each adapter fits a request before it spells it, by who wrote each assistant block and who reads
the call:

- User text, images, tool results, assistant text and tool calls always go. A tool call's id is
  rewritten (the same way in its result) only where the target refuses its pattern.
- A reasoning seal goes back only to what reads it: Anthropic's signatures and redacted thinking
  to the same model, Codex's encrypted reasoning to the same model and account. Otherwise the
  reasoning is left out, never sent as words: the new model would read them as its own reply.
- A server tool's blocks and cited text go back as they were only to the same provider and
  account; otherwise as their words, with each cited page's title and address.
- A sealed summary goes back to its own provider; any other reads its words.

A request with an `unnamed` target comes from a harness that judged accounts itself, so its server
tool's blocks and cited text go as harness sent them. Codex's encrypted reasoning, which that
harness never judged, goes only to a pinned, matching account, and is left out otherwise.
