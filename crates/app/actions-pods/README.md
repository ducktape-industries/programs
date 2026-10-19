# actions-pods

An actions runner that runs each check it is offered in a pod of the pods
service, which it reaches through egress, and reports how the pod ended.
Shapes: `src/lib.rs`; checks: `src/rules.rs`.

## Who may call each op

| Op | Who may call it |
|---|---|
| `Offer(offer)` | actions only |
| `Run { check }`, `Resume { check }` | actions-pods' own scheduled run |
| `Heard(event)` | the egress role's program only |
| `Sweep` | anyone |
| `SetConcurrent { most }` | anyone |

Every query is answered for anyone.
