//! A draft's files as they go up (picked, pasted or dropped; put, then committed to files under the
//! draft's message, landed once a block holds them), and the files messages carry, read from files
//! alone, by the path and snapshot the message names: each one's chunks to save, its picture.
use std::sync::Arc;

use chat::Block;
use chat::message::Attachment;
use files::{BlobId, Change, Content};
use futures::StreamExt;
use view::prelude::*;

use crate::composer::{Target, Upload, attachments, editor_id};
use crate::{Chat, Pane, words};

#[derive(Clone, Default)]
pub(crate) struct Shown {
    pub(crate) chunks: Option<Vec<[u8; 32]>>,
    pub(crate) picture: Option<Picture>,
}

pub(crate) type FileKey = (String, [u8; 32]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unread {
    Absent,
    Failed,
    Reading,
    ReadingAgain,
    Retrying,
    Refused,
}

enum Found {
    Shown(Shown),
    Absent(Option<BlobId>),
}

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

    fn advance(&mut self, target: &Target, cx: &mut Context<Self>) {
        let uploader = self.my_account();
        let Some(draft) = self.drafts.get_mut(&target.key()) else {
            return;
        };
        let Some(message) = draft.message_id.clone() else {
            let filing = draft.attachments.iter().any(|file| file.failed().is_none());
            let mints = !draft.minting && filing;
            if mints {
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
            Ok(id) => {
                draft.message_id = Some(id);
                draft.filed.clear();
            }
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
        let op = files::Op::Commit {
            base: None,
            message: said,
            changes: vec![change],
        };
        let ask = cx.host().ask::<Submit<files::Files>>(op);
        let on = target.clone();
        cx.spawn(async move |this, cx| {
            let host = cx.host();
            let mut moved = host.subscribe::<Changes<files::Files>>(());
            let landed = async {
                let answer = ask.await;
                let until = match &answer {
                    Ok(
                        view::methods::Submitted::Applied { until, .. }
                        | view::methods::Submitted::InDoubt { until, .. },
                    ) => *until,
                    _ => {
                        let why = design::failure(&answer).unwrap_or_default();
                        return Err(words::not_up_because(&why, &path));
                    }
                };
                let mut past = false;
                loop {
                    if let Some(snapshot) = held_at(&host, &path).await? {
                        return Ok(snapshot);
                    }
                    if past {
                        return Err(words::NOT_LANDED.to_owned());
                    }
                    past = match moved.next().await {
                        Some(Ok(change)) => change.is_some_and(|change| change.height > until),
                        Some(Err(refusal)) => return Err(design::sentence(&refusal)),
                        None => true,
                    };
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
            None => landed.ok().map(|_| path),
        };
        if let Some(path) = left {
            self.unlink(target, path, cx);
        }
        self.advance(target, cx);
        cx.notify();
    }

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
            let known = self.shown_files.contains_key(&key) || self.unread_files.contains_key(&key);
            let wanted = !known && !unread.contains(file);
            if wanted {
                unread.push(file.clone());
            }
        }
        for file in unread {
            self.read_file(file, cx);
        }
        self.follow_unread(cx);
    }

    fn read_file(&mut self, file: Attachment, cx: &mut Context<Self>) {
        let key = (file.path.clone(), file.snapshot);
        self.unread_files
            .insert(key.clone(), (file.clone(), Unread::Reading));
        let host = cx.host();
        cx.spawn(async move |this, cx| {
            let found = read_attachment(host, &file).await;
            let _ = this.update(cx, |chat, cx| chat.file_read(key, found, cx));
        })
        .detach();
    }

    fn file_read(&mut self, key: FileKey, found: Result<Found, Error>, cx: &mut Context<Self>) {
        let Some((file, was)) = self.unread_files.remove(&key) else {
            return;
        };
        let now = match found {
            Ok(Found::Shown(shown)) => {
                self.shown_files.insert(key.clone(), shown);
                None
            }
            Ok(Found::Absent(at)) => {
                if self.files_seen.is_none() {
                    self.files_seen = at;
                }
                Some(Unread::Absent)
            }
            Err(_) => Some(match was {
                Unread::Retrying => Unread::Refused,
                _ => Unread::Failed,
            }),
        };
        match (now, was) {
            (Some(_), Unread::ReadingAgain) => self.read_file(file, cx),
            (Some(now), _) => {
                self.unread_files.insert(key, (file, now));
            }
            (None, _) => {}
        }
        self.follow_unread(cx);
        cx.notify();
    }

    fn follow_unread(&mut self, cx: &mut Context<Self>) {
        if self.unread_files.is_empty() {
            self.files_moved = None;
            self.files_seen = None;
            return;
        }
        if self.files_moved.is_some() {
            return;
        }
        let follow = cx.follow::<Changes<files::Files>>((), |chat, change, cx| match change {
            Ok(Some(_)) => chat.files_block(cx),
            Ok(None) => chat.files_reopened(cx),
            Err(refusal) => cx.log_refused(words::FILES_MOVED, &refusal),
        });
        self.files_moved = Some(follow);
    }

    fn files_block(&mut self, cx: &mut Context<Self>) {
        let failed: Vec<Attachment> = self
            .unread_files
            .values()
            .filter(|(_, unread)| *unread == Unread::Failed)
            .map(|(file, _)| file.clone())
            .collect();
        if !failed.is_empty() {
            cx.notify();
        }
        for file in failed {
            let key = (file.path.clone(), file.snapshot);
            self.read_file(file, cx);
            if let Some((_, unread)) = self.unread_files.get_mut(&key) {
                *unread = Unread::Retrying;
            }
        }
        if self.files_seen.is_none() {
            return self.read_again(false, cx);
        }
        match self.files_diffing {
            true => self.files_diff_again = true,
            false => self.diff_unread(cx),
        }
    }

    fn files_reopened(&mut self, cx: &mut Context<Self>) {
        self.read_again(true, cx);
    }

    fn read_again(&mut self, refused_too: bool, cx: &mut Context<Self>) {
        let again: Vec<Attachment> = self
            .unread_files
            .values_mut()
            .filter_map(|(file, unread)| match *unread {
                Unread::Reading | Unread::ReadingAgain => {
                    *unread = Unread::ReadingAgain;
                    None
                }
                Unread::Retrying if refused_too => {
                    *unread = Unread::ReadingAgain;
                    None
                }
                Unread::Absent => Some(file.clone()),
                Unread::Failed | Unread::Refused if refused_too => Some(file.clone()),
                Unread::Retrying | Unread::Failed | Unread::Refused => None,
            })
            .collect();
        if !again.is_empty() {
            cx.notify();
        }
        for file in again {
            self.read_file(file, cx);
        }
    }

    fn diff_unread(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.files_seen else {
            return;
        };
        let waiting: Vec<&str> = self
            .unread_files
            .iter()
            .filter(|(_, (_, unread))| !matches!(unread, Unread::Failed | Unread::Refused))
            .map(|((path, _), _)| path.as_str())
            .collect();
        if waiting.is_empty() {
            return;
        }
        let prefix = folder_of(&waiting);
        self.files_diffing = true;
        let host = cx.host();
        cx.spawn(async move |this, cx| {
            let touched = async {
                let Some(head) = host.query(files::ask::Head).await? else {
                    return Ok((from, Vec::new()));
                };
                let diff = files::ask::Diff {
                    from,
                    to: head.id,
                    prefix,
                };
                let written = host.query(diff).await?;
                let paths = written.into_iter().map(|entry| entry.path).collect();
                Ok::<_, Error>((head.id, paths))
            }
            .await;
            let _ = this.update(cx, |chat, cx| chat.diffed(touched, cx));
        })
        .detach();
    }

    fn diffed(&mut self, touched: Result<(BlobId, Vec<String>), Error>, cx: &mut Context<Self>) {
        self.files_diffing = false;
        match touched {
            Ok((head, paths)) => {
                self.files_seen = Some(head);
                let mut again = Vec::new();
                for ((path, _), (file, unread)) in self.unread_files.iter_mut() {
                    if !paths.contains(path) {
                        continue;
                    }
                    match *unread {
                        Unread::Reading => *unread = Unread::ReadingAgain,
                        Unread::Absent => again.push(file.clone()),
                        Unread::ReadingAgain
                        | Unread::Failed
                        | Unread::Retrying
                        | Unread::Refused => {}
                    }
                }
                if !again.is_empty() {
                    cx.notify();
                }
                for file in again {
                    self.read_file(file, cx);
                }
            }
            Err(refusal) => cx.log_refused(words::FILES_MOVED, &refusal),
        }
        if std::mem::take(&mut self.files_diff_again) {
            self.diff_unread(cx);
        }
    }

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

    pub(crate) fn open_in_files(&mut self, path: &str, cx: &mut Context<Self>) {
        self.enlarged = None;
        match crate::links::file_link(&self.session.chain_id, path) {
            Some(url) => cx.host().open_link(&url),
            None => cx.host().log("no link to open: the session names no chain"),
        }
        cx.notify();
    }
}

async fn held_at(host: &Host, path: &str) -> Result<Option<[u8; 32]>, String> {
    let stat = files::ask::Stat {
        path: path.to_owned(),
        at: None,
    };
    let answer = host.query(stat).await;
    let (snapshot, entry) = answer.map_err(|refusal| design::sentence(&refusal))?;
    match (snapshot, entry.map(|entry| entry.node)) {
        (Some(BlobId::Sha256(snapshot)), Some(files::Node::File { .. })) => Ok(Some(snapshot)),
        _ => Ok(None),
    }
}

fn folder_of(paths: &[&str]) -> String {
    let mut folder: &str = paths.first().copied().unwrap_or_default();
    for path in paths {
        let shared = folder
            .bytes()
            .zip(path.bytes())
            .take_while(|(one, other)| one == other)
            .count();
        let cut = folder.as_bytes()[..shared]
            .iter()
            .rposition(|byte| *byte == b'/');
        folder = &folder[..cut.map_or(0, |at| at + 1)];
    }
    folder.to_owned()
}

async fn read_attachment(host: Host, file: &Attachment) -> Result<Found, Error> {
    let stat = |at| files::ask::Stat {
        path: file.path.clone(),
        at,
    };
    let recorded = Some(BlobId::Sha256(file.snapshot));
    let (at, entry) = match host.query(stat(recorded)).await {
        Err(refusal) if refusal.code == program::code::NOT_FOUND => host.query(stat(None)).await?,
        answer => answer?,
    };
    let Some(files::Node::File { file: object, .. }) = entry.map(|entry| entry.node) else {
        return Ok(Found::Absent(at));
    };
    let object: files::FileObj = abi::decode(&blob(&host, &object).await?)
        .map_err(|refusal| view::host::malformed(refusal.sentence))?;
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
        true => picture(&host, &object.chunks).await?,
        false => None,
    };
    Ok(Found::Shown(Shown {
        chunks: Some(chunks),
        picture,
    }))
}

async fn blob(host: &Host, id: &BlobId) -> Result<Vec<u8>, Error> {
    let hash = match id {
        BlobId::Sha256(_) => "sha256",
        BlobId::Sha1(_) => "sha1",
    };
    let id = format!("{hash}:{}", abi::hex(id.digest()));
    let bytes = host.ask::<BlobGet>(id).await?;
    bytes.ok_or_else(|| view::host::malformed(words::NOT_THERE.into()))
}

async fn picture(host: &Host, chunks: &[BlobId]) -> Result<Option<Picture>, Error> {
    let mut bytes = Vec::new();
    for chunk in chunks {
        bytes.extend(blob(host, chunk).await?);
    }
    let format = view::picture::picture_format(&bytes);
    let size = view::picture::picture_size(&bytes);
    Ok(format.zip(size).map(|(format, size)| Picture {
        image: Arc::new(Image::from_bytes(format, bytes)),
        size,
    }))
}
