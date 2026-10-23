# resources

The network's resource controller: what each provider offers, who asked for what, which provider each request was placed on and why, and what each placement used. A **kind** names a resource (`model.anthropic`, `service.jev-rank`, `compute.linux-x86_64`, `build`). Its owner sets its **policy** and its **admitted** providers. A provider **offers** slots of a kind and **beats** while they're there. Anyone **requests** a use. The program alone writes the **assignment** between them, with its reason. Use is counted by whoever can count it (the provider, the requester, the chain) and kept per assignment. Placement follows the kind's policy and nothing else: no agent picks a provider.

**Status:** the rules are in `src/rules.rs`, and `src/contract.rs` runs each liveness claim below on a MockChain. fleet places its runs of a kind by that kind's policy (fleet README, Placement): it reads `InForce` and `Admission`, and runs `src/ordering.rs`, the classes, groups, fair share, quotas and queue-walk order both placers share, over its own ledger and queue.

## Rows
| table | key | what |
|---|---|---|
| `kind/` | name | `KindRow`: owner, `KindSpec` (meters, slot, lapse, notary kind, `walk`: requests one queue-walk step examines), height. **Owned by its registrant** (kind `resources.kind`) |
| `policy/` | kind | `Policy`, written by `SetPolicy` |
| `admitted/` | (kind, provider) | `AdmittedRow`: who admitted the provider, and when |
| `rank/` | (kind, provider) | the tier the kind's owner ranked the provider in, when not 0 |
| `offer/` | (provider, name) | `OfferRow`: the offer as declared (labels, capacity in slots, `Reach`: an egress service and node, a fleet machine, or the provider directly; `Terms`; `Isolation`), its state, slots held, free since |
| `kind-offer/` | (kind, provider, name) | index: a kind's offers, admitted or not |
| `idle/` | (kind, free since, provider, name) | index: a kind's `Ready` offers with a free slot, the one free longest first |
| `notary-of/` | (notary kind, kind) | index: the kinds whose `KindSpec.notary` names a kind, so a notary coming back books a step of their queue walks |
| `walk-at/` | (kind, height) | a `Place { kind }` booked for that height, kept so one height books one step |
| `walk/` | kind | a walk in progress: its class and group order and the queue key it stopped at |
| `beat/` | (provider, name) | `BeatRow`: the newest beat only |
| `request/` | id | `RequestRow`, written once. **Owned by the account it is for** (kind `resources.request`) |
| `state/` | id | the request's attempt and `RequestState` |
| `queue/` | (kind, class, group, id) | index: the queued requests, each under the class and group it was filed in |
| `assigned/` | (request, attempt) | `Assignment`: offer, notary, height, `Reason`, state |
| `holds/` | (provider, name, request) | index: what an offer holds now |
| `declined/` | (request, provider, name) | the offers that declined a request |
| `used/` | (request, attempt) | `Usage`: each side's count, the chain's count, evidence, `Standing` |
| `ledger/` | (kind, group, meter, window start) | a group's running use, for fair share and quotas |
| `record/` | (provider, name) | `Record`: an offer's running counts |
| `check/` | (provider, name) | the height of an offer's next booked `Lapse` |
| `disputed/` | (request, attempt) | an assignment already counted in its offer's `Record.disputed` |
| `charged/` | (request, attempt, meter) | the highest count of a meter charged to the ledger for that assignment |
| `last` | | the newest request id |

## Ops
- `RegisterKind { name, spec }`: anyone, for a name nobody holds; the sender owns the kind from then. On a held name, the kind row's ownership policy decides (its owner always), and the spec is replaced. A meter whose `checked_by` is its own `reported_by` is refused `invalid_input`: a count can't check itself. Notarizing someone's kind is an obligation, so it needs consent, and consent bounds the list: a spec that names a notary kind it didn't name before is sent by whom that kind's row ownership policy admits (its owner always), else refused `unauthorized`.
- `SetPolicy { kind, policy }`, `Admit { kind, provider }`, `Evict { kind, provider }`: whom the kind row's ownership policy admits (its owner always). This is the ownership record of `kind/<name>`, not the `Policy` struct, which lists no one who may administer the kind. A new policy rules placements from the next block. `Admit` takes effect in its frame. `Evict` stops placement on the provider's offers of that kind in its frame and books a `Lapse` of each at the next block, which loses what they hold.
- `Rank { kind, provider, tier }`: whom the kind row's ownership policy admits (its owner always). Ranks the provider's capacity of the kind in a tier, 0 (every provider's until ranked) first. fleet places on a free machine in the lowest tier that takes a run before the one free longest; resources' own placement (retired at J-568 step 8) doesn't read it.
- `Offer { offer }`: anyone, as a provider of its own offers (the sender is the provider). Declares or replaces the sender's offer `offer.name`. An offer is visible at once and takes work only while its kind admits its provider. It books a step of the kind's queue walk (Queue walks, below).
- `Withdraw { name }`: the sender's offer. It becomes `Draining`, and `Gone` once it holds nothing.
- `Beat { name, free, readings }`: the sender's offer. A `Missed` or `Lapsed` offer is `Ready` again.
- `Request { spec }`: anyone. The request is the sending account's (`RequestRow.by`). A program asking for an account it runs sends it as that account through the kernel (`emit_as`, checked by the kernel against who runs the account), and the program is kept as `via` (the frame's origin). There is no field naming another account. A class whose `may_use` doesn't admit the account is refused. It is placed in this frame on an offer that takes it now, else queued. When the offer it would take also takes one of the first `walk` requests of its kind's queue (in key order, over quota and waiting on a notary passed by), it is queued instead and a step is booked for the next block, so it never takes a slot a queued request it sees would be given. A queue of requests that offer doesn't take holds no one back. One held back only by a quota books a step at that quota window's end. Returns the id.
- `Pick { spec }`: the same parties as `Request`. Places the request in this frame, for a client that reaches the provider itself (egress, for a model call or a service). The assignment starts `Claimed`. Refused `wrong_state`, naming what held it back, when no admitted offer takes it now or a queued request takes that offer first, as `Request` reads it. Returns the `Placement`.
- `Cancel { request }`: whom the request row's policy admits (the account it is for always). A queued request ends `Cancelled`. A placed one becomes `Stopping`, which its provider sees in `Holds`, until it reports the end.
- `Claim { request, attempt }`, `Decline { request, attempt, why }`: the provider of the assigned offer, at that attempt. A decline places the request again, never on that offer.
- `Report { request, attempt, outcome, readings, evidence }`: the provider of the offer, at that attempt. Its readings are the provider's count of the meters the kind says the provider reports or checks, and each report replaces the provider's counts before. A report may come mid-run with no outcome. The report that carries an outcome ends the assignment, frees its slot, and is the last one.
- `Count { request, attempt, outcome, readings }`: the program the request came through (`RequestRow.via`), at that attempt. Its readings are the chain's count of the meters the kind says the chain reports or checks, under the same rule: each replaces the last, and the one with an outcome ends the assignment and is the last.
- `Missed { request, attempt }`: the program the request came through, at that attempt. It says the provider never answered. The assignment and the offer become `Missed`, and the offer is passed over until it beats. The request goes as its `on_loss` says: `Retry` places it again as its next attempt, and `Park` leaves it `Lost`, since the provider may have done the work.
- `Confirm { request, attempt, readings }`: the account the request is for, and only it. Its readings are that account's counts of the meters the kind says the requester reports or checks, and each confirm replaces the last. The use's standing is worked out again from both sides (below).
- `Lapse { offer }`, `Unclaimed { request, attempt }`: resources books each for itself. Anyone may send them, and each acts only on what the chain shows.
  - An offer silent past its kind's lapse (`KindSpec.lapse`, or egress's `DEFAULT_WAIT` when none) becomes `Lapsed`, then loses what it holds, so nothing it held is placed back on it. `OnLoss::Retry` places each request again as its next attempt, and `OnLoss::Park` leaves it `Lost`.
  - An assignment unclaimed past the lapse makes its offer `Missed` and places the request again. It never ran, so `on_loss` doesn't apply.
- `Place { kind }`: anyone; resources also books it itself (Queue walks, below). Takes the next step of the kind's queue walk and acts only on what the chain shows, so a request held back by nothing the chain still shows is placed whoever sends it.

## Placement
For a request R of kind K, and offers O of K:
1. O takes R only when all of these hold:
   - O's provider is on K's admitted list;
   - O is `Ready` with `units` free;
   - O holds every label `needs.labels` selects (`Selector::Is`: that key with that value; `Has`: the key with any value);
   - O's isolation covers `needs.isolation`: `Isolation::None` is the floor, otherwise the same backend, and for Landlock an ABI at least the one asked;
   - O's `terms.serves` admits R's account;
   - O hasn't declined R;
   - for a notarized R, `terms.notarizes` is set.

   `Pin::Only` admits that offer alone.
2. R is notarized when `needs.notarized` or its class's `notarized` is set. It then also needs a notary: a `Ready` offer of `KindSpec.notary`, admitted there, from a provider other than O's (found through that kind's `idle/` index). If none exists, R waits with `Waiting::NoNotary`, and every offer that would otherwise take it is passed over as `Passed::NoNotary`.
3. Requests come in this order:
   - Class: `Policy.classes` in its order, highest first.
   - Group, within a class: the policy's groups ordered by `used / weight` in the share window, read from the ledger at that height (one ledger read per group the policy names), then the default group `""` at weight 1. Ties go by the policy's order. `Policy.share` says what is counted: a meter (none: assigned units) over a window of blocks (none: since the policy was set).
   - Request, within a group: the `queue/(K, class, group)` prefix in id order, which is oldest first.

   A group over a quota in its window is skipped. Requests filed under a class or group the current policy no longer names come after every named one, in key order. So the walk follows the policy's class order and its fair share, never the names' byte order. A walk's first step reads the policy's classes × groups ledger rows once to fix its order, and every step examines at most `KindSpec.walk` requests (Queue walks, below). Nothing is collected and sorted. An account is in the first group whose `members` policy admits it, else in `""`. Its class and group are fixed when it is filed.
4. Offers for one request: `Pin::Prefer`'s offer while it takes R. Otherwise the first offer that takes R in K's `idle/` index, which is the one free longest, then by id.
5. The assignment keeps the class, the group, the group's use and weight at that height, and which rule chose the offer (`Reason`). `Why` reads it back. `PassedOver` walks K's `kind-offer/` index and says, per offer, why it doesn't take R now.

### Queue walks
A queued request is placed by a walk of its kind's queue, in the order of rule 3, taken in steps. Each step is its own `Place { kind }` frame, never the frame of the op that started the walk. A step examines up to `KindSpec.walk` queued requests, a number the kind's owner sets at `RegisterKind` (there is no program default). It places each one a free admitted offer takes. A step ends the walk in two cases: when no offer of the kind has a free slot left, and when it reaches the queue's end. Either way it clears `walk/<kind>` and books nothing, because the next capacity event starts a fresh walk with a fresh order. Only a step that used up its `walk` count with free slots left books the next step, for the next block. The walk's class and group order, fixed at its first step, and its position are kept in `walk/<kind>` meanwhile, so each step goes on from where the last stopped. A walk started while one is running joins it. A request filed in a class and group the running walk has already passed makes the walk start over once it reaches the queue's end, rather than end, so a free slot never waits for the next capacity event while a request it takes is queued.

These events book a step for the next block, once per (kind, height) through `walk-at/`:
- capacity frees: an assignment ends (a `Report` or `Count` with an outcome, `Decline`, `Missed`, `Unclaimed`, a `Lapse` that loses what an offer held, `Cancel` of a placed request once it ends);
- capacity appears: `Offer`, `Beat` taking an offer back to `Ready`, `Admit`;
- a notary appears: any of those on a notary kind books a step for every kind `notary-of/` names for it, each there by the notary kind's consent;
- `SetPolicy`, whose policy rules from the next block.

A step that passes a group over its quota also books one at that quota window's end (`Waiting::OverQuota.until`).

So the op that frees or adds capacity pays for one booking and never for the queue. A step's cost is bounded by the owner's `walk`. A queue of requests no offer takes costs one step per `walk` requests, spread over blocks, and never blocks a provider's `Report` or `Beat`. `Request` and `Pick` place only their own request in their frame, which is bounded by the kind's offers.

Every number placement reads (weights, quotas, windows, tolerance) is set by the kind's owner in its policy. With no policy, a kind has one class, one group of everyone, no quota and tolerance 0: plain first come, first served over the offers free longest.

## Use and what it rests on
Each meter of a kind says who reports it (`Meter.reported_by`) and who, if anyone, checks it (`Meter.checked_by`). Model tokens, for example, are reported by the provider and checked by the requester, and calls are reported by the chain and checked by no one. A `Usage` row keeps each side's latest count with its height:
- the provider's, from `Report`;
- the requester's, from `Confirm`;
- the chain's, from `Count`.

It also keeps any evidence blob. Its `Standing`, worked out again at each count, is one of:
- `Counted`: only chain-reported meters are in;
- `Attested`: the provider's count is in and the checker's isn't yet;
- `Claimed`: the requester's count is in and the provider's isn't yet (an egress client may see its tokens first);
- `Confirmed`: every checked meter's two counts agree within `Policy.tolerance_ppm` of the larger;
- `Disputed { meters }`: the checked meters where they don't. Both counts stay on the row. An assignment counts once in its offer's `Record.disputed`, however often its counts change.

What a count of a meter the kind reports rose past the highest count charged before is added to the group's `ledger/` rows, one per window the policy reads for that meter: the share window (since the policy was set, when it names none) and each quota's window, aligned to multiples of its length. Units assigned count as the meter `""`.

Nothing here proves that an upstream behind a provider did the work. A provider is trusted with its output exactly as far as its kind's owner admitted it, and eviction is the remedy.

## Who is told
`Spec.notify` says who hears that the request was assigned (`Event::Assigned(Placement)`), lost or ended:
- `Program { program, deliver }` hears `deliver ++ borsh(Event)` for each of these.
- `Chat { channel }` gets one chat notice for each, concerning only the account the request is for.

## Queries
`Kind { name }`, `Kinds { page }`, `Policy { kind }`, `Admitted { kind, page }`, `Offer { offer }`, `Offers { kind, page }` (over `kind-offer/`), `Request { id }`, `Queued { kind, page }` (by class, group and id), `Assignment { request, attempt }`, `Holds { offer, page }`, `Usage { request, attempt }`, `Ledger { kind, group, page }`, `Record { offer }`, `Why { request }`, `PassedOver { request, page }` (over `kind-offer/`), `InForce { kind }` (the kind row, the policy in force at this height and the height it rules since), `Admission { kind, provider }` (one provider's admitted row and tier). Each page walks one table or index in its key order and stops at its limit.

## Interface
`Op`, `Query`, `Reply` and every stored type only grow at their end. `Spec` rides in `Request` and `Pick`, and `Policy` in `SetPolicy`, so a new field means a new op variant and a new row, not a changed type.

Not in this interface yet, each a later append:
- spot checks by replay (a `Verdict` op and a `Reproducible` standing);
- preemption between classes;
- settlement.
