//! Which pictures a view draws whole: the formats the app decodes, told by
//! a file's first bytes, and the most bytes a picture is read whole for.
use crate::ImageFormat;

// ponytail: a picture shown is held whole in the view's memory and read
// a chunk per query, then crosses to the app once; the app draws at most
// 64 MiB of pixels (about 4096 x 4096), whatever the file holds. Read a
// smaller rendition (made at upload) to show larger files.
pub const SHOW_MOST: u64 = 8 << 20;

// PNG, JPEG, GIF, WebP and BMP; `None` for anything else. An SVG is text,
// and shows as its text.
pub fn picture_format(bytes: &[u8]) -> Option<ImageFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, ..] => Some(ImageFormat::Png),
        [0xff, 0xd8, 0xff, ..] => Some(ImageFormat::Jpeg),
        [b'G', b'I', b'F', b'8', b'7' | b'9', b'a', ..] => Some(ImageFormat::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(ImageFormat::Webp),
        // a bitmap's reserved bytes are zero, which no text has
        [b'B', b'M', _, _, _, _, 0, 0, 0, 0, ..] => Some(ImageFormat::Bmp),
        _ => None,
    }
}
