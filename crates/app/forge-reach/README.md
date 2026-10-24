# forge-reach

Exact-commit check and patch-review receipts.
The program stores each result under its reporting account, repository, commit and context.
It neither watches forge nor carries, forwards, enqueues or re-requests results.
Each successful Report or ReportPatch sends Forge Watch with an empty delivery prefix as this program, removing any previous subscription before Forge retires its event interface.
A Set does not rerun init; installation is followed by a real receipt write, whose unsubscribe is verified before producer retirement.

## Who may call each op

- `Report { repo, commit, context, state, url, reach, change }`: any account, recorded as the sender. A full lowercase commit id, nonblank context and nonempty valid relative read paths are required; `""` names the whole tree. Repeating a result replaces its state, URL and reach, and replaces the same identity's patch result. `change` is descriptive input, not a scheduling instruction.
- `ReportPatch { repo, n, commit, context, state, url }`: any account, recorded as the sender. The commit and context follow the same validation. It replaces the same identity's tree result. `n` is descriptive input; results never carry between commits.

## Query

`Reports { repo, commit, page }` returns exact results by reporter and context, tree results followed by patch results with empty reach.
Pagination is bound to the repository and commit; a cursor for another identity is refused.

## Tables

| prefix | key | row |
| --- | --- | --- |
| `r/` | repo, commit, reporter, context | tree result state, URL and height |
| `p/` | repo, commit, reporter, context, path | declared read path |
| `s/` | repo, commit, reporter, context | patch result state, URL and height |

Retired operation tag0 and query/reply tag1 are gaps, never reused.
Terminal operation `Retell` tag3 and query/reply `Wanted` tag2 remain wire tombstones; both requests always refuse and cannot schedule work.
No checks or reviews on another source or target are inferred from stored receipts.
