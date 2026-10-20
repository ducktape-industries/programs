// Git delta application: base/result size headers, copy and insert instructions.

use crate::error::{Error, Result};

pub(crate) fn apply(base: &[u8], delta: &[u8], max_result_size: usize) -> Result<Vec<u8>> {
    let mut cursor = 0;
    let base_size = read_size(delta, &mut cursor)?;
    let result_size = read_size(delta, &mut cursor)?;
    let base_matches = base_size == base.len();
    if !base_matches {
        return Err(Error::BadDelta);
    }
    let too_large = result_size > max_result_size;
    if too_large {
        return Err(Error::ObjectTooLarge);
    }
    // room for what the base and the delta hold, at most: the header only claims the result size
    let mut out = Vec::with_capacity(result_size.min(base.len().saturating_add(delta.len())));
    while cursor < delta.len() {
        let command = delta[cursor];
        cursor += 1;
        let is_copy = command & 0x80 != 0;
        if is_copy {
            let (offset, length) = read_copy(delta, &mut cursor, command)?;
            let in_base = offset
                .checked_add(length)
                .is_some_and(|end| end <= base.len());
            // each copy can repeat the whole base, so the claimed size is held to as the result grows
            let fits = out.len() + length <= result_size;
            if !in_base || !fits {
                return Err(Error::BadDelta);
            }
            out.extend_from_slice(&base[offset..offset + length]);
            continue;
        }
        let length = usize::from(command);
        let is_reserved = length == 0;
        if is_reserved {
            return Err(Error::BadDelta);
        }
        let available = cursor + length <= delta.len();
        if !available {
            return Err(Error::Truncated);
        }
        let fits = out.len() + length <= result_size;
        if !fits {
            return Err(Error::BadDelta);
        }
        out.extend_from_slice(&delta[cursor..cursor + length]);
        cursor += length;
    }
    let size_matches = out.len() == result_size;
    if !size_matches {
        return Err(Error::BadDelta);
    }
    Ok(out)
}

fn read_size(delta: &[u8], cursor: &mut usize) -> Result<usize> {
    let mut value: usize = 0;
    let mut shift = 0;
    loop {
        let Some(byte) = delta.get(*cursor).copied() else {
            return Err(Error::Truncated);
        };
        *cursor += 1;
        let overflow = shift >= usize::BITS;
        if overflow {
            return Err(Error::BadDelta);
        }
        value |= usize::from(byte & 0x7f) << shift;
        shift += 7;
        let last = byte & 0x80 == 0;
        if last {
            return Ok(value);
        }
    }
}

fn read_copy(delta: &[u8], cursor: &mut usize, command: u8) -> Result<(usize, usize)> {
    let mut offset: usize = 0;
    let mut length: usize = 0;
    for bit in 0..4 {
        let present = command & (1 << bit) != 0;
        if present {
            offset |= usize::from(take(delta, cursor)?) << (8 * bit);
        }
    }
    for bit in 0..3 {
        let present = command & (0x10 << bit) != 0;
        if present {
            length |= usize::from(take(delta, cursor)?) << (8 * bit);
        }
    }
    let default_length = length == 0;
    if default_length {
        length = 0x10000;
    }
    Ok((offset, length))
}

fn take(delta: &[u8], cursor: &mut usize) -> Result<u8> {
    let Some(byte) = delta.get(*cursor).copied() else {
        return Err(Error::Truncated);
    };
    *cursor += 1;
    Ok(byte)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_and_insert() {
        let base = b"hello world";
        let delta = [11, 7, 0x91, 6, 5, 2, b'h', b'i'];
        assert_eq!(apply(base, &delta, 100).unwrap(), b"worldhi");
    }

    #[test]
    fn wrong_base_size_is_rejected() {
        assert_eq!(apply(b"abc", &[2, 1, 1, b'x'], 100), Err(Error::BadDelta));
    }

    #[test]
    fn result_size_is_capped() {
        assert_eq!(
            apply(b"abc", &[3, 1, 1, b'x'], 0),
            Err(Error::ObjectTooLarge)
        );
    }

    #[test]
    fn copy_past_base_is_rejected() {
        assert_eq!(apply(b"abc", &[3, 5, 0x90, 5], 100), Err(Error::BadDelta));
    }

    #[test]
    fn a_result_past_its_claimed_size_is_refused_at_once() {
        // 1 byte claimed, and the copy alone makes 3: it stops there, before the truncated insert after it
        assert_eq!(
            apply(b"abc", &[3, 1, 0x90, 3, 5, b'x'], 100),
            Err(Error::BadDelta)
        );
    }

    #[test]
    fn claimed_result_size_is_not_allocated() {
        // a result of 2^63 - 1 bytes claimed, nothing to make it: room for it was asked for first
        let delta = [12, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f];
        assert_eq!(
            apply(b"hello world\n", &delta, usize::MAX),
            Err(Error::BadDelta)
        );
    }
}
