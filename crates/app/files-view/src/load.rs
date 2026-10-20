//! The loaders: what the page on top of the stack shows, read with
//! `cx.load`, which keeps what is on screen while it reads again and draws
//! only what changed; and who reads, the name their home is called by.
use view::prelude::*;

use files::BlobId;

use crate::state::{Opened, Route, parent_of};
use crate::{Files, queries};

impl Files {
    pub(crate) fn read_page(&mut self, cx: &mut Context<Self>) {
        self.read(false, cx);
    }

    pub(crate) fn read_all(&mut self, cx: &mut Context<Self>) {
        self.read(true, cx);
    }

    fn read(&mut self, all: bool, cx: &mut Context<Self>) {
        let page = self.page().clone();
        if all
            || page == Route::History
            || matches!(*self.history, Loadable::Idle | Loadable::Failed(_))
        {
            self.read_history(cx);
        }
        match page {
            Route::Places | Route::History => {}
            Route::Folder(path) => self.read_folder(path, !all, cx),
            Route::File(path) => {
                let folder = parent_of(&path);
                if all || self.folder_at(folder).is_none() {
                    self.read_folder(folder.to_owned(), !all, cx);
                }
                self.read_file(path, !all, cx);
            }
            Route::Snapshot(id) => self.read_snapshot(id, cx),
        }
    }

    fn read_folder(&mut self, path: String, moved: bool, cx: &mut Context<Self>) {
        let another = match self.folder.ready() {
            Some(shown) => shown.path != path,
            None => moved,
        };
        if another {
            *self.folder = Loadable::Idle;
        }
        cx.reread(
            self,
            |view| &mut view.folder,
            |view, cx| queries::folder(cx.host(), path, view.reader.account),
            Self::reread_folder,
        );
    }

    fn reread_folder(&mut self, cx: &mut Context<Self>) {
        match self.page().clone() {
            Route::Folder(path) => self.read_folder(path, false, cx),
            Route::File(path) => self.read_folder(parent_of(&path).to_owned(), false, cx),
            Route::Places | Route::History | Route::Snapshot(_) => {}
        }
    }

    fn read_file(&mut self, path: String, moved: bool, cx: &mut Context<Self>) {
        let shown = self.file_at(&path);
        let kept = shown.and_then(Opened::kept_picture);
        let another = match self.file.ready() {
            Some(shown) => shown.path != path,
            None => moved,
        };
        if another {
            *self.file = Loadable::Idle;
        }
        cx.reread(
            self,
            |view| &mut view.file,
            move |view, cx| queries::file(cx.host(), path, view.reader.account, kept),
            Self::reread_file,
        );
    }

    fn reread_file(&mut self, cx: &mut Context<Self>) {
        if let Route::File(path) = self.page().clone() {
            self.read_file(path, false, cx);
        }
    }

    fn read_history(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.history,
            |_, cx| queries::history(cx.host()),
            Self::read_history,
        );
        if self.names.is_idle() {
            self.read_names(cx);
        }
    }

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

    pub(crate) fn read_names(&mut self, cx: &mut Context<Self>) {
        cx.reread(
            self,
            |view| &mut view.names,
            |_, cx| queries::names(cx.host()),
            Self::read_names,
        );
    }

    pub(crate) fn read_reader(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.reader.account else {
            return;
        };
        cx.reread(
            self,
            |view| &mut view.reader.name,
            move |_, cx| queries::name(cx.host(), account),
            Self::read_reader,
        );
    }

    pub(crate) fn session_changed(&mut self, session: Session, cx: &mut Context<Self>) {
        if session.chain_id != self.chain {
            self.chain = session.chain_id;
            cx.notify();
        }
        if session.account == self.reader.account {
            return;
        }
        self.reader.account = session.account;
        *self.reader.name = Loadable::Idle;
        self.sheet = None;
        self.note = None;
        self.read_reader(cx);
        self.read_page(cx);
        cx.notify();
    }
}
