//! What the screen is showing and what the reader has typed. Everything the
//! programs speak is folded to rows at render time, so a snapshot carries
//! navigation and drafts, never wire records.
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use view::Paged;
use view::prelude::*;

use forge::{CommitInfo, LineComment, Query, Reply, Revision, Side, Verdict};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Talk {
    pub(crate) channel: String,
}

impl Talk {
    pub(crate) fn of(change: &forge::Change) -> Option<Self> {
        let own = chat::namespace::program(&change.channel) == Some(forge::PROGRAM);
        own.then(|| Talk {
            channel: change.channel.clone(),
        })
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Forge {
    pub(crate) nav: Nav,
    pub(crate) session: Session,
    pub(crate) filter: Filter,
    pub(crate) search: TextField,
    pub(crate) change_search: TextField,
    pub(crate) tree_search: TextField,
    pub(crate) reviews: BTreeMap<String, ReviewSession>,
    pub(crate) viewed: BTreeSet<String>,
    pub(crate) new_repo: Option<NewRepo>,
    pub(crate) form: Option<ChangeForm>,
    pub(crate) repo_settings: Option<SettingsForm>,
    pub(crate) reply: TextField,
    pub(crate) notice: String,
    #[serde(skip)]
    pub(crate) copied: Option<String>,
    #[serde(skip)]
    pub(crate) title: Option<String>,
    #[serde(skip)]
    pub(crate) showing: Option<String>,
    #[serde(skip)]
    pub(crate) badge: Option<i64>,
    pub(crate) layout: Layout,
    #[serde(skip)]
    pub(crate) data: BTreeMap<Query, Loadable<Rc<Reply>>>,
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
    // each change conversation's rows, by where it is read
    pub(crate) messages: BTreeMap<Talk, Loadable<Vec<chat::MsgRow>>>,
    /// a refusal on screen, read again with the block: the read lives
    /// here because the slot holds the refusal, and a newer block's read
    /// stored in its place cancels it
    #[serde(skip)]
    pub(crate) rereading: BTreeMap<Query, Task<()>>,
    #[serde(skip)]
    pub(crate) rereading_messages: BTreeMap<Talk, Task<()>>,
    #[serde(skip)]
    pub(crate) pending: Vec<Pending>,
    #[serde(skip)]
    pub(crate) block: u64,
    // out of sight (`host.visible` false): nothing is read until it shows again
    #[serde(skip)]
    pub(crate) hidden: bool,
    #[serde(skip)]
    pub(crate) diff_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) log_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) tree_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) files_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) refs_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) changes_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) strip_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) reveal: Option<Vec<u8>>,
    #[serde(skip)]
    pub(crate) next_pending: u64,
    #[serde(skip)]
    pub(crate) closing: Option<String>,
    #[serde(skip)]
    pub(crate) touch: bool,
    #[serde(skip)]
    pub(crate) menu: Option<Menu>,
    #[serde(skip)]
    pub(crate) sheet: Option<Sheet>,
    #[serde(skip)]
    pub(crate) details: bool,
    #[serde(skip)]
    pub(crate) refs_tags: bool,
    #[serde(skip)]
    pub(crate) wrap: bool,
    #[serde(skip)]
    pub(crate) source: bool,
    #[serde(skip)]
    pub(crate) lines: crate::ui::phone::Block,
    #[serde(skip)]
    pub(crate) people_search: TextField,
    #[serde(skip)]
    pub(crate) menu_at: (f32, f32),
    #[serde(skip)]
    pub(crate) menu_cursor: usize,
    #[serde(skip)]
    pub(crate) menu_search: TextField,
    #[serde(skip)]
    pub(crate) menu_tags: bool,
    #[serde(skip)]
    pub(crate) menu_scroll: UniformListScrollHandle,
    #[serde(skip)]
    pub(crate) repos_cursor: Option<(String, usize)>,
    #[serde(skip)]
    pub(crate) diff_cursor: Option<(usize, usize)>,
    #[serde(skip)]
    pub(crate) rail_cursor: Option<String>,
    #[serde(skip)]
    pub(crate) list_cursor: Option<(&'static str, usize, usize)>,
    #[serde(skip)]
    pub(crate) tree_walked: bool,
    #[serde(skip)]
    pub(crate) tab_cursor: Option<RepoTab>,
    #[serde(skip)]
    pub(crate) change_tab_cursor: Option<ChangeTab>,
    #[serde(skip)]
    pub(crate) blob_cache: crate::blob::BlobCache,
    #[serde(skip)]
    pub(crate) diff_cache: crate::ui::diff::DiffCache,
    #[serde(skip)]
    pub(crate) repo_policy: policy_view::Editing,
    #[serde(skip)]
    pub(crate) change_policy: policy_view::Editing,
    #[serde(skip)]
    pub(crate) settings_policy: policy_view::Editing,
}

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
    #[serde(skip)]
    pub linked: Option<Vec<String>>,
    #[serde(skip)]
    pub gone: Option<(String, String)>,
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
        let rev = self.rev.clone().unwrap_or_else(|| head.to_vec());
        match rev.starts_with(b"refs/") {
            true => Revision::Ref(rev),
            false => Revision::Oid(String::from_utf8_lossy(&rev).into_owned()),
        }
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
                let levels = self
                    .folder
                    .as_ref()
                    .map_or(0, |path| path.split(|byte| *byte == b'/').count());
                pages.extend(std::iter::repeat_n(Page::Folder, levels));
                let lost = self.missing.is_some() || self.gone.is_some();
                let landing = self.goto.is_some() || self.linked.is_some();
                if self.blob.is_some() {
                    pages.push(Page::Blob);
                } else if lost {
                    pages.push(Page::Missing);
                } else if landing {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Repos,
    Repo,
    Change,
    File,
    Commit,
    Folder,
    Blob,
    Missing,
    Landing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overlay {
    Menu(Menu),
    Sheet(Sheet),
    NewRepository,
    FinishReview,
    Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sheet {
    About,
    ChangeActions,
    Grant,
    Reviewers,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum RepoTab {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Menu {
    Ref,
    Head,
    Source,
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

pub(crate) type Anchor = (Vec<u8>, bool, u64);

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum ReviewRefusal {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Progress {
    Submitting,
    Accepted,
    Refused(Answer),
}

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

const TREE_MIN: f32 = 160.;
const TREE_MAX: f32 = 480.;
const FILES_MIN: f32 = 180.;
const FILES_MAX: f32 = 640.;
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

pub(crate) fn change_key(repo: &str, n: u64) -> String {
    format!("{repo}#{n}")
}
