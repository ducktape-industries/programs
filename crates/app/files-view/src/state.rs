//! The state Files keeps: where the reader is (a stack of pages), who reads,
//! and what each page read. Rows are the program's own types, worded only
//! where they are drawn (`words.rs`, `ui/`).
use std::collections::BTreeMap;
use std::sync::Arc;

use abi::role::access::Writers;
use files::{BlobId, DiffEntry, EntryInfo, Node, Party, PinRow, SnapshotRow};
use serde::{Deserialize, Serialize};
use view::borsh_bytes;
use view::design::Pages;
use view::prelude::*;

use crate::menu::Menu;
use crate::write::Form;

pub const SHARED_PATH: &str = "/shared";
/// The whole tree.
pub const ROOT: &str = "/";

/// The glyphs (Lucide names) rows lead with where the model, not a node,
/// names what the row is.
pub const HOME_GLYPH: &str = "house";
pub const HISTORY_GLYPH: &str = "clock";
pub const SNAPSHOT_GLYPH: &str = "git-commit-horizontal";
pub const PIN_GLYPH: &str = "pin";

/// What a file holds, as both designs draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holds {
    /// a picture the app draws
    Picture,
    Text,
    /// bytes neither: shown by their size
    Bytes,
}

/// What a file named `name` holds, by what its name ends with: a listing
/// carries no bytes. The page that opens it reads the bytes and knows.
pub fn holds_by_name(name: &str) -> Holds {
    const PICTURES: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
    const TEXTS: [&str; 18] = [
        "txt", "md", "markdown", "json", "toml", "yaml", "yml", "csv", "log", "html", "css", "js",
        "ts", "rs", "py", "sh", "svg", "xml",
    ];
    let extension = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => extension.to_ascii_lowercase(),
        _ => return Holds::Bytes,
    };
    if PICTURES.contains(&extension.as_str()) {
        Holds::Picture
    } else if TEXTS.contains(&extension.as_str()) {
        Holds::Text
    } else {
        Holds::Bytes
    }
}

/// The glyph an entry's row leads with: a folder's, a link's, else what
/// its name says the file holds.
pub fn entry_glyph(entry: &EntryInfo) -> &'static str {
    match entry.node {
        Node::Dir { .. } => "folder",
        Node::Symlink { .. } => "link",
        Node::File { .. } => file_glyph(name_of(&entry.path)),
    }
}

/// The glyph a file's row leads with, by what its name says it holds.
pub fn file_glyph(name: &str) -> &'static str {
    match holds_by_name(name) {
        Holds::Picture => "image",
        Holds::Text => "file-text",
        Holds::Bytes => "file",
    }
}

pub use view::picture::picture_format;

/// The places beside the reader's home, in the order both designs list
/// them; each opens its folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Shared,
    Everything,
}

impl Place {
    pub const ALL: [Place; 2] = [Place::Shared, Place::Everything];

    /// What its row is keyed by: `files/place/<key>`.
    pub fn key(self) -> &'static str {
        match self {
            Place::Shared => "shared",
            Place::Everything => "everything",
        }
    }

    pub fn path(self) -> &'static str {
        match self {
            Place::Shared => SHARED_PATH,
            Place::Everything => ROOT,
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            Place::Shared => "users",
            Place::Everything => "hard-drive",
        }
    }
}

/// One page of the browser. Both designs draw the same stack: a phone
/// shows its top alone with Back, a desktop window the top under its bar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    /// where to start: the reader's home, `/shared`, the whole tree, the
    /// newest snapshots
    Places,
    /// a folder's entries, by path
    Folder(String),
    /// one file, by path
    File(String),
    /// every snapshot, newest first, and the pins
    History,
    /// one snapshot, by its id, and what it changed
    Snapshot(#[serde(with = "borsh_bytes")] BlobId),
}

impl Route {
    /// The page an entry of a folder opens: a folder's, else a file's.
    pub fn of(entry: &EntryInfo) -> Route {
        match entry.node {
            Node::Dir { .. } => Route::Folder(entry.path.clone()),
            Node::File { .. } | Node::Symlink { .. } => Route::File(entry.path.clone()),
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Reader {
    pub account: Option<u64>,
    pub name: Reread<Option<String>>,
}

/// Why the reader has no home to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoHome<'a> {
    /// no account is signed in
    SignedOut,
    Asking,
    Unnamed,
    Refused(&'a Error),
}

impl Reader {
    /// The reader's home, `/home/<their name>`, or why there is none.
    pub fn home(&self) -> Result<String, NoHome<'_>> {
        if self.account.is_none() {
            return Err(NoHome::SignedOut);
        }
        match &*self.name {
            Loadable::Ready(Some(name)) | Loadable::Reloading(Some(name), _) => {
                Ok(format!("/home/{name}"))
            }
            Loadable::Ready(None) | Loadable::Reloading(None, _) => Err(NoHome::Unnamed),
            Loadable::Failed(refusal) => Err(NoHome::Refused(refusal)),
            Loadable::Idle | Loadable::Loading(_) => Err(NoHome::Asking),
        }
    }

    pub fn writes(&self, writers: &Writers) -> bool {
        let Some(account) = self.account else {
            return false;
        };
        writers.allows(&[Party::Account(account)])
    }
}

/// A folder as `files` listed it at one snapshot.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Folder {
    pub path: String,
    /// the snapshot it was read at; `None` before the first commit. A
    /// write made from this folder is based on it, so `files` refuses it
    /// as stale if what it touches moved since.
    #[serde(with = "borsh_bytes")]
    pub snapshot: Option<BlobId>,
    /// folders first, then files and links, each by name
    #[serde(with = "borsh_bytes")]
    pub entries: Vec<EntryInfo>,
    /// who writes the folder itself; `None` for `/`, which no change names
    #[serde(with = "borsh_bytes")]
    pub writers: Option<Writers>,
}

impl Folder {
    pub fn children(&self) -> Writers {
        self.writers.clone().unwrap_or(Writers::Anyone)
    }

    pub fn holds_namespace_roots(&self) -> bool {
        files::paths::holds_namespace_roots(&segments_of(&self.path))
    }
}

/// How much of a file is read to show it, and the most a file may hold
/// for its text to be edited here: an edit writes the whole file back.
pub const PREVIEW: u64 = 64 * 1024;

/// One file as `files` answered it at one snapshot.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Opened {
    pub path: String,
    #[serde(with = "borsh_bytes")]
    pub snapshot: Option<BlobId>,
    /// what is at the path (`Stat`); `None`: nothing any more. A folder
    /// made at the path since is not this page's: see [`Opened::file`].
    #[serde(with = "borsh_bytes")]
    pub entry: Option<EntryInfo>,
    /// its first [`PREVIEW`] bytes (`Read`); a link's are its target
    pub bytes: Vec<u8>,
    /// `bytes` is the whole file
    pub whole: bool,
    #[serde(skip)]
    pub picture: Option<Arc<Image>>,
    /// who writes it
    #[serde(with = "borsh_bytes")]
    pub writers: Option<Writers>,
    // a markdown file's text as its blocks, read once as it lands. Never
    // kept: a restored view reads the file again
    #[serde(skip)]
    pub doc: Option<Arc<Vec<markdown::Block>>>,
}

impl Opened {
    // Its text as markdown's blocks, when its name says it is markdown
    // (`.md`, `.markdown`) and it reads as text.
    pub fn parsed(&self) -> Option<Arc<Vec<markdown::Block>>> {
        let named = markdown::highlight::language_of(name_of(&self.path)) == Some("markdown");
        let text = self.text().filter(|_| named)?;
        Some(Arc::new(markdown::parse(text)))
    }

    /// What is at the path, while it is a file or a link: a folder made at
    /// the path since the page opened is not a file to show.
    pub fn file(&self) -> Option<&EntryInfo> {
        self.entry
            .as_ref()
            .filter(|entry| !matches!(entry.node, Node::Dir { .. }))
    }

    /// The picture on screen and the content it was read from: kept,
    /// unread, while a later read finds the file's content the same.
    pub fn kept_picture(&self) -> Option<(BlobId, Arc<Image>)> {
        match (self.file().map(|entry| &entry.node), &self.picture) {
            (Some(Node::File { file, .. }), Some(picture)) => Some((*file, picture.clone())),
            _ => None,
        }
    }

    /// What the file holds, by its first bytes.
    pub fn holds(&self) -> Holds {
        match (picture_format(&self.bytes), self.text()) {
            (Some(_), _) => Holds::Picture,
            (None, Some(_)) => Holds::Text,
            (None, None) => Holds::Bytes,
        }
    }

    /// The bytes as text, when they are UTF-8; a read cut inside a
    /// character's bytes still is, up to that character.
    pub fn text(&self) -> Option<&str> {
        if picture_format(&self.bytes).is_some() {
            return None;
        }
        match std::str::from_utf8(&self.bytes) {
            Ok(text) => Some(text),
            Err(cut) if cut.error_len().is_none() && !self.whole => {
                std::str::from_utf8(&self.bytes[..cut.valid_up_to()]).ok()
            }
            Err(_) => None,
        }
    }

    /// The file's size as its tree records it; a link's, the length of
    /// what it points to.
    pub fn size(&self) -> u64 {
        match self.entry.as_ref().map(|entry| &entry.node) {
            Some(Node::File { size, .. }) => *size,
            _ => self.bytes.len() as u64,
        }
    }

    /// Whether its text can be edited here: a file, read whole, as text.
    pub fn editable(&self) -> bool {
        let file = matches!(
            self.file().map(|entry| &entry.node),
            Some(Node::File { .. })
        );
        file && self.whole && self.text().is_some()
    }
}

/// The newest snapshots, and every pin.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    /// newest first, at most [`HISTORY_PAGE`]
    #[serde(with = "borsh_bytes")]
    pub snapshots: Vec<SnapshotRow>,
    /// the history holds older snapshots than these
    pub older: bool,
    #[serde(with = "borsh_bytes")]
    pub pins: Vec<PinRow>,
}

impl History {
    /// The snapshot `pin` names, when the history lists it.
    pub fn named(&self, pin: &PinRow) -> Option<&SnapshotRow> {
        self.snapshots.iter().find(|row| row.id == pin.snapshot)
    }
}

/// How many snapshots the history reads, newest first.
// ponytail: one page and no more; older snapshots open only from a pin
// that names them. Page with `PageRequest::after` when histories outgrow it.
pub const HISTORY_PAGE: u64 = 100;

/// One snapshot, and what it changed against its parent (`Diff`).
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// the snapshot asked for
    #[serde(with = "borsh_bytes")]
    pub id: BlobId,
    /// its row; `None`: files names no such snapshot
    #[serde(with = "borsh_bytes")]
    pub row: Option<SnapshotRow>,
    /// what it changed; empty for the first, whose whole tree is new
    #[serde(with = "borsh_bytes")]
    pub changes: Vec<DiffEntry>,
}

/// Every account's name, by number, as identity lists them: who made a
/// snapshot.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Names(pub BTreeMap<u64, String>);

impl Names {
    pub fn of(&self, account: u64) -> Option<&str> {
        self.0.get(&account).map(String::as_str)
    }
}

/// The line over the page: what the last write or copy did, or, with no
/// form open to show it in, why it did not land. Worded when drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Note {
    /// the commit landed: its message
    Committed(String),
    /// a link is on the clipboard
    Copied,
    /// a write that did not land, in every view's words
    /// (`design::failure`)
    Refused(String),
}

/// What is open over the page.
pub enum Sheet {
    /// the page's writes as a list: a phone has no room for their
    /// buttons (a desktop draws them on the page and never opens this)
    Actions,
    /// a menu at the pointer, on a row or a folder's empty room: a
    /// desktop's right press (a phone never opens this)
    Menu(Menu),
    /// one write being filled in
    Write(Box<Form>),
}

#[derive(Serialize, Deserialize)]
pub struct Files {
    pub(crate) pages: Pages<Route>,
    pub(crate) reader: Reader,
    pub(crate) folder: Reread<Folder>,
    pub(crate) file: Reread<Opened>,
    pub(crate) history: Reread<History>,
    pub(crate) snapshot: Loadable<Snapshot>,
    pub(crate) names: Reread<Names>,
    pub(crate) chain: String,
    pub(crate) note: Option<Note>,
    #[serde(skip)]
    pub(crate) sheet: Option<Sheet>,
    #[serde(skip)]
    pub(crate) forms: u64,
    #[serde(skip)]
    pub(crate) width: f32,
    #[serde(skip)]
    pub(crate) picking: bool,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            pages: Pages::root(Route::Places),
            reader: Reader::default(),
            folder: Reread::default(),
            file: Reread::default(),
            history: Reread::default(),
            snapshot: Loadable::Idle,
            names: Reread::default(),
            chain: String::new(),
            note: None,
            sheet: None,
            forms: 0,
            width: 0.,
            picking: false,
        }
    }
}

impl Files {
    pub(crate) fn page(&self) -> &Route {
        self.pages.top()
    }

    pub(crate) fn folder_at(&self, path: &str) -> Option<&Folder> {
        self.folder.ready().filter(|folder| folder.path == path)
    }

    pub(crate) fn file_at(&self, path: &str) -> Option<&Opened> {
        self.file.ready().filter(|file| file.path == path)
    }

    pub(crate) fn snapshot_at(&self, id: BlobId) -> Option<&Snapshot> {
        self.snapshot.ready().filter(|snapshot| snapshot.id == id)
    }

    pub(crate) fn sheet_shown(&self) -> Option<&Sheet> {
        match &self.sheet {
            Some(Sheet::Actions) if self.offered().is_empty() => None,
            Some(Sheet::Menu(menu)) if self.items(&menu.on).is_empty() => None,
            sheet => sheet.as_ref(),
        }
    }

    pub(crate) fn form(&self) -> Option<&Form> {
        match &self.sheet {
            Some(Sheet::Write(form)) => Some(form),
            _ => None,
        }
    }
}

/// A path's last segment: a folder's or a file's name; `/` for the root.
pub fn name_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some((_, "")) | None => ROOT,
        Some((_, name)) => name,
    }
}

/// The folder a path is in; `/` for the root's own entries.
pub fn parent_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) | None => ROOT,
        Some((folder, _)) => folder,
    }
}

/// `name` inside `folder`.
pub fn child_of(folder: &str, name: &str) -> String {
    match folder {
        ROOT => format!("/{name}"),
        folder => format!("{folder}/{name}"),
    }
}

pub fn segments_of(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn is_namespace_root(path: &str) -> bool {
    files::paths::is_namespace_root(&segments_of(path))
}
