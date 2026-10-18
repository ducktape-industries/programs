//! An upload: files the reader picks on their device (`file.pick`), put in
//! a folder by their own names in one commit. A file's bytes never ride in
//! a transaction: each file goes to the network first as `chunk` blobs
//! (`op.put`, the host reading them from the picked file, never through
//! this view), and the commit names its chunks ([`Files::submit`]). A file
//! larger than [`UPLOAD_MOST`], one whose name `files` takes no path from,
//! one whose name a folder or a link holds there, or a second of one name
//! is left out and says why. Opened by [`Files::pick`].
use files::{Change, Content, EntryInfo, Node};
use view::methods::{BlobBody, FilePick, NewBlob, Pick, Picked};
use view::prelude::*;

use crate::state::Note;
use crate::write::{Write, path_in};
use crate::{Files, words};

/// The most a file may hold to be uploaded here.
pub const UPLOAD_MOST: u64 = 32 << 20;

/// One picked file, as the upload's form holds it.
pub(crate) struct Upload {
    /// its own name on the reader's device
    pub(crate) name: String,
    pub(crate) size: u64,
    /// the handle the host reads it by
    file: u64,
    /// where it goes, or why it is left out
    pub(crate) path: Result<String, String>,
    /// a file of its name is in the folder: this one replaces it
    pub(crate) replaces: bool,
}

impl Upload {
    /// Its blobs: a `chunk` per [`files::CHUNK_SIZE`] of it, in order, each
    /// read by the host from the file picked.
    pub(crate) fn blobs(&self) -> Vec<NewBlob> {
        (0..self.size.div_ceil(files::CHUNK_SIZE))
            .map(|index| {
                let offset = index * files::CHUNK_SIZE;
                NewBlob {
                    kind: files::CHUNK.into(),
                    body: BlobBody::Picked {
                        file: self.file,
                        offset,
                        len: files::CHUNK_SIZE.min(self.size - offset),
                    },
                }
            })
            .collect()
    }
}

/// The files picked, as the form holds them: each one's path in `folder`
/// or why it is left out. `listed` is what the folder holds already.
pub(crate) fn uploads(folder: &str, picked: Vec<Picked>, listed: &[EntryInfo]) -> Vec<Upload> {
    let mut named: Vec<String> = Vec::new();
    picked
        .into_iter()
        .map(|picked| {
            let path = match picked.size {
                size if size > UPLOAD_MOST => Err(words::too_big_to_upload(size)),
                _ => path_in(folder, &picked.name),
            };
            let there = path
                .as_ref()
                .ok()
                .and_then(|path| listed.iter().find(|entry| entry.path == *path));
            let path = path.and_then(|path| match there.map(|entry| &entry.node) {
                // a file in its place is replaced; a folder or a link is not
                Some(node @ (Node::Dir { .. } | Node::Symlink { .. })) => Err(words::held_by(node)),
                _ if named.contains(&path) => Err(words::PICKED_TWICE.into()),
                _ => Ok(path),
            });
            if let Ok(path) = &path {
                named.push(path.clone());
            }
            Upload {
                name: picked.name,
                size: picked.size,
                file: picked.file,
                replaces: path.is_ok() && there.is_some(),
                path,
            }
        })
        .collect()
}

/// The files the upload sends, in order, each with its path: every one
/// not left out.
pub(crate) fn going(uploads: &[Upload]) -> impl Iterator<Item = (&Upload, &str)> {
    uploads
        .iter()
        .filter_map(|upload| Some((upload, upload.path.as_deref().ok()?)))
}

/// The names of the files the upload sends.
pub(crate) fn sent(uploads: &[Upload]) -> Vec<&str> {
    going(uploads)
        .map(|(upload, _)| upload.name.as_str())
        .collect()
}

pub(crate) fn handles(uploads: &[Upload]) -> Vec<u64> {
    uploads.iter().map(|upload| upload.file).collect()
}

/// What the upload's commit puts: each file it sends, with its content
/// (`content(n)` for the `n`th); or why it puts nothing.
pub(crate) fn puts(
    uploads: &[Upload],
    content: impl Fn(usize) -> Content,
) -> Result<Vec<Change>, String> {
    let puts: Vec<Change> = going(uploads)
        .enumerate()
        .map(|(index, (_, path))| Change::Put {
            path: path.to_owned(),
            exec: false,
            meta: Default::default(),
            content: content(index),
        })
        .collect();
    match puts.is_empty() {
        true => Err(words::NOTHING_TO_UPLOAD.into()),
        false => Ok(puts),
    }
}

impl Files {
    /// Asks the reader for files on their device, to upload into `folder`;
    /// picked, their form opens. Only a press reaches here: the host opens
    /// its picker for nothing else.
    pub(crate) fn pick(&mut self, folder: String, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let ask = cx.host().ask::<FilePick>(Pick { multiple: true });
        cx.land(ask, move |view, answer, cx| view.picked(folder, answer, cx))
            .detach();
    }

    /// The reader picked files, or none; or the host could not ask.
    fn picked(
        &mut self,
        folder: String,
        answer: Result<Vec<Picked>, Error>,
        cx: &mut Context<Self>,
    ) {
        self.picking = false;
        let picked = match answer {
            Ok(picked) => picked,
            Err(refusal) => {
                self.note = Some(Note::Refused(design::sentence(&refusal)));
                cx.notify();
                return;
            }
        };
        // cancelled
        if picked.is_empty() {
            return;
        }
        // the folder left the screen while the picker was open: what it
        // holds, and so what a file would replace, is not known
        let Some(listed) = self.folder_at(&folder) else {
            self.note = Some(Note::Refused(words::FOLDER_LEFT.into()));
            cx.notify();
            return;
        };
        let base = listed.snapshot;
        let uploads = uploads(&folder, picked, &listed.entries);
        self.open_upload(Write::Upload { folder }, base, uploads, cx);
    }
}
