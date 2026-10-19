//! A draft's files as they go up (picked, pasted or dropped; put, then
//! committed to files under the draft's message), and the files messages
//! carry as the room reads them: each one's chunks to save, its picture.
use std::sync::Arc;

use chat::Block;
use chat::message::Attachment;
use files::{BlobId, Change, Content};
use futures::StreamExt;
use view::prelude::*;

use crate::composer::{Target, Upload, attachments, editor_id};
use crate::{Chat, Pane, words};

// What a file a message carries is, once read: its chunks, which Save
// hands the host, and its picture where it is one the app draws.
#[derive(Clone, Default)]
pub(crate) struct Shown {
    pub(crate) chunks: Option<Vec<[u8; 32]>>,
    pub(crate) picture: Option<Picture>,
}

// A picture a message carries: the image, and its width and height as its
// header names them, which every place that draws it fits to its room.
#[derive(Clone)]
pub(crate) struct Picture {
    pub(crate) image: Arc<Image>,
    pub(crate) size: (u32, u32),
}

fn release(file: Option<u64>, cx: &mut Context<Chat>) {
    if let Some(file) = file {
        cx.host().notify::<FileRelease>(vec![file]);
    }
}

fn release_all(files: &[Picked], cx: &mut Context<Chat>) {
    if !files.is_empty() {
        cx.host()
            .notify::<FileRelease>(files.iter().map(|file| file.file).collect());
    }
}

impl Chat {
    pub(crate) fn pick_files(&mut self, target: Target, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<FilePick>(Pick { multiple: true });
        cx.spawn(async move |this, cx| {
            let picked = ask.await;
            let _ = this.update(cx, |chat, cx| match picked {
                Ok(files) => chat.attach(&target, files, cx),
                Err(refusal) => chat.say(&target, design::sentence(&refusal), cx),
            });
        })
        .detach();
    }

    // The paste key: the clipboard's files attach; else its text goes in
    // over the selection.
    pub(crate) fn paste(&mut self, target: Target, cx: &mut Context<Self>) {
        let ask = cx.host().ask::<ClipboardFiles>(());
        cx.spawn(async move |this, cx| {
            let pasted = ask.await;
            let _ = this.update_in(cx, |chat, window, cx| match pasted {
                Ok(pasted) => chat.pasted(&target, pasted, window, cx),
                Err(refusal) => chat.say(&target, design::sentence(&refusal), cx),
            });
        })
        .detach();
    }

    fn pasted(
        &mut self,
        target: &Target,
        pasted: Pasted,
        window: &mut view::Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(target, Target::Post { .. }) && !pasted.files.is_empty() {
            return self.attach(target, pasted.files, cx);
        }
        // an edit takes no files: what it pastes is the text
        release_all(&pasted.files, cx);
        let key = target.key();
        let edit = self
            .drafts
            .get(&key)
            .and_then(|draft| draft.paste(&editor_id(&key), &pasted.text));
        if let Some(edit) = edit {
            window.dispatch(edit);
        }
        cx.notify();
    }

    fn say(&mut self, target: &Target, said: String, cx: &mut Context<Self>) {
        if let Some(draft) = self.drafts.get_mut(&target.key()) {
            draft.note = said;
        }
        cx.notify();
    }

    // Files the host holds for the draft of `target`, each put on the
    // network at once; an edit's composer takes none, and lets them go.
    pub(crate) fn attach(&mut self, target: &Target, files: Vec<Picked>, cx: &mut Context<Self>) {
        let posts = matches!(target, Target::Post { .. });
        let Some(draft) = self.drafts.get_mut(&target.key()).filter(|_| posts) else {
            return release_all(&files, cx);
        };
        let attached: Vec<_> = files.into_iter().map(|file| draft.attach(file)).collect();
        for (id, file, size) in attached {
            self.put(target, id, file, size, cx);
        }
        self.advance(target, cx);
        cx.notify();
    }

    fn put(&mut self, target: &Target, id: u64, file: u64, size: u64, cx: &mut Context<Self>) {
        // an empty file has no chunk to put
        if size == 0 {
            return self.putting(target, id, Ok(Putting::Held(Vec::new())), cx);
        }
        let blobs = attachments::blobs(file, size);
        let mut putting = cx.host().subscribe::<OpPut>(PutBlobs { blobs });
        let on = target.clone();
        let task = cx.spawn(async move |this, cx| {
            while let Some(item) = putting.next().await {
                let last = !matches!(item, Ok(Putting::Sent(_)));
                let told = this.update(cx, |chat, cx| chat.putting(&on, id, item, cx));
                if last || told.is_err() {
                    return;
                }
            }
            let closed = Error::new(view::methods::refusal::REQUEST_CLOSED, words::PUT_CLOSED);
            let _ = this.update(cx, |chat, cx| chat.putting(&on, id, Err(closed), cx));
        });
        self.uploads.insert((target.key(), id), task);
    }

    fn putting(
        &mut self,
        target: &Target,
        id: u64,
        item: Result<Putting, Error>,
        cx: &mut Context<Self>,
    ) {
        let file = self
            .drafts
            .get_mut(&target.key())
            .and_then(|draft| draft.attachments.iter_mut().find(|file| file.id == id));
        let Some(file) = file.filter(|file| matches!(file.upload, Upload::Putting { .. })) else {
            return;
        };
        match item {
            Ok(Putting::Sent(sent)) => file.upload = Upload::Putting { sent },
            Ok(Putting::Held(chunks)) => {
                file.upload = Upload::Held(chunks);
                release(file.file.take(), cx);
            }
            Err(refusal) => {
                file.upload = Upload::Failed(design::sentence(&refusal));
                release(file.file.take(), cx);
            }
        }
        self.advance(target, cx);
        cx.notify();
    }

    // Moves the draft's files on: the message id their folder is named by,
    // minted with the first; a commit for each one held; and a send that
    // waited, once every file it waited on is up, or back to the reader
    // when one failed.
    fn advance(&mut self, target: &Target, cx: &mut Context<Self>) {
        let uploader = self.my_account();
        let Some(draft) = self.drafts.get_mut(&target.key()) else {
            return;
        };
        let Some(message) = draft.message_id.clone() else {
            if !draft.minting && !draft.attachments.is_empty() {
                draft.minting = true;
                let ask = cx.host().ask::<HostId>("message".into());
                let on = target.clone();
                cx.spawn(async move |this, cx| {
                    let minted = ask.await;
                    let _ = this.update(cx, |chat, cx| chat.minted(&on, minted, cx));
                })
                .detach();
            }
            return;
        };
        let channel = target.channel();
        let commits: Vec<_> = draft
            .attachments
            .iter_mut()
            .filter_map(|file| {
                let Upload::Held(chunks) = &file.upload else {
                    return None;
                };
                let chunks = chunks.iter().copied().map(BlobId::Sha256).collect();
                let path = attachments::path(channel, &message, &file.name);
                let change = Change::Put {
                    path: path.clone(),
                    exec: false,
                    meta: attachments::meta(channel, &message, uploader, file),
                    content: Content::Chunks {
                        size: file.size,
                        chunks,
                    },
                };
                file.upload = Upload::Committing;
                Some((file.id, file.name.clone(), path, change))
            })
            .collect();
        draft
            .filed
            .extend(commits.iter().map(|(_, name, ..)| name.clone()));
        let failed = draft
            .attachments
            .iter()
            .find_map(|file| Some((file.name.clone(), file.failed()?.to_owned())));
        let mut post = None;
        if draft.queued.is_some() {
            if let Some((name, why)) = failed {
                if let Some((send, _)) = draft.queued.take() {
                    draft.failed(send);
                }
                draft.note = words::not_up(&name, &why);
            } else if !draft.uploading() {
                post = draft.queued.take();
            }
        }
        for (id, name, path, change) in commits {
            self.commit(target, id, path, change, words::attached(&name), cx);
        }
        if let Some((send, target)) = post {
            self.post_draft(target, send, cx);
        }
    }

    fn minted(&mut self, target: &Target, minted: Result<String, Error>, cx: &mut Context<Self>) {
        let key = target.key();
        let Some(draft) = self.drafts.get_mut(&key) else {
            return;
        };
        draft.minting = false;
        match minted {
            // a new message's folder holds nothing yet
            Ok(id) => {
                draft.message_id = Some(id);
                draft.filed.clear();
            }
            // nothing is filed under no message: each file not up says why
            // and is let go; the next one attached asks again
            Err(refusal) => {
                let why = design::sentence(&refusal);
                for file in &mut draft.attachments {
                    if file.landed().is_none() && file.failed().is_none() {
                        file.upload = Upload::Failed(why.clone());
                        release(file.file.take(), cx);
                        self.uploads.remove(&(key.clone(), file.id));
                    }
                }
            }
        }
        self.advance(target, cx);
        cx.notify();
    }

    fn commit(
        &mut self,
        target: &Target,
        id: u64,
        path: String,
        change: Change,
        said: String,
        cx: &mut Context<Self>,
    ) {
        // `base: None` only creates: a taken path is refused, never written over
        let op = files::Op::Commit {
            base: None,
            message: said,
            changes: vec![change],
        };
        let ask = cx.host().ask::<Submit<files::Files>>(op);
        let on = target.clone();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let landed = async {
                // files keeps a file under a chat message its uploader's
                // alone, so the commit is all an upload asks of a program
                if let Some(why) = design::failure(&ask.await) {
                    return Err(words::not_up_because(&why, &path));
                }
                let read = files::ask::Stat {
                    path: path.clone(),
                    at: None,
                };
                let (snapshot, entry) = host
                    .query(read)
                    .await
                    .map_err(|refusal| design::sentence(&refusal))?;
                match (snapshot, entry.map(|entry| entry.node)) {
                    (Some(BlobId::Sha256(snapshot)), Some(files::Node::File { .. })) => {
                        Ok(snapshot)
                    }
                    _ => Err(words::NOT_THERE.to_owned()),
                }
            }
            .await;
            let _ = this.update(cx, |chat, cx| chat.committed(&on, id, path, landed, cx));
        })
        .detach();
    }

    fn committed(
        &mut self,
        target: &Target,
        id: u64,
        path: String,
        landed: Result<[u8; 32], String>,
        cx: &mut Context<Self>,
    ) {
        let file = self
            .drafts
            .get_mut(&target.key())
            .and_then(|draft| draft.attachments.iter_mut().find(|file| file.id == id));
        let left: Option<String> = match file {
            Some(file) => {
                file.upload = match landed {
                    Ok(snapshot) => Upload::Landed(Attachment {
                        path,
                        snapshot,
                        name: file.name.clone(),
                        size: file.size,
                        content_type: file.content_type.clone(),
                    }),
                    Err(why) => Upload::Failed(why),
                };
                None
            }
            // taken out while its commit was out: what it left goes too
            None => landed.ok().map(|_| path),
        };
        if let Some(path) = left {
            self.unlink(target, path, cx);
        }
        self.advance(target, cx);
        cx.notify();
    }

    // Removes a file the draft no longer carries from files: the uploader's
    // own, at the files head (the snapshot it went up in is the node's
    // prediction, which a block may have minted under another id).
    fn unlink(&mut self, target: &Target, path: String, cx: &mut Context<Self>) {
        let name = path.rsplit('/').next().unwrap_or_default().to_owned();
        let host = cx.host();
        let on = target.clone();
        cx.spawn(async move |this, cx| {
            let removed = async {
                let head = host.query(files::ask::Head).await;
                let head = head.map_err(|refusal| design::sentence(&refusal))?;
                let op = files::Op::Commit {
                    base: head.map(|row| row.id),
                    message: words::unattached(&name),
                    changes: vec![Change::Remove { path }],
                };
                match design::failure(&host.ask::<Submit<files::Files>>(op).await) {
                    Some(said) => Err(said),
                    None => Ok(()),
                }
            }
            .await;
            if let Err(said) = removed {
                let _ = this.update(cx, |chat, cx| chat.say(&on, said, cx));
            }
        })
        .detach();
    }

    // A file taken out of the draft before it is sent: its put stops, the
    // host lets it go, and a committed one is removed again.
    pub(crate) fn remove_file(&mut self, target: &Target, id: u64, cx: &mut Context<Self>) {
        let key = target.key();
        let Some(draft) = self.drafts.get_mut(&key) else {
            return;
        };
        let Some(at) = draft
            .attachments
            .iter()
            .position(|file| file.id == id && !file.sent)
        else {
            return;
        };
        let mut gone = draft.attachments.remove(at);
        self.uploads.remove(&(key, id));
        release(gone.file.take(), cx);
        if let Upload::Landed(file) = gone.upload {
            self.unlink(target, file.path, cx);
        }
        self.advance(target, cx);
        cx.notify();
    }

    // The files the message at `seq` carries: an edit keeps them.
    pub(crate) fn kept_files(&self, pane: Pane, seq: u64) -> Vec<Attachment> {
        let messages = self.messages(pane);
        let Some(message) = messages.iter().find(|message| message.seq == seq) else {
            return Vec::new();
        };
        message
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Attachment(file) => Some(file.clone()),
                _ => None,
            })
            .collect()
    }

    // Reads each file the room's messages carry, once it is on screen: the
    // frame sees every way the rows arrive.
    pub(crate) fn settle_files(&mut self, cx: &mut Context<Self>) {
        let Some(room) = &self.room else {
            return;
        };
        let replies = room
            .thread
            .iter()
            .filter_map(|thread| thread.replies.ready())
            .flatten();
        let rows = room
            .messages
            .ready()
            .into_iter()
            .flatten()
            .chain(replies)
            .chain(&room.pending);
        let mut unread: Vec<Attachment> = Vec::new();
        for block in rows.filter(|row| !row.deleted).flat_map(|row| &row.blocks) {
            let Block::Attachment(file) = block else {
                continue;
            };
            let key = (file.path.clone(), file.snapshot);
            if !self.shown_files.contains_key(&key) && !unread.contains(file) {
                unread.push(file.clone());
            }
        }
        for file in unread {
            let key = (file.path.clone(), file.snapshot);
            self.shown_files.insert(key.clone(), Shown::default());
            let host = cx.host();
            cx.spawn(async move |this, cx| {
                let shown = shown(host, &file).await.unwrap_or_default();
                let _ = this.update(cx, |chat, cx| {
                    chat.shown_files.insert(key, shown);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    // Save: the host writes the file's chunks to where the person says.
    pub(crate) fn save_file(&mut self, file: &Attachment, cx: &mut Context<Self>) {
        let shown = self.shown_files.get(&(file.path.clone(), file.snapshot));
        let Some(blobs) = shown.and_then(|shown| shown.chunks.clone()) else {
            return;
        };
        let save = SaveFile {
            name: file.name.clone(),
            blobs,
        };
        let ask = cx.host().ask::<FileSave>(save);
        cx.spawn(async move |this, cx| {
            if let Err(refusal) = ask.await {
                let _ = this.update(cx, |chat, cx| {
                    chat.notice = design::sentence(&refusal);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(crate) fn enlarge(&mut self, file: &Attachment, cx: &mut Context<Self>) {
        self.enlarged = Some(file.clone());
        cx.notify();
    }

    // Files shows the file by the link to its path, beside the room, so the
    // picture over the room closes
    pub(crate) fn open_in_files(&mut self, path: &str, cx: &mut Context<Self>) {
        self.enlarged = None;
        match crate::links::file_link(&self.session.chain_id, path) {
            Some(url) => cx.host().open_link(&url),
            None => cx.host().log("no link to open: the session names no chain"),
        }
        cx.notify();
    }
}

// The file at the snapshot its message names: its chunks, from the file
// object files keeps, and the whole picture when it is one.
async fn shown(host: Host, file: &Attachment) -> Result<Shown, Error> {
    // the file at its path as it stands: the path is the message's own,
    // and only its uploader writes it (`commit` lists it so). The snapshot
    // a message names is the one the node predicted as the file went up (a
    // view reads the preconfirmed layer); the block that carried it may
    // have minted it under another id
    let at: Option<BlobId> = None;
    let stat = files::ask::Stat {
        path: file.path.clone(),
        at,
    };
    let (_, entry) = host.query(stat).await?;
    let Some(files::Node::File { file: object, .. }) = entry.map(|entry| entry.node) else {
        return Err(view::host::malformed(words::NOT_THERE.into()));
    };
    let id = format!("sha256:{}", abi::hex(object.digest()));
    let bytes = host
        .ask::<BlobGet>(id)
        .await?
        .ok_or_else(|| view::host::malformed(words::NOT_THERE.into()))?;
    let object: files::FileObj =
        abi::decode(&bytes).map_err(|refusal| view::host::malformed(refusal.sentence))?;
    let chunks = object
        .chunks
        .iter()
        .filter_map(|chunk| match chunk {
            BlobId::Sha256(digest) => Some(*digest),
            BlobId::Sha1(_) => None,
        })
        .collect();
    let drawn = file.content_type.starts_with("image/");
    let picture = match drawn {
        true => picture(&host, file, at).await?,
        false => None,
    };
    Ok(Shown {
        chunks: Some(chunks),
        picture,
    })
}

async fn picture(
    host: &Host,
    file: &Attachment,
    at: Option<BlobId>,
) -> Result<Option<Picture>, Error> {
    let mut bytes = Vec::new();
    loop {
        let read = files::ask::Read {
            path: file.path.clone(),
            at,
            offset: bytes.len() as u64,
            len: files::CHUNK_SIZE,
        };
        let contents = host.query(read).await?;
        // a read short of the size it states would never end
        if contents.bytes.is_empty() && !contents.eof {
            return Err(view::host::malformed(words::NOT_THERE.into()));
        }
        bytes.extend(contents.bytes);
        if contents.eof {
            break;
        }
    }
    let format = view::picture::picture_format(&bytes);
    let size = view::picture::picture_size(&bytes);
    Ok(format.zip(size).map(|(format, size)| Picture {
        image: Arc::new(Image::from_bytes(format, bytes)),
        size,
    }))
}
