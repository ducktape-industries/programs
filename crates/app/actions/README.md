# actions

Checks for forge repositories: each workflow file at a pushed or previewed
commit starts a run, whose jobs are queued as checks and offered to the
runners whose labels cover them; each check's state reaches forge as its
commit's status. Shapes: `src/lib.rs`; checks: `src/rules.rs`.

## Who may call each op

| Op | Who may call it |
|---|---|
| `Watched(event)` | forge only (its watch deliveries) |
| `Trigger { repo, commit }` | any account |
| `Register { labels, deliver }` | any account, for its own runner row; a program that registers is offered checks |
| `Claim { check }` | any account, while the check is queued |
| `Report { check, state, summary, log }` | the account that claimed the check, while it is running; any account while it is queued, to end it errored or superseded (as claiming it first would let it) |
| `Rerun { check }` | any account, once the check is neither queued nor waiting |
| `Preview { repo, n }` | anyone |
| `Outputs { check, files }` | the account that claimed the check, while it is running; who sent them is kept |

Every query is answered for anyone.
