//! What an agent sees: each image a message it hears carries, or `image_view` opens, kept as where
//! files holds it by the frame it came with (never its bytes), read when a request is built, and
//! shown to the model as the image or, where the model can't take it, as a sentence saying why.
use std::collections::BTreeMap;

use ::program::{BlobId, Error, ExecCtx};
use base64::Engine;
use borsh::{BorshDeserialize, BorshSerialize};
use files::{EntryInfo, Node};

use crate::calls::refusal;
use crate::state::{CALL_PICTURES, HELD_PICTURES, HIDDEN, PICTURES, WITHHELD};
use crate::tools::files::{read_all, stat};
use crate::{Frame, Wire, wire};

#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Picture {
    pub name: String,
    pub path: String,
    pub snapshot: BlobId,
}

impl Picture {
    fn label(&self) -> String {
        let at = abi::hex(self.snapshot.digest());
        format!("{} (files {} at snapshot {at})", self.name, self.path)
    }
}

// the pictures up to frame `through` that the model refused a request over, in its words
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Hidden {
    pub through: u64,
    pub why: String,
}

pub(crate) enum Look {
    Image {
        label: String,
        media: &'static str,
        data: String,
    },
    Words(String),
}

pub(crate) type Looks = BTreeMap<u64, Vec<Look>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl Format {
    pub(crate) fn media(self) -> &'static str {
        match self {
            Format::Png => "image/png",
            Format::Jpeg => "image/jpeg",
            Format::Gif => "image/gif",
            Format::Webp => "image/webp",
        }
    }
}

pub(crate) fn format(bytes: &[u8]) -> Option<Format> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(Format::Png);
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(Format::Gif);
    }
    let webp = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]);
    webp.then_some(Format::Webp)
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    let pair = bytes.get(at..at + 2)?;
    Some((u32::from(pair[0]) << 8) | u32::from(pair[1]))
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    let pair = bytes.get(at..at + 2)?;
    Some((u32::from(pair[1]) << 8) | u32::from(pair[0]))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    Some(le16(bytes, at)? | (u32::from(*bytes.get(at + 2)?) << 16))
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some((be16(bytes, at)? << 16) | be16(bytes, at + 2)?)
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(le16(bytes, at)? | (le16(bytes, at + 2)? << 16))
}

pub(crate) fn dimensions(bytes: &[u8], format: Format) -> Option<(u32, u32)> {
    match format {
        Format::Png => match bytes.get(12..16)? == b"IHDR" {
            true => Some((be32(bytes, 16)?, be32(bytes, 20)?)),
            false => None,
        },
        Format::Gif => Some((le16(bytes, 6)?, le16(bytes, 8)?)),
        Format::Jpeg => jpeg(bytes),
        Format::Webp => webp(bytes),
    }
}

fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xFF {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            0xFF => at += 1,
            0x01 | 0xD0..=0xD7 => at += 2,
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                return Some((be16(bytes, at + 7)?, be16(bytes, at + 5)?));
            }
            _ => at += 2 + be16(bytes, at + 2)? as usize,
        }
    }
}

fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        b"VP8 " => Some((le16(bytes, 26)? & 0x3FFF, le16(bytes, 28)? & 0x3FFF)),
        b"VP8L" => {
            let bits = le32(bytes, 21)?;
            Some(((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1))
        }
        b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
        _ => None,
    }
}

pub(crate) fn of_message(blocks: &[chat::Block]) -> Vec<Picture> {
    blocks
        .iter()
        .filter_map(|block| match block {
            chat::Block::Attachment(file) if file.content_type.starts_with("image/") => {
                Some(Picture {
                    name: file.name.clone(),
                    path: file.path.clone(),
                    snapshot: BlobId::Sha256(file.snapshot),
                })
            }
            chat::Block::Attachment(_)
            | chat::Block::Paragraph(_)
            | chat::Block::Code { .. }
            | chat::Block::Quote(_)
            | chat::Block::Divider
            | chat::Block::Notice(_) => None,
        })
        .collect()
}

pub(crate) fn keep(ctx: &ExecCtx, thread: u64, seq: u64, pictures: Vec<Picture>) {
    if !pictures.is_empty() {
        PICTURES.put(ctx, &(thread, seq), &pictures);
    }
}

pub(crate) fn hold(ctx: &ExecCtx, thread: u64, index: u64, pictures: Vec<Picture>) {
    if !pictures.is_empty() {
        HELD_PICTURES.put(ctx, &(thread, index), &pictures);
    }
}

pub(crate) fn unhold(ctx: &ExecCtx, thread: u64, index: u64, seq: u64) -> Result<(), Error> {
    if let Some(pictures) = HELD_PICTURES.get(ctx, &(thread, index))? {
        PICTURES.put(ctx, &(thread, seq), &pictures);
        HELD_PICTURES.remove(ctx, &(thread, index));
    }
    Ok(())
}

pub(crate) fn opened(ctx: &ExecCtx, thread: u64, call: &str, pictures: Vec<Picture>) {
    CALL_PICTURES.put(ctx, &(thread, call.to_owned()), &pictures);
}

pub(crate) fn called(ctx: &ExecCtx, thread: u64, call: &str, seq: u64) -> Result<(), Error> {
    let key = (thread, call.to_owned());
    if let Some(pictures) = CALL_PICTURES.get(ctx, &key)? {
        PICTURES.put(ctx, &(thread, seq), &pictures);
        CALL_PICTURES.remove(ctx, &key);
    }
    Ok(())
}

pub(crate) fn dropped(ctx: &ExecCtx, thread: u64, call: &str) {
    CALL_PICTURES.remove(ctx, &(thread, call.to_owned()));
}

pub(crate) fn forked(ctx: &ExecCtx, from: u64, to: u64, frames: &[Frame]) -> Result<(), Error> {
    for frame in frames {
        if let Some(pictures) = PICTURES.get(ctx, &(from, frame.seq))? {
            PICTURES.put(ctx, &(to, frame.seq), &pictures);
        }
    }
    if let Some(hidden) = HIDDEN.get(ctx, &from)? {
        HIDDEN.put(ctx, &to, &hidden);
    }
    Ok(())
}

pub(crate) fn hide(ctx: &ExecCtx, thread: u64, why: &str) -> Result<bool, Error> {
    let held = PICTURES.scan(ctx, PICTURES.prefix_of(&thread))?;
    let newest = held.into_iter().map(|((_, seq), _)| seq).max();
    let before = HIDDEN.get(ctx, &thread)?.map_or(0, |hidden| hidden.through);
    let Some(through) = newest.filter(|seq| *seq > before) else {
        return Ok(false);
    };
    let why = why.to_owned();
    HIDDEN.put(ctx, &thread, &Hidden { through, why });
    Ok(true)
}

pub(crate) fn looks(
    ctx: &ExecCtx,
    thread: u64,
    wire: Wire,
    frames: &[Frame],
) -> Result<Looks, Error> {
    let hidden = HIDDEN.get(ctx, &thread)?;
    let mut looks = Looks::new();
    for frame in frames {
        if WITHHELD.has(ctx, &(thread, frame.seq)) {
            continue;
        }
        let Some(pictures) = PICTURES.get(ctx, &(thread, frame.seq))? else {
            continue;
        };
        let refused = hidden.as_ref().filter(|hidden| frame.seq <= hidden.through);
        let shown = pictures
            .iter()
            .map(|picture| look(ctx, wire, picture, refused))
            .collect();
        looks.insert(frame.seq, shown);
    }
    Ok(looks)
}

fn look(ctx: &ExecCtx, wire: Wire, picture: &Picture, hidden: Option<&Hidden>) -> Look {
    let label = picture.label();
    if let Some(hidden) = hidden {
        let why = &hidden.why;
        return Look::Words(format!(
            "{label}: not shown, as the model refused a request with it: {why}"
        ));
    }
    match image(ctx, wire, picture) {
        Ok(image) => {
            let side = match image.side {
                Some((width, height)) => format!(", {width}×{height} px"),
                None => String::new(),
            };
            let media = image.format.media();
            let label = format!("{label}: {media}, {}{side}", amount(image.size));
            Look::Image {
                label,
                media,
                data: image.data,
            }
        }
        Err(why) => Look::Words(format!("{label}: not shown, {why}")),
    }
}

struct Image {
    format: Format,
    size: u64,
    side: Option<(u32, u32)>,
    data: String,
}

fn image(ctx: &ExecCtx, wire: Wire, picture: &Picture) -> Result<Image, String> {
    let at = Some(&picture.snapshot);
    let unread = |error: Error| format!("files answered {}", refusal(&error));
    let found = stat(ctx, &picture.path, at).map_err(unread)?;
    let Some(EntryInfo {
        node: Node::File { size, .. },
        ..
    }) = found
    else {
        return Err("files holds no file there at that snapshot".to_owned());
    };
    wire::image_size(wire, size).map_err(|why| format!("{}, {why}", amount(size)))?;
    let bytes = read_all(ctx, &picture.path, at).map_err(unread)?;
    let format = format(&bytes).ok_or("it is not a png, jpeg, gif or webp image")?;
    let side = dimensions(&bytes, format);
    if let Some((width, height)) = side {
        wire::image_side(wire, width, height)?;
    }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(Image {
        format,
        size,
        side,
        data,
    })
}

fn amount(bytes: u64) -> String {
    let kilo = 1024;
    if bytes < kilo {
        return format!("{bytes} bytes");
    }
    if bytes < kilo * kilo {
        return format!("{} KB", bytes.div_ceil(kilo));
    }
    let tenths = (bytes * 10).div_ceil(kilo * kilo);
    format!("{}.{} MB", tenths / 10, tenths % 10)
}
