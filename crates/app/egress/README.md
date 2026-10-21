# egress

Outbound HTTP for programs. A service registered here is served by the
member nodes assigned to it, and a program's call is answered by one of
them, whole or as events. Shapes: `src/lib.rs`; checks: `src/rules.rs`.

## Who may call each op

| Op | Who may call it |
|---|---|
| `Register { service }` | any account for a new name; it owns the service from then on. An unregistered name stays reserved to its last owner: that owner or the validators’ vote may register it again, retaining that owner |
| `Assign { service, nodes }` | the service's owner, or whom the access list on `services/<name>` names |
| `Serve { services }` | a member key (the validators role's `Members`), with or without a node, signed by its own key; a key with no node serves over halo's `ducktape/egress` lane through a validator's node (core's `ducktape egress serve`). An empty list, which stops its serving, needs no membership |
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
