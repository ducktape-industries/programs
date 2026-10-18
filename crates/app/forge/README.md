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
  number through a rebase.
- Changes keep their title, body, author account, and lifecycle on the forge record.
  A repository's number counter allocates the shared future issue/change number space.
- Every table is declared once in `src/state.rs` as a typed `store::Map`/`Set`/`Item`:
  records (repos, refs, changes, reviews), the indexes over them (activity,
  involvement, authored reviews) and the counters. A remove path takes its index
  rows with it. Git objects are the one exception: an object is the blob whose
  id is its oid, with no table between them (`src/objects.rs`).
- Forge emits chat creation for `forge:<repo>:<n>` and system lines for opening,
  closing, merging, and submitting a review. Chat owns all conversation replies.
  The kernel runs each emission once the handler returns Ok, in the same frame,
  so the record and its channel land in one block or not at all.
- A review is one immutable operation: verdict, body, pinned head and base, and
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

Repos sort by descending activity height then name. Refs use byte-name order,
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
| `Compare { repo: r, from: feature, into: main }` | resolved endpoints, merge base, ahead/behind, mergeability |
| `Activity { repo: r }` | `last_height`: last successful forge operation in the repo |
| `Changes { repo: r, filter: ChangeFilter { state: Some(ChangeState::Open), ..Default::default() }, cursor: None, limit: 20 }` | `page<ChangeSummary>` with author, endpoints, counts and historical verdict totals |
| `Change { repo: r, n: 1, cursor: None, limit: 20 }` | full change record/body/channel, optional current source/target heads, `reviews: PageResponse<Review>` |
| `Judgment { principal: reviewer, cursor: None, limit: 20 }` | `page<Judgment>`: open changes requesting this account at the current head or containing an answered thread it authored |
| `Advertise { repo: r, service: Service::UploadPack }` | raw Git protocol v2 advertisement |
| `Upload { repo: r, request: git_v2_ls_refs_bytes }` | raw Git protocol v2 `ls-refs` response |
| `Closure { repo: r, walk: Walk::of(gitcore::walk::Walk::of(&wants, &haves)) }` | `listed`: the ids and kinds of the objects `wants` reach less what `haves` reach, and `next`, the walk to send back for the next page |
| `Protected { repo: r, cursor: None, limit: 20 }` | `page<Vec<u8>>`: the protected branch names in byte order; the head is protected besides |

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

`Blob` returns `Text`, `Binary` (NUL or invalid UTF-8), or `Oversize` (total size
above `Bounds.blob_bytes`). Binary/oversize replies have a header and no content
bytes, even when a small range was requested. Text ranges use byte offsets,
may split a UTF-8 code point, and clamp their end at EOF. Zero-length text is valid.

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
that holds no account are refused. Any account may open/review. The author edits title/body/review requests,
or whoever the change's list at `forge::change_key(repo, n)` in the access
program allows, and so re-points it (`ChangeRepoint`); a `Rebase` needs the
change's key and the repository's; the author, or a repository writer, closes; repository writers merge.
A repository's writers are whoever its list in the access program allows, at
`forge::repo_key(repo)`: anyone unless the owner sets a list there
(`access::Op::Set`). `Push`, `Merge`, `SquashMerge`, `Stage`, `DeleteBranch` and
a `ChangeClose` by anyone but the change's author need that key; `Configure` and
`Protect` need `forge::settings_key(repo)`, which follows the same list unless the
owner sets one of its own.
Reviews remain appendable on closed/merged changes.
Closing is terminal; there is no reopen operation.

| Operation example | Effect/output |
| --- | --- |
| `Create { repo: r, hash: HashKind::Sha1 }` | actor owns a new repo; empty output |
| `Configure { repo: r, settings: Settings::default() }` | owner configures default ref and force/delete policy; empty output |
| `Grant { repo: r, principal: writer }`, `Revoke { .. }` | always refused: a repository's writers are its list in the access program |
| `Push { repo: r, request: receive_pack_commands, pack: Some(pack) }` | Git receive-pack: the ref commands in `request`, the objects in the held `pack`; Git's report |
| `Stage { repo: r, pack }` | stores the held pack's objects and moves no ref; empty output |
| `ChangeOpen { repo: r, from: feature, into: b"refs/heads/main".to_vec(), title: "Fix parser".into(), body: "Why this changes".into(), reviewers: vec![reviewer] }` | assigns `n`, stores body, queues channel/opened line; `OpReply::Change { height, n }` |
| `ChangeEdit { repo: r, n: 1, title: None, body: Some("Updated rationale".into()), reviewers: None }` | `None` leaves a field unchanged; `Some(vec![])` clears requests; `OpReply::Change` |
| `ChangeClose { repo: r, n: 1 }` | open to closed, queues system line; `OpReply::Change` |
| `ReviewSubmit { repo: r, n: 1, review: ReviewDraft { commit_oid: b, base_oid: Some(a), verdict: Verdict::RequestChanges, body: "Please check this".into(), comments: vec![LineComment { path: b"src/lib.rs".to_vec(), side: Side::New, line: 12, body: "Check this bound".into() }] } }` | one immutable review and all comments, one chat root; `OpReply::Review { height, n, id }` |
| `Merge { repo: r, into: b"refs/heads/main".to_vec(), from: feature, expected_into: a, expected_from: b, result: client_result_oid, change: Some(1) }` | both-head CAS and open to merged together; `OpReply::Merged { height, oid, change }` |
| `SquashMerge { repo: r, into: b"refs/heads/main".to_vec(), from: feature, expected_into: a, expected_from: b, message: None, change: Some(1) }` | one squash commit built in consensus on `a`, the same CAS, open to merged together; `OpReply::Merged { height, oid, change }` |
| `Rebase { repo: r, n: 1, onto: main, expected_head: b }` | one squash commit of the change on `onto`, and the change's source moved to it under a CAS on `b`; `OpReply::Rebased { height, n, oid }` |
| `ChangeRepoint { repo: r, n: 1, from: feature }` | the change's source becomes `from`; `OpReply::Change` |
| `DeleteBranch { repo: r, branch: b"refs/heads/old".to_vec(), expected: a }` | deletes an unprotected branch at `a` that no open change names; empty output |
| `Protect { repo: r, branch: b"refs/heads/dev".to_vec(), protected: true }` | adds (`false`: removes) a protected branch; empty output |

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

Review OIDs and anchors receive syntactic validation, not object reads in
consensus. Paths must be relative, lines positive, bodies nonblank, and anchors
distinct within a batch. Old-side comments require a base. Empty approve/request
verdicts are valid; a comment verdict needs body text or line comments. Clients
validate positions against the displayed endpoint bytes, stage one draft per
anchor, and preserve drafts on failed submission.

## Bounds, refusals, and the current host seam

New bounds are `page_size`, `log_walk`, `tree_walk`, `diff_bytes`, `blob_bytes`,
and `record_bytes`. Defaults live in qa's `forge_smoke::default_bounds()` (which also writes a founding's `forge.params`); founding
chooses deployment values. `log_walk` bounds a complete DAG walk and the total
review lookup count in one judgment query. A log beyond that complete-walk bound
refuses rather than inventing partial ancestry; each page repeats the bounded
walk. `tree_walk` bounds object reads in tree/blob/diff queries; log and compare
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
