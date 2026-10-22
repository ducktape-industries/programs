# actions

Checks for forge repositories: each workflow file at a pushed or previewed
commit starts a run, whose jobs are queued as checks and offered to the
runners whose labels cover them; each check's state reaches forge as its
commit's status. Shapes: `src/lib.rs`; checks: `src/rules.rs`.

## Who may call each op

| Op | Who may call it |
|---|---|
| `Watched(event)` | forge only (its watch deliveries) |
| `Trigger { repo, commit }` | any account; it runs every workflow at the commit but those whose `on.trigger` is false |
| `Register { labels, deliver }` | any account, for its own runner row; a program that registers is offered checks |
| `Claim { check }` | any account, while the check is queued; only the accounts its job's `runners` names, when it names any |
| `Report { check, state, summary, log }` | the account that claimed the check, while it is running; any account while it is queued, to end it errored or superseded (as claiming it first would let it), or only those `runners` names |
| `Rerun { check }` | any account, once the check is neither queued nor waiting |
| `Preview { repo, n }` | anyone |
| `Outputs { check, files }` | the account that claimed the check, while it is running; who sent them is kept |

Every query is answered for anyone.

## What a job asks of its runner

A job's `cpu`, `memory` and `scratch` keys are kept as written and answered by `Query::Asks`
for every attempt of its check. actions reads none of them: what they mean is the runner's
(actions-pods reads `cpu` and `memory` as Kubernetes quantities).

A job's `timeout-minutes` is how many minutes its check may run, at least 1 (a workflow stating
0 is an errored run). It is kept and answered by `Query::TimeLimit` for every attempt of the check.
A job that leaves it out has no limit: actions sets no default and no cap of its own. Ending a
check that runs past it is the runner's: actions-pods reports it failed.

## What it tells chat

A check of a push or dispatch run that ends failed or errored, in a workflow whose file names
`notify.channel`, is one chat notice (`Notify`) in that channel, message id
`actions-check-<check>`: kind `check`, group `check <check>`, subject
`<workflow> on <repo> <branch>` (a dispatch: `<workflow> on <repo> dispatch <event type>`),
status Failed, detail `<failed|errored> at <commit>: check <check> (job <job>, attempt <n>).
<summary>`, link the check's log (a whole link as it is, a files path as `files/<path>` on the
reader's chain), and concerns the account that pushed or dispatched, so only they are told.
Such a run that errs before it queues any check (a dispatch payload that is not text) is the
same notice as `actions-run-<run>`, group `run <run>`, detail
`errored at <commit>: run <run>, before it queued a check. <why>`, with no link. A workflow file
that does not parse (jobs that need each other in a loop among them) names no channel, so its errored run tells nobody; its errored forge status
is what says so. A channel that refuses a notice leaves the report standing.

## When a change's checks run

A workflow whose `on.change` names `opened` or `updated` runs on each Preview of a change: actions
asks forge for one the block after the change opens and after each push to its source, and forge
makes one for the change at the front of a merge queue. A move of the target previews nothing, so
a change's checks say how it did on the target head its Preview was built on (forge
`Query::Preview`'s `into_head`, the Preview commit's parent). A merge queue checks the change
again on the target's tip before it lands, and the target's own `on.push` workflows check each
landing.

On a target with required checks (forge `RequireChecks`), a Preview built on an older tip can
never land: forge merges only a Preview of the target's current tip. So when such a target moves,
every check of a Preview of an open change into it, built on another tip, that has not started
yet ends superseded (`superseded: <branch> moved to <commit>, past the tip this Preview was built
on`), leaving the runners to checks a merge can use. A started check runs on. The queue previews
its front again on the new tip, and a change that moves is previewed again as before. A target
with no required checks keeps every Preview's checks, as nothing previews them again.
