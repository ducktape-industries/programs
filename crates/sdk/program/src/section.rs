//! A custom section of a program's code, read out of its blob a section header at a time, so
//! a reader finds one section without reading the whole module.
use abi::BlobId;

use crate::{Error, QueryCtx, not_found};

pub const MODULE: &[u8] = b"\0asm\x01\0\0\0";
pub const CUSTOM: u8 = 0;
const LEB_U32: u64 = 5;

// The payload of the first custom section called `name` in the module `code` holds; `None`
// for code that is no module or carries no such section, refused when the blob is not held.
pub fn read(ctx: &QueryCtx, code: BlobId, name: &str) -> Result<Option<Vec<u8>>, Error> {
    let read = |offset: u64, len: u64| {
        ctx.blob_read(code, offset, len)
            .ok_or_else(|| not_found(format!("the code blob {code:?} is not held here")))
    };
    if read(0, MODULE.len() as u64)? != MODULE {
        return Ok(None);
    }
    let mut offset = MODULE.len() as u64;
    loop {
        let head = read(offset, 1 + LEB_U32)?;
        let Some((&id, rest)) = head.split_first() else {
            return Ok(None);
        };
        let Some((size, used)) = leb(rest) else {
            return Ok(None);
        };
        let body = offset + 1 + used;
        let end = body + size;
        if id == CUSTOM {
            let named = read(body, LEB_U32)?;
            let Some((length, used)) = leb(&named) else {
                return Ok(None);
            };
            let name_at = body + used;
            let payload_at = name_at + length;
            let same_length = length == name.len() as u64;
            let ours = same_length && read(name_at, length)? == name.as_bytes();
            if ours {
                let Some(payload) = end.checked_sub(payload_at) else {
                    return Ok(None);
                };
                return read(payload_at, payload).map(Some);
            }
        }
        offset = end;
    }
}

fn leb(bytes: &[u8]) -> Option<(u64, u64)> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().take(LEB_U32 as usize).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index as u64 + 1));
        }
    }
    None
}
