//! What the screen is showing and what the reader has typed. Everything the
//! programs speak is folded to rows at render time, so a snapshot carries
//! navigation and drafts, never wire records.
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use view::Paged;
use view::prelude::*;

use forge::{CommitInfo, LineComment, Query, Reply, Revision, Side, Verdict};

#[derive(Default, Serialize, Deserialize)]
pub struct Forge {
    pub(crate) nav: Nav,
    pub(crate) session: Session,
    pub(crate) filter: Filter,
    /// the repo-list filter box
    pub(crate) search: TextField,
    /// the change-list search box, apart from the repo filter so a name
    /// typed to find a repository never hides that repository's changes
    pub(crate) change_search: TextField,
    /// the file-tree filter box
    pub(crate) tree_search: TextField,
    pub(crate) reviews: BTreeMap<String, ReviewSession>,
    /// `<repo>#<n>:<path>` of every file the reader ticked off
    pub(crate) viewed: BTreeSet<String>,
    pub(crate) new_repo: Option<NewRepo>,
    pub(crate) form: Option<ChangeForm>,
    pub(crate) repo_settings: Option<SettingsForm>,
    /// the conversation composer of the open change, multi-line
    pub(crate) reply: TextField,
    pub(crate) notice: String,
    /// the repository whose address was copied last, so its row says so
    #[serde(skip)]
    pub(crate) copied: Option<String>,
    /// the window's title last sent
    #[serde(skip)]
    pub(crate) title: Option<String>,
    /// the route last named to the host as the one shown (`host.showing`)
    #[serde(skip)]
    pub(crate) showing: Option<String>,
    /// the host badge last posted: 1 while a change waits on my judgment
    #[serde(skip)]
    pub(crate) badge: Option<i64>,
    pub(crate) layout: Layout,
    #[serde(skip)]
    /// every read on screen, keyed by the query that asked it
    pub(crate) data: BTreeMap<Query, Loadable<Rc<Reply>>>,
    /// every log on screen, keyed like `data`: read a page at a time, as
    /// far as its list is scrolled. Beside each, the view hearing it land.
    #[serde(skip)]
    pub(crate) logs: BTreeMap<Query, (Entity<Paged<CommitInfo>>, Subscription)>,
    #[serde(skip)]
    pub(crate) names: Loadable<chat::view::Names>,
    // each account's picture by its card's blob (identity's), re-read on
    // identity's heads, and the pictures read so far
    #[serde(skip)]
    pub(crate) avatars: BTreeMap<u64, abi::BlobId>,
    #[serde(skip)]
    pub(crate) pictures: Option<Entity<view::pictures::Pictures>>,
    #[serde(skip)]
    pub(crate) writers: BTreeMap<String, Loadable<Option<identity::Ownership>>>,
    #[serde(skip)]
    /// each change channel's rows
    pub(crate) messages: BTreeMap<String, Loadable<Vec<chat::MsgRow>>>,
    /// a refusal on screen, read again with the block: the read lives
    /// here because the slot holds the refusal, and a newer block's read
    /// stored in its place cancels it
    #[serde(skip)]
    pub(crate) rereading: BTreeMap<Query, Task<()>>,
    #[serde(skip)]
    pub(crate) rereading_messages: BTreeMap<String, Task<()>>,
    #[serde(skip)]
    pub(crate) pending: Vec<Pending>,
    /// the height of the last forge block heard: an op issued before it
    /// is carried by the time its receipt lands
    #[serde(skip)]
    pub(crate) block: u64,
    #[serde(skip)]
    pub(crate) diff_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) log_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) tree_scroll: UniformListScrollHandle,
    /// the Files tab's list, kept on the file picked and the row the
    /// arrows are on
    #[serde(skip)]
    pub(crate) files_scroll: UniformListScrollHandle,
    /// the Refs tab's list, kept on the row the arrows are on
    #[serde(skip)]
    pub(crate) refs_scroll: UniformListScrollHandle,
    /// the Changes tab's list, kept on the row the arrows are on
    #[serde(skip)]
    pub(crate) changes_scroll: UniformListScrollHandle,
    /// a commit's file strip, kept on the row the arrows are on
    #[serde(skip)]
    pub(crate) strip_scroll: UniformListScrollHandle,
    /// a file picked to show alone whose row the Files tab's list has
    /// not yet been scrolled to: its diff is on its way
    #[serde(skip)]
    pub(crate) reveal: Option<Vec<u8>>,
    #[serde(skip)]
    pub(crate) next_pending: u64,
    /// the change whose Close waits for a second press: closing is final
    #[serde(skip)]
    pub(crate) closing: Option<String>,
    /// drawn under a finger (`window.touch()`): the phone's screens,
    /// whose change strip counts every tab as Expo's does, so a change's
    /// conversation and diff are read whichever tab is open
    #[serde(skip)]
    pub(crate) touch: bool,
    /// the dropdown open under its button, if any
    #[serde(skip)]
    pub(crate) menu: Option<Menu>,
    /// the sheet open over the phone's page, if any (boards 21, 23)
    #[serde(skip)]
    pub(crate) sheet: Option<Sheet>,
    /// the phone's change page shows its Details tab (board 23)
    #[serde(skip)]
    pub(crate) details: bool,
    /// the phone's Refs tab lists the tags, not the branches
    #[serde(skip)]
    pub(crate) refs_tags: bool,
    /// the phone's file page wraps its lines
    #[serde(skip)]
    pub(crate) wrap: bool,
    /// the phone's file page shows a markdown file's source, not the
    /// document
    #[serde(skip)]
    pub(crate) source: bool,
    /// the list the phone's code and diff rows are drawn in
    #[serde(skip)]
    pub(crate) lines: crate::ui::phone::Block,
    /// the phone's people sheet's filter (reviewers, a writer to grant)
    #[serde(skip)]
    pub(crate) people_search: TextField,
    /// where the open dropdown sits: under the press that opened it
    #[serde(skip)]
    pub(crate) menu_at: (f32, f32),
    /// the dropdown item the arrows are on
    #[serde(skip)]
    pub(crate) menu_cursor: usize,
    /// the open dropdown's filter box, empty on every open
    #[serde(skip)]
    pub(crate) menu_search: TextField,
    /// the ref picker lists tags rather than branches
    #[serde(skip)]
    pub(crate) menu_tags: bool,
    /// the open dropdown's list, kept on the item the arrows are on
    #[serde(skip)]
    pub(crate) menu_scroll: UniformListScrollHandle,
    /// the repository the arrows are on in the list, and which of its
    /// cells: Open, Copy, activity
    #[serde(skip)]
    pub(crate) repos_cursor: Option<(String, usize)>,
    /// the reviewable diff line the arrows are on, and which of its
    /// gutters; none until they walk it, when the first gutter is claimed
    /// for the keys and none is drawn
    #[serde(skip)]
    pub(crate) diff_cursor: Option<(usize, usize)>,
    /// the repository the arrows are on in the rail
    #[serde(skip)]
    pub(crate) rail_cursor: Option<String>,
    /// the row the arrows are on in a commit, change or ref list, and
    /// which of its cells: the list's id, the row, the cell
    #[serde(skip)]
    pub(crate) list_cursor: Option<(&'static str, usize, usize)>,
    /// the arrows have walked the file tree since the last move: its
    /// cursor is drawn
    #[serde(skip)]
    pub(crate) tree_walked: bool,
    /// the repository tab the arrows are on but have not opened
    #[serde(skip)]
    pub(crate) tab_cursor: Option<RepoTab>,
    /// the change tab the arrows are on but have not opened
    #[serde(skip)]
    pub(crate) change_tab_cursor: Option<ChangeTab>,
    #[serde(skip)]
    pub(crate) blob_cache: crate::blob::BlobCache,
    #[serde(skip)]
    pub(crate) diff_cache: crate::ui::diff::DiffCache,
    // who can edit the open repository and the open change (`policies.rs`)
    #[serde(skip)]
    pub(crate) repo_policy: policy_view::Editing,
    #[serde(skip)]
    pub(crate) change_policy: policy_view::Editing,
}

/// Where the reader is. One flat record: every screen is a projection of it,
/// and a snapshot restores the same screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Nav {
    pub repo: Option<String>,
    pub tab: RepoTab,
    /// the picked ref, a full name such as `refs/heads/main`
    pub rev: Option<Vec<u8>>,
    /// the directories the code tree has open, by full path
    pub expanded: BTreeSet<Vec<u8>>,
    /// the tree row the keyboard is on (a file or a directory)
    pub cursor: Option<Vec<u8>>,
    /// the open file: its path and blob oid
    pub blob: Option<(Vec<u8>, String)>,
    /// the folder the phone's Code tab shows on a page of its own, by
    /// full path, each of its levels a page of the stack ([`Nav::pages`],
    /// Expo `code/[...path].tsx`); the desktop opens folders in place
    /// (`expanded`)
    pub folder: Option<Vec<u8>>,
    /// a file a link named, opened once its folder's tree lands
    #[serde(skip)]
    pub goto: Option<Vec<u8>>,
    /// a path a link named that the browsed ref does not hold: the
    /// phone's Code tab says so on a page of its own (Expo
    /// `code/[...path].tsx`), the desktop in its notice
    #[serde(skip)]
    pub missing: Option<Vec<u8>>,
    pub commit: Option<String>,
    pub change: Option<u64>,
    pub change_tab: ChangeTab,
    /// single-file mode in the change's diff
    pub diff_path: Option<Vec<u8>>,
    /// the phone's All files page: every file of the change's diff on
    /// one page (Expo `diff.tsx` without a path); the desktop's Files tab
    /// shows them all in place
    pub all_files: bool,
    pub dock: Option<Dock>,
}

impl Nav {
    pub fn revision(&self, head: &[u8]) -> Revision {
        Revision::Ref(self.rev.clone().unwrap_or_else(|| head.to_vec()))
    }

    /// The page stack this record projects to, root first: `Nav` is the one
    /// route model (every screen reads it, a snapshot restores it), so the
    /// stack is derived from it each render rather than kept beside it. The
    /// phone shows the top page alone and Back pops one level
    /// ([`Forge::back`](crate::Forge::back)); the desktop docks the same
    /// pages as panes.
    pub fn pages(&self) -> Vec<Page> {
        let mut pages = vec![Page::Repos];
        if self.repo.is_none() {
            return pages;
        }
        pages.push(Page::Repo);
        if self.change.is_some() {
            pages.push(Page::Change);
            match self.change_tab {
                ChangeTab::Files if self.diff_path.is_some() || self.all_files => {
                    pages.push(Page::File)
                }
                ChangeTab::Commits if self.commit.is_some() => pages.push(Page::Commit),
                _ => {}
            }
            return pages;
        }
        match self.tab {
            RepoTab::Code => {
                // a page per folder level, as Expo pushes a screen per
                // folder: `a/b` is `b`'s page over `a`'s, so Back walks
                // up one and a link into `a/b` lands with `a` under it
                let levels = self
                    .folder
                    .as_ref()
                    .map_or(0, |path| path.split(|byte| *byte == b'/').count());
                pages.extend(std::iter::repeat_n(Page::Folder, levels));
                if self.blob.is_some() {
                    pages.push(Page::Blob);
                } else if self.missing.is_some() {
                    pages.push(Page::Missing);
                } else if self.goto.is_some() {
                    pages.push(Page::Landing);
                }
            }
            RepoTab::Commits if self.commit.is_some() => pages.push(Page::Commit),
            _ => {}
        }
        pages
    }

    /// `0` at the repositories: how many pages Back has to pop.
    pub fn depth(&self) -> usize {
        self.pages().len() - 1
    }
}

/// One page of the stack [`Nav::pages`] projects, the screen the phone
/// shows alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Repos,
    Repo,
    Change,
    /// one file of a change's diff, shown alone, or all of them (the
    /// phone's All files)
    File,
    /// one commit of a repository's or a change's log
    Commit,
    /// one level of the phone's open folder in the Code tab: `a/b` is two
    Folder,
    /// one file of the Code tab
    Blob,
    /// a path a link named that the ref does not hold
    Missing,
    /// a path a link named, until its folder's tree says what it is: it
    /// stands where its folder, file or missing page will, so the link
    /// slides once (Expo `code/[...path].tsx` before its `entry` is read)
    Landing,
}

/// What lies over the phone's page, read from the model each frame
/// ([`Forge::overlay`](crate::Forge::overlay)): the one the phone draws, and
/// the one its Back closes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overlay {
    /// a ref picker (`menu`)
    Menu(Menu),
    /// a sheet a page opened (`sheet`)
    Sheet(Sheet),
    /// the New repository form
    NewRepository,
    /// Finish your review
    FinishReview,
    // who can edit the open repository or change (`policies.rs`)
    Policy,
}

/// A sheet a phone page opens over itself; Back closes it first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sheet {
    /// a repository's About (the desktop docks it, `Nav::dock`)
    About,
    /// what an open change offers its author and writers: Edit, Close
    ChangeActions,
    /// a member to grant write access to (the Settings tab)
    Grant,
    /// a member to ask for a review (the change form)
    Reviewers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RepoTab {
    /// the repository's front page: its README, rendered
    #[default]
    Readme,
    Code,
    Commits,
    Changes,
    Refs,
    Settings,
}

impl RepoTab {
    pub const ALL: [Self; 6] = [
        Self::Readme,
        Self::Code,
        Self::Commits,
        Self::Changes,
        Self::Refs,
        Self::Settings,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Readme => "README",
            Self::Code => "Code",
            Self::Commits => "Commits",
            Self::Changes => "Changes",
            Self::Refs => "Refs",
            Self::Settings => "Settings",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Readme => "readme",
            Self::Code => "code",
            Self::Commits => "commits",
            Self::Changes => "changes",
            Self::Refs => "refs",
            Self::Settings => "settings",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ChangeTab {
    #[default]
    Conversation,
    Commits,
    Files,
}

impl ChangeTab {
    pub const ALL: [Self; 3] = [Self::Conversation, Self::Commits, Self::Files];
    pub fn label(self) -> &'static str {
        match self {
            Self::Conversation => "Conversation",
            Self::Commits => "Commits",
            Self::Files => "Files",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Commits => "commits",
            Self::Files => "files",
        }
    }
}

/// The panel a repository docks beside its tabs. A change carries its own
/// details sidebar instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Dock {
    About,
}

impl Dock {
    pub fn label(self) -> &'static str {
        match self {
            Self::About => "About",
        }
    }
}

/// A dropdown open under the button that opened it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    /// the ref every repository tab reads
    Ref,
    /// the default head in the repository's settings
    Head,
    /// a new change's source (the phone's change form)
    Source,
    /// a new change's target (the phone's change form)
    Target,
}

impl Menu {
    /// The element that opens it; its list is `{key}-menu`, its filter
    /// box `{key}-filter`.
    pub fn key(self) -> &'static str {
        match self {
            Menu::Ref => "forge-ref-picker",
            Menu::Head => "forge-settings-head",
            Menu::Source => "forge-change-from",
            Menu::Target => "forge-change-into",
        }
    }
}

/// The change-list filters. "Needs my judgment" is its own query, not a
/// `ChangeFilter`: the program answers it from the reader's key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Filter {
    Judgment,
    #[default]
    Open,
    Merged,
    Closed,
    Authored,
    Involves,
}

impl Filter {
    pub fn label(self) -> &'static str {
        match self {
            Self::Judgment => "Needs my judgment",
            Self::Open => "Open",
            Self::Merged => "Merged",
            Self::Closed => "Closed",
            Self::Authored => "Authored by me",
            Self::Involves => "Involves me",
        }
    }
    pub fn slug(self) -> &'static str {
        match self {
            Self::Judgment => "judgment",
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
            Self::Authored => "authored",
            Self::Involves => "involves",
        }
    }
}

/// Where a line comment sits: the file's path, the new side (or the old),
/// the line; what a diff line's comment button carries.
pub(crate) type Anchor = (Vec<u8>, bool, u64);

/// A review being written: pending comments staged against one pinned pair
/// of endpoints, published as exactly one `ReviewSubmit`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ReviewSession {
    pub commit: String,
    pub base: Option<String>,
    pub comments: Vec<PendingComment>,
    /// the anchor whose composer is open
    pub open: Option<PendingComment>,
    /// what the review says overall, multi-line
    pub body: TextField,
    /// the verdict picked in the finish panel; none picked is a comment
    #[serde(skip)]
    pub verdict: Option<Verdict>,
    pub finishing: bool,
    /// why finishing it was refused before it was sent; each design words
    /// it
    pub refused: Option<ReviewRefusal>,
}

/// Why a review was refused before it was sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReviewRefusal {
    /// a comment review with no body and no line comment
    EmptyComment,
}

impl ReviewSession {
    /// One draft per anchor: re-staging the same line replaces it.
    pub fn stage(&mut self, comment: PendingComment) {
        match self
            .comments
            .iter_mut()
            .find(|staged| staged.anchors(&comment))
        {
            Some(staged) => *staged = comment,
            None => self.comments.push(comment),
        }
    }
    pub fn staged(&self, path: &[u8], new_side: bool, line: u64) -> Option<&PendingComment> {
        self.comments
            .iter()
            .find(|c| c.path == path && c.new_side == new_side && c.line == line)
    }
    pub fn line_comments(&self) -> Vec<LineComment> {
        self.comments
            .iter()
            .map(|c| LineComment {
                path: c.path.clone(),
                side: if c.new_side { Side::New } else { Side::Old },
                line: c.line,
                body: c.body.text().clone(),
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingComment {
    pub path: Vec<u8>,
    pub new_side: bool,
    pub line: u64,
    pub body: TextField,
}

impl PendingComment {
    pub fn anchors(&self, other: &Self) -> bool {
        self.path == other.path && self.new_side == other.new_side && self.line == other.line
    }
    pub fn anchor(&self) -> String {
        format!(
            "{}:{} ({})",
            String::from_utf8_lossy(&self.path),
            self.line,
            if self.new_side { "new" } else { "old" }
        )
    }
}

pub(crate) fn verdict_label(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "Approve",
        Verdict::RequestChanges => "Request changes",
        Verdict::Comment => "Comment",
    }
}

/// What a reviewer did, as the conversation says it after their name.
pub(crate) fn verdict_verb(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Approve => "approved",
        Verdict::RequestChanges => "requested changes",
        Verdict::Comment => "commented",
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct ChangeForm {
    /// the repository it was opened in, the only one that shows it
    pub repo: String,
    /// `Some(n)` edits that change instead of opening a new one
    pub edit: Option<u64>,
    pub from: Vec<u8>,
    pub into: Vec<u8>,
    pub title: TextField,
    /// the multi-line body, as the host's editor holds it
    pub body: TextField,
    pub reviewers: Vec<forge::Principal>,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct NewRepo {
    pub name: TextField,
    /// SHA-256 unless SHA-1 is picked (to push an existing git project)
    pub sha1: bool,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct SettingsForm {
    pub head: Vec<u8>,
    pub allow_force: bool,
    pub allow_delete: bool,
    pub grant: TextField,
}

/// An operation the reader issued: shown where they issued it until the
/// next query reconciles it, or refused with the reason inline.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub id: u64,
    /// the forge block heard last as the op was issued: a later one
    /// carries it
    pub issued: u64,
    /// the screen the row belongs to
    pub scope: String,
    pub label: String,
    pub progress: Progress,
}

/// Where an issued operation stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Submitting,
    /// accepted by the node, waiting for the block that carries it
    Accepted,
    /// it did not land: the host's answer (refused, in doubt, or never
    /// sent), which both designs word as every view does
    /// (`design::failure`)
    Refused(Answer),
}

/// What the host answered a write.
pub(crate) type Answer = Result<view::methods::Submitted, view::host::Error>;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Layout {
    /// the pane's size, read from the window each render
    #[serde(skip)]
    pub width: f32,
    #[serde(skip)]
    pub height: f32,
    /// the repositories rail
    pub tree: f32,
    /// the Code tab's file tree
    pub files: f32,
    pub tree_open: bool,
    pub dock_open: bool,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            width: 0.,
            height: 0.,
            tree: 220.,
            files: 300.,
            tree_open: false,
            dock_open: false,
        }
    }
}

/// The repositories rail and the Code tab's file tree, dragged, stay
/// within these widths so the pane beside them stays usable.
const TREE_MIN: f32 = 160.;
const TREE_MAX: f32 = 480.;
const FILES_MIN: f32 = 180.;
const FILES_MAX: f32 = 640.;
/// Under this window width the side panes fold away behind toggles.
const NARROW_BELOW: f32 = 880.;

impl Layout {
    /// A dragged pane keeps its neighbour usable.
    pub fn clamp(&mut self) {
        self.tree = self.tree.clamp(TREE_MIN, TREE_MAX);
        self.files = self.files.clamp(FILES_MIN, FILES_MAX);
    }
    pub fn narrow(&self) -> bool {
        self.width < NARROW_BELOW
    }
    pub fn tree_visible(&self) -> bool {
        !self.narrow() || self.tree_open
    }
    pub fn dock_visible(&self) -> bool {
        !self.narrow() || self.dock_open
    }
}

/// The key a change's screens and drafts hang on.
pub(crate) fn change_key(repo: &str, n: u64) -> String {
    format!("{repo}#{n}")
}
