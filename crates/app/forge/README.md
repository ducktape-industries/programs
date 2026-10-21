# Forge read and review contract

The source of truth is `src/contract.rs`, `src/read_contract.rs`, and
`src/review_contract.rs`, re-exported by `forge`. All forge requests, records,
UI replies, and operation outputs use Borsh. Serde is for fixture sidecars
only.

## Decisions in one screen

- `gitcore` reads loose commit/tree/blob objects through the sandbox; queries never
  parse packs. No gix, host Git mirror, or forge-specific host storage is needed.
- Reads run off consensus. Every change/review mutation is an operation. `Merge`
  lands a result the client computed and published: it checks both endpoint OIDs
  and atomically changes the target ref and optional change record, without
  inspecting objects. `SquashMerge` builds the result itself in consensus: one
  squash commit on the target's head, under the same CAS. Neither enforces
  review verdicts. `Rebase` builds the same kind of commit for an open change on
  a newer base and moves the change's own source to it, so a change keeps its
  number through a rebase. `Preview` builds, without moving the target, the squash
  a change's merge would land on its target's current head; a branch with required
  checks (`RequireChecks`) moves only by a merge of a change whose latest Preview
  its reporter's `Status`es mark Passed.
- Changes keep their title, body, author account, and lifecycle on the forge record.
  A change's authorship is its own owned row (`o/`, kind `forge.change`), its author's
  from `ChangeOpen`: `ChangeEdit`, `ChangeRepoint` and `Rebase` write it, so the change's
  policy decides them, while reviews, closes and merges write only the shared record.
  A repository's custody is its own owned row (`k/`, kind `forge.repo`), its creator's
  from `Create`: every op the repository's policy decides (a push, a merge, a staged
  pack, a branch deleted, protected or given required checks, `Configure`, the merge
  queue, a rebase, and closing someone else's change) writes it, while the shared
  record (`p/`) takes every op's activity.
  A repository's number counter allocates the shared future issue/change number space.
- Every table is declared once in `src/state.rs` as a typed `store::Map`/`Set`/`Item`:
  records (repos, refs, changes, reviews), the indexes over them (activity,
  involvement, authored reviews) and the counters. A remove path takes its index
  rows with it. Git objects are the one exception: an object is the blob whose
  id is its oid, with no table between them (`src/objects.rs`).
- Forge emits chat creation for `forge:<repo>:<n>`, named `pr/<repo>#<n> <title>`, and a
  notice (chat `Notify`) for opening, closing, merging and submitting a review, told only to
  the accounts it concerns (see "A change's notices"). Chat owns all conversation replies.
  A change from a `j-NNN/…` branch talks in its job's room `j-NNN` instead, while chat
  holds that room unarchived and open to post as it opens (see "A job's room").
  The kernel runs each emission once the handler returns Ok, in the same frame,
  so the record and its channel land in one block or not at all.
- A change that ends (`Merge` or `SquashMerge` naming it, `ChangeClose`) has its
  own channel archived (a job room is left as it is): forge sends chat `SetChannelArchived` as itself, and a refusal
  never fails the op (see "Archiving a change's channel").
- A review is owned by its author (`Owned<Review>`, kind `forge.review`): the
  ownership role records it as theirs. A review is one immutable operation: verdict, body, pinned head and base, and
  its line comments. An anchor is `(path, side, line)`;
  `Old` addresses `base_oid`, `New` addresses `commit_oid`. Both are retained.
  Compare the review's head to `Change.source_head` to show outdated; never move
  positions. A stale draft may be submitted against its original pin.
- All UI lists take `cursor` and `limit`; all UI replies, including refusals,
  carry the answering `height`. No unbounded nested review history is returned.
- Chat's colon namespace is reserved to its exact program prefix (or Root).
  External keys/accounts cannot create these channels or reserve system-message
  IDs. Hide colon channels in the view's rail/search; they are not private rooms.
- Forge's messages to chat act as forge's own account (the identity role's
  `OfProgram`), which the kernel registered as it admitted forge.

## Common types and pagination

`Revision = Ref(Vec<u8>) | Oid(String)`. Refs are full names such as
`refs/heads/main`. OIDs are full hexadecimal IDs of the repo's hash algorithm;
replies normalize them to lowercase. Paths, Git names, messages and source lines
are byte vectors, never lossy UTF-8 conversions. Tree root is `[]`, not `/`.
An agent writes them through harness's `network_call`/`network_query` as hex or
as `{"utf8": "refs/heads/dev"}`, the same bytes (harness's codec takes that form
for any bytes field); replies stay hex. `RefNames` and `DiffStat` answer names
and paths as text for a reader that wants them so.

`PageResponse<T> = { items: Vec<T>, next: Option<Cursor> }`. Pass `next` back unchanged.
The cursor is bound to the original query arguments and height; changing only
`limit` is allowed. A changed height returns `stale`, so restart the list.
Filtered change/judgment pages can be empty **with a next cursor**: continue until
`next == None`. Limits must be `1..=Bounds.page_size`; they are not silently clamped.

Repos sort by descending activity height then name, and leave archived ones out
(`ArchivedRepos` lists those, by name). Refs use byte-name order,
trees use Git entry order, diffs use path-byte order, changes use ascending item
number, and reviews use submission order. Judgment scans repo then item number.
Log covers all parents, ordered by descending committer time then OID, matching
`gitcore`'s walk (not a promise of topological order under clock skew).

## Queries and examples

In these Rust-shaped examples, `r = "project".into()`, `main` and `feature` are
`Revision::Ref(b"refs/heads/main".to_vec())` and the corresponding feature ref;
`a` and `b` are real full commit IDs; `blob` is a tree entry's blob ID. `None, 20`
means a first page; continue with the returned cursor. Encode with `abi::encode`
and decode a `forge::Reply` from the concatenated `Respond` bytes.

| Query example | Reply data besides `height` |
| --- | --- |
| `Repos { cursor: None, limit: 20 }` | `page<RepoInfo>`: name, hash, owner, settings, ref count, last activity |
| `Repo { repo: r }` | repo record (its owner among it) and founded bounds; who else writes is the repository's list in the access program |
| `Refs { repo: r, cursor: None, limit: 20 }` | `page<RefInfo>`: full ref name and target OID |
| `RefNames { repo: r, prefix: "refs/heads/j-049/".into(), page }` | `page<RefName>`: the refs whose name starts with `prefix` (`""`: every ref), in byte-name order, each name as a `String` (a byte that is not UTF-8 reads as U+FFFD; `Refs` keeps the bytes) and its target OID; one key range, not a filter |
| `Log { repo: r, from: feature, cursor: None, limit: 20 }` | resolved tip, `page<CommitInfo>`: OID, tree, all parents, full message, author and committer with time/timezone |
| `Tree { repo: r, at: b, path: b"src".to_vec(), cursor: None, limit: 20 }` | resolved tree OID and `page<TreeInfo>`: name, OID, kind/mode; `at` accepts commit or tree |
| `Blob { repo: r, oid: blob, range: Some(ByteRange { offset: 0, len: 100 }) }` | OID, total size, classification, actual byte range and bytes |
| `Diff { repo: r, base: Some(a), head: b, path: None, cursor: None, limit: 20 }` | normalized endpoints, total matching file count, `page<FileDiff>` |
| `DiffStat { repo: r, base: Some(a), head: b, page }` | what `Diff` with no path answers, less the hunks: `page<FileStat>`, each file's path as a `String` (its new path, or its old one for a delete), status, kinds, sizes, content class and line counts |
| `Compare { repo: r, from: feature, into: main }` | resolved endpoints, merge base, ahead/behind, mergeability; each side is walked from its tip to where the two histories meet, within `Bounds.log_walk` commits |
| `Compares { repo: r, into: main, from: vec![feature, clean] }` | `into` resolved, and for each of `from` in order a `Compared`: `Tally::Counted(Comparison)` as `Compare` answers it, or `Tally::Refused(why)` for a branch past the walk bound or no such ref, each counted on a read budget of its own. A view's list of branches reads every standing in this one query |
| `Activity { repo: r }` | `last_height`: last successful forge operation in the repo |
| `Changes { repo: r, filter: ChangeFilter { state: Some(ChangeState::Open), ..Default::default() }, cursor: None, limit: 20 }` | `page<ChangeSummary>` with author, endpoints, counts and historical verdict totals |
| `Change { repo: r, n: 1, cursor: None, limit: 20 }` | full change record/body/channel, optional current source/target heads, `reviews: PageResponse<Review>` |
| `Judgment { principal: reviewer, cursor: None, limit: 20 }` | `page<Judgment>`: open changes requesting this account at the current head or containing an answered thread it authored |
| `Advertise { repo: r, service: Service::UploadPack }` | raw Git protocol v2 advertisement |
| `Upload { repo: r, request: git_v2_ls_refs_bytes }` | raw Git protocol v2 `ls-refs` response |
| `Closure { repo: r, walk: Walk::of(gitcore::walk::Walk::of(&wants, &haves)) }` | `listed`: the ids and kinds of the objects `wants` reach less what `haves` reach, and `next`, the walk to send back for the next page |
| `History { repo: r, walk: Walk::of(gitcore::walk::Walk::of(&wants, &haves)) }` | `Reply::Closure` less the trees: the commits `wants` reach less what `haves` reach, newest first, and the tags, trees and blobs a want names itself; `next` as for `Closure` |
| `Shallow { repo: r, walk: DeepWalk::of(gitcore::walk::Deep::of(&wants, &haves, Some(depth), &client_shallows)) }` | `History` cut `depth` commits below each want (`git fetch --depth`): `listed` and `next` as for `History`, and once the depths are walked the boundary, `shallow` (the new boundary commits) and `unshallow` (the client's boundaries the walk went past) |
| `Protected { repo: r, cursor: None, limit: 20 }` | `page<Vec<u8>>`: the protected branch names in byte order; the head is protected besides |
| `Preview { repo: r, n: 1 }` | `Option<PreviewInfo>`: change 1's latest Preview, the target and source heads it read and the squash it made (`merge`) |
| `Statuses { repo: r, commit: c, page }` | `page<CommitStatus>`: every status of `c`, by reporter then context, each with its state, url and height |
| `Required { repo: r, page }` | `page<RequiredBranch>`: the branches with required checks, in byte-name order, each with its reporter and contexts |
| `Queue { repo: r, branch: b"refs/heads/dev".to_vec(), page }` | `page<QueueEntry>`: the branch's merge queue, first in line first, each with its head when it joined and who queued it |
| `Archived { repo: r }` | `archived`: whether the repository is archived (`Archive`) |
| `ArchivedRepos { page }` | `page<RepoInfo>`: the archived repositories, which `Repos` leaves out, in name order |

`Advertise` and `Upload` speak Git's own smart-HTTP framing, so they carry no
Borsh height envelope. They are not UI list APIs. `Upload` answers `ls-refs`
and refuses `fetch`: a git client fetches by listing the objects with
`Closure` and reading their bytes from its node's blobs.

`Closure` lists ids, never bytes. Each page reads at most `Bounds.tree_walk`
objects and `Bounds.diff_bytes` bytes. Commits are walked newest first, as
git's upload-pack walks them: a commit `haves` reaches is the client's, and so
are its ancestors, so the walk stops wherever the client's history begins,
not only at the commits it named. Each listed commit's tree is compared with
its parents' trees, so an unchanged subtree is not listed again. The client
keeps the walk: it sends `next` back, less the objects it has already been
listed, until `next` is done (`gitcore::walk::Listing`). A git object is stored as the blob git
would hash it as (`<kind> <len>\0<body>`), under the repository's hash, so an
object's blob id is its oid: `forge::blob_id_of(&oid)` names the blob a node
serves at `/v1/blobs`.

`History` walks as `Closure` does and leaves the trees to the client. It lists the
commits, and the tags, trees and blobs a want names itself, reading each commit
and its parents once, so a page holds about `Bounds.tree_walk` commits where a
`Closure` page holds that many objects of every kind. The client reads the listed
commits from its node's blobs, queues each one's tree against its parents' trees
(and each listed tree against none) in `gitcore::walk::Trees`, and expands them a
level at a time from the tree bodies `Trees::wanted` names, which a fetch's pack
holds anyway. What it ends with is what `Closure` lists. A fetch spends one query
per page under the node's lock, so this takes a clone of a few thousand commits
from dozens of pages to a handful (git-remote-duck asks `History` first, and
`Closure` where a forge answers no `History`).

`Shallow` answers a shallow fetch. It first walks the depths, as git's upload-pack
does: breadth first from the wants (each at depth 1), through every commit
whatever the client holds, as far as `deepen`. A commit at depth `deepen` is a
new boundary (`shallow`), unless the client already names it `shallow`; a client
boundary nearer than that is lifted (`unshallow`) and its parents are wanted.
Then it walks as `History` does with every boundary's parents cut, the new ones
and the client's, so neither the walk nor the client's `haves` reach past one.
`deepen: None` walks no depths and cuts only at the client's boundaries: a
shallow client's plain fetch. The depths walked so far travel in `next` with
the walk, each page reading at most `Bounds.tree_walk` objects, and `shallow`
and `unshallow` are final on the page whose depths are done (every page after
repeats them). The client expands the trees as for `History`, a boundary's tree
against no parents, and sends `shallow` and `unshallow` in git's `shallow-info`
section before the pack (`gitcore::wire::upload::respond_shallow`). forge's
upload-pack advertisement does not offer `shallow` yet.

An unborn ref returns `not_found`; an empty directory/list is a successful empty
page. Missing current change refs are `None` on the detail, so a deleted branch
does not make the conversation or review history unreadable.

`ChangeFilter` combines optional state, author and involved account with AND.
Involvement means author, requested reviewer, or submitted reviewer. A reviewer
taken off the request stays involved only if they submitted a review. Chat-only participation is represented by chat itself.
Judgment additionally joins ordinary chat-authored threads and **all** reviews
by the account, not just its latest review. The newest answered root is returned as
`ReplyAttention { review: Option<u64>, root_seq, last_reply_seq }`; `None` identifies
a conversation root. Requested judgment clears when that account submits any verdict
at the current source head, and returns when the head moves. Read/unread markers
and dismissals are local view state, not notification records or new operations.
Authors, reviewers and filters are accounts (`Principal`), never keys:
an account acts through any of its keys.

## Blobs, diffs, and mergeability

`Blob` returns `Text`, `Binary` (NUL or invalid UTF-8), or `Oversize`. A blob up to
`Bounds.blob_bytes` is read and judged whole. A blob above it answers `Oversize`
when no range is asked. A range of it (at most `blob_bytes` long, as any range) is
read alone and judged on its own bytes: `Text` with them when it holds no NUL and
is UTF-8 but for a code point cut at either edge, else `Binary` in that range, its
`range` naming the bytes judged (a blob judged whole answers `Binary` with an empty
range). Binary/oversize replies carry no content bytes. Text ranges use byte
offsets, may split a UTF-8 code point, and clamp their end at EOF. Zero-length
text is valid.

`Diff` compares trees directly; use `base: None` for a root commit. For a change's
merge-base diff, first use `Compare.base`, then `Diff { base, head: from, ... }`.
An optional path selects one exact changed file. Each file carries old/new paths,
OIDs, modes, sizes, status, content classification, additions/deletions and hunks.
Renames appear as delete/add, with no heuristic rename tracking. Gitlinks have
headers, not blob reads. A file is `Oversize` when either side is above
`blob_bytes` or its edit is too long to search under `merge_cost`. Binary/oversize
files have no hunks; line stats are zero and not applicable to those classifications.

Hunks have three context lines on each edge. A range starts at a **one-based**
line, except a zero-length range names the preceding line (0 at file start).
`DiffLine` is typed `Context | Added | Deleted`, with optional old/new line numbers
and exact bytes including the original newline, if any. Literal `++ x` is data,
not a patch header. Use endpoint trees/blobs to expand context or comment on any
line of a changed file. No text-patch parsing is needed in the view or host.

`Compare` reports ancestry facts only: `UpToDate`, `FastForward`, `Diverged`
or `Unrelated`, with the merge base (the deterministic lowest-OID choice among
several) and ahead/behind counts. Whether diverged endpoints merge cleanly is
found by merging: a git client merges, pushes the result and the both-head CAS
of `Merge` decides whether it still applies, or `SquashMerge` merges in
consensus and refuses a conflict. Approvals and
requested changes never block that CAS.

## Operations and examples

Authors are always the account the frame acts as (`ctx.sender_account()`), never
the payload: a signed frame's key's account, a program's own, or the agent a
program sends the op for (harness, for the agents it runs); the chain itself and a key
that holds no account are refused. Any account may open/review and run `ArchiveEnded`. The author edits title/body/review requests,
or whoever the change's list at `forge::change_key(repo, n)` in the access
program allows, and so re-points it (`ChangeRepoint`); a `Rebase` needs the
change's key and the repository's; the author, or a repository writer, closes; repository writers merge.
A repository's writers are whoever its list in the access program allows, at
`forge::repo_key(repo)`: anyone unless the owner sets a list there
(`access::Op::Set`). `Push`, `Merge`, `SquashMerge`, `Stage`, `DeleteBranch` and
a `ChangeClose` by anyone but the change's author need that key; `Configure`,
`Protect`, `RequireChecks` and `Archive` need `forge::settings_key(repo)`, which follows the same list unless the
owner sets one of its own. `Preview` and `Status` need no key: anyone previews an open
change, and anyone reports a status, under their own account.
Reviews remain appendable on closed/merged changes.
Closing is terminal; there is no reopen operation.

| Operation example | Effect/output |
| --- | --- |
| `Create { repo: r, hash: HashKind::Sha1 }` | actor owns a new repo; empty output |
| `Configure { repo: r, settings: Settings::default() }` | owner configures default ref and force/delete policy; empty output |
| `Grant { repo: r, principal: writer }`, `Revoke { .. }` | always refused: a repository's writers are its list in the access program |
| `Push { repo: r, request: receive_pack_commands, pack: Some(pack) }` | Git receive-pack: the ref commands in `request`, the objects in the held `pack`; Git's report |
| `Stage { repo: r, pack }` | stores the held pack's objects and moves no ref; empty output |
| `ChangeOpen { repo: r, from: feature, into: b"refs/heads/main".to_vec(), title: "Fix parser".into(), body: "Why this changes".into(), reviewers: vec![reviewer] }` | assigns `n`, stores body, queues channel/opened notice; `OpReply::Change { height, n }` |
| `ChangeEdit { repo: r, n: 1, title: None, body: Some("Updated rationale".into()), reviewers: None }` | `None` leaves a field unchanged; `Some(vec![])` clears requests; `OpReply::Change` |
| `ChangeClose { repo: r, n: 1 }` | open to closed, queues a notice; `OpReply::Change` |
| `ReviewSubmit { repo: r, n: 1, review: ReviewDraft { commit_oid: b, base_oid: Some(a), verdict: Verdict::RequestChanges, body: "Please check this".into(), comments: vec![LineComment { path: b"src/lib.rs".to_vec(), side: Side::New, line: 12, body: "Check this bound".into() }] } }` | one immutable review and all comments, one chat root; `OpReply::Review { height, n, id }` |
| `Merge { repo: r, into: b"refs/heads/main".to_vec(), from: feature, expected_into: a, expected_from: b, result: client_result_oid, change: Some(1) }` | both-head CAS and open to merged together; `OpReply::Merged { height, oid, change }` |
| `SquashMerge { repo: r, into: b"refs/heads/main".to_vec(), from: feature, expected_into: a, expected_from: b, message: None, change: Some(1) }` | one squash commit built in consensus on `a`, the same CAS, open to merged together; `OpReply::Merged { height, oid, change }` |
| `Rebase { repo: r, n: 1, onto: main, expected_head: b }` | one squash commit of the change on `onto`, and the change's source moved to it under a CAS on `b`; `OpReply::Rebased { height, n, oid }` |
| `ChangeRepoint { repo: r, n: 1, from: feature }` | the change's source becomes `from`; `OpReply::Change` |
| `DeleteBranch { repo: r, branch: b"refs/heads/old".to_vec(), expected: a }` | deletes an unprotected branch at `a` that no open change names; empty output |
| `Protect { repo: r, branch: b"refs/heads/dev".to_vec(), protected: true }` | adds (`false`: removes) a protected branch; empty output |
| `Preview { repo: r, n: 1 }` | change 1's squash on its target's head, under `refs/changes/1/merge`; `OpReply::Previewed { height, n, oid }` |
| `Status { repo: r, commit: c, context: "gates.toml#gates".into(), state: CheckState::Passed, url: Some(log) }` | the sender's status of `c` in that context; empty output |
| `RequireChecks { repo: r, branch: b"refs/heads/dev".to_vec(), required: Some(Required { reporter, contexts }) }` | the branch moves only by a gated merge (`None` drops it); empty output |
| `Enqueue { repo: r, n: 1, expected_head: b }` | change 1 joins its target's merge queue; empty output |
| `Dequeue { repo: r, n: 1 }` | change 1 leaves its queue; empty output |
| `ArchiveEnded { repo: r, after: None, limit: 20 }` | archives the channels of ended changes numbered above `after`, `limit` numbers of them; `OpReply::ArchivedEnded { height, archived, next }` |
| `Archive { repo: r, archived: true }` | archives the repository (`false`: restores it); empty output |

A push's objects never ride inside its frame. The client puts git's pack as a
blob of kind `pack` (`forge::PACK_KIND`) with `ducktape blob put FILE --kind
pack --wait`, which returns once two thirds of the current validators hold it,
then submits the op naming the blob. `request` holds only git's ref commands
and capabilities, through their flush; a request with bytes after the flush is
refused. forge reads the pack, checks it as git's receive-pack does, and puts
each object. A pack the blobs role does not hold is `not_found`; a held blob
of another kind is `invalid`.

A pack may go in parts: each part a `Stage`, then a
`Push` with the last part moves the refs. Every object a part names must be
held when the part lands, in that part or already stored, so the parts go in
the order git's objects reference each other: blobs and trees before the
commits naming them. A delta may be against an object already stored. The
repository's writers push and stage.

A merge requires a nonzero result that advances the target, correct endpoint
hash kinds, writer authority and matching current heads. It does not validate
ancestry or possession of the client result; the writer is responsible for
publishing it first (e.g. push a prepared result ref). `change: None` changes only
the target ref.

`SquashMerge` makes the result itself, with the same CAS, endpoint and change
checks. From the merge base of both heads (`log_walk` bounds the walk; no base is
refused, `wrong_state`), gitcore's 3-way merge takes each path changed on one
side from that side and merges a file changed on both line by line, held to
`merge_cost` per file. Edits of the two sides that overlap or touch (adjacent
lines, two insertions at one point), a binary file or a mode both changed, a
change against a delete and a file against another kind are conflicts: the op
is refused `wrong_state`, naming every path, and writes nothing. The commit has
one parent, the target's head. Its message is `message`, or the change's
`<title> (change N)`, a blank line and its body; without a change, `message` is
required. The change's author is the commit's author and the caller its
committer (the caller is both without a change), each by their identity name at
`<account>@<chain id>`, at the block's time, so every validator makes the same
commit id.

`Rebase` rebuilds an open change on `onto` (a branch or a commit, usually the
change's target) with `SquashMerge`'s machinery: from the merge base of the
change's head and `onto`, what the change's side changed is merged onto `onto`'s
tree, held to `merge_cost` per file, and a conflict is refused `wrong_state`,
naming every path, writing nothing. The result is one commit whose one parent is
`onto`; its message is the change's title, a blank line and its body, its author
the change's author and its committer the caller, each signed as a squash is, at
the block's time. The change's source branch moves to it (a commit source takes
the new id), under a CAS on `expected_head` (`stale` once the source moved). The
move needs no `allow_force`: the change keeps its number, reviews and channel,
and reviews pinned at the old head read as outdated, as after any push. A rebase
is refused `wrong_state` when `onto` already holds the change, when the change
already sits on `onto`, and when its source is a protected branch.

`ChangeRepoint` points an open change at another source, a branch under
`refs/heads/` or a commit, checked as `ChangeOpen` checks one (it exists and
differs from the target's head); the number, reviews and channel stay.

A repository's head (`Settings.head`) is always protected; `Protect { branch,
protected }` adds or removes another protected branch (the head can't be
unprotected), and `Query::Protected` lists them. No `Rebase` moves a protected
branch and no `DeleteBranch` deletes one. `DeleteBranch` deletes a branch still at
`expected` (`stale` otherwise) unless it is protected or an open change names it as
its source, which the refusal lists. It ignores `allow_delete`, which governs only
the deletions a git push asks for. The branch's objects stay, so it can be pushed
again.

`Preview { n }` builds open change `n`'s squash on its target's current head with
`SquashMerge`'s machinery (the same merge base, 3-way merge, `merge_cost`, message,
author and committer), points `refs/changes/<n>/merge` at it and records the two
heads it read (`Query::Preview`); a conflict is refused `wrong_state`, naming every
path, writing nothing. While both heads stay where they were, it answers the same
commit and writes nothing; a watcher hears `Watched::Previewed` for each new one.
`Status` records the sender's state of a commit (`Pending`, `Passed`, `Failed`,
`Errored`) in a context, with a url: each reporter's own, so no one writes another's.
`RequireChecks { branch, required }` names a reporter and its contexts for a branch.
From then on a push that moves the branch is refused, and a `SquashMerge` or `Merge`
into it is refused `wrong_state` unless it names its change, that change's latest
Preview read the very heads being merged (else it is stale: preview again), and the
reporter's status of the Preview's commit is `Passed` in every context. The refusal
names each context that is pending, failed or errored, and says "no status from
<reporter>" for one never reported (the checker is down, not the code). The squash a
gated merge lands has the Preview's parent and tree, since the same merge runs on the
same commits. The break-glass is the settings owner sending `RequireChecks` with
`required: None`, one op on chain; the refusals say so.

`Enqueue { n, expected_head }` puts open change `n`, still at that head, at the end of
its target's merge queue; the target must require checks (`Enqueue` and `Dequeue` need
what `SquashMerge` needs). Only the first in line is previewed, as it reaches the front,
with whoever queued it as committer (a Preview of the same heads made already is used as
is). The op that makes that Preview green in every required context, a `Status` usually,
lands it: the branch moves to the Preview's very commit, so what landed is what was
tested, the change is merged by whoever queued it, and the next in line is previewed on
the new head. An entry leaves the queue, with a `left queue` notice in its change's channel for
whoever queued it, when a required context goes `Failed` or `Errored`, when its Preview
is refused (a conflict names its paths), or when its head moves; a change that merged or
closed otherwise just leaves. Each op that moves a ref, ends or re-points a change,
reports a status or changes what a branch requires moves the repository's queues on.

`Dispatch { event_type, payload, at }` tells every watcher (`Watched::Dispatched`) that a
writer dispatched `event_type` at the commit `at` resolves to: a branch ref goes to its head,
and an oid must be a full commit id forge holds; any other `at`, and an empty `event_type`,
is refused, telling no one. It answers to the gate a `Push` answers to (the same access
need on the repository, and its custody written again for the ownership guard), so whoever
may push may dispatch and nobody else. `payload` reaches the watchers as given; actions runs
the workflows whose `on.dispatch` names `event_type` at that commit.

### Archiving a repository

`Archive { repo, archived: true }` archives a repository; the repository's policy
decides it, as `Configure`'s does (`forge::settings_key(repo)`). An archived
repository leaves `Repos` (its activity row goes, so pages stay whole) and is
listed by `ArchivedRepos`; `Archived { repo }` says whether one is. forge refuses
every op that names it, before writing anything, `wrong_state` ("<repo> is
archived: un-archive it …"): pushes, stages, changes and reviews, merges, branch
rules, statuses and previews alike, so its open changes stay as they are. `Create`
of its name still answers that the name is taken. Every query answers as before,
the git wire (`Advertise`, `Upload`, `Closure`, `History`, `Shallow`) included, so a clone, a fetch or a
pinned dependency keeps working. `Archive { archived: false }` restores it: it is
listed again and takes writes again. Archiving an archived repository, or
restoring one that is not, is `wrong_state`.

### Archiving a change's channel

When a change merges (`Merge` or `SquashMerge` with `change: Some(n)`) or closes,
forge sends chat `SetChannelArchived { channel_id, archived: true }` for its
`forge:<repo>:<n>` channel, as forge's own account, which owns the channel and so
is the one account chat lets archive it. It asks for chat's answer
(`Reply::Wanted`): a refusal is absorbed, so the merge or close still lands, and
forge posts an `archive refused` notice (detail `archive refused: <reason>: <sentence>`)
into the channel; that notice's own refusal (a channel that is gone) is absorbed too. Closing
is terminal, so nothing un-archives a channel.

`ArchiveEnded { repo, after, limit }` clears the channels changes left open before
forge archived them. Anyone may run it. It looks at the change numbers above
`after` (`None`: from the first), `limit` of them (`1..=Bounds.page_size`, else
`invalid`), and archives the channel of each merged or closed one that chat holds
and shows unarchived; open changes, archived channels and missing ones are left.
`archived` lists the numbers it archived and `next` is the `after` of the next
run, `None` once it reached the repository's last number. It takes its own
`after`, not a listing cursor, because a cursor is pinned to forge's write count
and the op is a write. A second run archives nothing.

### A change's notices

Each thing that happens to a change is one chat notice (`Notify`, a `Block::Notice`) in the
change's room, a root and never a thread reply, under the message id forge gives it: a
review's card finds its notice by `Review.message_id`, and replies hang under it.

| event | group | status | detail | concerns |
|---|---|---|---|---|
| `ChangeOpen` | `opened <repo> <n>` | Running | `opened` | the reviewers, or the repository's owner when none is asked |
| a new head: a push or a merge moving its source branch, `Rebase`, `ChangeRepoint` to another commit | `head <repo> <n>` | Running | `new head <oid>` | the reviewers |
| `ChangeEdit` asking reviewers | `requested <repo> <n>` | Running | `review requested` | the reviewers it adds |
| `ReviewSubmit` | `review <id> <repo> <n>` | Passed (approve), Failed (request changes), Note (comment) | `review <id>: approved`, `changes requested` or `commented` | the author and the reviewers |
| a merge naming the change | `merged <repo> <n>` | Passed | `merged into <branch>` | the author and the reviewers |
| `ChangeClose` | `closed <repo> <n>` | Note | `closed` | the author and the reviewers |
| a refused archive | `archive refused <repo> <n>` | Failed | `archive refused: <reason>: <sentence>` | whoever closed or merged it |
| a merge queue's leave | `left queue <repo> <n>` | Failed | `left the merge queue of <branch>: <why>` | whoever queued it |

Every notice's kind is `change` (`forge::NOTICE`), its subject `<repo> #<n>: <title>`, and
its link the change's path on the reader's chain, `forge/<repo>/<n>` (`forge::notice_link`). The account that acted is never among its concerns, nor is the chain
(`Root`): only the accounts it concerns are told, and everyone else in the room sees it. A
new head and a request are posted only when they concern someone besides that account, so a
change nobody else is asked to review moves without a line. forge finds the open changes a
moved branch is the source of in an index of open changes by source branch, kept as each change
is saved. The first time a branch of a repository moves under a forge that keeps the index, forge
indexes that repository's open changes once, so a change opened before the index existed is told
its new heads too.

A change's own room opens named `pr/<repo>#<n> <title>`, so change rooms group under `pr` in
the lists. Whenever forge posts a notice into a change's own room after it opened, it reads
the room's name from chat and renames it (`RenameChannel`, its answer absorbed) when it
differs: a room named before this, or a change whose title was edited, takes the name at its
next notice.

### Watchers

A program that sends `Watch { deliver }` (a program only: `sending_program`) hears each
event below as a message of its own, `deliver ++ borsh(Watched)`, sent with
`Reply::Wanted`: forge absorbs the answer, so a watcher's refusal never fails forge's
write. An empty `deliver` stops it. Each event names who acted.

| `Watched` | when |
|---|---|
| `Moved { reference, old, new, by }` | a push, a merge or a squash moved a ref (`old` or `new` None where it was made or deleted) |
| `Opened { n, from, into, head, by }` | `ChangeOpen` |
| `Previewed { n, merge, by }` | a `Preview` made a new commit |
| `Closed { n, by }` | `ChangeClose` |
| `Merged { n, oid, by }` | a merge naming the change |
| `Reviewed { n, id, verdict, by }` | `ReviewSubmit` |
| `LeftQueue { n, into, why, queued_by }` | forge took the change out of a merge queue for the reason it names (its head moved, it no longer previews, a required check went red); `queued_by` queued it |

### A job's room

`ChangeOpen` from a branch whose name under `refs/heads/` starts with a `j-<digits>/`
segment (`refs/heads/j-101/folders`) asks chat for the room `j-<digits>`. While chat
holds it unarchived and open to post (not members-only), the change talks there: its
`channel` is `j-101`, forge creates no `forge:<repo>:<n>` channel, and its notices go
there. Otherwise (another branch, a room chat
does not hold, an archived or members-only one) the change gets its own channel as
above. The room is decided once, at open, and kept on the change.

forge posts into a job room with `Reply::Wanted` and absorbs chat's answer, so a room
archived after the change opened never fails a review, a merge or a close: the notice is
just not there. forge never archives a job room: an ending change leaves it, and
`ArchiveEnded` skips it. `Judgment` counts only the replies to the change's own review
notices in a job room, never the room's other threads.

Review OIDs and anchors receive syntactic validation, not object reads in
consensus. Paths must be relative, lines positive, bodies nonblank, and anchors
distinct within a batch. Old-side comments require a base. Empty approve/request
verdicts are valid; a comment verdict needs body text or line comments. Clients
validate positions against the displayed endpoint bytes, stage one draft per
anchor, and preserve drafts on failed submission.

## Bounds, refusals, and the current host seam

New bounds are `page_size`, `log_walk`, `tree_walk`, `diff_bytes`, `blob_bytes`,
and `record_bytes`. Defaults live in qa's `forge_smoke::default_bounds()` (which also writes a founding's `forge.params`); founding
chooses deployment values. `log_walk` bounds a DAG walk and the total
review lookup count in one judgment query. A log with no `exclude` walks only as
far as its page: the commits before its cursor, the page and one more, in the
walk's own order. A log with `exclude` walks to where the two histories meet.
Either refuses past `log_walk` commits rather than inventing partial ancestry,
and each page repeats its walk. A push reads the objects its pack brings and
only the refs its commands name; moving an existing branch walks from the new
tip until it meets the old one, within `push_walk`. `tree_walk` bounds object reads in tree/blob/diff queries; log and compare
have derived read budgets (`2 * log_walk + 1` and `8 * log_walk + tree_walk`).
`diff_bytes` bounds aggregate object bytes read, including metadata; existing
`merge_cost` bounds the Myers edit distance searched per file: the common head and
tail are matched first and a one-sided middle (a file added, deleted or grown in one
place) is not searched; a file past it is answered `Oversize`, the rest of its page
comes. `record_bytes` bounds each encoded change/review.

A UI query failure is an ABI `Refusal { reason, sentence }`. Stable reasons
include `object_not_held`, `capacity`, `invalid_input`, `not_found`, `stale`, and
`unexpected_reply`. Operation failures use ABI `Refusal`, including `unauthorized`
and `wrong_state`; no record/emit survives a rejected operation. Malformed Borsh
requests and Git protocol failures use the ABI refusal path.

The current kernel revision `5d1d1f61d89960164cab919dfbb16a1d8cdb3b36` distinguishes
unrostered blobs (`None`) from rostered blobs absent locally (`BlobUnavailable`).
The latter is a host error before the program receives a usable reply: `BlobStat`
is not a local-availability probe either. The program therefore returns
`object_not_held` on every observable missing-object result, with **no retry or
wait loop in a query**, but cannot intercept the host's required-blob failure.
Core/SDK must provide a query-local nonblocking blob read/stat, or convert
`BlobUnavailable` at the query boundary. Do not change required reads in execution
into refusals. This limitation is not proven away by the in-process smoke in qa.

Likewise, `send` commits an outbox item, not the receiver's execution. The guarded
chat namespace and validated payloads prevent ordinary delivery refusals, but
chat must be seated as `chat` and forge as `forge`. Truly simultaneous record+
channel creation would require a host transaction spanning both programs; this
version proves atomic record+queue and next-block delivery, including restart.

## View and SDK handoff

- Decode Borsh using these exact types. Carry opaque cursors and answering heights;
  show typed refusals and restart stale pages. Do not embed a Git pack parser.
- Provide virtual lists for all paged collections, an identity picker/display
  resolver for keys, and the existing Markdown surface for bodies/README.
- The host code/diff surface needs byte text, typed hunk lines, old/new gutters,
  line/range selection, unified/side-by-side modes, anchors, and lazy context from
  Blob. Retain endpoint IDs and each line's side through event callbacks.
- Compose chat's conversation/thread component. `MessageById` turns a review's
  `message_id` into its root `seq`; replies use chat `PostMessage { thread }`.
  Different anchors share a review root; include the anchor when composing a reply.
  Subscribe to both forge and chat state: a chat reply does not change forge's
  root, and newly opened channels become available after outbox delivery.
- `Compare` does not return a merge pack. A merge commit for `Merge` is the
  client's to build and publish; a squash is forge's own (`SquashMerge`).
- Keep pending drafts, viewed-file checks, unread heights/reply sequences and
  last-reviewed filtering in view state. Do not silently retarget draft anchors.
- Deploy the matching chat adapter: it exposes `MessageById` and `ThreadAttention`
  and answers attention by account. Screenshots and live app walks are outside
  this program task.

See [the fixture manifest](fixtures/FIXTURES.md) for the program's real
response bytes and the regeneration command.
