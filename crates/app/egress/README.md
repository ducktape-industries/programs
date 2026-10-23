# egress

Outbound HTTP for programs. A service registered here is served by the
keys its owner assigns to it, member nodes or not, and a program's call is answered by one of
them, whole or as events. Shapes: `src/lib.rs`; checks: `src/rules.rs`.
Three services are egress's own and never registered: `http`, `relay` and
`crosspond`. Any member node that serves one may be elected for it, and
each is asked only as its own `Request` variant (`Http`, `Relay`,
`Crosspond`).

## Request failures

Expiry resolves a request with error code `egress::EXPIRED` (`egress_expired`).
`EndStream::Cut` resolves it with `egress::STREAM_CUT` (`egress_stream_cut`).
`Answer::Failed` keeps `wrong_state`; it reports the serving node's failure,
not an expiry or a cut. Callers classify these by code, never by message text.
HTTP errors such as 429 or 503 remain HTTP responses, not egress errors.

## Who may call each op

| Op | Who may call it |
|---|---|
| `Register { service }` | any account for a new name; it owns the service from then on. An unregistered name stays reserved to its last owner: that owner or the validators’ vote may register it again, retaining that owner |
| `Assign { service, nodes }` | the service's owner, or whom the access list on `services/<name>` names |
| `Serve { services }` | any key, signed by its own key. Serving `http`, `relay` or `crosspond` takes a member key (the validators role's `Members`); a registered service is served by the keys its `Assign` names, member or not, so a list of registered names alone needs no membership. A key with no node of its own serves through a validator's node (core's `ducktape egress serve`). An empty list stops its serving |
| `Request { request, delivery, wait }` | programs only: a call from another program (`Cause::Call`), which awaits the answer. An account's own frame is refused, so an account off the chain reaches a service only through a program (agents through harness). It needs a scheduler role on the network |
| `Answer`, `Events`, `EndStream` | the key the request was elected to, signed by it, in a later block than the request |
| `Expire { id }` | egress's own scheduled run for that request |
| `Migrate { height, first, last }` | anyone: rewrites each open request's `requester` by the programs role's `Renames(height)` (a height already reached), over the requests `first..=last` by id. Its renames come from the role, never from the sender, so a second run changes nothing |
| `Unregister { service }` | the same service-key write as `Assign`: its owner, or whom its policy admits. Refused while nodes are assigned: assign it no nodes first. Removes it from `Service`/`Services`, keeping its last owner under `retired/<name>`. Open requests do not block it and remain answerable by their elected node until they expire; new requests are refused |

Every query is answered for anyone. `Sender { id }` answers the account an open
request was sent as: the agent's when a program sends it as an agent (harness for
an agent's tool call), else the sending program's own, or `None` when the frame
acted as no account. Its row goes when the request closes. A service the request
reaches reads it to know who asked, unforgeably, by the `ducktape-request` number
the node puts on the call.

## Repository overview

a program's call to something outside the chain, bound to the egress role
(`abi::role::egress`, whose `Op` and leading queries it speaks). A `Request` names a
service: `Relay` carries a note and is answered with nothing, a later block's call back;
`Http` names a service, an `http::Head` (a method, a target, headers) and the body as
blob parts. For the reserved `http` service the target is a full `http://` or `https://`
URL on the public internet; any other service is registered (`Register`) by an account,
which owns the name and assigns the node keys that may serve it (`Assign`), and its
target is a path under the base the serving node's operator gives it. A member node's
key says which services it serves (`Serve`). A program calls egress with a request and
awaits it; egress checks the head (the framing headers and every `ducktape-` header are
the node's to set) and that its parts are on the network, then elects in turn among the
member nodes serving that service, and for a registered service only those its owner
assigned; that node alone answers (`Answer`), in a later block than the request, and its
answer, or its reported failure as a refusal, resolves the program's call. An `Http`
request's call answers an encoded `http::Response`. One that asks for its reply as
events (`Delivery::Events { deliver }`) hears each event of a `text/event-stream` reply
(`Event`) as its own op, `deliver` followed by the encoded event, and its call answers
the stream's end (`EndStream`): `Complete` as the `http::Response` with its status and
headers and an empty body, a cut as a refusal (`egress::STREAM_CUT`, `egress_stream_cut`);
`Answer::Failed` keeps `wrong_state`, and expiry uses `egress::EXPIRED`
(`egress_expired`). Each event moves its expiry `wait` blocks on, and any other reply comes whole. An event's `seq` counts from 0 per call the node
makes: a repeated `seq` is a repeat, and a `seq` back at 0 is the node calling the
service again (it restarted), which voids what came before; egress passes `seq` through
unchecked, for the requester to act on. A relay is answered whole, and asking one with
`Delivery::Events` is refused. A request waits `wait` blocks, `DEFAULT_WAIT` (64) when
the caller names none: egress schedules its own `Expire` for that height through the
scheduler role as it records the request (a network with no scheduler role refuses every
request), cancels it when the answer comes, and an `Expire` from that scheduled run
alone drops the request and answers the call with a refusal that says it expired; a
later answer is refused. The node itself does the answering: ducktape's `noded` reads
this program after each block, calls what it was elected for with the services its
`egress.toml` names, and signs `Answer` and `Serve` with its key
