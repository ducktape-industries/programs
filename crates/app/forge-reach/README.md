# forge-reach

A check's result carried to a change's Preview when nothing the check read changed, the way a GitHub Checks app reports on a pull request. forge knows nothing of it: forge-reach watches forge, reads it through forge's own queries, and reports forge `Status` as its own account. A network that wants it deploys it and names its account as a branch's `RequireChecks` reporter; one that doesn't never runs it.

A reporter (a lane, a CI runner) reports its result for a commit together with its reach: the paths in the repository its check read (a crate's folder, `Cargo.lock`, `""` for the whole tree). When forge makes a change's Preview, forge-reach takes every result reported for the change's head (or for the Preview's own commit), and for each compares the tree entry at every reached path between the reported commit and the Preview. All the same: it reports the reported state on the Preview. One differs: it reports `Pending`, with a url to the change's verdict (`duck://<chain>/forge-reach/<repo>/<n>`), which names the paths that differ. Comparing the results rather than what dev changed since the merge base also catches a 3-way merge that rewrote a reached file. A re-gate needs no rebase: run the check on the Preview's commit (`refs/changes/<n>/merge`) and report that commit.

The status's context is `<reporter account>/<context>`, so a branch's `RequireChecks { reporter: forge-reach's account, contexts: ["23/build.sh#lane"] }` already says whose results it trusts; forge-reach keeps no list of its own.

Known limit: forge's `RequireChecks` names one reporter per branch, so a branch that requires forge-reach can't require actions' statuses beside it. The fix, if a network needs both, is forge's `RequireChecks` taking several reporters, never a relay inside forge-reach.

## Tables

| prefix | key | row |
| --- | --- | --- |
| `r/` | repo, commit, reporter, context | the reported state, url and height |
| `p/` | repo, commit, reporter, context, path | one reached path |
| `v/` | repo, change | the head and the commit of the change's latest Preview heard |
| `h/` | repo, commit, change | a commit that is an open change's latest Preview's head or commit |

## Who may call each op

- `Report { repo, commit, context, state, url, reach, change }`: any account; recorded as the sender's (`sender_account`). The commit is its full lowercase hex id, the context is not blank, and the reach names at least one path, each relative to the root with no empty, `.` or `..` segment (`""` alone is the whole tree). Saying it again replaces the state, url and reach. With `change`, it reports on that change's latest Preview at once; without it, on the latest Preview of every open change whose head or Preview commit is the reported commit (`h/`), so a reporter that knows no change (the pods service's lane line) is carried to a Preview made before it reported.
- `Watched(event)`: forge only (`sent_by(forge)`), as forge's `Watch` delivers it; forge-reach sends `Watch` from its `init`. A `Previewed` event keeps that Preview's head and commit (`v/`, `h/`) and reports on it; `Merged` and `Closed` drop what was kept for the change; every other event is heard and dropped. forge tells a Preview only when it makes a new commit (a Preview of heads already previewed answers the one made), which is why a later report must find the Preview itself.

## Queries

- `Reports { repo, commit, page }`: every result reported for a commit, each with its reach.
- `Verdict { repo, n }`: for change `n`'s latest Preview, each result reported for its head or its commit: the status context, the commit reported, the state reported, the state forge-reach reports, and the reached paths that differ; null while the change has no Preview.

## Cost

Each result on a Preview costs, per reached path, one forge `Tree` read of the path's parent folder at the reported commit and one at the Preview (a page at a time, at forge's founded page size), or one read of the root for `""`. A result reported for the Preview's own commit compares nothing.
