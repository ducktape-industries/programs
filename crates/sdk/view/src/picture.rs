//! Which pictures a view draws, told by a file's first bytes: the formats
//! the app decodes, each picture's width and height as its header says,
//! and the size it draws at in the room it has.
use crate::ImageFormat;

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

// A picture's width and height in pixels, as its header names them: a
// PNG's IHDR, a GIF's logical screen, a BMP's info header, a JPEG's frame
// (SOFn), a WebP's VP8, VP8L or VP8X chunk. `None` where it names none, or
// a side of zero.
pub fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let size = match picture_format(bytes)? {
        ImageFormat::Png => (be32(bytes, 16)?, be32(bytes, 20)?),
        ImageFormat::Gif => (le16(bytes, 6)?, le16(bytes, 8)?),
        ImageFormat::Bmp => bmp(bytes)?,
        ImageFormat::Jpeg => jpeg(bytes)?,
        ImageFormat::Webp => webp(bytes)?,
        _ => return None,
    };
    (size.0 > 0 && size.1 > 0).then_some(size)
}

// The size a picture of `natural` size draws at in `room`: its own where it
// fits, else scaled down until it does, its aspect kept. Never past 1:1.
pub fn fit(natural: (u32, u32), room: (f32, f32)) -> (f32, f32) {
    let (width, height) = (natural.0 as f32, natural.1 as f32);
    let (room_w, room_h) = (room.0.max(0.), room.1.max(0.));
    if width <= room_w && height <= room_h {
        return (width, height);
    }
    match width * room_h >= height * room_w {
        true => (room_w, height * room_w / width),
        false => (width * room_h / height, room_h),
    }
}

fn be16(bytes: &[u8], at: usize) -> Option<u32> {
    let pair = bytes.get(at..at + 2)?;
    Some(u16::from_be_bytes([pair[0], pair[1]]).into())
}

fn le16(bytes: &[u8], at: usize) -> Option<u32> {
    let pair = bytes.get(at..at + 2)?;
    Some(u16::from_le_bytes([pair[0], pair[1]]).into())
}

fn be32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn le24(bytes: &[u8], at: usize) -> Option<u32> {
    let three = bytes.get(at..at + 3)?;
    Some(u32::from_le_bytes([three[0], three[1], three[2], 0]))
}

// the info header after the 14-byte file header: a core header's sides
// are 16 bits, every later one's 32, its height negative for a top-down
// bitmap
fn bmp(bytes: &[u8]) -> Option<(u32, u32)> {
    match le32(bytes, 14)? {
        12 => Some((le16(bytes, 18)?, le16(bytes, 20)?)),
        _ => {
            let width = le32(bytes, 18)? as i32;
            let height = le32(bytes, 22)? as i32;
            Some((width.unsigned_abs(), height.unsigned_abs()))
        }
    }
}

// the first start-of-frame marker's lines and samples per line, walking
// the segments before it by their lengths
fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut at = 2;
    loop {
        if *bytes.get(at)? != 0xff {
            return None;
        }
        let marker = *bytes.get(at + 1)?;
        match marker {
            0xff => at += 1,
            0x01 | 0xd0..=0xd7 => at += 2,
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                return Some((be16(bytes, at + 7)?, be16(bytes, at + 5)?));
            }
            _ => at += 2 + usize::try_from(be16(bytes, at + 2)?).ok()?,
        }
    }
}

// the first chunk after `WEBP`: a lossy key frame's 14-bit sides after
// its start code, a lossless stream's 14-bit sides less one after its
// signature, an extended file's 24-bit canvas sides less one
fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    match bytes.get(12..16)? {
        b"VP8 " if bytes.get(23..26)? == [0x9d, 0x01, 0x2a] => {
            Some((le16(bytes, 26)? & 0x3fff, le16(bytes, 28)? & 0x3fff))
        }
        b"VP8L" if *bytes.get(20)? == 0x2f => {
            let bits = le32(bytes, 21)?;
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le24(bytes, 24)? + 1, le24(bytes, 27)? + 1)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend(13u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes
    }

    fn riff(chunk: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
        bytes.extend(chunk);
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend(body);
        bytes
    }

    #[test]
    fn each_format_says_its_size_from_its_header() {
        assert_eq!(picture_size(&png(640, 480)), Some((640, 480)));
        let screen: [u8; 7] = [0x80, 0x02, 0xe0, 0x01, 0, 0, 0];
        let gif = [b"GIF89a".as_slice(), &screen].concat();
        assert_eq!(picture_size(&gif), Some((640, 480)));
        let mut bmp = b"BM\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        bmp.extend(40u32.to_le_bytes());
        bmp.extend(640i32.to_le_bytes());
        bmp.extend((-480i32).to_le_bytes());
        assert_eq!(picture_size(&bmp), Some((640, 480)));
        let start: [u8; 2] = [0xff, 0xd8];
        let app0: [u8; 6] = [0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        let frame: [u8; 10] = [0xff, 0xc2, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80, 0x03];
        let jpeg = [start.as_slice(), &app0, &frame].concat();
        assert_eq!(picture_size(&jpeg), Some((640, 480)));
        let lossy: [u8; 10] = [0, 0, 0, 0x9d, 0x01, 0x2a, 0x80, 0x02, 0xe0, 0x01];
        assert_eq!(picture_size(&riff(b"VP8 ", &lossy)), Some((640, 480)));
        let sides: u32 = 639 | (479 << 14);
        let lossless = [[0x2f_u8].as_slice(), &sides.to_le_bytes()].concat();
        assert_eq!(picture_size(&riff(b"VP8L", &lossless)), Some((640, 480)));
        let canvas: [u8; 10] = [0, 0, 0, 0, 0x7f, 0x02, 0x00, 0xdf, 0x01, 0x00];
        assert_eq!(picture_size(&riff(b"VP8X", &canvas)), Some((640, 480)));
    }

    #[test]
    fn a_header_cut_short_or_naming_no_size_has_none() {
        assert_eq!(picture_size(&png(640, 480)[..20]), None);
        assert_eq!(picture_size(&png(0, 480)), None);
        assert_eq!(picture_size(&[0xff, 0xd8, 0xff, 0xd9]), None);
        assert_eq!(picture_size(b"<svg width=\"640\"/>"), None);
    }

    #[test]
    fn a_picture_fits_its_room_keeping_its_aspect_and_never_grows() {
        assert_eq!(fit((96, 96), (360., 240.)), (96., 96.));
        assert_eq!(fit((4000, 1000), (360., 240.)), (360., 90.));
        assert_eq!(fit((1000, 4000), (360., 240.)), (60., 240.));
        assert_eq!(fit((4000, 3000), (800., 600.)), (800., 600.));
        assert_eq!(fit((360, 240), (360., 240.)), (360., 240.));
        assert_eq!(fit((10, 10), (-5., 240.)), (0., 0.));
    }
}
