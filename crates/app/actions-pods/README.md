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
| `Sweep` | anyone; actions-pods books its own a whole egress wait (`DEFAULT_WAIT`) out while a check waits on a pod the cluster did not make |
| `SetConcurrent { most }` | the validators' vote, or an account holding the name `actions-pods` in the programs role (`programs::Query::Owner`) |

`SetConcurrent { most }` sets how many pods this runner holds at once (unset: 1; 0 is refused). A check offered past it waits, as one the quota had no room for does, until one of this runner's pods ends.

Every query is answered for anyone.

## Where a check's log goes

A check's `/out/log` is copied into files at
`/home/<this program's id>/checks/<repo>/<commit>/<workflow>/<job>-<attempt>/log`, the
runner's own home on any network with files, and the report names that path. The pods service
writes it, as its own account, through `/v1/pods/<pod>/file`: a write list set on that home that
leaves the pods service out refuses the copy, and the check's summary then says the log was not
kept. Every other file in `/out` is put as a blob of kind `output` and recorded with actions
`Outputs`.

## What a job asks

A check's `Asks` (actions `Query::Asks`) become its pod's requests: `cpu` and `memory` as
Kubernetes quantities (`"16"`, `"500m"`, `"40Gi"`), `scratch` as memory-backed paths, and cargo
runs as many jobs as the CPUs asked. A `cpu` or `memory` that is no Kubernetes quantity errs the
check, naming the field and what it read, and makes no pod.
