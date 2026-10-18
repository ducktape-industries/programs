// Pack writing without deltas: PackWriter streams to a sink, write collects into a Vec.

use super::{SIGNATURE, kind_code};
use crate::error::{Error, Result};
use crate::object::Object;
use crate::oid::{Hash, Hasher, Oid};
use miniz_oxide::deflate::compress_to_vec_zlib;

/// How hard each entry is deflated. The receiver keeps the pack as sent, so this trades kept
/// bytes against write time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deflate {
    /// zlib level 6: a pack written once and held, such as a push's blob.
    Small,
    /// zlib level 1: a fetch's answer, written while git waits.
    Fast,
}

impl Deflate {
    fn level(self) -> u8 {
        match self {
            Deflate::Small => 6,
            Deflate::Fast => 1,
        }
    }
}

pub struct PackWriter<'a> {
    sink: &'a mut dyn FnMut(&[u8]),
    hasher: Hasher,
    deflate: Deflate,
}

impl<'a> PackWriter<'a> {
    pub fn new(
        sink: &'a mut dyn FnMut(&[u8]),
        hash: Hash,
        count: u32,
        deflate: Deflate,
    ) -> PackWriter<'a> {
        let mut writer = PackWriter {
            sink,
            hasher: Hasher::new(hash),
            deflate,
        };
        let mut header = Vec::with_capacity(12);
        header.extend_from_slice(SIGNATURE);
        header.extend_from_slice(&2u32.to_be_bytes());
        header.extend_from_slice(&count.to_be_bytes());
        writer.emit(&header);
        writer
    }

    pub fn add(&mut self, object: &Object) {
        self.emit(&entry_header(kind_code(object.kind), object.body.len()));
        self.emit(&compress_to_vec_zlib(&object.body, self.deflate.level()));
    }

    pub fn finish(self) -> Result<Oid> {
        let checksum = self.hasher.finish()?;
        (self.sink)(checksum.as_bytes());
        Ok(checksum)
    }

    fn emit(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
        (self.sink)(bytes);
    }
}

fn entry_header(type_code: u8, size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(10);
    let mut byte = (type_code << 4) | (size & 0x0f) as u8;
    let mut rest = size >> 4;
    while rest > 0 {
        out.push(byte | 0x80);
        byte = (rest & 0x7f) as u8;
        rest >>= 7;
    }
    out.push(byte);
    out
}

pub fn write<I>(objects: I, hash: Hash) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = Object>,
    I::IntoIter: ExactSizeIterator,
{
    let objects = objects.into_iter();
    let count = u32::try_from(objects.len()).map_err(|_| Error::TooManyObjects)?;
    let mut out = Vec::new();
    let mut sink = |bytes: &[u8]| out.extend_from_slice(bytes);
    let mut writer = PackWriter::new(&mut sink, hash, count, Deflate::Small);
    for object in objects {
        writer.add(&object);
    }
    writer.finish()?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_header_matches_git_layout() {
        assert_eq!(entry_header(1, 13 | (14 << 4)), [0x9d, 0x0e]);
        assert_eq!(entry_header(3, 0), [0x30]);
        assert_eq!(entry_header(3, 16), [0xb0, 0x01]);
    }
}
