//! The writes `files` takes from this view, each one commit with a
//! message: a new folder, a new text file, files uploaded from the
//! reader's device (`upload.rs`), a text file's new text, a rename or
//! move, a removal. Which of them the page on screen offers is
//! [`Files::offered`], made of what a folder takes in it
//! ([`Files::made_in`]) and what a path takes itself ([`Files::own`]),
//! which a row's menu offers too (`menu.rs`); a form opens with
//! [`Files::begin`], its changes are built by [`built`] (pure, unit
//! tested), sent by [`Files::submit`] and settled by `landed`. A file's
//! bytes never ride in the commit: each file it puts goes to the network
//! first as `chunk` blobs (`op.put`, answered once the network holds
//! them), and the commit names its chunks. The app's confirm card is the
//! yes for every op; nothing here asks again.
use std::collections::BTreeMap;

use files::{BlobId, Change, Content, Node, Ownership, Party};
use view::methods::{BlobBody, FileRelease, NewBlob, PutBlobs, Submitted};
use view::prelude::*;

use crate::state::{Note, Route, Sheet, child_of, is_namespace_root, parent_of};
use crate::upload::{self, Upload};
use crate::{Files, words};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Write {
    NewFolder {
        folder: String,
    },
    NewFile {
        folder: String,
    },
    Upload {
        folder: String,
    },
    Edit {
        path: String,
        exec: bool,
        meta: BTreeMap<String, String>,
    },
    Rename {
        path: String,
    },
    Remove {
        path: String,
        folder: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ListChoice {
    #[default]
    AsItsFolder,
    OnlyMe,
    Listed,
    Anyone,
}

impl ListChoice {
    pub(crate) const ALL: [ListChoice; 4] = [
        ListChoice::AsItsFolder,
        ListChoice::OnlyMe,
        ListChoice::Listed,
        ListChoice::Anyone,
    ];

    pub(crate) fn slug(self) -> &'static str {
        match self {
            ListChoice::AsItsFolder => "folder",
            ListChoice::OnlyMe => "me",
            ListChoice::Listed => "listed",
            ListChoice::Anyone => "anyone",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Naming {
    Name,
    Path,
}

impl Write {
    pub(crate) fn on(&self) -> &str {
        match self {
            Write::NewFolder { folder } | Write::NewFile { folder } | Write::Upload { folder } => {
                folder
            }
            Write::Edit { path, .. } | Write::Rename { path } | Write::Remove { path, .. } => path,
        }
    }

    pub(crate) fn naming(&self) -> Option<Naming> {
        match self {
            Write::NewFolder { .. } | Write::NewFile { .. } => Some(Naming::Name),
            Write::Rename { .. } => Some(Naming::Path),
            Write::Upload { .. } | Write::Edit { .. } | Write::Remove { .. } => None,
        }
    }

    pub(crate) fn takes_list(&self) -> bool {
        matches!(self, Write::NewFolder { .. } | Write::NewFile { .. })
    }

    pub(crate) fn takes_text(&self) -> bool {
        matches!(self, Write::NewFile { .. } | Write::Edit { .. })
    }

    pub(crate) fn slug(&self) -> &'static str {
        match self {
            Write::NewFolder { .. } => "new-folder",
            Write::NewFile { .. } => "new-file",
            Write::Upload { .. } => "upload",
            Write::Edit { .. } => "edit",
            Write::Rename { .. } => "rename",
            Write::Remove { .. } => "remove",
        }
    }

    pub(crate) fn glyph(&self) -> &'static str {
        match self {
            Write::NewFolder { .. } => "folder-plus",
            Write::NewFile { .. } => "file-plus",
            Write::Upload { .. } => "upload",
            Write::Edit { .. } => "pencil",
            Write::Rename { .. } => "folder-input",
            Write::Remove { .. } => "trash",
        }
    }
}

pub(crate) struct Form {
    pub(crate) id: u64,
    pub(crate) write: Write,
    pub(crate) base: Option<BlobId>,
    pub(crate) name: TextField,
    pub(crate) text: TextField,
    pub(crate) upload: Vec<Upload>,
    pub(crate) contents: Vec<Content>,
    pub(crate) list: ListChoice,
    pub(crate) listed: TextField,
    pub(crate) message: TextField,
    pub(crate) problem: Option<String>,
    pub(crate) refused: Option<String>,
    pub(crate) sending: bool,
}

impl Form {
    pub(crate) fn committable(&self) -> bool {
        let empty =
            matches!(self.write, Write::Upload { .. }) && upload::sent(&self.upload).is_empty();
        !self.sending && !empty
    }

    pub(crate) fn progress(&self) -> Option<(u64, u64)> {
        let Write::Upload { .. } = self.write else {
            return None;
        };
        let all = upload::going(&self.upload).count() as u64;
        Some((self.contents.len() as u64, all))
    }

    fn content(&self, index: usize) -> Content {
        self.contents
            .get(index)
            .cloned()
            .unwrap_or(Content::Chunks {
                size: 0,
                chunks: Vec::new(),
            })
    }

    pub(crate) fn placeholder(&self) -> String {
        if let Write::Upload { .. } = self.write {
            return words::uploads(&upload::sent(&self.upload));
        }
        built(self).map_or_else(|_| words::MESSAGE.to_owned(), |(message, _)| message)
    }
}

pub(crate) fn files_of(form: &Form) -> Vec<Vec<NewBlob>> {
    match &form.write {
        Write::NewFile { .. } | Write::Edit { .. } => {
            let text = form.text.text().into_bytes();
            let chunks = text
                .chunks(files::CHUNK_SIZE as usize)
                .map(|chunk| NewBlob {
                    kind: files::CHUNK.into(),
                    body: BlobBody::Bytes(chunk.to_vec()),
                });
            vec![chunks.collect()]
        }
        Write::Upload { .. } => upload::going(&form.upload)
            .map(|(upload, _)| upload.blobs())
            .collect(),
        Write::NewFolder { .. } | Write::Rename { .. } | Write::Remove { .. } => Vec::new(),
    }
}

fn size(blobs: &[NewBlob]) -> u64 {
    blobs
        .iter()
        .map(|blob| match &blob.body {
            BlobBody::Bytes(bytes) => bytes.len() as u64,
            BlobBody::Picked { len, .. } => *len,
        })
        .sum()
}

struct Sent {
    form: u64,
    changes: Vec<Change>,
    message: String,
    list: Option<access::Op>,
}

impl Files {
    pub(crate) fn offered(&self) -> Vec<Write> {
        match self.page() {
            Route::Folder(path) => {
                let mut offered = self.made_in(path);
                offered.extend(self.own(path));
                offered
            }
            Route::File(path) => self.own(path),
            Route::Places | Route::History | Route::Snapshot(_) => Vec::new(),
        }
    }

    pub(crate) fn made_in(&self, folder: &str) -> Vec<Write> {
        let Some(listed) = self.folder_at(folder) else {
            return Vec::new();
        };
        if !self.reader.writes(&listed.children()) || listed.holds_namespace_roots() {
            return Vec::new();
        }
        let folder = folder.to_owned();
        vec![
            Write::NewFolder {
                folder: folder.clone(),
            },
            Write::NewFile {
                folder: folder.clone(),
            },
            Write::Upload { folder },
        ]
    }

    pub(crate) fn own(&self, path: &str) -> Vec<Write> {
        if let Some(file) = self.file_at(path) {
            let (Some(entry), Some(own)) = (file.file(), &file.writers) else {
                return Vec::new();
            };
            if !self.reader.writes(own) || is_namespace_root(path) {
                return Vec::new();
            }
            let mut writes = Vec::new();
            if let (Node::File { exec, .. }, true) = (&entry.node, file.editable()) {
                writes.push(Write::Edit {
                    path: path.to_owned(),
                    exec: *exec,
                    meta: entry.meta.clone(),
                });
            }
            writes.extend(moved(path, false));
            return writes;
        }
        if let Some(folder) = self.folder_at(path) {
            let own = folder.writers.as_ref();
            let moves = own.is_some_and(|own| self.reader.writes(own));
            return match moves && !is_namespace_root(path) {
                true => moved(path, true),
                false => Vec::new(),
            };
        }
        let Some(listed) = self.folder_at(parent_of(path)) else {
            return Vec::new();
        };
        let Some(entry) = listed.entries.iter().find(|entry| entry.path == path) else {
            return Vec::new();
        };
        match self.reader.writes(&listed.children()) && !is_namespace_root(path) {
            true => moved(path, matches!(entry.node, Node::Dir { .. })),
            false => Vec::new(),
        }
    }

    fn read_at(&self, path: &str) -> Option<BlobId> {
        match self.file_at(path) {
            Some(file) => file.snapshot,
            None => self
                .folder_at(path)
                .or_else(|| self.folder_at(parent_of(path)))
                .and_then(|folder| folder.snapshot),
        }
    }

    pub(crate) fn begin(&mut self, write: Write, cx: &mut Context<Self>) {
        if let Write::Upload { folder } = write {
            return self.pick(folder, cx);
        }
        let base = self.read_at(write.on());
        let name = match &write {
            Write::Rename { path } => path.clone(),
            _ => String::new(),
        };
        let text = match &write {
            Write::Edit { path, .. } => self.file_at(path).and_then(|file| file.text()),
            _ => None,
        };
        let text = text.unwrap_or_default().to_owned();
        self.forms += 1;
        self.sheet = Some(Sheet::Write(Box::new(Form {
            id: self.forms,
            write,
            base,
            name: TextField::new(name),
            text: TextField::new(text),
            upload: Vec::new(),
            contents: Vec::new(),
            list: ListChoice::default(),
            listed: TextField::default(),
            message: TextField::default(),
            problem: None,
            refused: None,
            sending: false,
        })));
        cx.notify();
    }

    pub(crate) fn pick_list(&mut self, choice: ListChoice, cx: &mut Context<Self>) {
        let Some(Sheet::Write(form)) = &mut self.sheet else {
            return;
        };
        form.list = choice;
        cx.notify();
    }

    pub(crate) fn open_upload(
        &mut self,
        write: Write,
        base: Option<BlobId>,
        upload: Vec<Upload>,
        cx: &mut Context<Self>,
    ) {
        if let Some(Sheet::Write(old)) = &self.sheet {
            release(&old.upload, cx);
        }
        self.forms += 1;
        self.sheet = Some(Sheet::Write(Box::new(Form {
            id: self.forms,
            write,
            base,
            name: TextField::default(),
            text: TextField::default(),
            upload,
            contents: Vec::new(),
            list: ListChoice::default(),
            listed: TextField::default(),
            message: TextField::default(),
            problem: None,
            refused: None,
            sending: false,
        })));
        cx.notify();
    }

    pub(crate) fn submit(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::Write(form)) = &mut self.sheet else {
            return;
        };
        if form.sending {
            return;
        }
        form.problem = None;
        form.refused = None;
        form.upload.iter_mut().for_each(|file| file.refused = None);
        if form.write.takes_text() {
            form.contents.clear();
        }
        if let Err(problem) = built(form).and_then(|_| write_list(form)) {
            form.problem = Some(problem);
            cx.notify();
            return;
        }
        self.send(cx);
    }

    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::Write(form)) = &mut self.sheet else {
            return;
        };
        form.sending = true;
        if let Some(blobs) = files_of(form).into_iter().nth(form.contents.len()) {
            let size = size(&blobs);
            if blobs.is_empty() {
                form.contents.push(Content::Chunks {
                    size,
                    chunks: Vec::new(),
                });
                return self.send(cx);
            }
            let id = form.id;
            let ask = cx.host().put(PutBlobs { blobs });
            cx.land(ask, move |view, answer, cx| view.held(id, size, answer, cx))
                .detach();
            cx.notify();
            return;
        }
        let made =
            built(form).and_then(|(message, changes)| Ok((message, changes, write_list(form)?)));
        let (message, changes, list) = match made {
            Ok(made) => made,
            Err(problem) => {
                form.sending = false;
                form.problem = Some(problem);
                cx.notify();
                return;
            }
        };
        let op = files::Op::Commit {
            base: form.base,
            message: message.clone(),
            changes: changes.clone(),
        };
        let sent = Sent {
            form: form.id,
            changes,
            message,
            list,
        };
        let ask = cx.host().ask::<Submit<files::Files>>(op);
        cx.land(ask, move |view, answer, cx| view.landed(sent, answer, cx))
            .detach();
        cx.notify();
    }

    fn held(
        &mut self,
        form: u64,
        size: u64,
        answer: Result<Vec<[u8; 32]>, Error>,
        cx: &mut Context<Self>,
    ) {
        let Some(Sheet::Write(open)) = &mut self.sheet else {
            return self.stopped(cx);
        };
        if open.id != form {
            return self.stopped(cx);
        }
        match answer {
            Ok(digests) => {
                let chunks = digests.into_iter().map(BlobId::Sha256).collect();
                open.contents.push(Content::Chunks { size, chunks });
                self.send(cx);
            }
            Err(refusal) => {
                open.sending = false;
                let why = design::sentence(&refusal);
                let at = open.contents.len();
                let mut going = open.upload.iter_mut().filter(|file| file.path.is_ok());
                match going.nth(at) {
                    Some(file) => file.refused = Some(why),
                    None => open.refused = Some(why),
                }
                cx.notify();
            }
        }
    }

    fn stopped(&mut self, cx: &mut Context<Self>) {
        self.note = Some(Note::Refused(words::STOPPED.into()));
        cx.notify();
    }

    fn landed(&mut self, sent: Sent, answer: Result<Submitted, Error>, cx: &mut Context<Self>) {
        let problem = design::failure(&answer);
        let form = match &mut self.sheet {
            Some(Sheet::Write(form)) if form.id == sent.form => Some(form),
            _ => None,
        };
        match (problem, form) {
            (None, form) => {
                if let Some(form) = form {
                    release(&form.upload, cx);
                    self.sheet = None;
                }
                sent.changes.iter().for_each(|change| self.follow(change));
                self.note = Some(Note::Committed(sent.message));
                self.read_all(cx);
                if let Some(op) = sent.list {
                    self.set_list(op, cx);
                }
            }
            (Some(problem), Some(form)) => {
                form.sending = false;
                form.refused = Some(problem);
            }
            (Some(problem), None) => self.note = Some(Note::Refused(problem)),
        }
        cx.notify();
    }

    fn set_list(&mut self, op: access::Op, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<Submit<access::Access>>(op);
        cx.land(ask, |view, answer, cx| view.list_set(answer, cx))
            .detach();
    }

    fn list_set(&mut self, answer: Result<Submitted, Error>, cx: &mut Context<Self>) {
        match design::failure(&answer) {
            None => self.read_all(cx),
            Some(problem) => self.note = Some(Note::Refused(problem)),
        }
        cx.notify();
    }

    fn follow(&mut self, change: &Change) {
        match (self.page().clone(), change) {
            (Route::File(shown), Change::Move { from, to }) if shown == *from => {
                self.pages.replace(Route::File(to.clone()))
            }
            (Route::Folder(shown), Change::Move { from, to }) if shown == *from => {
                self.pages.replace(Route::Folder(to.clone()))
            }
            (Route::File(shown) | Route::Folder(shown), Change::Remove { path })
                if shown == *path =>
            {
                self.pages.pop();
            }
            _ => {}
        }
    }
}

fn release(uploads: &[Upload], cx: &mut Context<Files>) {
    let handles = upload::handles(uploads);
    if !handles.is_empty() {
        cx.host().notify::<FileRelease>(handles);
    }
}

fn moved(path: &str, folder: bool) -> Vec<Write> {
    vec![
        Write::Rename {
            path: path.to_owned(),
        },
        Write::Remove {
            path: path.to_owned(),
            folder,
        },
    ]
}

pub(crate) fn built(form: &Form) -> Result<(String, Vec<Change>), String> {
    let text = || form.content(0);
    let change = match &form.write {
        Write::NewFolder { folder } => Change::Mkdir {
            path: path_in(folder, &form.name.text())?,
        },
        Write::NewFile { folder } => Change::Put {
            path: path_in(folder, &form.name.text())?,
            exec: false,
            meta: BTreeMap::new(),
            content: text(),
        },
        Write::Upload { .. } => {
            let puts = upload::puts(&form.upload, |index| form.content(index))?;
            let said = words::uploads(&upload::sent(&form.upload));
            return Ok((message(form, said), puts));
        }
        Write::Edit { path, exec, meta } => Change::Put {
            path: path.clone(),
            exec: *exec,
            meta: meta.clone(),
            content: text(),
        },
        Write::Rename { path } => {
            let to = form.name.text().trim().to_owned();
            checked(&to)?;
            if to == *path {
                return Err(words::SAME_PATH.into());
            }
            Change::Move {
                from: path.clone(),
                to,
            }
        }
        Write::Remove { path, .. } => Change::Remove { path: path.clone() },
    };
    Ok((message(form, words::message(&change)), vec![change]))
}

pub(crate) fn write_list(form: &Form) -> Result<Option<access::Op>, String> {
    let folder = match &form.write {
        Write::NewFolder { folder } | Write::NewFile { folder } => folder,
        Write::Upload { .. } | Write::Edit { .. } | Write::Rename { .. } | Write::Remove { .. } => {
            return Ok(None);
        }
    };
    let list = match form.list {
        ListChoice::AsItsFolder => return Ok(None),
        ListChoice::OnlyMe => Ownership::Owner,
        ListChoice::Anyone => Ownership::Anyone,
        ListChoice::Listed => {
            let parties = parties(&form.listed.text());
            if parties.is_empty() {
                return Err(words::LIST_SOMEONE.into());
            }
            Ownership::Listed(parties)
        }
    };
    Ok(Some(access::Op::Set {
        program: files::PROGRAM.into(),
        key: path_in(folder, &form.name.text())?,
        list: Some(list),
    }))
}

pub(crate) fn parties(typed: &str) -> Vec<Party> {
    typed
        .split([',', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| match word.parse::<u64>() {
            Ok(number) => Party::Account(number),
            Err(_) => Party::Program(word.to_owned()),
        })
        .collect()
}

fn message(form: &Form, said: String) -> String {
    let typed = form.message.text().trim().to_owned();
    match typed.is_empty() {
        true => said,
        false => typed,
    }
}

pub(crate) fn path_in(folder: &str, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(words::NAME_IT.into());
    }
    if name.contains('/') {
        return Err(words::ONE_NAME.into());
    }
    let path = child_of(folder, name);
    checked(&path)?;
    Ok(path)
}

fn checked(path: &str) -> Result<(), String> {
    files::paths::change_path(path)
        .map(drop)
        .map_err(|refusal| refusal.message)
}
