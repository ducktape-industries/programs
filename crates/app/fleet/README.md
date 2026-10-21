# fleet

The network's default way to run workloads (J-129). A program keeps the canonical part: what someone asked to run, which machine took each run, its state and its result. Machines run the work. A machine is an account with a key, running `ducktape machine`, either as a node's child or on its own. It says it is ready, claims what fleet assigns it, beats while it runs, and reports how each run ended. Logs and outputs are files the machine writes. fleet keeps only their folder and snapshot.

## Rows
| table | key | what |
|---|---|---|
| `w/` | run id | `Workload { id, by, height, spec }`, written once by `Ask` |
| `r/` | run id | `RunRow`: attempt, state, machine, heights, summary, `out` |
| `c/` | run id | the run's claim row, **owned by its asker** (kind `fleet.claim`), written by `Ask`, `Claim`, `Cancel` and `Requeue` |
| `q/` | run id | index: the runs no machine holds |
| `m/` | (account, name) | `MachineRow`: labels, slots, serves, busy, idle since, gone, next lapse check |
| `h/` | (account, name, run) | index: the runs a machine holds (assigned, running, stopping) |
| `b/` | (account, name) | the machine's last beat, one height |
| `d/` | (run, account, name) | the machines that declined a run |
| `last` | | the last run id |
| `t/` | message seq | an end told and not answered yet: its run and attempt |
| `told` | | how many ends were told in chat |

## Ops
- `Ask { spec }`: anyone. Queues run 1 and returns its id. fleet places it in its own run the block after (`Place`), because the guard records the claim row's owner and policy only once the Ask returns, and placement reads that policy.
- `Ready { name, labels, slots, serves }`: the sender's machine `name`. It sets labels, how many runs it takes at once (0 takes none) and whose runs it serves. Queued runs it covers are assigned to it in the same frame.
- `Claim { run, attempt }`: the account of the machine the run is assigned to, at that attempt. It writes the claim row, so the asker's `fleet.claim` policy decides whether the machine may. With no default of the asker's for `fleet.claim` (or for fleet, or for everything), the claim row links to the asker's "Only me": only the asker's own machines, and programs acting as the asker, take their runs until the asker sets one (`SetDefault { scope: Kind("fleet.claim"), policy }` naming a policy that lists the machines' accounts, or anyone).
- `Beat { name }`: the sender's machine, for everything it holds. A machine that went silent and beats again is ready again.
- `Report { run, attempt, state, summary, out }`: the claimant, at its attempt. The state is `Passed`, `Failed`, `Errored` or `Cancelled`. A lost run's machine may report it too. A `Service` whose end nobody asked for is queued as its next attempt instead.
- `Decline { run, attempt, why }`: the machine a run is assigned to. The run is queued again and never assigned to that machine again.
- `Cancel { run }`: whoever the claim row's policy admits (the asker always). A queued, assigned or lost run ends `Cancelled`. A running one becomes `Stopping` until its machine reports.
- `Requeue { run }`: the same parties. A lost or ended run is queued as its next attempt.
- `Lapse { machine }`, `Unclaimed { run, attempt }`, `Place { run }`: fleet books these for itself. Anyone may send them, and each acts only on what the chain shows:
  - A machine holding a running run that hasn't beaten for `DEFAULT_WAIT` blocks (egress's silence bound) loses its runs. `OnLoss::Retry` queues each one again. `OnLoss::Park` makes a job `Lost`. Any other run its machine was stopping (`Stopping`) ends `Cancelled` instead of being queued again.
  - An assignment unclaimed for `DEFAULT_WAIT` blocks marks its machine gone and queues the run again.
  - A queued run is placed.

## Placement
A queued run goes to the ready machine that has been free longest (`idle_since`, then the machine id) and that:
- has a free slot and isn't gone;
- covers the run's affinity: none means anywhere, `Machine(id)` means that machine, `Labels` means a machine holding every one of them;
- serves the run's asker;
- hasn't declined the run;
- is admitted by the run's claim policy, read through the ownership role (`Record`, `Effective`) before the assignment. As ownership resolves a party, a listed program admits the account it acts as (identity `OfProgram`).

A freed slot takes the oldest queued run it covers in the same frame.

## Who is told
`Spec.notify`: `Chat { channel }` gets one chat notice (`Notify`) per end, each with a message id of its own (`fleet-run-<run>-<attempt>-<n>`), so a lost job's late end is told too: kind `run`, group and subject `run <run>`, status Passed, Failed (failed, errored, lost) or Note (cancelled), detail `<state> (attempt <n>)[: <summary>]`, link the run's out folder as `files/<path>` on the reader's chain, and concerns the asker (`Workload.by`) alone, so the room's listeners aren't told. `Program { program, deliver }` hears `deliver ++ borsh(Ended { run, attempt, state })` when a run ends or is lost. A refusal leaves the run as it ended, and is added to its summary (`not told: …`) while the run is still at that attempt.

## Queries
`Workload { id }`, `Run { id }`, `Queued { page }`, `Machine { machine }`, `Machines { page }`, `Holds { machine, page }`.

## Interface
`Op`, `Query` and `Reply` only grow at their end. `Spec` rides in `Op::Ask`, so a new field means a new `Ask` variant, not a changed `Spec`.
