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

pub(crate) fn path_text(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

pub(crate) fn ref_label(name: &[u8]) -> String {
    let text = path_text(name);
    text.strip_prefix("refs/heads/")
        .or_else(|| text.strip_prefix("refs/tags/"))
        .unwrap_or(&text)
        .to_owned()
}

pub(crate) fn block(height: u64) -> String {
    format!("block {}", design::grouped(height))
}

pub(crate) fn bytes(size: u64) -> String {
    format!("{size} bytes")
}

pub(crate) fn endpoints(from: &[u8], into: &[u8]) -> String {
    format!("{} → {}", ref_label(from), ref_label(into))
}

pub(crate) fn channel(id: &str) -> String {
    format!("# {id}")
}

pub(crate) fn talked_in(room: &str) -> String {
    format!("This change talked in # {room}.")
}

pub(crate) fn hash_kind(hash: abi::HashKind) -> &'static str {
    match hash {
        abi::HashKind::Sha1 => "sha1",
        abi::HashKind::Sha256 => "sha256",
    }
}

pub(crate) const ACTIONS: &str = "Actions";

pub(crate) const ARCHIVED: &str = "Archived";

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
pub(crate) const COMPARING: &str = "comparing…";

pub(crate) const SUBMITTING: &str = "Submitting…";
pub(crate) const WAITING_FOR_BLOCK: &str = "Waiting for the next block…";
pub(crate) const REPLYING: &str = "Replying…";

pub(crate) const NO_REPOSITORIES_YET: &str = "No repositories yet";
pub(crate) const CREATE_ONE: &str = "Create one with New repository, then push to it.";
pub(crate) const NOTHING_MATCHES: &str = "Nothing matches";
pub(crate) const NO_REPOSITORY_MATCHES: &str = "No repository here reads like that.";
pub(crate) const FILTER_BY_NAME: &str = "Filter by name";
pub(crate) const NEW_REPOSITORY: &str = "New repository";

pub(crate) fn repo_facts(owner: &str, refs: u64, active: u64) -> String {
    format!(
        "owner {owner} · {} · active block {active}",
        design::plural(refs, "ref", "refs")
    )
}
pub(crate) const REPOSITORY_NAME: &str = "Repository name";
pub(crate) const NAME_RULE: &str = "letters, digits, dot, dash, underscore";
pub(crate) const OBJECT_FORMAT: &str = "Object format";
pub(crate) const FORMAT_RULE: &str =
    "Pick SHA-1 to push an existing Git project. The format is fixed once created.";

pub(crate) const PICK_A_REF: &str = "Pick a ref";
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

pub(crate) const FILTER_FILES: &str = "Filter files";
pub(crate) const NOTHING_HERE: &str = "Nothing here";
pub(crate) const NO_FILE_KEPT: &str = "This tree holds no file the filter keeps.";
pub(crate) const BINARY_FILE: &str = "Binary file";
pub(crate) const TOO_LARGE_TO_SHOW: &str = "Too large to show";
pub(crate) const SUBMODULE_POINTS_AWAY: &str = "This entry points at another repository.";

pub(crate) fn no_path(path: &[u8]) -> String {
    format!("No {} on this ref.", path_text(path))
}
pub(crate) const NOT_ON_THIS_REF: &str = "The Code tab has this ref's files.";

pub(crate) fn ref_gone(name: &str) -> String {
    format!("Branch {name} doesn’t exist; it may have merged.")
}
pub(crate) const GONE_DETAIL: &str = "The same path may be on the default branch.";

pub(crate) fn open_on(name: &str) -> String {
    format!("Open on {name}")
}

pub(crate) fn not_text(size: u64) -> String {
    format!("{size} bytes. Nothing here reads as text.")
}

pub(crate) fn too_large(size: u64) -> String {
    format!(
        "{size} bytes, above this network's inline blob bound. Fetch the repository to read it."
    )
}

pub(crate) const NO_COMMITS: &str = "No commits";
pub(crate) const NO_HISTORY: &str = "This ref carries no history yet.";
pub(crate) const ROOT_COMMIT: &str = "root commit";

pub(crate) const NO_REFS: &str = "No refs";
pub(crate) const UNBORN: &str = "This repository is unborn: nothing has been pushed to it yet.";
pub(crate) const BROWSING: &str = "browsing";

pub(crate) fn forbids(allow_force: bool, allow_delete: bool) -> Option<String> {
    let what = match (allow_force, allow_delete) {
        (false, false) => "force pushes and ref deletions",
        (false, true) => "force pushes",
        (true, false) => "ref deletions",
        (true, true) => return None,
    };
    Some(format!("This repository forbids {what}."))
}

pub(crate) fn compared(comparison: &Comparison) -> String {
    let word = match comparison.mergeability {
        Mergeability::UpToDate => "merged",
        Mergeability::FastForward => "fast-forward",
        Mergeability::Diverged => "diverged",
        Mergeability::Unrelated => "unrelated",
    };
    format!("{} · {word}", distance(comparison))
}

pub(crate) fn distance(comparison: &Comparison) -> String {
    format!("{} ahead · {} behind", comparison.ahead, comparison.behind)
}

pub(crate) const ALLOW_FORCE_PUSHES: &str = "Allow force pushes";
pub(crate) const ALLOW_FORCE_PUSHES_RULE: &str = "Let writers rewrite a branch's history.";
pub(crate) const ALLOW_REF_DELETION: &str = "Allow ref deletion";
pub(crate) const ALLOW_REF_DELETION_RULE: &str = "Let writers delete branches and tags.";
pub(crate) const CAN_WRITE: &str = "Can write";
pub(crate) const EVERYONE: &str = "Everyone";
pub(crate) const READING_WRITERS: &str = "Reading who may write…";
pub(crate) const WRITES_ALREADY: &str = "They can write here already";
pub(crate) const GRANT_WRITE_ACCESS: &str = "Grant write access";
pub(crate) const PICK_A_WRITER: &str = "Pick a member. They can push and merge here.";
pub(crate) const SEARCH_MEMBERS: &str = "Search members";

pub(crate) const SEARCH_TITLES: &str = "Search titles";
pub(crate) const NO_ACCOUNT: &str = "No account";
pub(crate) const CREATE_AN_ACCOUNT: &str = "Create an account to see the changes that involve you.";
pub(crate) const NOTHING_WAITS: &str = "Nothing waits on you";
pub(crate) const NOTHING_ASKED: &str = "No change asks for your review.";
pub(crate) const NO_CHANGES_HERE: &str = "No changes here";
pub(crate) const COMPARE_TO_OPEN: &str = "Compare a branch from the Refs tab to open one.";
pub(crate) const REVIEW_REQUESTED: &str = "review requested";

pub(crate) fn comments(count: u64) -> String {
    design::plural(count, "comment", "comments")
}

pub(crate) fn change_state(state: ChangeState) -> &'static str {
    match state {
        ChangeState::Open => "open",
        ChangeState::Merged => "merged",
        ChangeState::Closed => "closed",
    }
}

pub(crate) const NEW_CHANGE: &str = "New change";

pub(crate) fn review_refusal(refusal: ReviewRefusal) -> &'static str {
    match refusal {
        ReviewRefusal::EmptyComment => "A comment review needs a body or a line comment",
    }
}
pub(crate) const TITLE_PROMPT: &str = "What this change does";
pub(crate) const BODY_PROMPT: &str = "Why it changes";

pub(crate) fn edit_change(n: u64) -> String {
    format!("Edit change #{n}")
}

pub(crate) const CLOSE_FOR_GOOD: &str = "Close for good";
pub(crate) const CLOSING_IS_FINAL: &str = "Closing is final: a closed change cannot be reopened.";
pub(crate) const REPLY_IN_CHANGE: &str = "Reply in this change";
pub(crate) const OPENED_IT: &str = "opened this change";
pub(crate) const CLOSED_IT: &str = "closed this change";

pub(crate) fn merged_into(target: &[u8], oid: &str) -> String {
    format!(
        "merged into {} as {}",
        ref_label(target),
        design::short_hex(oid)
    )
}

pub(crate) const MERGE_STATUS: &str = "Merge status";
pub(crate) const SOURCE_HEAD: &str = "Source head";
pub(crate) const TARGET_HEAD: &str = "Target head";
pub(crate) const GONE: &str = "gone";
pub(crate) const LINE_COMMENTS: &str = "Line comments";
pub(crate) const NOBODY_REVIEWED: &str = "Nobody has reviewed this change yet.";
pub(crate) const NOBODY_ON_A_LINE: &str = "Nobody has written on a line of this change yet.";
pub(crate) const ADVISORY: &str = "advisory";
pub(crate) const ASKED: &str = "asked";
pub(crate) const OUTDATED: &str = "outdated";

pub(crate) fn merge_status(mergeability: Mergeability) -> &'static str {
    match mergeability {
        Mergeability::UpToDate => "already contained",
        Mergeability::FastForward => "fast-forward",
        Mergeability::Diverged => "diverged",
        Mergeability::Unrelated => "unrelated histories",
    }
}

pub(crate) fn standing(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "approved",
        Verdict::RequestChanges => "changes",
        Verdict::Comment => "commented",
    }
}

pub(crate) fn pending_at(path: &[u8], line: u64) -> String {
    format!("{}:{line} · pending", path_text(path))
}

pub(crate) fn line_comment(path: &[u8], line: u64, body: &str) -> String {
    format!("{}:{line} — {body}", path_text(path))
}

pub(crate) const ALL_FILES: &str = "All files";
pub(crate) const TICK_A_FILE: &str = "Tick a file once you have read it.";
pub(crate) const NO_CHANGES: &str = "No changes";
pub(crate) const SAME_TREE: &str = "These endpoints hold the same tree.";

pub(crate) fn files_changed(files: u64, added: u64, deleted: u64) -> String {
    format!(
        "{} · +{added} −{deleted}",
        design::plural(files, "file", "files")
    )
}

pub(crate) fn file_path(file: &FileDiff) -> String {
    match (&file.old_path, &file.new_path) {
        (Some(old), Some(new)) if old != new => format!("{} → {}", path_text(old), path_text(new)),
        _ => path_text(&path_of(file).unwrap_or_default()),
    }
}

pub(crate) fn hunk(hunk: &DiffHunk) -> String {
    format!(
        "@@ -{},{} +{},{} @@",
        hunk.old.start, hunk.old.count, hunk.new.start, hunk.new.count
    )
}

pub(crate) fn file_status(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Deleted => "deleted",
        FileStatus::Modified => "modified",
        FileStatus::ModeChanged => "mode changed",
        FileStatus::TypeChanged => "type changed",
    }
}

pub(crate) fn why_no_lines(file: &FileDiff) -> &'static str {
    match file.content {
        Content::Binary => "Binary file — no lines to show",
        Content::Oversize => "Too large to diff inline",
        Content::Gitlink => "Submodule pointer",
        Content::Text => "No line changes",
    }
}

pub(crate) const START_REVIEW: &str = "Start review";
pub(crate) const FINISH_YOUR_REVIEW: &str = "Finish your review";
pub(crate) const SUBMIT_REVIEW: &str = "Submit review";
pub(crate) const REVIEW_BODY: &str = "Review body";
pub(crate) const REVIEW_PROMPT: &str = "What this review says overall";
pub(crate) const COMMENT_PROMPT: &str = "What should change here?";
pub(crate) const YOUR_PENDING_COMMENT: &str = "Your pending comment";

pub(crate) const VERDICTS: [(Verdict, &str, &str); 3] = [
    (Verdict::Comment, "comment", "Feedback without a verdict."),
    (Verdict::Approve, "approve", "Ready to merge as it is."),
    (
        Verdict::RequestChanges,
        "request-changes",
        "Needs work before it merges.",
    ),
];

pub(crate) const CHECKS: &str = "Checks";
pub(crate) const READING_CHECKS: &str = "Reading the checks…";
pub(crate) const NO_CHECKS: &str = "Nothing has reported on this commit yet.";

pub(crate) fn checked_on(commit: &str) -> String {
    format!("On the head {}", design::short_hex(commit))
}

pub(crate) fn reported(reporter: &str, context: &str) -> String {
    format!("{reporter} / {context}")
}
