//! A draft's files: each one picked, pasted or dropped, named in its
//! message's folder (`/chat/<channel>/<message>/<name>`), put as chunk blobs
//! and committed to files with the meta that says where it came from.
use std::collections::BTreeMap;

use chat::message::Attachment;
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use view::methods::{BlobBody, NewBlob, Picked};

use super::{Draft, Target};

// what a name files takes no segment from becomes
const FALLBACK: &str = "file";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Upload {
    // the host puts its chunks: the bytes it has sent so far
    Putting { sent: u64 },
    // its chunks are held; the commit waits for the draft's message id
    Held(Vec<[u8; 32]>),
    Committing,
    Landed(Attachment),
    // why it is not up, in every view's words
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attaching {
    pub id: u64,
    // its name in the message's folder
    pub name: String,
    // its name as the device gave it
    pub original: String,
    pub size: u64,
    pub content_type: String,
    pub upload: Upload,
    // the host's handle while it holds the file
    #[serde(skip)]
    pub file: Option<u64>,
    // carried by a send still in flight
    #[serde(skip)]
    pub sent: bool,
}

impl Attaching {
    pub fn landed(&self) -> Option<&Attachment> {
        match &self.upload {
            Upload::Landed(file) => Some(file),
            _ => None,
        }
    }

    pub fn failed(&self) -> Option<&str> {
        match &self.upload {
            Upload::Failed(why) => Some(why),
            _ => None,
        }
    }

    // the bytes the host has sent of all it sends for the file
    pub fn progress(&self) -> Option<(u64, u64)> {
        match self.upload {
            Upload::Putting { sent } => Some((sent, framed(self.size))),
            _ => None,
        }
    }
}

impl Draft {
    // `picked` among the draft's files, named apart from the others and
    // from every name its message's folder took; its id, the host's handle
    // and its size, to put it by
    pub fn attach(&mut self, picked: Picked) -> (u64, u64, u64) {
        let taken: Vec<&str> = self
            .attachments
            .iter()
            .map(|file| file.name.as_str())
            .chain(self.filed.iter().map(String::as_str))
            .collect();
        let name = named(&picked.name, &taken);
        self.attached += 1;
        self.attachments.push(Attaching {
            id: self.attached,
            content_type: content_type(&name).to_owned(),
            name,
            original: picked.name,
            size: picked.size,
            upload: Upload::Putting { sent: 0 },
            file: Some(picked.file),
            sent: false,
        });
        (self.attached, picked.file, picked.size)
    }

    // a file of the draft's, not yet sent, is still on its way up
    pub fn uploading(&self) -> bool {
        self.attachments
            .iter()
            .any(|file| !file.sent && file.landed().is_none())
    }

    // the files a send carries, each spoken for until the send lands, and
    // the message id their folder is named by; an edit keeps its own
    pub fn take_files(&mut self, target: &Target) -> (Vec<Attachment>, Option<String>) {
        if let Target::Edit { .. } = target {
            return (self.kept.clone(), None);
        }
        let files: Vec<Attachment> = self
            .attachments
            .iter_mut()
            .filter(|file| !file.sent)
            .filter_map(|file| {
                let landed = file.landed()?.clone();
                file.sent = true;
                Some(landed)
            })
            .collect();
        let message = match files.is_empty() {
            true => None,
            false => self.message_id.take(),
        };
        (files, message)
    }

    // a send of `files` landed: they leave the draft
    pub fn sent_files(&mut self, files: &[Attachment]) {
        self.attachments.retain(|file| {
            !(file.sent && file.landed().is_some_and(|landed| files.contains(landed)))
        });
    }

    // a send of `files` did not land: they are the draft's to send again,
    // under the message id they were filed by
    pub fn unsent_files(&mut self, files: &[Attachment], message: Option<String>) {
        for file in &mut self.attachments {
            if file.landed().is_some_and(|landed| files.contains(landed)) {
                file.sent = false;
            }
        }
        if self.message_id.is_none() {
            self.message_id = message;
        }
    }
}

// The device's name as files spells a path's segment (NFC), or the
// fallback where it makes none; a name `taken` already gets a number.
pub fn named(original: &str, taken: &[&str]) -> String {
    let name: String = original.nfc().collect();
    let one_segment = !name.is_empty() && !name.contains('/');
    let name = match one_segment && files::paths::change_path(&format!("/{name}")).is_ok() {
        true => name,
        false => FALLBACK.to_owned(),
    };
    if !taken.contains(&name.as_str()) {
        return name;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, Some(extension)),
        _ => (name.as_str(), None),
    };
    (2..)
        .map(|number| match extension {
            Some(extension) => format!("{stem} ({number}).{extension}"),
            None => format!("{stem} ({number})"),
        })
        .find(|candidate| !taken.contains(&candidate.as_str()))
        .unwrap_or(name)
}

// A MIME type by the name's extension; the host names none.
pub fn content_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("txt" | "log") => "text/plain",
        Some("md" | "markdown") => "text/markdown",
        Some("csv") => "text/csv",
        Some("json") => "application/json",
        Some("html" | "htm") => "text/html",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

pub fn path(channel: &str, message: &str, name: &str) -> String {
    format!("/{}/{channel}/{message}/{name}", chat::PROGRAM)
}

// What the file's put says of where it came from.
pub fn meta(
    channel: &str,
    message: &str,
    uploader: Option<u64>,
    file: &Attaching,
) -> BTreeMap<String, String> {
    let mut meta = BTreeMap::from([
        ("source.program".to_owned(), chat::PROGRAM.to_owned()),
        ("source.channel".to_owned(), channel.to_owned()),
        ("source.message".to_owned(), message.to_owned()),
        ("name".to_owned(), file.original.clone()),
        ("content-type".to_owned(), file.content_type.clone()),
    ]);
    meta.extend(uploader.map(|number| ("uploader".to_owned(), number.to_string())));
    meta
}

// A chunk blob per `files::CHUNK_SIZE` of the held file, in order, each
// read by the host from the file, never through the view.
pub fn blobs(file: u64, size: u64) -> Vec<NewBlob> {
    (0..size.div_ceil(files::CHUNK_SIZE))
        .map(|index| {
            let offset = index * files::CHUNK_SIZE;
            NewBlob {
                kind: files::CHUNK.into(),
                body: BlobBody::Picked {
                    file,
                    offset,
                    len: files::CHUNK_SIZE.min(size - offset),
                },
            }
        })
        .collect()
}

// What the host sends for a file of `size` bytes: each chunk framed with
// its kind (`<kind> <len>\0<body>`), as its progress counts them.
pub fn framed(size: u64) -> u64 {
    (0..size.div_ceil(files::CHUNK_SIZE))
        .map(|index| {
            let len = files::CHUNK_SIZE.min(size - index * files::CHUNK_SIZE);
            format!("{} {len}\0", files::CHUNK).len() as u64 + len
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::super::draft::host;
    use super::super::{Outcome, Send};
    use super::*;
    use view::wire;

    fn picked(name: &str, file: u64) -> Picked {
        Picked {
            name: name.into(),
            size: 3,
            file,
        }
    }

    #[test]
    fn a_name_is_its_nfc_and_a_second_of_one_name_is_numbered() {
        assert_eq!(named("e\u{301}te\u{301}.png", &[]), "\u{e9}t\u{e9}.png");
        assert_eq!(named("image.png", &["image.png"]), "image (2).png");
        let taken = ["image.png", "image (2).png"];
        assert_eq!(named("image.png", &taken), "image (3).png");
        assert_eq!(named("README", &["README"]), "README (2)");
        assert_eq!(named(".profile", &[".profile"]), ".profile (2)");
        for unnamed in ["", "a/b", ".", ".."] {
            assert_eq!(named(unnamed, &[]), "file", "{unnamed:?}");
        }
    }

    #[test]
    fn a_type_is_named_by_the_extension() {
        assert_eq!(content_type("A.PNG"), "image/png");
        assert_eq!(content_type("notes.txt"), "text/plain");
        assert_eq!(content_type("Makefile"), "application/octet-stream");
    }

    #[test]
    fn progress_is_of_the_framed_bytes_the_host_counts() {
        assert_eq!(framed(0), 0);
        assert_eq!(framed(3), "chunk 3\0".len() as u64 + 3);
        let whole = files::CHUNK_SIZE;
        let first = format!("chunk {whole}\0").len() as u64 + whole;
        assert_eq!(framed(whole + 1), first + "chunk 1\0".len() as u64 + 1);
        let blobs = blobs(4, whole + 1);
        let lens: Vec<u64> = blobs
            .iter()
            .map(|blob| match blob.body {
                BlobBody::Picked { file: 4, len, .. } => len,
                ref other => panic!("a slice of the held file: {other:?}"),
            })
            .collect();
        assert_eq!(lens, [whole, 1]);
    }

    #[test]
    fn the_paste_key_is_claimed_and_its_text_replaces_the_selection_as_one_edit() {
        let mut draft = Draft::from_body("hi you", &[]);
        let keystroke = view::Keystroke::parse("cmd-v").unwrap();
        let paste = wire::keyboard::KeyState::from(&keystroke);
        assert_eq!(draft.key_tag(&paste, false, &[]).as_deref(), Some("paste"));
        assert!(draft.claims(&[]).iter().any(|claim| {
            claim.command && claim.key == wire::keyboard::Key::Character("v".into())
        }));
        let (asks, outcome) = draft.act("paste", "c/editor", &[]);
        assert!(asks.is_empty());
        assert_eq!(outcome, Outcome::Action("paste".into()));
        host::selects(&draft.field, 3..6);
        let Some(wire::WidgetCommand::Replace {
            range,
            text,
            cursor,
            ..
        }) = draft.paste("c/editor", "there\r\n")
        else {
            panic!("one edit")
        };
        let replaced = (range.range(), text.as_str(), cursor.start as usize);
        assert_eq!(replaced, (3..6, "there\n", 9));
        assert!(draft.paste("c/editor", "").is_none());
    }

    #[test]
    fn files_alone_send_and_a_file_that_failed_holds_the_send_back() {
        let mut draft = Draft::default();
        assert!(!draft.can_send());
        draft.attach(picked("a.txt", 1));
        assert_eq!(draft.attach(picked("a.txt", 2)), (2, 2, 3));
        assert_eq!(draft.attachments[1].name, "a (2).txt");
        assert!(draft.can_send() && draft.uploading());
        let (asks, outcome) = draft.act("send", "c/editor", &[]);
        assert!(asks.is_empty(), "no text to clear");
        assert_eq!(outcome, Outcome::Action("send".into()));
        assert_eq!(
            draft.submitted,
            Some(Send {
                body: String::new()
            })
        );
        draft.attachments[0].upload = Upload::Failed("refused".into());
        assert!(!draft.can_send());
    }
}
