# egress

Outbound HTTP for programs. A service registered here is served by the
member nodes assigned to it, and a program's call is answered by one of
them, whole or as events. Shapes: `src/lib.rs`; checks: `src/rules.rs`.

## Who may call each op

| Op | Who may call it |
|---|---|
| `Register { service }` | any account; it owns the service from then on |
| `Assign { service, nodes }` | the service's owner, or whom the access list on `services/<name>` names |
| `Serve { services }` | a member node (the validators role's `Members`), signed by its own key; an empty list, which stops its serving, needs no membership |
| `Request { request, delivery, wait }` | programs only: a call from another program (`Cause::Call`), which awaits the answer. An account's own frame is refused, so an account off the chain reaches a service only through a program (agents through harness). It needs a scheduler role on the network |
| `Answer`, `Events`, `EndStream` | the node the request was elected to, signed by its key, in a later block than the request |
| `Expire { id }` | egress's own scheduled run for that request |

Every query is answered for anyone.
