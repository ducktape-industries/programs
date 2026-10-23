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

A check of a Preview built on an older tip of its target can decide nothing: a merge lands the
change on the target's current tip, and a merge queue (forge `RequireChecks`) merges only a Preview
of that tip. So when a target moves, in any repository and whether or not
it requires checks, every check of the current Preview of an open change into it, built on another
tip (forge `Query::Preview`'s `into_head`), that has not started yet ends superseded (`superseded:
<branch> moved to <commit>, past the tip this Preview was built on`), leaving the runners to checks
a merge can use. A started check runs on. A Preview built on the new tip keeps its checks, and a
run on a Preview forge has since replaced is left to that newer Preview, whose `Previewed` ends it. This is
decided on forge's `Moved` of the target, never by polling: actions reads its own runs first and
asks forge `Change` and `Preview` only for a run with a check not yet started.

Each change whose checks it ends is told once, in the change's room (forge's `Change.channel`), by
one chat notice (`Notify`), message id `actions-superseded-<first check>`: kind `check`, group
`check <first check>`, subject `Preview checks of <repo>#<n>`, status Note, detail `<checks N and
M> superseded: <branch> moved to <commit>, past the tip this Preview was built on. The change's
next push, rebase or turn at the front of a merge queue previews it again.`, no link, and concerns
the change's author. The queue previews its front again on the new tip, and a change that moves is
previewed again as before.

## Repository overview

CI on forge. `actions` watches forge: a push, or a change's Preview (forge `Preview`,
asked for from actions' own run the block after a change opens or its source or target
moves, its open `Preview` op; a change it holds no rows for joins on its first Preview;
one forge refuses, a conflict, makes an errored run that says why), reads
`.forge/workflows/*.toml` at that commit (`on.push` branches, `on.change`
opened/updated, `on.dispatch` (the `event_type`s of forge `Dispatch` that run it, at the
commit the dispatch names, each job given the payload as `DISPATCH_PAYLOAD`),
`on.trigger` (false: `Trigger` passes the file over), `on.supersede` ("unstarted",
below), `[notify] channel`, `[[jobs]]` with `id`, `runs-on`, `image` (for a runner that
makes a container; none for one that is its own environment), `steps`, `env`, `inputs`,
`needs`: the jobs of the file that must pass first, where a need that names no job, the
job itself or a loop makes the run errored, and `runners`: the accounts whose claim and
report count for the job, offered to no other runner and claimed, reported and given
outputs by no other account; a job that names none is any runner's, and an empty list
makes the run errored) and queues one check per job (waiting while a job it needs has
not passed, queued, running, passed, failed, errored, superseded, or skipped when a job
it needs ended otherwise, and so on down what waits on it; each is its commit's forge
`Status` in `<workflow>#<job>`, waiting as pending and skipped as errored), offered to
the program runners whose labels cover `runs-on` (a program runner that refuses its
offer ends the check errored, saying why; an account registers as a runner too: it hears
no offer, finds its checks in `Queued`, and claims and reports them with its own key, as
eddy-claude does for `claude`); a runner claims it, and the account that claimed it
reports it and records its outputs while it runs (one whose claim was refused reports it
errored), anyone reruns it (a rerun waits again on the newest attempt of each job it
needs, and an attempt that passes brings back, as its next attempt, each job of the run
its failure had skipped); a runner records what a running check produced with `Outputs`
(files by blob id, read back with `Query::Outputs`), and `Query::Needs` names, for each
job a check needs, the passed check that met it, so a runner places a check's inputs
from the network alone; and a red check of a push or a dispatch, or such a run that errs
before it queues a check, is told in the workflow's channel (a workflow file that does
not parse names none). `Runs { repo, event, page }` lists a repository's runs of one
event, oldest first: a branch's pushes (`Push`), a change's Previews (`Preview`) or the
runs triggered by hand (`Manual`); runs recorded before actions kept that index are not
listed. A check still queued or running ends superseded, posting nothing, when the next
push to its branch, the next Preview of its change or the next dispatch of its event
type starts, or its change merges or closes; forge hears it as errored. A workflow with
`on.supersede = "unstarted"` gives way on its line only while no runner has claimed a
check of its run: a run a runner took goes on to its end, its waiting and queued jobs
included. `Trigger { repo, commit }` runs every workflow at a commit but those with
`on.trigger = false`. `actions-pods` is a runner: it books each offer and claims it in
its own run the block after, so how deep the offer arrived never matters; it runs a
claimed check as a pod of the pods service through egress, hears it end on the pod's
`wait` stream, keeps its `/out` in two requests (its `log` copied into files at
`/home/<its program id>/checks/<repo>/<commit>/<workflow>/<job>-<attempt>/log`, the
runner's own home, which the report links and which is all that folder holds; every
other file put as a blob of kind `output` with one `/blob {paths}` and recorded as the
check's outputs, found with actions `Query::Outputs`), places what the jobs a check
needs produced at `/needs/<job>/<path>` as blob inputs of its one pod (every runner
exports `OUT` and `NEEDS` naming its outputs and needs folders, here `/out` and
`/needs`, so a workflow's steps write `"$OUT/…"` and read `"$NEEDS/<job>/…"` whatever
runner claims them) (an output path that would leave that folder errs the check),
reports and deletes the pod; before it counts the pods it holds, it deletes those of
checks that ended (a superseded one among them); a pod the pods service answers it did
not make (a 5xx, such as the cluster's apiserver timing out) leaves the check queued,
never errored, tried again when one of its pods ends or at a `Sweep`, and while such a
check waits it books itself a `Sweep` a whole egress wait (`DEFAULT_WAIT`) out; a 4xx
still errs the check at once; `Waiting` lists only the checks still queued, so one that
ended elsewhere is not counted before its retry drops it; waiting checks take free room
oldest first, except that a check on the Preview a merge queue's front waits on (forge
`Required`, that branch's `Queue`, then the change's `Preview`) goes before every other
