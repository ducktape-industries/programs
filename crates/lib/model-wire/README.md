# model-wire

The harness wire: the one vocabulary harness speaks to a model, and an adapter per provider that
translates it to that provider's own wire and back. harness (on chain) writes requests and reads
events in it; the adapter service (forge repo `harness-model-adapter`, beside ccs) runs the
adapters. A provider's field names appear only in its adapter's module: `anthropic` (Messages,
through ccs's `/v1/`) and `codex` (OpenAI's Responses on the Codex surface a ChatGPT plan opens,
through ccs's `/backend-api/`: the system prompt rides as the developer's message, no
`max_output_tokens`, `store: false`).

## Endpoints the adapter service answers

| | request | answer |
|---|---|---|
| `POST /turn` | a `Request` | server-sent events, each `data:` one `Event`; the stream ends with exactly one `reply` or `failed`. A comment line (`: <name>`) carries no `Event`: the adapter sends one for each provider event that makes none, so egress hears a model that thinks in silence |
| `POST /summary` | a `Request` | a `Summarized` (200), else a `Failure` |
| `GET /models/{name}` | | a `Model` (200), else a `Failure` |
| `GET /ccs/status` | | ccs's own status, passed through |

An answer that is no reply carries a `Failure` as its body, under the provider's own status, with
the provider's own headers (rate-limit windows, ccs's `x-ccs-account`) passed back as they came.
There is no version anywhere: the vocabulary changes with harness, and this crate's tests hold both
sides to it.

## The vocabulary

- `Request`: model, max_tokens, thinking, system, tools (name, description, JSON schema), server
  tools, messages. A user message holds text, images and tool results; an assistant message holds
  `Block`s and the `Lane` (provider, model) that wrote it.
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

## The lane rule

An adapter takes back what a provider sealed only from a message its own provider wrote, and a
reasoning's seal only from its own model. Anything else goes back as its words (reasoning, cited
text, a summary) or not at all (redacted reasoning, server-tool use and results). That is how one
thread moves between models and providers.
