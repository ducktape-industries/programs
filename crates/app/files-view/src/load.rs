//! The loaders: what the page on top of the stack shows, read with
//! `cx.load`, which keeps what is on screen while it reads again and draws
//! only what changed; and who reads, the name their home is called by.
use view::prelude::*;

use files::BlobId;

use crate::state::{Opened, Route, parent_of};
use crate::{Files, queries};

impl Files {
    /// What the page on top shows, on a move to it (or a Retry): its own
    /// read, and what it is drawn with that is not read yet. A page read
    /// before shows while it is read again.
    pub(crate) fn read_page(&mut self, cx: &mut Context<Self>) {
        self.read(false, cx);
    }

    /// Everything on screen, again: on every commit or pin anyone lands,
    /// and when the view attaches.
    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        self.read(true, cx);
    }

    fn read(&mut self, all: bool, cx: &mut Context<Self>) {
        // the history page's own read; on every other page, the side's
        // pins, the places' newest snapshots and the history a snapshot is
        // open beside: read once, then again on each change
        let page = self.page().clone();
        if all
            || page == Route::History
            || matches!(self.history, Loadable::Idle | Loadable::Failed(_))
        {
            self.read_history(cx);
        }
        match page {
            Route::Places | Route::History => {}
            Route::Folder(path) => self.read_folder(path, cx),
            Route::File(path) => {
                // the folder it is in, which a wide desktop lists beside it
                let folder = parent_of(&path);
                if all || self.folder_at(folder).is_none() {
                    self.read_folder(folder.to_owned(), cx);
                }
                self.read_file(path, cx);
            }
            Route::Snapshot(id) => self.read_snapshot(id, cx),
        }
    }

    fn read_folder(&mut self, path: String, cx: &mut Context<Self>) {
        // another folder's entries are not this one's to show meanwhile
        if self.folder_at(&path).is_none() {
            self.folder = Loadable::Idle;
        }
        let read = queries::folder(cx.host(), path);
        cx.load(self, read, |view| &mut view.folder);
    }

    fn read_file(&mut self, path: String, cx: &mut Context<Self>) {
        let shown = self.file_at(&path);
        // the picture on screen, kept while the file's content is the same
        let kept = shown.and_then(Opened::kept_picture);
        if shown.is_none() {
            self.file = Loadable::Idle;
        }
        let read = queries::file(cx.host(), path, kept);
        cx.load(self, read, |view| &mut view.file);
    }

    fn read_history(&mut self, cx: &mut Context<Self>) {
        let read = queries::history(cx.host());
        cx.load(self, read, |view| &mut view.history);
        if self.names.is_idle() {
            self.read_names(cx);
        }
    }

    /// A snapshot never changes: read once it shows, it is not asked
    /// again on a commit; a refusal is asked again by Retry.
    fn read_snapshot(&mut self, id: BlobId, cx: &mut Context<Self>) {
        if self.snapshot_at(id).is_some() {
            return;
        }
        self.snapshot = Loadable::Idle;
        let read = queries::snapshot(cx.host(), id);
        cx.load(self, read, |view| &mut view.snapshot);
        if self.names.is_idle() {
            self.read_names(cx);
        }
    }

    /// Every author's name: once, and again when identity changes.
    pub(crate) fn read_names(&mut self, cx: &mut Context<Self>) {
        let read = queries::names(cx.host());
        cx.load(self, read, |view| &mut view.names);
    }

    /// The reader's name, which names their home; asked again when the
    /// session names another account, or identity changes.
    pub(crate) fn read_reader(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.reader.account else {
            return;
        };
        let read = queries::name(cx.host(), account);
        cx.load(self, read, |view| &mut view.reader.name);
    }

    /// A session: the chain links are minted in, and another account
    /// reads, or the same one again.
    pub(crate) fn session_changed(&mut self, session: Session, cx: &mut Context<Self>) {
        if session.chain_id != self.chain {
            self.chain = session.chain_id;
            cx.notify();
        }
        if session.account == self.reader.account {
            return;
        }
        self.reader.account = session.account;
        self.reader.name = Loadable::Idle;
        // another reader's writes, offered or half filled, are not this one's
        self.sheet = None;
        self.note = None;
        self.read_reader(cx);
        // what this reader may write in the page on screen
        self.read_page(cx);
        cx.notify();
    }
}
