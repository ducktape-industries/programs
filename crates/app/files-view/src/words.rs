//! Every sentence, label and status word both designs show, once: the
//! desktop's pages (`ui/`) and the phone's screens (`ui/phone/`) word
//! things from here and never type a sentence the other also types.
use std::sync::Arc;

use files::{Change, DiffEntry, DiffKind, EntryInfo, Node, Principal, SnapshotRow};
use view::Image;
use view::design::{date, grouped, plural};

use crate::state::{
    Holds, Names, NoHome, Note, Opened, PREVIEW, Place, SHOW_MOST, holds_by_name, name_of,
};
use crate::upload::UPLOAD_MOST;
use crate::write::{ListChoice, Naming, Write};

pub const TITLE: &str = "Files";
pub const PLACES: &str = "Places";
pub const HOME: &str = "Home";
pub const SHARED: &str = "Shared";
pub const EVERYTHING: &str = "Everything";
pub const HISTORY: &str = "History";
pub const PINS: &str = "Pins";
pub const CHANGES: &str = "Changes";
pub const LATEST: &str = "Latest snapshots";
pub const CHAIN: &str = "the chain";
/// What the desktop's tab list is called to assistive tech.
pub const TABS: &str = "Pages";
/// What a wide desktop's side is called to assistive tech.
pub const SIDE: &str = "Places and history";
/// What a folder's or a file's crumbs are called to assistive tech.
pub const PATH: &str = "Path";
/// What a folder's list of entries is called to assistive tech.
pub const ENTRIES: &str = "Entries";

pub const SHARED_CAPTION: &str = "Any account writes here";
pub const EVERYTHING_CAPTION: &str = "The whole tree";
pub const NO_NAME: &str = "No home: your account has no name in identity";
pub const SIGNED_OUT: &str = "Sign in to have a home";
pub const FINDING_HOME: &str = "Finding your home…";

pub const READING_FOLDER: &str = "Reading the folder…";
pub const READING_FILE: &str = "Reading the file…";
pub const READING_HISTORY: &str = "Reading the history…";
pub const READING_SNAPSHOT: &str = "Reading the snapshot…";
pub const READING_PICTURE: &str = "Reading the image…";
/// What an image that does not draw says in its place.
pub const NOT_DRAWN: &str = "This image does not draw here.";

/// What the detail pane beside a folder's list says before a file opens.
pub const PICK_A_FILE: &str = "Open a file to see it here.";
/// What the detail pane beside the history says before a snapshot opens.
pub const PICK_A_SNAPSHOT: &str = "Open a snapshot to see what it changed.";

pub const EMPTY_FOLDER: &str = "Nothing here yet";
pub const EMPTY_WRITABLE: &str = "Make a folder or a text file here, or upload files.";
pub const EMPTY_READ_ONLY: &str = "Its writers have put nothing here.";
pub const NO_SNAPSHOTS: &str = "No snapshot yet";
pub const NO_SNAPSHOTS_DETAIL: &str = "The first commit to files makes one.";
pub const NO_PINS: &str = "No snapshot is pinned.";
pub const OLDER: &str = "Older snapshots are not shown.";
pub const OLDER_SNAPSHOT: &str = "an older snapshot";
pub const NO_SUCH_SNAPSHOT: &str = "Files names no such snapshot.";
pub const FIRST_SNAPSHOT: &str = "The first snapshot: everything in it is new.";
pub const NOTHING_CHANGED: &str = "It changed nothing.";
pub const GONE: &str = "Nothing is at this path any more.";

pub const NAME_IT: &str = "Name it.";
pub const NOTHING_TO_UPLOAD: &str = "None of these files can be uploaded.";
pub const REPLACES: &str = "Replaces the file there";
/// A second picked file of a name already picked.
pub const PICKED_TWICE: &str = "Picked twice: only the first goes";
/// The folder an upload was picked for left the screen while the
/// picker was open.
pub const FOLDER_LEFT: &str = "The folder closed while you picked: upload again from it.";
/// A form closed while its files went to the network, before its commit.
pub const STOPPED: &str = "Stopped before the commit: nothing was committed.";
pub const ONE_NAME: &str = "A name has no /: rename it afterwards to move it.";
pub const SAME_PATH: &str = "It is at that path already.";

pub const NAME: &str = "Name";
pub const WHO_WRITES: &str = "Who may write it";
pub const LISTED: &str = "Account numbers and programs";
pub const LIST_SOMEONE: &str =
    "List the account numbers and programs that may write it, like 7, 12, harness.";
pub const AS_ITS_FOLDER_NOTE: &str =
    "No list of its own: the nearest list above it decides, and with none, anyone writes it.";
pub const NEW_PATH: &str = "New path";
pub const TEXT: &str = "Text";
pub const MESSAGE: &str = "Message";
pub const CANCEL: &str = "Cancel";
pub const COMMIT: &str = "Commit";
pub const COMMITTING: &str = "Committing…";
pub const ACTIONS: &str = "Actions";
pub const OPEN: &str = "Open";
pub const COPY_LINK: &str = "Copy link";
pub const LINK_COPIED: &str = "Link copied";

pub const SIZE: &str = "Size";
pub const KIND: &str = "Kind";
pub const TARGET: &str = "Points to";
pub const SNAPSHOT: &str = "Snapshot";
pub const AUTHOR: &str = "By";
pub const TIME: &str = "Time";
pub const BLOCK: &str = "Block";

/// An account by its name, else its number.
pub fn account(number: u64, name: Option<&str>) -> String {
    match name {
        Some(name) => name.to_owned(),
        None => format!("account {number}"),
    }
}

/// Who made a snapshot or a pin: an account's name (once `names` is
/// read), else its number; the chain itself.
pub fn author(principal: &Principal, names: Option<&Names>) -> String {
    match principal {
        Principal::Account(number) => account(*number, names.and_then(|names| names.of(*number))),
        Principal::Root => CHAIN.into(),
    }
}

/// Who made a snapshot, and when.
pub fn byline(author: String, time_ms: u64) -> String {
    format!("{author} · {}", date(time_ms))
}

/// What a pin's row says under its name: the snapshot it names (its place
/// in the history, when the history lists it), and who pinned it.
pub fn pin_caption(seq: Option<u64>, owner: String) -> String {
    let named = seq.map_or_else(|| OLDER_SNAPSHOT.to_owned(), snapshot);
    format!("{named} · {owner}")
}

pub fn list_choice(choice: ListChoice) -> &'static str {
    match choice {
        ListChoice::AsItsFolder => "As its folder",
        ListChoice::OnlyMe => "Only me",
        ListChoice::Listed => "Me and…",
        ListChoice::Anyone => "Anyone",
    }
}

/// Why the reader has no home to open.
pub fn no_home(why: &NoHome) -> String {
    match why {
        NoHome::SignedOut => SIGNED_OUT.into(),
        NoHome::Asking => FINDING_HOME.into(),
        NoHome::Unnamed => NO_NAME.into(),
        NoHome::Refused(refusal) => view::design::sentence(refusal),
    }
}

/// What a snapshot page says in place of its changes: that it is the
/// first, whose whole tree is new, or that it changed nothing; `None`
/// when it lists them.
pub fn changes_note(row: &SnapshotRow, changes: &[DiffEntry]) -> Option<&'static str> {
    match (row.parent, changes.is_empty()) {
        (None, _) => Some(FIRST_SNAPSHOT),
        (Some(_), true) => Some(NOTHING_CHANGED),
        (Some(_), false) => None,
    }
}

/// A place's title, and the caption under it.
pub fn place(place: Place) -> (&'static str, &'static str) {
    match place {
        Place::Shared => (SHARED, SHARED_CAPTION),
        Place::Everything => (EVERYTHING, EVERYTHING_CAPTION),
    }
}

/// What an empty folder says would fill it.
pub fn empty_folder(writable: bool) -> &'static str {
    match writable {
        true => EMPTY_WRITABLE,
        false => EMPTY_READ_ONLY,
    }
}

/// A count of bytes, in the unit that reads best.
pub fn size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    match bytes {
        bytes if bytes < KB => plural(bytes, "byte", "bytes"),
        bytes if bytes < MB => format!("{:.1} KB", bytes as f64 / KB as f64),
        bytes => format!("{:.1} MB", bytes as f64 / MB as f64),
    }
}

/// What a folder's row says under its name: how many entries, how big,
/// or where a link points.
pub fn entry_caption(node: &Node) -> String {
    match node {
        Node::Dir { entries, .. } => plural(*entries, "item", "items"),
        Node::File { size: bytes, .. } => size(*bytes),
        Node::Symlink { target } => format!("→ {target}"),
    }
}

/// What an entry's Size column says: a link has no size of its own.
pub fn entry_size(node: &Node) -> String {
    match node {
        Node::Symlink { .. } => "—".into(),
        node => entry_caption(node),
    }
}

/// What a node holds, in a word: a folder, a link, an image, text, or
/// bytes that are neither; a file by `holds`.
pub fn kind(node: &Node, holds: Holds) -> &'static str {
    match (node, holds) {
        (Node::Symlink { .. }, _) => "Link",
        (Node::Dir { .. }, _) => "Folder",
        (_, Holds::Picture) => "Image",
        (_, Holds::Text) => "Text",
        (_, Holds::Bytes) => "File",
    }
}

/// What a folder's entry holds, in a word, by its name: a listing
/// carries no bytes.
pub fn entry_kind(entry: &EntryInfo) -> &'static str {
    kind(&entry.node, holds_by_name(name_of(&entry.path)))
}

/// The picture a file draws, or what shows in its place: it is too large
/// to be read here, or (a restored view reads it again) still reading.
pub fn picture(file: &Opened) -> Result<&Arc<Image>, String> {
    match &file.picture {
        Some(picture) => Ok(picture),
        None if file.size() > SHOW_MOST => Err(too_big_to_show(file.size())),
        None => Err(READING_PICTURE.to_owned()),
    }
}

/// What a picture too large to draw here says in its place.
pub fn too_big_to_show(size: u64) -> String {
    format!(
        "An image of {}: only one of {} or less is drawn here.",
        self::size(size),
        self::size(SHOW_MOST)
    )
}

/// Why only part of a file shows.
pub fn cut(size: u64) -> String {
    format!(
        "Showing the first {} of {}; a file this large is not edited here.",
        self::size(PREVIEW),
        self::size(size)
    )
}

/// What a file that is not text shows in place of its bytes.
pub fn not_text(size: u64) -> String {
    format!("{} that are not text.", self::size(size))
}

/// The snapshot's place in the history.
pub fn snapshot(seq: u64) -> String {
    format!("Snapshot {}", grouped(seq))
}

/// What a write is called on its button or sheet row.
pub fn verb(write: &Write) -> &'static str {
    match write {
        Write::NewFolder { .. } => "New folder",
        Write::NewFile { .. } => "New text file",
        Write::Upload { .. } => "Upload files",
        Write::Edit { .. } => "Edit text",
        Write::Rename { .. } => "Rename or move",
        Write::Remove { .. } => "Remove",
    }
}

/// What a write's form is headed by: the write and what it is made on.
pub fn form_title(write: &Write) -> String {
    match write {
        Write::NewFolder { folder } | Write::NewFile { folder } => {
            format!("{} in {}", verb(write), name_of(folder))
        }
        Write::Upload { folder } => format!("Upload to {}", name_of(folder)),
        Write::Edit { path, .. } | Write::Rename { path } | Write::Remove { path, .. } => {
            format!("{} {}", verb(write), name_of(path))
        }
    }
}

/// The commit button: Commit; an upload's files going to the network,
/// how far they are; then, sent, that it waits on its answer.
pub fn commit(sending: bool, progress: Option<(u64, u64)>) -> String {
    match (sending, progress) {
        (true, Some((held, all))) if held < all => {
            format!("Sending {} of {}…", grouped(held + 1), grouped(all))
        }
        (true, _) => COMMITTING.into(),
        (false, _) => COMMIT.into(),
    }
}

/// An upload's commit message: what it sends.
pub fn uploads(names: &[&str]) -> String {
    match names {
        [name] => format!("Upload {name}"),
        names => format!("Upload {}", plural(names.len() as u64, "file", "files")),
    }
}

/// What a file too big to upload says on its row.
pub fn too_big_to_upload(size: u64) -> String {
    format!(
        "{}: only files of {} or less upload here",
        self::size(size),
        self::size(UPLOAD_MOST)
    )
}

/// Why a picked file is left out: a folder or a link holds its name.
pub fn held_by(node: &Node) -> String {
    let what = match node {
        Node::Dir { .. } => "A folder",
        _ => "A link",
    };
    format!("{what} of this name is here")
}

/// What a form's first field is labelled.
pub fn naming(naming: Naming) -> &'static str {
    match naming {
        Naming::Name => NAME,
        Naming::Path => NEW_PATH,
    }
}

/// What a removal's form says it does.
pub fn removes(path: &str, folder: bool) -> String {
    match folder {
        true => format!("{path} and everything in it go from the next snapshot on."),
        false => format!("{path} goes from the next snapshot on."),
    }
}

/// A commit's message when none is typed: what its change does.
pub fn message(change: &Change) -> String {
    match change {
        Change::Mkdir { path } => format!("Make {}", name_of(path)),
        Change::Put { path, .. } => format!("Write {}", name_of(path)),
        Change::Move { from, to } => format!("Move {from} to {to}"),
        Change::Remove { path } => format!("Remove {}", name_of(path)),
        Change::Symlink { path, .. } => format!("Link {}", name_of(path)),
        Change::SetOwnership { path, .. } => format!("Set who writes {}", name_of(path)),
    }
}

/// What a snapshot changed at a path, in a word.
pub fn diff_kind(kind: DiffKind) -> &'static str {
    match kind {
        DiffKind::Added => "added",
        DiffKind::Removed => "removed",
        DiffKind::Modified => "changed",
    }
}

/// A snapshot's message, or what it is without one.
pub fn message_or_none(message: &str) -> &str {
    match message.trim().is_empty() {
        true => "No message",
        false => message,
    }
}

/// The line over the page.
pub fn note(note: &Note) -> String {
    match note {
        Note::Committed(message) => format!("Committed: {message}"),
        Note::Copied => LINK_COPIED.into(),
        Note::Refused(why) => why.clone(),
    }
}
