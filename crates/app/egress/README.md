# egress

Outbound HTTP for programs. A service registered here is served by the
keys its owner assigns to it, member nodes or not, and a program's call is answered by one of
them, whole or as events. Shapes: `src/lib.rs`; checks: `src/rules.rs`.
Three services are egress's own and never registered: `http`, `relay` and
`crosspond`. Any member node that serves one may be elected for it, and
each is asked only as its own `Request` variant (`Http`, `Relay`,
`Crosspond`).

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
