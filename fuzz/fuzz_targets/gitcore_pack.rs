//! gitcore's pack reader under the crate's own generous limits. The first byte's bits: 1 picks
//! SHA-256 over SHA-1; 2 reads the rest as the whole pack, where otherwise the target closes it
//! with its checksum so the reader gets past that to the entries; 4 makes every base outside the
//! pack a blob of its id's bytes, where otherwise none is held; 8 reads the rest as entries the
//! target writes, so the reader gets past each one's zlib checksum to what it holds (a delta's
//! instructions): each is a type byte, a length byte and that many bytes, of which an ofs delta's
//! first says how many entries back its base is and a ref delta's first `hash.size()` are its
//! base's id, both left uncompressed.
#![no_main]

use gitcore::{Hash, Kind, Limits, Object, pack};
use libfuzzer_sys::fuzz_target;
use miniz_oxide::deflate::compress_to_vec_zlib;
use sha1_checked::Digest;

const OFS_DELTA: u8 = 6;
const REF_DELTA: u8 = 7;

fuzz_target!(|data: &[u8]| {
    let [flags, body @ ..] = data else {
        return;
    };
    let hash = match flags & 1 {
        0 => Hash::Sha1,
        _ => Hash::Sha256,
    };
    let mut bytes = match flags & 8 {
        0 => body.to_vec(),
        _ => written(body, hash),
    };
    if flags & 2 == 0 {
        let checksum = match hash {
            Hash::Sha1 => sha1_checked::Sha1::digest(&bytes).to_vec(),
            Hash::Sha256 => sha2::Sha256::digest(&bytes).to_vec(),
        };
        bytes.extend_from_slice(&checksum);
    }
    let held = flags & 4 != 0;
    let _ = pack::read(&bytes, hash, &Limits::generous(), |id| {
        Ok(held.then(|| Object::new(Kind::Blob, id.as_bytes().to_vec())))
    });
});

// The pack, but its checksum, of the entries `body` spells; an entry whose base is not there is
// left out.
fn written(mut body: &[u8], hash: Hash) -> Vec<u8> {
    let mut offsets = Vec::new();
    let mut entries = Vec::new();
    while let [code, length, rest @ ..] = body {
        let Some((spelled, next)) = rest.split_at_checked(usize::from(*length)) else {
            break;
        };
        body = next;
        let code = code & 7;
        let offset = 12 + entries.len();
        let (before, raw) = match code {
            OFS_DELTA => {
                let [back, raw @ ..] = spelled else {
                    continue;
                };
                let back = usize::from(*back).max(1);
                let Some(base) = offsets.len().checked_sub(back) else {
                    continue;
                };
                (distance(offset - offsets[base]), raw)
            }
            REF_DELTA => {
                let Some((base, raw)) = spelled.split_at_checked(hash.size()) else {
                    continue;
                };
                (base.to_vec(), raw)
            }
            _ => (Vec::new(), spelled),
        };
        offsets.push(offset);
        entries.extend(header(code, raw.len()));
        entries.extend(before);
        entries.extend(compress_to_vec_zlib(raw, 6));
    }
    let count = u32::try_from(offsets.len()).unwrap_or(u32::MAX);
    [
        &b"PACK"[..],
        &2u32.to_be_bytes(),
        &count.to_be_bytes(),
        &entries,
    ]
    .concat()
}

// An entry's type and size, as gitcore's writer puts them.
fn header(code: u8, size: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut byte = (code << 4) | (size & 0x0f) as u8;
    let mut rest = size >> 4;
    while rest > 0 {
        out.push(byte | 0x80);
        byte = (rest & 0x7f) as u8;
        rest >>= 7;
    }
    out.push(byte);
    out
}

// An ofs delta's distance back to its base: big-endian sevens, one added per continuation.
fn distance(mut distance: usize) -> Vec<u8> {
    let mut out = vec![(distance & 0x7f) as u8];
    distance >>= 7;
    while distance > 0 {
        distance -= 1;
        out.push(0x80 | (distance & 0x7f) as u8);
        distance >>= 7;
    }
    out.reverse();
    out
}
