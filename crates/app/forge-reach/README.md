# forge-reach

A check's result carried to a change's Preview when nothing the check read changed, the way a GitHub Checks app reports on a pull request. forge knows nothing of it: forge-reach watches forge, reads it through forge's own queries, and reports forge `Status` as its own account. A network that wants it deploys it and names its account as a branch's `RequireChecks` reporter; one that doesn't never runs it.

A reporter (a lane, a CI runner) reports its result for a commit together with its reach: the paths in the repository its check read (a crate's folder, `Cargo.lock`, `""` for the whole tree). When forge makes a change's Preview, forge-reach takes every result reported for the change's head (or for the Preview's own commit), and for each compares the tree entry at every reached path between the reported commit and the Preview. All the same: it reports the reported state on the Preview. One differs: it reports `Pending`, with a url to the change's verdict (`duck://<chain>/forge-reach/<repo>/<n>`), which names the paths that differ. Comparing the results rather than what dev changed since the merge base also catches a 3-way merge that rewrote a reached file. A re-gate needs no rebase: run the check on the Preview's commit (`refs/changes/<n>/merge`) and report that commit.

A result survives a Rebase or a push when the patch is the same. forge-reach keeps every commit that has been a change's head or Preview (each Preview heard, and each Report that names the change). A reporter with no result on the Preview's head or commit has its newest result on any of those earlier commits taken instead, and it carries only when both hold: every reached path is the same at that commit and at the Preview, and the patch is the same, meaning forge `DiffStat` from the commit's merge base with the Preview's target head names the same paths, with the same status and kinds, as `DiffStat` from that target head to the Preview. Otherwise it is `Pending`, naming the paths that differ (`""` when forge can't tell the patch). So a review whose reach is every path the change touches carries across a clean rebase onto a dev that left those files alone, and needs a fresh report when dev edited one of them or the change grew.

The status's context is `<reporter account>/<context>`, so a branch's `RequireChecks { reporter: forge-reach's account, contexts: ["23/build.sh#lane"] }` already says whose results it trusts; forge-reach keeps no list of its own.

## A result asked for again

A result that went stale on a Preview is asked for again there, so its reporter can run its check on the Preview's commit with no hand step. forge-reach keeps a wanted row for a Preview's commit, a reporter and a context while all of these hold: the change's target branch has `RequireChecks` naming forge-reach as reporter and listing the context; the reporter's result was `Passed` and is `Pending` on this Preview because a path differs; and every other context the branch requires is `Passed` on this Preview. A context like a review whose reach is every path the change touches turns `Pending` when the change's own content moves, so a row means the target moved under a change that is otherwise ready. Its reporter reads the rows with `Wanted` and reports the Preview's commit; that report, a newer Preview of the change, or its merge or close drops the row. A Preview's commit is asked for once: the row is the same row however often it is told.

When a change merges into a branch that requires forge-reach, forge-reach previews the other open changes into that branch whose results in every context the branch requires were `Passed`, each at the next block through the scheduler role, so their stale results are asked for again before anyone reaches them. A change that can't be previewed (a conflict) is left as it is.

Known limit: forge's `RequireChecks` names one reporter per branch, so a branch that requires forge-reach can't require actions' statuses beside it. The fix, if a network needs both, is forge's `RequireChecks` taking several reporters, never a relay inside forge-reach.

## Tables

| prefix | key | row |
| --- | --- | --- |
| `r/` | repo, commit, reporter, context | the reported state, url and height |
| `p/` | repo, commit, reporter, context, path | one reached path |
| `v/` | repo, change | the head and the commit of the change's latest Preview heard |
| `h/` | repo, commit, change | a commit that is an open change's latest Preview's head or commit |
| `c/` | repo, change, commit | a commit that has been an open change's head or Preview, or that a Report named for it |
| `w/` | reporter, repo, Preview commit, context | the change whose Preview asks the reporter's result again |
| `u/` | repo, Preview commit, reporter, context | the same row, found from the Preview |

## Who may call each op

- `Report { repo, commit, context, state, url, reach, change }`: any account; recorded as the sender's (`sender_account`). The commit is its full lowercase hex id, the context is not blank, and the reach names at least one path, each relative to the root with no empty, `.` or `..` segment (`""` alone is the whole tree). Saying it again replaces the state, url and reach. With `change`, it keeps the commit as one of that change's heads (`c/`) and reports on that change's latest Preview at once; without it, on the latest Preview of every open change whose head or Preview commit is the reported commit (`h/`), so a reporter that knows no change (the pods service's lane line) is carried to a Preview made before it reported.
- `Watched(event)`: forge only (`sent_by(forge)`), as forge's `Watch` delivers it; forge-reach sends `Watch` from its `init`. A `Previewed` event keeps that Preview's head and commit (`v/`, `h/`, `c/`), reports on it and keeps or drops its wanted rows (`w/`, `u/`); `Merged` and `Closed` drop what was kept for the change, and `Merged` also schedules a forge `Preview`, as forge-reach, of each other change it would ask again (above); every other event is heard and dropped. forge tells a Preview only when it makes a new commit (a Preview of heads already previewed answers the one made), which is why a later report must find the Preview itself.

## Queries

- `Reports { repo, commit, page }`: every result reported for a commit, each with its reach.
- `Verdict { repo, n }`: for change `n`'s latest Preview, each result reported for its head or its commit, or else the reporter's newest result on an earlier head of the change: the status context, the commit reported, the state reported, the state forge-reach reports, and the paths that differ (reached paths, else paths the two patches change differently); null while the change has no Preview.
- `Wanted { reporter, page }`: the Previews that ask a reporter's result again, each as its repository, change, Preview commit and context (the context as the reporter reported it, without its account).

## Cost

Each result on a Preview costs, per reached path, one forge `Tree` read of the path's parent folder at the reported commit and one at the Preview (a page at a time, at forge's founded page size), or one read of the root for `""`. A result reported for the Preview's own commit compares nothing. A result taken from an earlier head whose reach holds also costs one forge `Compare` and its `DiffStat` pages, and the Preview's own `DiffStat` pages once per Verdict. A Preview with a result gone stale also reads the change (forge `Change`), the repository's required checks a page at a time (forge `Required`) and forge-reach's own account (identity `OfProgram`). A merge reads the same for the merged change, and forge `Change` once for each other open change whose kept results all passed.
