// Pack reading: header, entry headers, inflate, delta resolution (ofs, ref, thin) and checksum.

use super::{Limits, SIGNATURE, delta, kind_from_code};
use crate::error::{Error, Result};
use crate::object::{Kind, Object};
use crate::oid::{Hash, Hasher, Oid};
use miniz_oxide::inflate::TINFLStatus;
use miniz_oxide::inflate::core::inflate_flags::{
    TINFL_FLAG_PARSE_ZLIB_HEADER, TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF,
};
use miniz_oxide::inflate::core::{DecompressorOxide, decompress};
use std::collections::BTreeMap;

const OFS_DELTA: u8 = 6;
const REF_DELTA: u8 = 7;

enum Payload {
    Full(Kind, Vec<u8>),
    OfsDelta { base_offset: usize, delta: Vec<u8> },
    RefDelta { base: Oid, delta: Vec<u8> },
}

struct Entry {
    offset: usize,
    payload: Payload,
}

struct Resolved {
    id: Oid,
    object: Object,
    depth: usize,
}

pub fn read<F>(
    bytes: &[u8],
    hash: Hash,
    limits: &Limits,
    resolve_base: F,
) -> Result<Vec<(Oid, Object)>>
where
    F: FnMut(&Oid) -> Result<Option<Object>>,
{
    let body = verify_checksum(bytes, hash)?;
    let count = parse_header(body)?;
    let too_many = count > limits.max_objects;
    if too_many {
        return Err(Error::TooManyObjects);
    }
    let entries = parse_entries(body, count, hash, limits)?;
    resolve_all(entries, hash, limits, resolve_base)
}

fn verify_checksum(bytes: &[u8], hash: Hash) -> Result<&[u8]> {
    let Some(body_len) = bytes.len().checked_sub(hash.size()) else {
        return Err(Error::Truncated);
    };
    let (body, trailer) = bytes.split_at(body_len);
    let mut hasher = Hasher::new(hash);
    hasher.update(body);
    let actual = hasher.finish()?;
    let matches = actual.as_bytes() == trailer;
    if !matches {
        return Err(Error::BadChecksum);
    }
    Ok(body)
}

fn parse_header(body: &[u8]) -> Result<usize> {
    let has_header = body.len() >= 12;
    if !has_header {
        return Err(Error::Truncated);
    }
    let signed = &body[..4] == SIGNATURE;
    if !signed {
        return Err(Error::BadPackHeader);
    }
    let version = u32::from_be_bytes([body[4], body[5], body[6], body[7]]);
    let supported = version == 2;
    if !supported {
        return Err(Error::UnsupportedPackVersion(version));
    }
    let count = u32::from_be_bytes([body[8], body[9], body[10], body[11]]);
    Ok(count as usize)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn byte(&mut self) -> Result<u8> {
        let Some(byte) = self.bytes.get(self.pos).copied() else {
            return Err(Error::Truncated);
        };
        self.pos += 1;
        Ok(byte)
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let available = self.pos + len <= self.bytes.len();
        if !available {
            return Err(Error::Truncated);
        }
        let slice = &self.bytes[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }
}

fn parse_entries(body: &[u8], count: usize, hash: Hash, limits: &Limits) -> Result<Vec<Entry>> {
    let mut cursor = Cursor {
        bytes: body,
        pos: 12,
    };
    // grown as entries are read: the header only claims the count
    let mut entries = Vec::new();
    for _ in 0..count {
        let offset = cursor.pos;
        let (type_code, size) = read_entry_header(&mut cursor)?;
        let too_large = size > limits.max_object_size;
        if too_large {
            return Err(Error::ObjectTooLarge);
        }
        let payload = match type_code {
            OFS_DELTA => {
                let distance = read_offset(&mut cursor)?;
                let Some(base_offset) = offset.checked_sub(distance) else {
                    return Err(Error::BadDelta);
                };
                let delta = inflate_at(&mut cursor, size)?;
                Payload::OfsDelta { base_offset, delta }
            }
            REF_DELTA => {
                let base = Oid::from_bytes(hash, cursor.take(hash.size())?)?;
                let delta = inflate_at(&mut cursor, size)?;
                Payload::RefDelta { base, delta }
            }
            other => {
                let Some(kind) = kind_from_code(other) else {
                    return Err(Error::UnknownObjectKind);
                };
                let data = inflate_at(&mut cursor, size)?;
                Payload::Full(kind, data)
            }
        };
        entries.push(Entry { offset, payload });
    }
    let trailing_garbage = cursor.pos != body.len();
    if trailing_garbage {
        return Err(Error::BadPackHeader);
    }
    Ok(entries)
}

fn read_entry_header(cursor: &mut Cursor<'_>) -> Result<(u8, usize)> {
    let first = cursor.byte()?;
    let type_code = (first >> 4) & 0x07;
    let mut size = usize::from(first & 0x0f);
    let mut shift = 4;
    let mut more = first & 0x80 != 0;
    while more {
        let byte = cursor.byte()?;
        let overflow = shift >= usize::BITS;
        if overflow {
            return Err(Error::BadPackHeader);
        }
        size |= usize::from(byte & 0x7f) << shift;
        shift += 7;
        more = byte & 0x80 != 0;
    }
    Ok((type_code, size))
}

fn read_offset(cursor: &mut Cursor<'_>) -> Result<usize> {
    let mut byte = cursor.byte()?;
    let mut distance = usize::from(byte & 0x7f);
    while byte & 0x80 != 0 {
        byte = cursor.byte()?;
        distance = distance
            .checked_add(1)
            .and_then(|d| d.checked_shl(7))
            .ok_or(Error::BadDelta)?
            | usize::from(byte & 0x7f);
    }
    Ok(distance)
}

fn inflate_at(cursor: &mut Cursor<'_>, expected_size: usize) -> Result<Vec<u8>> {
    let (data, consumed) = inflate(cursor.rest(), expected_size)?;
    cursor.pos += consumed;
    Ok(data)
}

// The output grows as the stream fills it, up to `expected_size`: the entry header only claims
// the size, so room for it all is not asked for before the stream gives it.
pub(crate) fn inflate(input: &[u8], expected_size: usize) -> Result<(Vec<u8>, usize)> {
    let mut out = vec![0u8; expected_size.min(input.len().saturating_mul(2))];
    let mut state = DecompressorOxide::new();
    let flags = TINFL_FLAG_PARSE_ZLIB_HEADER | TINFL_FLAG_USING_NON_WRAPPING_OUTPUT_BUF;
    let (mut consumed, mut written) = (0, 0);
    loop {
        let rest = input.get(consumed..).ok_or(Error::Inflate)?;
        let (status, read, wrote) = decompress(&mut state, rest, &mut out, written, flags);
        consumed += read;
        written += wrote;
        match status {
            TINFLStatus::Done if written == expected_size => return Ok((out, consumed)),
            TINFLStatus::HasMoreOutput if out.len() < expected_size => {
                let grown = out.len().saturating_mul(2).max(1).min(expected_size);
                out.resize(grown, 0);
            }
            _ => return Err(Error::Inflate),
        }
    }
}

fn resolve_all<F>(
    entries: Vec<Entry>,
    hash: Hash,
    limits: &Limits,
    mut resolve_base: F,
) -> Result<Vec<(Oid, Object)>>
where
    F: FnMut(&Oid) -> Result<Option<Object>>,
{
    let mut resolved: Vec<Option<Resolved>> = entries.iter().map(|_| None).collect();
    let mut by_oid: BTreeMap<Oid, usize> = BTreeMap::new();
    let by_offset: BTreeMap<usize, usize> = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.offset, index))
        .collect();
    let mut external: BTreeMap<Oid, Object> = BTreeMap::new();

    for (index, entry) in entries.iter().enumerate() {
        let Payload::Full(kind, data) = &entry.payload else {
            continue;
        };
        let object = Object::new(*kind, data.clone());
        let id = object.id(hash)?;
        by_oid.insert(id, index);
        resolved[index] = Some(Resolved {
            id,
            object,
            depth: 0,
        });
    }

    loop {
        let mut progress = false;
        for (index, entry) in entries.iter().enumerate() {
            let already = resolved[index].is_some();
            if already {
                continue;
            }
            let produced = {
                let (delta, base) = match &entry.payload {
                    Payload::Full(..) => continue,
                    Payload::OfsDelta { base_offset, delta } => {
                        let Some(base_index) = by_offset.get(base_offset) else {
                            return Err(Error::BadDelta);
                        };
                        let base = resolved[*base_index].as_ref().map(|r| (&r.object, r.depth));
                        (delta, base)
                    }
                    Payload::RefDelta { base, delta } => {
                        let in_pack = by_oid
                            .get(base)
                            .and_then(|i| resolved[*i].as_ref())
                            .map(|r| (&r.object, r.depth));
                        let base = in_pack.or_else(|| external.get(base).map(|o| (o, 0)));
                        (delta, base)
                    }
                };
                let Some((base_object, base_depth)) = base else {
                    continue;
                };
                let depth = base_depth + 1;
                let too_deep = depth > limits.max_delta_depth;
                if too_deep {
                    return Err(Error::DeltaTooDeep);
                }
                let body = delta::apply(&base_object.body, delta, limits.max_object_size)?;
                let object = Object::new(base_object.kind, body);
                let id = object.id(hash)?;
                Resolved { id, object, depth }
            };
            by_oid.insert(produced.id, index);
            resolved[index] = Some(produced);
            progress = true;
        }
        let all_done = resolved.iter().all(Option::is_some);
        if all_done {
            break;
        }
        if progress {
            continue;
        }
        let mut fetched = false;
        let mut first_missing = None;
        for (index, entry) in entries.iter().enumerate() {
            let pending = resolved[index].is_none();
            let Payload::RefDelta { base, .. } = &entry.payload else {
                continue;
            };
            let needs_fetch = pending && !by_oid.contains_key(base) && !external.contains_key(base);
            if !needs_fetch {
                continue;
            }
            let Some(object) = resolve_base(base)? else {
                first_missing.get_or_insert(*base);
                continue;
            };
            external.insert(*base, object);
            fetched = true;
        }
        if fetched {
            continue;
        }
        return Err(match first_missing {
            Some(base) => Error::MissingBase(base),
            None => Error::Cycle,
        });
    }

    Ok(resolved
        .into_iter()
        .flatten()
        .map(|r| (r.id, r.object))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_header_varint() {
        let mut cursor = Cursor {
            bytes: &[0x9d, 0x0e],
            pos: 0,
        };
        let (kind, size) = read_entry_header(&mut cursor).unwrap();
        assert_eq!(kind, 1);
        assert_eq!(size, 13 | (14 << 4));
        assert_eq!(cursor.pos, 2);
    }

    #[test]
    fn offset_varint_adds_one_per_continuation() {
        let mut cursor = Cursor {
            bytes: &[0x80, 0x00],
            pos: 0,
        };
        assert_eq!(read_offset(&mut cursor).unwrap(), 128);
        let mut single = Cursor {
            bytes: &[0x05],
            pos: 0,
        };
        assert_eq!(read_offset(&mut single).unwrap(), 5);
    }

    #[test]
    fn truncated_pack_is_truncated() {
        assert_eq!(
            read(b"PACK", Hash::Sha1, &Limits::generous(), |_| Ok(None)),
            Err(Error::Truncated)
        );
    }

    #[test]
    fn output_grows_to_an_object_compressed_far_below_it() {
        let blob = Object::new(Kind::Blob, vec![0; 1 << 20]);
        for hash in [Hash::Sha1, Hash::Sha256] {
            let pack = crate::pack::write([blob.clone()], hash).unwrap();
            let read = read(&pack, hash, &Limits::generous(), |_| Ok(None)).unwrap();
            assert_eq!(read, [(blob.id(hash).unwrap(), blob.clone())]);
        }
    }

    #[test]
    fn claimed_size_is_not_allocated() {
        // one blob of 0x50549a2e91 bytes claimed, no stream after: room for it was asked for first
        let pack = b"PACK\0\0\0\x02\0\0\0\x7a\xb1\xe9\xc5\xa6\xaa\x50\xfe\xe6\x18\x08\x74\x23\x90\x5f\xd9\xa3\x84\xe2\xa5\x4e\xee\xd4\x03\x2d\x9e\x89";
        assert_eq!(
            read(pack, Hash::Sha1, &Limits::generous(), |_| Ok(None)),
            Err(Error::Inflate)
        );
    }

    #[test]
    fn claimed_count_is_not_allocated() {
        // 2^32 - 1 objects claimed, none there: room for them all was asked for first
        let pack = b"PACK\0\0\0\x02\xff\xff\xff\xff\x80\xb6\x91\xb3\x01\xf3\x96\x89\x52\x1b\x5b\xb8\x13\xb9\x92\x9e\xb4\x35\x1e\x73";
        assert_eq!(
            read(pack, Hash::Sha1, &Limits::generous(), |_| Ok(None)),
            Err(Error::Truncated)
        );
    }
}
