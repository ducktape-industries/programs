//! A program's code blob as genesis packs it: a core wasm module that is
//! nothing but custom sections, the view in `ducktape.view` and the
//! describe module in `ducktape.describe`, which the app reads out by name
//! (`backend::views::view_section`, `describe::section`).

/// A core module holding `sections` and nothing else.
pub fn wrapper(sections: &[(&str, &[u8])]) -> Vec<u8> {
    let mut program = b"\0asm\x01\0\0\0".to_vec();
    for (name, data) in sections {
        let mut body = Vec::new();
        leb128(&mut body, name.len() as u64);
        body.extend_from_slice(name.as_bytes());
        body.extend_from_slice(data);
        program.push(0);
        leb128(&mut program, body.len() as u64);
        program.extend_from_slice(&body);
    }
    program
}

/// The custom section `name` of a core module, as [`wrapper`] wrote it or
/// as any core module carries it: the first by that name, so a view's
/// manifest is read as the rig's host reads it, not checked for a twin.
pub fn section<'a>(bytes: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let mut rest = bytes.strip_prefix(b"\0asm\x01\0\0\0")?;
    while let Some((&id, after)) = rest.split_first() {
        let (size, after) = read_leb128_length(after)?;
        let (body, after) = after.split_at_checked(size)?;
        rest = after;
        if id != 0 {
            continue;
        }
        let (len, body) = read_leb128_length(body)?;
        let (found, data) = body.split_at_checked(len)?;
        if found == name.as_bytes() {
            return Some(data);
        }
    }
    None
}

fn read_leb128_length(bytes: &[u8]) -> Option<(usize, &[u8])> {
    let mut value: u64 = 0;
    for (at, byte) in bytes.iter().enumerate().take(10) {
        value |= u64::from(byte & 0x7f) << (7 * at);
        if byte & 0x80 == 0 {
            return Some((usize::try_from(value).ok()?, &bytes[at + 1..]));
        }
    }
    None
}

fn leb128(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrapper_is_a_module_of_named_custom_sections() {
        let program = wrapper(&[("ducktape.view", b"view bytes")]);
        let mut expected = b"\0asm\x01\0\0\0".to_vec();
        let name = b"ducktape.view";
        let mut section = vec![name.len() as u8];
        section.extend_from_slice(name);
        section.extend_from_slice(b"view bytes");
        expected.push(0);
        expected.push(section.len() as u8);
        expected.extend_from_slice(&section);
        assert_eq!(program, expected);
    }

    #[test]
    fn a_section_is_read_back_by_name() {
        let long = vec![7u8; 200];
        let program = wrapper(&[("ducktape.view", &long), ("ducktape.describe", b"d")]);
        assert_eq!(section(&program, "ducktape.view"), Some(&long[..]));
        assert_eq!(section(&program, "ducktape.describe"), Some(&b"d"[..]));
        assert_eq!(section(&program, "ducktape.view.manifest"), None);
        assert_eq!(section(b"not a module", "ducktape.view"), None);
        assert_eq!(
            section(&program[..program.len() - 1], "ducktape.describe"),
            None
        );
    }

    #[test]
    fn a_long_section_takes_a_two_byte_size() {
        let data = vec![7u8; 200];
        let program = wrapper(&[("d", &data)]);
        assert_eq!(&program[8..12], &[0, 0xca, 0x01, 1]);
        assert_eq!(program.len(), 8 + 3 + 202);
    }
}
