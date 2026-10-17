//! Every sentence, status word and problem text both designs say, once:
//! the desktop's screens (`ui`) and the phone's (`ui::phone`, the Expo
//! app's forge, whose screens say the same words) read them from here, so
//! the two never word one thing two ways. A word only one design says
//! stays in that design's file; a one-word label (a tab, a field, a verb on
//! a button) stays where it is drawn. A form's problem (a bad repository
//! name, a change with no title) is the model's: it lives in `actions.rs`,
//! where the form is checked, and both designs show it as it is.
use forge::{
    ChangeState, Comparison, Content, DiffHunk, FileDiff, FileStatus, Mergeability, Verdict,
};
use view::design;

use crate::select::path_of;
use crate::state::ReviewRefusal;

// ---------------------------------------------------------------- names

/// A path's bytes as something a screen can say out loud.
pub(crate) fn path_text(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

/// A ref's full name shortened to what a reader calls it.
pub(crate) fn ref_label(name: &[u8]) -> String {
    let text = path_text(name);
    text.strip_prefix("refs/heads/")
        .or_else(|| text.strip_prefix("refs/tags/"))
        .unwrap_or(&text)
        .to_owned()
}

/// `block 1,024`: where something landed.
pub(crate) fn block(height: u64) -> String {
    format!("block {}", design::grouped(height))
}

/// `12 bytes`.
pub(crate) fn bytes(size: u64) -> String {
    format!("{size} bytes")
}

/// A change's source and target by their short names: `feature → main`.
pub(crate) fn endpoints(from: &[u8], into: &[u8]) -> String {
    format!("{} → {}", ref_label(from), ref_label(into))
}

/// `# forge:project:1`: a chat room by its id.
pub(crate) fn channel(id: &str) -> String {
    format!("# {id}")
}

/// What object format a repository hashes with: `sha1`, `sha256`.
pub(crate) fn hash_kind(hash: abi::HashKind) -> &'static str {
    match hash {
        abi::HashKind::Sha1 => "sha1",
        abi::HashKind::Sha256 => "sha256",
    }
}

/// A change's writes (Edit, Close, Copy link), a mix: its title row draws
/// them behind `ellipsis-vertical`, and the control and its sheet are
/// named Actions, not the More of the capsule beside them.
pub(crate) const ACTIONS: &str = "Actions";

// ---------------------------------------------------------------- reading

pub(crate) const READING: &str = "Reading…";
pub(crate) const READING_REPOSITORIES: &str = "Reading the repositories…";
pub(crate) const READING_REPOSITORY: &str = "Reading this repository…";
pub(crate) const RESOLVING_REF: &str = "Resolving the ref…";
pub(crate) const READING_TREE: &str = "Reading the tree…";
pub(crate) const READING_README: &str = "Reading the README…";
pub(crate) const READING_FILE: &str = "Reading the file…";
pub(crate) const READING_HISTORY: &str = "Reading the history…";
pub(crate) const READING_REFS: &str = "Reading refs…";
pub(crate) const READING_CHANGES: &str = "Reading the changes…";
pub(crate) const READING_CHANGE: &str = "Reading this change…";
pub(crate) const READING_CONVERSATION: &str = "Reading the conversation…";
pub(crate) const READING_DIFF: &str = "Reading the diff…";
pub(crate) const READING_ROSTER: &str = "Reading the roster…";
pub(crate) const COMPARING_ENDPOINTS: &str = "Comparing the endpoints…";
/// A comparison not read yet, where its word goes.
pub(crate) const COMPARING: &str = "comparing…";

/// What a write on its way says (Expo `src/write/words.ts` `PROGRESS`);
/// one that did not land is the host's answer, worded by
/// `design::failure`.
pub(crate) const SUBMITTING: &str = "Submitting…";
pub(crate) const WAITING_FOR_BLOCK: &str = "Waiting for the next block…";
/// A reply in a change's room, while it is on its way (Expo
/// `write/optimistic.ts` `reply`).
pub(crate) const REPLYING: &str = "Replying…";

// ---------------------------------------------------------------- repositories

pub(crate) const NO_REPOSITORIES_YET: &str = "No repositories yet";
pub(crate) const CREATE_ONE: &str = "Create one with New repository, then push to it.";
pub(crate) const NOTHING_MATCHES: &str = "Nothing matches";
pub(crate) const NO_REPOSITORY_MATCHES: &str = "No repository here reads like that.";
pub(crate) const FILTER_BY_NAME: &str = "Filter by name";
pub(crate) const NEW_REPOSITORY: &str = "New repository";

/// What a repository is, under its name on the phone's root and its page:
/// `owner Ada · 3 refs · active block 1024`, the height as it is.
pub(crate) fn repo_facts(owner: &str, refs: u64, active: u64) -> String {
    format!(
        "owner {owner} · {} · active block {active}",
        design::plural(refs, "ref", "refs")
    )
}
pub(crate) const REPOSITORY_NAME: &str = "Repository name";
/// What a repository's name may hold, under its field.
pub(crate) const NAME_RULE: &str = "letters, digits, dot, dash, underscore";
pub(crate) const OBJECT_FORMAT: &str = "Object format";
pub(crate) const FORMAT_RULE: &str =
    "Pick SHA-1 to push an existing Git project. The format is fixed once created.";

// ---------------------------------------------------------------- a repository

pub(crate) const PICK_A_REF: &str = "Pick a ref";
/// The ref picker's label while the repository has none.
pub(crate) const NO_REFS_YET: &str = "no refs";
pub(crate) const BRANCHES_OR_TAGS: &str = "Branches or tags";
pub(crate) const DEFAULT_HEAD: &str = "Default head";
pub(crate) const DEFAULT_HEAD_RULE: &str =
    "What a clone checks out and where changes point by default.";
pub(crate) const PAGE_SIZE: &str = "Page size";
pub(crate) const INLINE_BLOB_BOUND: &str = "Inline blob bound";
pub(crate) const ONLY_THE_OWNER: &str = "Only the owner writes here.";
pub(crate) const NO_COMMITS_YET: &str = "No commits yet";
pub(crate) const NO_README: &str = "No README";
pub(crate) const README_NOT_TEXT: &str = "This README is not text.";

// Code
pub(crate) const FILTER_FILES: &str = "Filter files";
pub(crate) const NOTHING_HERE: &str = "Nothing here";
pub(crate) const NO_FILE_KEPT: &str = "This tree holds no file the filter keeps.";
pub(crate) const BINARY_FILE: &str = "Binary file";
pub(crate) const TOO_LARGE_TO_SHOW: &str = "Too large to show";
pub(crate) const SUBMODULE_POINTS_AWAY: &str = "This entry points at another repository.";

/// A path a link named that the browsed ref does not hold.
pub(crate) fn no_path(path: &[u8]) -> String {
    format!("No {} on this ref.", path_text(path))
}
pub(crate) const NOT_ON_THIS_REF: &str = "The Code tab has this ref's files.";

/// A binary blob's body: its size, and that it is not text.
pub(crate) fn not_text(size: u64) -> String {
    format!("{size} bytes. Nothing here reads as text.")
}

/// An oversize blob's body: its size, and where to read it.
pub(crate) fn too_large(size: u64) -> String {
    format!(
        "{size} bytes, above this network's inline blob bound. Fetch the repository to read it."
    )
}

// Commits
pub(crate) const NO_COMMITS: &str = "No commits";
pub(crate) const NO_HISTORY: &str = "This ref carries no history yet.";
pub(crate) const ROOT_COMMIT: &str = "root commit";

// Refs
pub(crate) const NO_REFS: &str = "No refs";
pub(crate) const UNBORN: &str = "This repository is unborn: nothing has been pushed to it yet.";
/// The ref the repository's tabs read, in its row.
pub(crate) const BROWSING: &str = "browsing";

/// What the repository's settings forbid a push, if anything:
/// `This repository forbids force pushes.`
pub(crate) fn forbids(allow_force: bool, allow_delete: bool) -> Option<String> {
    let what = match (allow_force, allow_delete) {
        (false, false) => "force pushes and ref deletions",
        (false, true) => "force pushes",
        (true, false) => "ref deletions",
        (true, true) => return None,
    };
    Some(format!("This repository forbids {what}."))
}

/// How a branch compares with the head, in its row and the change form:
/// `2 ahead · 0 behind · fast-forward`.
pub(crate) fn compared(comparison: &Comparison) -> String {
    let word = match comparison.mergeability {
        Mergeability::UpToDate => "merged",
        Mergeability::FastForward => "fast-forward",
        Mergeability::Diverged => "diverged",
        Mergeability::Unrelated => "unrelated",
    };
    format!("{} · {word}", distance(comparison))
}

/// `2 ahead · 0 behind`.
pub(crate) fn distance(comparison: &Comparison) -> String {
    format!("{} ahead · {} behind", comparison.ahead, comparison.behind)
}

// Settings
pub(crate) const ALLOW_FORCE_PUSHES: &str = "Allow force pushes";
pub(crate) const ALLOW_FORCE_PUSHES_RULE: &str = "Let writers rewrite a branch's history.";
pub(crate) const ALLOW_REF_DELETION: &str = "Allow ref deletion";
pub(crate) const ALLOW_REF_DELETION_RULE: &str = "Let writers delete branches and tags.";
pub(crate) const CAN_WRITE: &str = "Can write";
pub(crate) const GRANT_WRITE_ACCESS: &str = "Grant write access";
pub(crate) const PICK_A_WRITER: &str = "Pick a member. They can push and merge here.";
pub(crate) const SEARCH_MEMBERS: &str = "Search members";

// ---------------------------------------------------------------- changes

pub(crate) const SEARCH_TITLES: &str = "Search titles";
pub(crate) const NO_ACCOUNT: &str = "No account";
pub(crate) const CREATE_AN_ACCOUNT: &str = "Create an account to see the changes that involve you.";
pub(crate) const NOTHING_WAITS: &str = "Nothing waits on you";
pub(crate) const NOTHING_ASKED: &str =
    "No review is requested of you and no thread of yours has an answer.";
pub(crate) const NO_CHANGES_HERE: &str = "No changes here";
pub(crate) const COMPARE_TO_OPEN: &str = "Compare a branch from the Refs tab to open one.";
/// What a change asks of me, in its row.
pub(crate) const REVIEW_REQUESTED: &str = "review requested";
pub(crate) const NEW_REPLY: &str = "new reply";

/// `3 comments`.
pub(crate) fn comments(count: u64) -> String {
    design::plural(count, "comment", "comments")
}

/// A change's state, in its badge.
pub(crate) fn change_state(state: ChangeState) -> &'static str {
    match state {
        ChangeState::Open => "open",
        ChangeState::Merged => "merged",
        ChangeState::Closed => "closed",
    }
}

// The change form
pub(crate) const NEW_CHANGE: &str = "New change";

/// Why a review was refused before it was sent, as the desktop says it
/// (the phone says Expo's own words, `ui::phone::review`).
pub(crate) fn review_refusal(refusal: ReviewRefusal) -> &'static str {
    match refusal {
        ReviewRefusal::EmptyComment => "A comment review needs a body or a line comment",
    }
}
pub(crate) const TITLE_PROMPT: &str = "What this change does";
pub(crate) const BODY_PROMPT: &str = "Why it changes";

/// The change form's title when it edits change `n`.
pub(crate) fn edit_change(n: u64) -> String {
    format!("Edit change #{n}")
}

// ---------------------------------------------------------------- a change

pub(crate) const CLOSE_FOR_GOOD: &str = "Close for good";
pub(crate) const CLOSING_IS_FINAL: &str = "Closing is final: a closed change cannot be reopened.";
pub(crate) const REPLY_IN_CHANGE: &str = "Reply in this change";
pub(crate) const OPENED_IT: &str = "opened this change";
pub(crate) const CLOSED_IT: &str = "closed this change";

/// What merging did, after who did it.
pub(crate) fn merged_into(target: &[u8], oid: &str) -> String {
    format!(
        "merged into {} as {}",
        ref_label(target),
        design::short_hex(oid)
    )
}

// Details
pub(crate) const MERGE_STATUS: &str = "Merge status";
pub(crate) const SOURCE_HEAD: &str = "Source head";
pub(crate) const TARGET_HEAD: &str = "Target head";
/// A head its branch no longer has.
pub(crate) const GONE: &str = "gone";
pub(crate) const LINE_COMMENTS: &str = "Line comments";
pub(crate) const NOBODY_REVIEWED: &str = "Nobody has reviewed this change yet.";
pub(crate) const NOBODY_ON_A_LINE: &str = "Nobody has written on a line of this change yet.";
/// Reviews only advise: the merge is a compare-and-set on both heads.
pub(crate) const ADVISORY: &str = "advisory";
/// A reviewer asked who has not answered.
pub(crate) const ASKED: &str = "asked";
/// A comment the chain moved past its line.
pub(crate) const OUTDATED: &str = "outdated";

/// How a change can merge, in its details.
pub(crate) fn merge_status(mergeability: Mergeability) -> &'static str {
    match mergeability {
        Mergeability::UpToDate => "already contained",
        Mergeability::FastForward => "fast-forward",
        Mergeability::Diverged => "diverged",
        Mergeability::Unrelated => "unrelated histories",
    }
}

/// Where a reviewer stands, by their latest verdict.
pub(crate) fn standing(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "approved",
        Verdict::RequestChanges => "changes",
        Verdict::Comment => "commented",
    }
}

/// A comment of mine not sent yet, where it sits: `src/lib.rs:3 · pending`.
pub(crate) fn pending_at(path: &[u8], line: u64) -> String {
    format!("{}:{line} · pending", path_text(path))
}

/// A line comment quoted under its review: `src/lib.rs:3 — trim first`.
pub(crate) fn line_comment(path: &[u8], line: u64, body: &str) -> String {
    format!("{}:{line} — {body}", path_text(path))
}

// Files and the diff
pub(crate) const ALL_FILES: &str = "All files";
pub(crate) const TICK_A_FILE: &str = "Tick a file once you have read it.";
pub(crate) const NO_CHANGES: &str = "No changes";
pub(crate) const SAME_TREE: &str = "These endpoints hold the same tree.";

/// How many files a diff touches and its lines: `3 files · +12 −4`.
pub(crate) fn files_changed(files: u64, added: u64, deleted: u64) -> String {
    format!(
        "{} · +{added} −{deleted}",
        design::plural(files, "file", "files")
    )
}

/// A diff file's path, `old → new` once renamed (Expo `diff.ts` `paint`'s
/// `header`).
pub(crate) fn file_path(file: &FileDiff) -> String {
    match (&file.old_path, &file.new_path) {
        (Some(old), Some(new)) if old != new => format!("{} → {}", path_text(old), path_text(new)),
        _ => path_text(&path_of(file).unwrap_or_default()),
    }
}

/// A hunk's head: `@@ -1,4 +1,3 @@`.
pub(crate) fn hunk(hunk: &DiffHunk) -> String {
    format!(
        "@@ -{},{} +{},{} @@",
        hunk.old.start, hunk.old.count, hunk.new.start, hunk.new.count
    )
}

/// What a diff did to a file (Expo `src/forge/diff.ts` `statusLabel`).
pub(crate) fn file_status(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Deleted => "deleted",
        FileStatus::Modified => "modified",
        FileStatus::ModeChanged => "mode changed",
        FileStatus::TypeChanged => "type changed",
    }
}

/// Why a file of a diff has no lines to show.
pub(crate) fn why_no_lines(file: &FileDiff) -> &'static str {
    match file.content {
        Content::Binary => "Binary file — no lines to show",
        Content::Oversize => "Too large to diff inline",
        Content::Gitlink => "Submodule pointer",
        Content::Text => "No line changes",
    }
}

// ---------------------------------------------------------------- a review

pub(crate) const START_REVIEW: &str = "Start review";
pub(crate) const FINISH_YOUR_REVIEW: &str = "Finish your review";
pub(crate) const SUBMIT_REVIEW: &str = "Submit review";
pub(crate) const REVIEW_BODY: &str = "Review body";
pub(crate) const REVIEW_PROMPT: &str = "What this review says overall";
pub(crate) const COMMENT_PROMPT: &str = "What should change here?";
pub(crate) const YOUR_PENDING_COMMENT: &str = "Your pending comment";

/// The verdicts a review picks from, in the order both designs list them:
/// each with its slug (the element id's tail) and what it means.
pub(crate) const VERDICTS: [(Verdict, &str, &str); 3] = [
    (Verdict::Comment, "comment", "Feedback without a verdict."),
    (Verdict::Approve, "approve", "Ready to merge as it is."),
    (
        Verdict::RequestChanges,
        "request-changes",
        "Needs work before it merges.",
    ),
];
