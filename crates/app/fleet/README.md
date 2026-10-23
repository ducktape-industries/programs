# fleet

It runs as `ducktape-industries/fleet` (`fleet::PROGRAM`): a new program's id names its owner. harness's `fleet_run`, the `ducktape machine` CLI and the pods service each name it by that id.

The network's default way to run workloads (J-129). A program keeps the canonical part: what someone asked to run, which machine took each run, its state and its result. Machines run the work. A machine is an account with a key, running `ducktape machine`, either as a node's child or on its own. It says it is ready, claims what fleet assigns it, beats while it runs, and reports how each run ended. Logs and outputs are files the machine writes. fleet keeps only their folder and snapshot.

## Rows
| table | key | what |
|---|---|---|
| `w/` | run id | `Workload { id, by, height, spec }`, written once by `Ask` |
| `r/` | run id | `RunRow`: attempt, state, machine, heights, summary, `out` |
| `c/` | run id | the run's claim row, **owned by its asker** (kind `fleet.claim`), written by `Ask`, `Claim`, `Cancel` and `Requeue` |
| `f/` | run id | `Filed`: the run's kind (none for a run asked with none), class and group, and its limit |
| `qq/` | (kind, class, group, run id) | index: the runs no machine holds, each under its kind (`""` for none), class and group |
| `wk/` | kind | a walk of the kind's queue in progress: its class and group order and the key it stopped at |
| `wa/` | (kind, height) | a `Walk { kind }` booked for that height, so one height books one step |
| `lg/` | (kind, group, meter, window start) | a group's use of a kind through fleet, for fair share and quotas |
| `mk/` | (account, name) | the kind a machine is, when it named one |
| `i/` | (kind, idle since, account, name) | index: a kind's machines with a free slot, the one free longest first |
| `m/` | (account, name) | `MachineRow`: labels, slots, serves, busy, idle since, gone, next lapse check |
| `h/` | (account, name, run) | index: the runs a machine holds (assigned, running, stopping) |
| `b/` | (account, name) | the machine's last beat, one height |
| `d/` | (run, account, name) | the machines that declined a run |
| `last` | | the last run id |
| `t/` | message seq | an end told and not answered yet: its run and attempt |
| `told` | | how many ends were told in chat |
| `n/` | (run, attempt, call) | a call its machine sent through fleet: the number of its next piece |
| `p/` | (run, attempt, call, n) | `Piece`: an event of a streamed answer (with egress's `seq`), then the answer (`Answer`, the bytes egress resolved the call with) or why it failed (`Failed`) |

## Ops
- `Ask { spec }`: anyone. `AskIn` with no kind, no class and no limit.
- `AskIn { spec, kind, class, limit }`: anyone. Queues run 1 under its kind, class and group and returns its id. A kind must be one resources holds, the class one of its policy's (`""` when the policy names none), and the class's `may_use` must admit the sender, else `unauthorized`; the sender's group is the policy's first whose `members` admit it, else `""`, fixed here. A run with no kind takes no class. fleet places it from its own run the block after (`Walk`), because the guard records the claim row's owner and policy only once the Ask returns, and placement reads that policy. `limit` bounds each attempt's run time in blocks from its claim (`Expire`).
- `Ready { name, labels, slots, serves }`: `ReadyIn` with the kind the machine already is (none for a new machine), so a backend that knows only `Ready` keeps its kind. `ReadyIn` with no kind clears it.
- `ReadyIn { name, labels, slots, serves, kind }`: the sender's machine `name`. It sets labels, how many runs it takes at once (0 takes none), whose runs it serves, and the kind it is (none: it takes runs asked with no kind). A kind must be one resources holds. It books a step of that kind's walk for the next block.
- `Promote { run, class }`: whoever the claim row's policy admits (the asker always), into a class of the run's kind whose `may_use` admits the sender. A queued run is queued again under that class; a run assigned or running keeps the class for its next attempts. An ended run is refused `wrong_state`.
- `Claim { run, attempt }`: the account of the machine the run is assigned to, at that attempt. It writes the claim row, so the asker's `fleet.claim` policy decides whether the machine may. With no default of the asker's for `fleet.claim` (or for fleet, or for everything), the claim row links to the asker's "Only me": only the asker's own machines, and programs acting as the asker, take their runs until the asker sets one (`SetDefault { scope: Kind("fleet.claim"), policy }` naming a policy that lists the machines' accounts, or anyone).
- `Beat { name }`: the sender's machine, for everything it holds. A machine that went silent and beats again is ready again.
- `Report { run, attempt, state, summary, out }`: the claimant, at its attempt. The state is `Passed`, `Failed`, `Errored` or `Cancelled`. A lost run's machine may report it too. A `Service` whose end nobody asked for is queued as its next attempt instead.
- `Decline { run, attempt, why }`: the machine a run is assigned to. The run is queued again and never assigned to that machine again.
- `Cancel { run }`: whoever the claim row's policy admits (the asker always). A queued, assigned or lost run ends `Cancelled`. A running one becomes `Stopping` until its machine reports.
- `Requeue { run }`: the same parties. A lost or ended run is queued as its next attempt.
- `Lapse { machine }`, `Unclaimed { run, attempt }`, `Place { run }`: fleet books these for itself. Anyone may send them, and each acts only on what the chain shows:
  - A machine holding a running run that hasn't beaten for `DEFAULT_WAIT` blocks (egress's silence bound) loses its runs. `OnLoss::Retry` queues each one again. `OnLoss::Park` makes a job `Lost`. Any other run its machine was stopping (`Stopping`) ends `Cancelled` instead of being queued again.
  - An assignment unclaimed for `DEFAULT_WAIT` blocks marks its machine gone and queues the run again.
  - `Place { run }`: a queued run that no kind's queue holds is queued again, and its kind's walk takes a step.
  - `Walk { kind }`: one step of the kind's queue walk (Placement).
  - `Expire { run, attempt }`: an attempt still running `limit` blocks after its claim becomes `Stopping`, as `Cancel` makes it, and its machine stops it.
- `Call { run, attempt, call, request, stream, wait }`: the account of the machine running the run (`Running` or `Stopping`) at that attempt; any other account is refused `unauthorized`, and another attempt or state `wrong_state`. `call` is the machine's own number for the call, once per attempt. fleet sends egress `Request` as itself, `Delivery::Events` when `stream`, else `Whole`, so a backend reaches a service only a program may call (the pods service); egress's `Sender` for it is fleet's own account, since fleet runs no machine's account. Only an `Http` request is relayed (a relay is refused `invalid_input`), and fleet sets its headers `fleet-run`, `fleet-attempt`, `fleet-asker` (the workload's `by`) and `fleet-machine` (the machine's account) on it (`STAMPED`), replacing any the machine sent under those names in any case: a service authorizes a relayed call, and names its requester, from those headers alone. The frame waits on egress: each event lands as a `p/` row as it comes, and the end lands as the last one. This is the machine's `pods` backend's way to pods.
- `Heard { run, attempt, call, event }`: only the program bound to the egress role, with the events of a streamed `Call`.

## Placement
fleet places by the kind's policy in resources (`resources::ordering`, the code resources places by), read through resources' `InForce { kind }` at each step, so a new policy rules the first step after it takes effect. A run with no kind is under no policy: one class, one group, oldest first.

A kind's queue is placed by a walk, in steps. Each step is its own `Walk { kind }` frame, booked for the next block once per (kind, height) whenever a run is queued (`AskIn`, `Requeue`, a `Decline`, a lost or unclaimed run) or capacity frees or appears (`ReadyIn`, `Report`, `Cancel` of an assigned run, a machine back from silence). A step examines at most the kind's `walk` queued runs (1 for runs with no kind), in the policy's order: class in its order, then groups by `used / weight` in the share window from fleet's ledger, then oldest first. A group over a quota is passed by, and a step is booked for its window's end. A step that used up its count with a free machine left books the next for the next block; one out of free machines or at the queue's end ends the walk. Nothing reads every machine or every queued run in one frame.

fleet's ledger counts, per (kind, group): `""`, one per assignment, and `blocks`, the blocks from claim to the end of each attempt, counted by the chain when its machine reports or is lost. A kind that shares or bounds by blocks declares the meter `blocks`, reported by the chain.

A walked run goes to a machine of its kind that takes it, in the lowest tier the kind's owner ranked its account in (resources' `Rank`, 0 until ranked), and within a tier to the one free longest (`idle_since`, then the machine id). So a run never goes to a higher tier while a lower one has a free machine that takes it. A machine takes it when it:
- has a free slot and isn't gone;
- for a run of a kind, is a machine whose account that kind's owner admitted (resources' `Admission`, which also answers the tier), read at the step, so an `Evict` or a `Rank` takes effect at the next one;
- covers the run's affinity: none means anywhere, `Machine(id)` means that machine, `Labels` means a machine holding every one of them;
- serves the run's asker;
- hasn't declined the run;
- is admitted by the run's claim policy, read through the ownership role (`Record`, `Effective`) before the assignment. As ownership resolves a party, a listed program admits the account it acts as (identity `OfProgram`).

A freed slot books a step of its kind's walk for the next block.

## Who is told
`Spec.notify`: `Chat { channel }` gets one chat notice (`Notify`) per end, each with a message id of its own (`fleet-run-<run>-<attempt>-<n>`), so a lost job's late end is told too: kind `run`, group and subject `run <run>`, status Passed, Failed (failed, errored, lost) or Note (cancelled), detail `<state> (attempt <n>)[: <summary>]`, link the run's out folder as `files/<path>` on the reader's chain, and concerns the asker (`Workload.by`) alone, so the room's listeners aren't told. `Program { program, deliver }` hears `deliver ++ borsh(RunEnd { run, attempt, state })` when a run ends or is lost. harness's `fleet_run` tool asks as the agent with `Program { program: harness, deliver: harness::run_ended_as(thread) }`, so the end lands as harness `RunEnded` and wakes the conversation that asked. A refusal leaves the run as it ended, and is added to its summary (`not told: …`) while the run is still at that attempt.

## Queries
`Workload { id }`, `Run { id }`, `Queued { page }` (by kind, class, group, then oldest), `Machine { machine }`, `Machines { page }`, `Holds { machine, page }`, `Pieces { run, attempt, call, page }` (a call's pieces in order), `Filed { run }`, `KindOf { machine }`, `Ledger { kind, group, page }`.

## Interface
`Op`, `Query` and `Reply` only grow at their end. `Spec` rides in `Op::Ask`, so a new field means a new `Ask` variant, not a changed `Spec` (as `AskIn` added kind, class and limit, and `ReadyIn` the kind).
