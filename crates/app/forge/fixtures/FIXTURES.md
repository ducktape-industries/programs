# Forge replay fixtures

`replies.bin` is every reply back to back; `replies.idx` says, one line per
shape, `name offset len request-hex sha256` — the request is the borsh `Query`
or `Op` that produced the bytes. `loader.rs` reads both and is shared by
`#[path]` with forge-view's tests. The bytes are what the program answered over
`program::MockHost` in `tests/fixtures.rs`, which is what `forge.wasm` answers over
the host: same code, same bytes (checked once against the wasm-on-runtime
harness captures when the generator moved here). Three shapes carry git's own
framing, not borsh; a refusal is the borsh `abi::Refusal` (`program::Error`, the same bytes) the program answered
`Err` with, as the host hands it to a view.

Regenerate with `FORGE_REGENERATE_FIXTURES=1 cargo test -p forge --test
fixtures`; without the variable the test must reproduce the committed bytes.

| Shape | Request → bytes | What it shows |
| --- | --- | --- |
| `repos-empty` | `Query` → `Reply` | Empty founded program before Create. |
| `refs-empty` | `Query` → `Reply` | Unborn repository with no refs. |
| `log-unborn` | `Query` → `Refusal` | not_found for an unborn branch. |
| `changes-empty` | `Query` → `Reply` | Empty list before any change exists. |
| `judgment-empty` | `Query` → `Reply` | No outstanding work. |
| `repos` | `Query` → `Reply` | Repository list with activity/ref count. |
| `repo` | `Query` → `Reply` | Settings, owner, bounds, counts and granted writer page. |
| `refs` | `Query` → `Reply` | First two refs with continuation. |
| `refs-next` | `Query` → `Reply` | The rest after `refs`' cursor (feature, the default head main, unrelated, the tag v1), no continuation. |
| `log` | `Query` → `Reply` | First history page, full message/signatures/parents and `next`. |
| `log-next` | `Query` → `Reply` | Root commit on the final history page. |
| `tree` | `Query` → `Reply` | First root directory page with continuation. |
| `tree-next` | `Query` → `Reply` | The rest of the root directory, after the first page's cursor. |
| `tree-directory` | `Query` → `Reply` | Lazy child-directory page. |
| `blob` | `Query` → `Reply` | Complete UTF-8 text bytes, including an unterminated last line. |
| `blob-binary` | `Query` → `Reply` | Binary header with no bytes. |
| `blob-oversize` | `Query` → `Reply` | Oversize header with no bytes (inline bound is 64 bytes). |
| `blob-empty` | `Query` → `Reply` | Zero-length text blob. |
| `blob-range` | `Query` → `Reply` | Byte range crossing a newline. |
| `diff` | `Query` → `Reply` | First page of changed file headers/hunks. |
| `diff-next` | `Query` → `Reply` | Next page, including binary and oversize headers. |
| `diff-text` | `Query` → `Reply` | Typed context/deleted/added lines, literal ++ x, and no final newline. |
| `diff-mode` | `Query` → `Reply` | Executable-bit change with no text edits. |
| `diff-binary` | `Query` → `Reply` | Binary file header. |
| `diff-oversize` | `Query` → `Reply` | Oversize file header. |
| `diff-gitlink` | `Query` → `Reply` | Gitlink entry without a blob read. |
| `diff-deleted` | `Query` → `Reply` | Deleted text file and old-side lines. |
| `diff-added` | `Query` → `Reply` | Added text file. |
| `diff-root` | `Query` → `Reply` | Root commit against the empty tree, including zero-count old range. |
| `diff-empty` | `Query` → `Reply` | Identical endpoints produce no files. |
| `compare` | `Query` → `Reply` | Fast-forward comparison. |
| `compare-up-to-date` | `Query` → `Reply` | Source already contained in target. |
| `compare-diverged` | `Query` → `Reply` | Divergent histories: ahead/behind and the base, no merge attempted. |
| `compare-unrelated` | `Query` → `Reply` | Unrelated histories. |
| `activity` | `Query` → `Reply` | Repository last activity and answering height. |
| `advertise-receive` | `Query` → `git smart HTTP` | Raw receive-pack advertisement for a populated repository. |
| `advertise-upload` | `Query` → `git smart HTTP` | Raw upload-pack v2 capabilities. |
| `upload-refs` | `Query` → `git smart HTTP` | Raw v2 ls-refs response, including HEAD. |
| `refused-object-not-held` | `Query` → `Refusal` | Serving node cannot return the requested object (not_found). |
| `refused-not-found` | `Query` → `Refusal` | Repository does not exist. |
| `refused-invalid-input` | `Query` → `Refusal` | A page cursor that does not decode. |
| `refs-before-update` | `Query` → `Reply` | One-ref page whose cursor is later invalidated. |
| `refused-stale` | `Query` → `Refusal` | Continuation from an older answering height. |
| `refused-other-listing` | `Query` → `Refusal` | A cursor used on a listing it does not belong to (stale). |
| `op-change-open` | `Op` → `OpReply` | Assigned first item number. |
| `change` | `Query` → `Reply` | Open record with body, channel, current endpoints and empty review page. |
| `changes` | `Query` → `Reply` | Change summary list. |
| `judgment` | `Query` → `Reply` | Outstanding requested review. |
| `op-change-edit` | `Op` → `OpReply` | Change edit receipt. |
| `op-review-comment` | `Op` → `OpReply` | Batched comment review receipt. |
| `op-review-request-changes` | `Op` → `OpReply` | Batched request-changes review receipt. |
| `op-review-approve` | `Op` → `OpReply` | Batched approval receipt. |
| `change-reviewed` | `Query` → `Reply` | First two submitted reviews with a continuation. |
| `change-reviews-next` | `Query` → `Reply` | Final review page, including an approval. |
| `judgment-replies` | `Query` → `Reply` | A reply under a submitted review root. |
| `change-outdated` | `Query` → `Reply` | Pinned reviews plus a moved source head. |
| `judgment-head-moved` | `Query` → `Reply` | Requested review becomes pending again after a push. |
| `op-change-open-second` | `Op` → `OpReply` | The next shared item number. |
| `op-change-close` | `Op` → `OpReply` | Change close receipt. |
| `change-closed` | `Query` → `Reply` | Closed record. |
| `changes-filtered` | `Query` → `Reply` | Closed-state filter. |
| `op-merge` | `Op` → `OpReply` | CAS merge receipt linked to a change. |
| `change-merged` | `Query` → `Reply` | Merged record and result OID. |
| `judgment-conversation` | `Query` → `Reply` | Chat-authored thread with a reply; no forge review ID. |
| `refused-capacity` | `Query` → `Refusal` | Complete commit walk exceeds the founded bound. |
| `unarchived` | `Query` → `Reply` | `Archived` of a repository that is not archived. |
| `archived` | `Query` → `Reply` | `Archived` once `Archive` archived it. |
| `archived-repos` | `Query` → `Reply` | `ArchivedRepos`: the archived repository. |
| `repos-archived` | `Query` → `Reply` | `Repos` with the archived repository left out. |
| `preview-none` | `Query` → `Reply` | `Preview` of an open change nobody has previewed: null. |
| `op-preview` | `Op` → `OpReply` | `Previewed`: the squash change 1 would land, under `refs/changes/1/merge`. |
| `preview` | `Query` → `Reply` | The change's latest Preview: the heads it read and its squash. |
| `statuses` | `Query` → `Reply` | The change head's statuses: passed with a url, pending without, by two reporters. |
| `statuses-merge` | `Query` → `Reply` | The Preview squash's statuses: failed with a url, errored without. |
